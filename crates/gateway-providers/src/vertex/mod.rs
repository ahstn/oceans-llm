use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use gateway_core::{
    BatchCapabilities, CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest,
    ProviderBatchRequest, ProviderBatchResult, ProviderBatchState, ProviderCapabilities,
    ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    anthropic::{normalize_anthropic_response, normalize_anthropic_stream},
    http::{execute_request, map_reqwest_error},
    token::{
        AccessTokenSource, AdcTokenSource, CLOUD_PLATFORM_SCOPE, CachedAccessTokenSource,
        ServiceAccountTokenSource, StaticBearerTokenSource,
    },
};

mod anthropic;
mod batch;
mod embeddings;
mod error;
mod gemini;
mod google_request;
mod google_response;
mod google_stream;
mod google_tools;
#[cfg(test)]
mod tests;

use anthropic::map_vertex_anthropic_request;
use embeddings::{
    GoogleEmbeddingOutput, extract_google_embedding_outputs, map_google_embedding_request,
    normalize_google_embedding_outputs, partial_google_embedding_failure,
    validate_vertex_embedding_model, vertex_embedding_method,
};
use error::VertexAdapterError;
use google_request::map_google_request;
use google_response::normalize_google_response;
use google_stream::normalize_google_stream;

const ANTHROPIC_USAGE_SOURCE: &str = "vertex_anthropic";
const PROVIDER_NAMESPACE: &str = "gcp_vertex";

/// Vertex host that serves a location: `global` and the multi-region `us`/`eu` endpoints have
/// dedicated hosts; every other location is a regional `{region}-aiplatform.googleapis.com`.
#[must_use]
pub fn vertex_api_host_for_location(location: &str) -> String {
    match location {
        "global" => "aiplatform.googleapis.com".to_string(),
        "us" | "eu" => format!("aiplatform.{location}.rep.googleapis.com"),
        region => format!("{region}-aiplatform.googleapis.com"),
    }
}

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

    #[tracing::instrument(name = "gateway.provider.prepare_request", skip_all)]
    async fn build_request(
        &self,
        endpoint: &str,
        body: &Value,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let token = self.access_token_source.token().await?;
        let request = self.client.post(endpoint).bearer_auth(token).json(body);
        self.apply_request_headers(request, context)
            .build()
            .map_err(map_reqwest_error)
    }

    fn apply_request_headers(
        &self,
        mut request: reqwest::RequestBuilder,
        context: &ProviderRequestContext,
    ) -> reqwest::RequestBuilder {
        request = request.header("x-request-id", &context.request_id);
        for (name, value) in &self.config.default_headers {
            request = request.header(name, value);
        }
        for (name, value) in &context.extra_headers {
            if let Some(value) = value.as_str() {
                request = request.header(name, value);
            }
        }
        request
    }

    fn model_endpoint(&self, publisher: &str, model_id: &str, method: &str) -> String {
        let host = self.config.api_host.trim_end_matches('/');
        let scheme = if host.starts_with("http://") || host.starts_with("https://") {
            ""
        } else {
            "https://"
        };
        format!(
            "{scheme}{host}/v1/projects/{}/locations/{}/publishers/{publisher}/models/{model_id}:{method}",
            self.config.project_id, self.config.location
        )
    }

    /// Sends a mapped body and returns the upstream JSON, or the upstream HTTP failure.
    async fn post_json(
        &self,
        endpoint: &str,
        body: &Value,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let request = self.build_request(endpoint, body, context).await?;
        let response = execute_request(
            &self.client,
            request,
            PROVIDER_NAMESPACE,
            &self.config.provider_key,
        )
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
        serde_json::from_str(&text)
            .map_err(|error| ProviderError::Transport(format!("invalid JSON from vertex: {error}")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublisherFamily {
    Google,
    Anthropic,
}

fn parse_upstream_model(
    upstream_model: &str,
) -> Result<(PublisherFamily, &str, &str), VertexAdapterError> {
    let (publisher, model_id) = upstream_model
        .split_once('/')
        .filter(|(publisher, model_id)| !publisher.is_empty() && !model_id.is_empty())
        .ok_or_else(|| VertexAdapterError::InvalidUpstreamModel(upstream_model.to_string()))?;
    let family = match publisher {
        "google" => PublisherFamily::Google,
        "anthropic" => PublisherFamily::Anthropic,
        other => return Err(VertexAdapterError::UnsupportedPublisher(other.to_string())),
    };
    Ok((family, publisher, model_id))
}

#[async_trait]
impl ProviderClient for VertexProvider {
    fn provider_key(&self) -> &str {
        &self.config.provider_key
    }

    fn provider_type(&self) -> &str {
        PROVIDER_NAMESPACE
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::with_dimensions(true, true, false, true, true, true, true)
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
        context: &ProviderRequestContext,
    ) -> Result<ProviderBatchState, ProviderError> {
        self.inspect_batch_impl(provider_batch_id, context).await
    }

    async fn cancel_batch(
        &self,
        provider_batch_id: &str,
        context: &ProviderRequestContext,
    ) -> Result<ProviderBatchState, ProviderError> {
        self.cancel_batch_impl(provider_batch_id, context).await
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
        match family {
            PublisherFamily::Google => {
                let body = map_google_request(request, context, model_id, false)?;
                let endpoint = self.model_endpoint(publisher, model_id, "generateContent");
                let value = self.post_json(&endpoint, &body, context).await?;
                Ok(normalize_google_response(&value, context)?)
            }
            PublisherFamily::Anthropic => {
                let body = map_vertex_anthropic_request(
                    request,
                    context,
                    false,
                    &self.config.default_headers,
                )?;
                let endpoint = self.model_endpoint(publisher, model_id, "rawPredict");
                let value = self.post_json(&endpoint, &body, context).await?;
                Ok(normalize_anthropic_response(
                    &value,
                    context,
                    PROVIDER_NAMESPACE,
                    ANTHROPIC_USAGE_SOURCE,
                ))
            }
        }
    }

    async fn chat_completions_stream(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let (family, publisher, model_id) = parse_upstream_model(&context.upstream_model)?;
        let (endpoint, body) = match family {
            PublisherFamily::Google => (
                format!(
                    "{}?alt=sse",
                    self.model_endpoint(publisher, model_id, "streamGenerateContent")
                ),
                map_google_request(request, context, model_id, true)?,
            ),
            PublisherFamily::Anthropic => (
                self.model_endpoint(publisher, model_id, "streamRawPredict"),
                map_vertex_anthropic_request(request, context, true, &self.config.default_headers)?,
            ),
        };

        let request = self.build_request(&endpoint, &body, context).await?;
        let response = execute_request(
            &self.client,
            request,
            PROVIDER_NAMESPACE,
            &self.config.provider_key,
        )
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

        Ok(match family {
            PublisherFamily::Google => normalize_google_stream(upstream, stream_id, created, model),
            PublisherFamily::Anthropic => normalize_anthropic_stream(
                upstream,
                stream_id,
                created,
                model,
                PROVIDER_NAMESPACE,
                ANTHROPIC_USAGE_SOURCE,
            ),
        })
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
        let mut outputs: Vec<GoogleEmbeddingOutput> = Vec::with_capacity(mapped.input_count);
        for body in &mapped.bodies {
            let request = self.build_request(&endpoint, body, context).await?;
            let response = match execute_request(
                &self.client,
                request,
                PROVIDER_NAMESPACE,
                &self.config.provider_key,
            )
            .await
            {
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
            match extract_google_embedding_outputs(&value, outputs.len(), model_id) {
                Ok(batch) => outputs.extend(batch),
                Err(error) => return Err(partial_google_embedding_failure(error, &outputs, true)),
            }
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
