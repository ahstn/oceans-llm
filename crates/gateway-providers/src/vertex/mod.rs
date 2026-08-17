use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use async_stream::stream;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{
    BatchCapabilities, CoreChatMessage, CoreChatRequest, CoreContentPartType,
    CoreEmbeddingsRequest, CoreResponsesRequest, ProviderBatchRequest, ProviderBatchResult,
    ProviderBatchState, ProviderCapabilities, ProviderClient, ProviderError,
    ProviderRequestContext, ProviderStream, SseEventParser, Utf8ChunkDecoder,
    VERTEX_TEXT_EMBEDDING_MODEL_IDS, is_supported_vertex_text_embedding_model_id,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    http::map_reqwest_error,
    media::{infer_media_type_from_path, is_valid_media_type},
    streaming::{done_sse_chunk, openai_sse_error_chunk},
    token::{
        AccessTokenSource, AdcTokenSource, CLOUD_PLATFORM_SCOPE, CachedAccessTokenSource,
        ServiceAccountTokenSource, StaticBearerTokenSource,
    },
};

mod batch;

#[derive(Debug, Clone)]
pub enum VertexAuthConfig {
    Adc,
    ServiceAccount { credentials_path: PathBuf },
    Bearer { token: String },
}

#[derive(Debug, Clone)]
pub struct VertexProviderConfig {
    pub provider_key: String,
    pub project_id: String,
    pub location: String,
    pub api_host: String,
    pub auth: VertexAuthConfig,
    pub default_headers: BTreeMap<String, String>,
    pub request_timeout_ms: u64,
    pub batch: Option<VertexBatchConfig>,
}

#[derive(Debug, Clone)]
pub struct VertexBatchConfig {
    pub bigquery_project_id: String,
    pub dataset: String,
}

#[derive(Clone)]
pub struct VertexProvider {
    config: VertexProviderConfig,
    client: reqwest::Client,
    access_token_source: CachedAccessTokenSource,
}

impl VertexProvider {
    pub fn new(config: VertexProviderConfig) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .build()
            .map_err(map_reqwest_error)?;

        let source: Arc<dyn AccessTokenSource> = match &config.auth {
            VertexAuthConfig::Adc => {
                Arc::new(AdcTokenSource::new(CLOUD_PLATFORM_SCOPE.to_string())?)
            }
            VertexAuthConfig::ServiceAccount { credentials_path } => {
                Arc::new(ServiceAccountTokenSource::new(
                    credentials_path.clone(),
                    CLOUD_PLATFORM_SCOPE.to_string(),
                )?)
            }
            VertexAuthConfig::Bearer { token } => {
                Arc::new(StaticBearerTokenSource::new(token.clone()))
            }
        };

        Ok(Self {
            config,
            client,
            access_token_source: CachedAccessTokenSource::new(source),
        })
    }

    async fn build_request(
        &self,
        endpoint_suffix: &str,
        body: &Value,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let token = self.access_token_source.token().await?;
        let mut request = self
            .client
            .post(endpoint_suffix)
            .bearer_auth(token)
            .json(body);

        request = request.header("x-request-id", &context.request_id);

        for (name, value) in &self.config.default_headers {
            request = request.header(name, value);
        }

        for (name, value) in &context.extra_headers {
            if let Some(value) = value.as_str() {
                request = request.header(name, value);
            }
        }

        request.build().map_err(map_reqwest_error)
    }
    fn model_endpoint(&self, publisher: &str, model_id: &str, method: &str) -> String {
        let host = self.config.api_host.trim_end_matches('/');
        let base = if host.starts_with("http://") || host.starts_with("https://") {
            host.to_string()
        } else {
            format!("https://{host}")
        };

        format!(
            "{}/v1/projects/{}/locations/{}/publishers/{}/models/{}:{}",
            base, self.config.project_id, self.config.location, publisher, model_id, method
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublisherFamily {
    Google,
    Anthropic,
}

fn parse_upstream_model(
    upstream_model: &str,
) -> Result<(PublisherFamily, &str, &str), ProviderError> {
    let mut parts = upstream_model.splitn(2, '/');
    let publisher = parts.next().unwrap_or_default();
    let model_id = parts.next().unwrap_or_default();

    if publisher.is_empty() || model_id.is_empty() {
        return Err(ProviderError::InvalidRequest(format!(
            "vertex route upstream_model must be <publisher>/<model_id>, got `{upstream_model}`"
        )));
    }

    let family = match publisher {
        "google" => PublisherFamily::Google,
        "anthropic" => PublisherFamily::Anthropic,
        other => {
            return Err(ProviderError::InvalidRequest(format!(
                "vertex publisher `{other}` is not supported in this slice"
            )));
        }
    };

    Ok((family, publisher, model_id))
}

const VERTEX_EMBEDDING_TASK_TYPES: &[&str] = &[
    "RETRIEVAL_QUERY",
    "RETRIEVAL_DOCUMENT",
    "SEMANTIC_SIMILARITY",
    "CLASSIFICATION",
    "CLUSTERING",
    "QUESTION_ANSWERING",
    "FACT_VERIFICATION",
    "CODE_RETRIEVAL_QUERY",
];

const VERTEX_EMBED_CONTENT_MODEL_IDS: &[&str] = &["gemini-embedding-2"];

#[derive(Debug)]
struct GoogleEmbeddingRequestMapping {
    bodies: Vec<Value>,
}

#[derive(Debug)]
struct GoogleEmbeddingOutput {
    index: usize,
    embedding: Value,
    token_count: Option<i64>,
}

#[async_trait]
impl ProviderClient for VertexProvider {
    fn provider_key(&self) -> &str {
        &self.config.provider_key
    }

    fn provider_type(&self) -> &str {
        "gcp_vertex"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::with_dimensions(true, true, false, true, true, false, true)
    }

    fn batch_capabilities(&self) -> BatchCapabilities {
        self.batch_capabilities_impl()
    }

    async fn submit_batch(
        &self,
        request: &ProviderBatchRequest,
    ) -> gateway_core::ProviderBatchSubmission {
        self.submit_batch_impl(request).await
    }

    async fn inspect_batch(
        &self,
        provider_batch_id: &str,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderBatchState, ProviderError> {
        self.inspect_batch_impl(provider_batch_id).await
    }

    async fn cancel_batch(
        &self,
        provider_batch_id: &str,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderBatchState, ProviderError> {
        self.cancel_batch_impl(provider_batch_id).await
    }

    async fn batch_results(
        &self,
        state: &ProviderBatchState,
        context: &ProviderRequestContext,
    ) -> Result<Vec<ProviderBatchResult>, ProviderError> {
        self.batch_results_impl(state, context).await
    }

    async fn chat_completions(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let (family, publisher, model_id) = parse_upstream_model(&context.upstream_model)?;
        let endpoint = match family {
            PublisherFamily::Google => self.model_endpoint(publisher, model_id, "generateContent"),
            PublisherFamily::Anthropic => self.model_endpoint(publisher, model_id, "rawPredict"),
        };

        let body = match family {
            PublisherFamily::Google => map_google_request(request, context, false)?,
            PublisherFamily::Anthropic => map_anthropic_request(request, context, false)?,
        };

        let request = self.build_request(&endpoint, &body, context).await?;
        let response = self
            .client
            .execute(request)
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        let text = response.text().await.map_err(map_reqwest_error)?;
        if !status.is_success() {
            return Err(ProviderError::UpstreamHttp {
                status: status.as_u16(),
                body: text,
            });
        }

        let value: Value = serde_json::from_str(&text).map_err(|error| {
            ProviderError::Transport(format!("invalid JSON from vertex: {error}"))
        })?;

        let normalized = match family {
            PublisherFamily::Google => normalize_google_response(&value, context),
            PublisherFamily::Anthropic => normalize_anthropic_response(&value, context),
        };

        Ok(normalized)
    }

    async fn chat_completions_stream(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let (family, publisher, model_id) = parse_upstream_model(&context.upstream_model)?;
        let endpoint = match family {
            PublisherFamily::Google => {
                self.model_endpoint(publisher, model_id, "streamGenerateContent")
            }
            PublisherFamily::Anthropic => {
                self.model_endpoint(publisher, model_id, "streamRawPredict")
            }
        };
        let body = match family {
            PublisherFamily::Google => map_google_request(request, context, true)?,
            PublisherFamily::Anthropic => map_anthropic_request(request, context, true)?,
        };

        let request = self.build_request(&endpoint, &body, context).await?;
        let response = self
            .client
            .execute(request)
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.map_err(map_reqwest_error)?;
            return Err(ProviderError::UpstreamHttp {
                status: status.as_u16(),
                body,
            });
        }

        let stream_id = format!("chatcmpl-{}", Uuid::new_v4().simple());
        let created = OffsetDateTime::now_utc().unix_timestamp();
        let model = context.model_key.clone();
        let upstream = response.bytes_stream();

        let normalized = match family {
            PublisherFamily::Google => normalize_google_stream(upstream, stream_id, created, model),
            PublisherFamily::Anthropic => {
                normalize_anthropic_stream(upstream, stream_id, created, model)
            }
        };

        Ok(normalized)
    }

    async fn embeddings(
        &self,
        request: &CoreEmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let (family, publisher, model_id) = parse_upstream_model(&context.upstream_model)?;
        if family != PublisherFamily::Google {
            return Err(ProviderError::InvalidRequest(
                "vertex embeddings are only supported for google/* text embedding models"
                    .to_string(),
            ));
        }
        validate_vertex_embedding_model(model_id)?;

        let mapped = map_google_embedding_request(request, context, model_id)?;
        let endpoint = self.model_endpoint(publisher, model_id, vertex_embedding_method(model_id));
        let mut outputs = Vec::with_capacity(mapped.bodies.len());
        for (index, body) in mapped.bodies.iter().enumerate() {
            let request = self.build_request(&endpoint, body, context).await?;
            let response = match self.client.execute(request).await {
                Ok(response) => response,
                Err(error) => {
                    return Err(partial_google_embedding_failure(
                        map_reqwest_error(error),
                        &outputs,
                        false,
                    ));
                }
            };
            let status = response.status();
            let text = match response.text().await {
                Ok(text) => text,
                Err(error) => {
                    return Err(partial_google_embedding_failure(
                        map_reqwest_error(error),
                        &outputs,
                        true,
                    ));
                }
            };
            if !status.is_success() {
                return Err(partial_google_embedding_failure(
                    ProviderError::UpstreamHttp {
                        status: status.as_u16(),
                        body: text,
                    },
                    &outputs,
                    false,
                ));
            }

            let value: Value = match serde_json::from_str(&text) {
                Ok(value) => value,
                Err(error) => {
                    return Err(partial_google_embedding_failure(
                        ProviderError::Transport(format!(
                            "invalid JSON from vertex embeddings: {error}"
                        )),
                        &outputs,
                        true,
                    ));
                }
            };
            let output = match extract_google_embedding_output(&value, index, model_id) {
                Ok(output) => output,
                Err(error) => return Err(partial_google_embedding_failure(error, &outputs, true)),
            };
            outputs.push(output);
        }

        normalize_google_embedding_outputs(outputs, context)
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::InvalidRequest(
            "vertex responses are not supported in this v1 runtime".to_string(),
        ))
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::InvalidRequest(
            "vertex responses streaming is not supported in this v1 runtime".to_string(),
        ))
    }
}

mod anthropic_request;
mod anthropic_thinking;
mod embeddings;
mod google_request;
mod google_tools;
mod response;
mod streaming;

use anthropic_request::{map_anthropic_request, parse_openai_tool_arguments};
use anthropic_thinking::{
    apply_vertex_anthropic_thinking_compatibility, validate_vertex_anthropic_sampling_fields,
};
use embeddings::{
    extract_google_embedding_output, map_google_embedding_request,
    normalize_google_embedding_outputs, partial_google_embedding_failure,
    validate_vertex_embedding_model, vertex_embedding_method,
};
use google_request::{
    map_google_parts, map_google_request, merge_object_overrides, message_content_as_text,
};
use google_tools::{
    convert_openai_tools_for_google, map_google_anthropic_tool_result_part,
    map_google_anthropic_tool_use_part, map_google_assistant_parts, map_google_tool_result_part,
    record_google_tool_names,
};
use response::{
    extract_google_candidate_text, extract_google_candidate_tool_calls,
    map_anthropic_finish_reason, map_anthropic_stream_usage, map_google_finish_reason,
    map_google_usage, merge_openai_stream_usage, normalize_anthropic_response,
    normalize_anthropic_thinking_delta, normalize_anthropic_thinking_start,
    normalize_google_response, vertex_reasoning_metadata,
};
use streaming::{normalize_anthropic_stream, normalize_google_stream};

#[cfg(test)]
use streaming::JsonObjectParser;

#[cfg(test)]
mod tests;
