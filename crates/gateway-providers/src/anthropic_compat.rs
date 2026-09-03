use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use gateway_core::{
    CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, ProviderCapabilities,
    ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::anthropic::{
    AnthropicRequestOptions, map_anthropic_request, normalize_anthropic_response,
    normalize_anthropic_stream,
};
use crate::http::{execute_request, join_base_url, map_reqwest_error};

const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnthropicCompatAuthKind {
    #[default]
    XApiKey,
    Bearer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicCompatAuth {
    pub kind: AnthropicCompatAuthKind,
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct AnthropicCompatConfig {
    pub provider_key: String,
    pub provider_type: String,
    pub base_url: String,
    pub auth: Option<AnthropicCompatAuth>,
    pub default_headers: BTreeMap<String, String>,
    pub request_timeout_ms: u64,
}

impl AnthropicCompatConfig {
    #[must_use]
    pub fn new(provider_key: String, base_url: String) -> Self {
        Self {
            provider_key,
            provider_type: "anthropic_compat".to_string(),
            base_url,
            auth: None,
            default_headers: BTreeMap::new(),
            request_timeout_ms: 120_000,
        }
    }
}

pub struct AnthropicCompatProvider {
    config: AnthropicCompatConfig,
    client: reqwest::Client,
}

impl AnthropicCompatProvider {
    pub fn new(config: AnthropicCompatConfig) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .build()
            .map_err(map_reqwest_error)?;

        Ok(Self { config, client })
    }

    pub(crate) fn messages_endpoint_url(&self) -> Result<String, ProviderError> {
        let base = self.config.base_url.trim_end_matches('/');
        let parsed = url::Url::parse(base)
            .map_err(|err| ProviderError::InvalidRequest(format!("invalid base_url: {err}")))?;
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err(ProviderError::InvalidRequest(
                "base_url cannot contain query parameters or fragments".to_string(),
            ));
        }
        let endpoint = if base.ends_with("/v1") {
            "messages"
        } else {
            "v1/messages"
        };
        join_base_url(base, endpoint)
    }

    pub(crate) fn build_request(
        &self,
        body: Value,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let url = self.messages_endpoint_url()?;
        let mut request = self.client.post(url).json(&body);

        let mut has_version = false;
        for (header_name, header_value) in &self.config.default_headers {
            if header_name.eq_ignore_ascii_case("anthropic-version") {
                has_version = true;
            }
            request = request.header(header_name, header_value);
        }
        if !has_version {
            request = request.header("anthropic-version", DEFAULT_ANTHROPIC_VERSION);
        }

        if let Some(auth) = &self.config.auth {
            match auth.kind {
                AnthropicCompatAuthKind::XApiKey => {
                    request = request.header("x-api-key", &auth.token);
                }
                AnthropicCompatAuthKind::Bearer => {
                    request = request.header("authorization", format!("Bearer {}", auth.token));
                }
            }
        }

        for (header_name, value) in &context.extra_headers {
            if let Some(val) = value.as_str() {
                request = request.header(header_name, val);
            }
        }

        request = request.header("x-request-id", &context.request_id);

        request.build().map_err(map_reqwest_error)
    }
}

#[async_trait]
impl ProviderClient for AnthropicCompatProvider {
    fn provider_key(&self) -> &str {
        &self.config.provider_key
    }

    fn provider_type(&self) -> &str {
        &self.config.provider_type
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            chat_completions: true,
            responses: false,
            stream: true,
            embeddings: false,
            tools: true,
            vision: true,
            json_schema: false,
            developer_role: false,
        }
    }

    async fn chat_completions(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let options = AnthropicRequestOptions {
            include_model: true,
            anthropic_version_body: None,
            default_max_tokens: Some(4096),
            default_headers: Some(&self.config.default_headers),
        };
        let body = map_anthropic_request(request, context, false, &options)?;
        let req = self.build_request(body, context)?;

        let response = execute_request(
            &self.client,
            req,
            &self.config.provider_type,
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

        let value: Value = serde_json::from_str(&text).map_err(|err| {
            ProviderError::Transport(format!("invalid JSON from anthropic_compat: {err}"))
        })?;

        Ok(normalize_anthropic_response(
            &value,
            context,
            "anthropic_compat",
            "anthropic_compat",
        ))
    }

    async fn chat_completions_stream(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let options = AnthropicRequestOptions {
            include_model: true,
            anthropic_version_body: None,
            default_max_tokens: Some(4096),
            default_headers: Some(&self.config.default_headers),
        };
        let body = map_anthropic_request(request, context, true, &options)?;
        let req = self.build_request(body, context)?;

        let response = execute_request(
            &self.client,
            req,
            &self.config.provider_type,
            &self.config.provider_key,
        )
        .await
        .map_err(map_reqwest_error)?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.map_err(map_reqwest_error)?;
            return Err(ProviderError::UpstreamHttp {
                status: status.as_u16(),
                body: text,
            });
        }

        let stream_id = format!("chatcmpl-{}", Uuid::new_v4().simple());
        let created = OffsetDateTime::now_utc().unix_timestamp();

        Ok(normalize_anthropic_stream(
            response.bytes_stream(),
            stream_id,
            created,
            context.model_key.clone(),
            "anthropic_compat",
            "anthropic_compat",
        ))
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::NotImplemented(
            "anthropic_compat does not support embeddings".to_string(),
        ))
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::NotImplemented(
            "anthropic_compat does not support responses".to_string(),
        ))
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::NotImplemented(
            "anthropic_compat does not support responses streaming".to_string(),
        ))
    }
}
