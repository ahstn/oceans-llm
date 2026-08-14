use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use gateway_core::{
    CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, ProviderCapabilities,
    ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
    core_chat_request_to_openai, core_embeddings_request_to_openai,
    core_responses_request_to_openai,
};
use serde_json::Value;
use crate::http::{join_base_url, map_reqwest_error};
use crate::bedrock::{normalize_anthropic_messages_response, normalize_anthropic_messages_stream};
use crate::streaming::{normalize_openai_compat_responses_stream, normalize_openai_compat_stream};
use crate::token::CachedAccessTokenSource;

pub mod auth;
pub use auth::CopilotAuthConfig;

#[cfg(test)]
mod tests;

pub const DEFAULT_COPILOT_API_URL: &str = "https://api.githubcopilot.com";
pub const DEFAULT_COPILOT_EDITOR_VERSION: &str = "vscode/1.126.0";
pub const DEFAULT_COPILOT_INTEGRATION_ID: &str = "vscode-chat";
pub const DEFAULT_COPILOT_API_VERSION: &str = "2026-06-01";

#[derive(Debug, Clone)]
pub struct CopilotProviderConfig {
    pub provider_key: String,
    pub base_url: String,
    pub github_api_url: Option<String>,
    pub auth: CopilotAuthConfig,
    pub editor_version: String,
    pub integration_id: String,
    pub default_headers: BTreeMap<String, String>,
    pub request_timeout_ms: u64,
}

impl CopilotProviderConfig {
    #[must_use]
    pub fn new(provider_key: String, auth: CopilotAuthConfig) -> Self {
        Self {
            provider_key,
            base_url: DEFAULT_COPILOT_API_URL.to_string(),
            github_api_url: None,
            auth,
            editor_version: DEFAULT_COPILOT_EDITOR_VERSION.to_string(),
            integration_id: DEFAULT_COPILOT_INTEGRATION_ID.to_string(),
            default_headers: BTreeMap::new(),
            request_timeout_ms: 120_000,
        }
    }
}

#[derive(Clone)]
pub struct CopilotProvider {
    config: CopilotProviderConfig,
    client: reqwest::Client,
    access_token_source: CachedAccessTokenSource,
}

impl CopilotProvider {
    pub fn new(config: CopilotProviderConfig) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .build()
            .map_err(map_reqwest_error)?;

        let source = config
            .auth
            .build_source(config.github_api_url.as_deref())?;

        Ok(Self {
            config,
            client,
            access_token_source: CachedAccessTokenSource::new(source),
        })
    }

    /// Direct token accessor (retrieves cached token or fetches a fresh one).
    pub async fn token(&self) -> Result<String, ProviderError> {
        self.access_token_source.token().await
    }

    /// Selects the appropriate endpoint suffix based on the upstream model ID and format.
    #[must_use]
    pub fn resolve_chat_endpoint_suffix(model: &str) -> &'static str {
        let normalized = model.to_ascii_lowercase();
        if normalized.starts_with("claude-") {
            "v1/messages"
        } else if normalized.starts_with("gpt-5")
            || normalized.contains("-codex")
            || normalized.starts_with("codex")
        {
            "responses"
        } else {
            "chat/completions"
        }
    }

    fn apply_copilot_headers(
        &self,
        mut request: reqwest::RequestBuilder,
        token: &str,
        context: &ProviderRequestContext,
    ) -> reqwest::RequestBuilder {
        request = request
            .bearer_auth(token)
            .header("editor-version", &self.config.editor_version)
            .header("copilot-integration-id", &self.config.integration_id)
            .header("openai-intent", "conversation-panel")
            .header("x-initiator", "agent")
            .header("x-github-api-version", DEFAULT_COPILOT_API_VERSION)
            .header("x-request-id", &context.request_id);

        for (name, value) in &self.config.default_headers {
            request = request.header(name, value);
        }

        for (name, value) in &context.extra_headers {
            if let Some(val) = value.as_str() {
                request = request.header(name, val);
            }
        }

        request
    }

    async fn build_chat_request(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
        stream: bool,
    ) -> Result<reqwest::Request, ProviderError> {
        let token = self.token().await?;
        let endpoint_suffix = Self::resolve_chat_endpoint_suffix(&context.upstream_model);

        let url = join_base_url(&self.config.base_url, endpoint_suffix)?;

        let body = if endpoint_suffix == "v1/messages" {
            // Anthropic Messages wire format
            let mut wire = serde_json::to_value(request)
                .map_err(|err| ProviderError::Transport(err.to_string()))?;
            if let Some(map) = wire.as_object_mut() {
                map.insert("model".to_string(), Value::String(context.upstream_model.clone()));
                map.insert("stream".to_string(), Value::Bool(stream));
                if !map.contains_key("max_tokens") {
                    map.insert("max_tokens".to_string(), serde_json::json!(4096));
                }
            }
            wire
        } else if endpoint_suffix == "responses" {
            // OpenAI Responses API wire format
            let responses_req = CoreResponsesRequest {
                model: context.upstream_model.clone(),
                input: request
                    .messages
                    .iter()
                    .map(|m| {
                        serde_json::json!({
                            "role": m.role,
                            "content": m.content,
                        })
                    })
                    .collect(),
                stream,
                instructions: None,
                tools: None,
                tool_choice: None,
                reasoning: None,
                text: None,
                extra: request.extra.clone(),
            };
            let wire = core_responses_request_to_openai(&responses_req);
            serde_json::to_value(wire)
                .map_err(|err| ProviderError::Transport(err.to_string()))?
        } else {
            // Standard OpenAI chat/completions wire format
            let mut stream_request = request.clone();
            stream_request.stream = stream;
            let wire_request = core_chat_request_to_openai(&stream_request);
            let mut body = serde_json::to_value(wire_request)
                .map_err(|error| ProviderError::Transport(error.to_string()))?;

            if let Some(object) = body.as_object_mut() {
                object.insert(
                    "model".to_string(),
                    Value::String(context.upstream_model.clone()),
                );
            }
            body
        };

        let req_builder = self.client.post(url).json(&body);
        let req_builder = self.apply_copilot_headers(req_builder, &token, context);

        req_builder.build().map_err(map_reqwest_error)
    }

    async fn build_embeddings_request(
        &self,
        request: &CoreEmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let token = self.token().await?;
        let url = join_base_url(&self.config.base_url, "embeddings")?;

        let wire_request = core_embeddings_request_to_openai(request);
        let mut body = serde_json::to_value(wire_request)
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        if let Some(object) = body.as_object_mut() {
            object.insert(
                "model".to_string(),
                Value::String(context.upstream_model.clone()),
            );
        }

        let req_builder = self.client.post(url).json(&body);
        let req_builder = self.apply_copilot_headers(req_builder, &token, context);

        req_builder.build().map_err(map_reqwest_error)
    }

    async fn build_responses_request(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
        stream: bool,
    ) -> Result<reqwest::Request, ProviderError> {
        let token = self.token().await?;
        let url = join_base_url(&self.config.base_url, "responses")?;

        let mut stream_request = request.clone();
        stream_request.stream = stream;
        let wire_request = core_responses_request_to_openai(&stream_request);
        let mut body = serde_json::to_value(wire_request)
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        if let Some(object) = body.as_object_mut() {
            object.insert(
                "model".to_string(),
                Value::String(context.upstream_model.clone()),
            );
        }

        let req_builder = self.client.post(url).json(&body);
        let req_builder = self.apply_copilot_headers(req_builder, &token, context);

        req_builder.build().map_err(map_reqwest_error)
    }

    async fn execute_json_request(
        &self,
        request: reqwest::Request,
    ) -> Result<Value, ProviderError> {
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

        serde_json::from_str(&text).map_err(|error| ProviderError::Transport(error.to_string()))
    }

    async fn execute_stream_request(
        &self,
        request: reqwest::Request,
    ) -> Result<reqwest::Response, ProviderError> {
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

        Ok(response)
    }
}

#[async_trait]
impl ProviderClient for CopilotProvider {
    fn provider_key(&self) -> &str {
        &self.config.provider_key
    }

    fn provider_type(&self) -> &str {
        "github_copilot"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::all_enabled()
    }

    async fn chat_completions(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let endpoint_suffix = Self::resolve_chat_endpoint_suffix(&context.upstream_model);
        let request = self.build_chat_request(request, context, false).await?;
        let value = self.execute_json_request(request).await?;
        if endpoint_suffix == "v1/messages" {
            Ok(normalize_anthropic_messages_response(&value, context))
        } else {
            Ok(value)
        }
    }

    async fn chat_completions_stream(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let endpoint_suffix = Self::resolve_chat_endpoint_suffix(&context.upstream_model);
        let request = self.build_chat_request(request, context, true).await?;
        let response = self.execute_stream_request(request).await?;

        if endpoint_suffix == "v1/messages" {
            Ok(normalize_anthropic_messages_stream(
                response.bytes_stream(),
                context.clone(),
            ))
        } else {
            Ok(normalize_openai_compat_stream(response.bytes_stream()))
        }
    }

    async fn embeddings(
        &self,
        request: &CoreEmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let request = self.build_embeddings_request(request, context).await?;
        self.execute_json_request(request).await
    }

    async fn responses(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let request = self.build_responses_request(request, context, false).await?;
        self.execute_json_request(request).await
    }

    async fn responses_stream(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let request = self.build_responses_request(request, context, true).await?;
        let response = self.execute_stream_request(request).await?;

        Ok(normalize_openai_compat_responses_stream(
            response.bytes_stream(),
        ))
    }
}
