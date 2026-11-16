use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use gateway_core::{
    CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, ProviderCapabilities,
    ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
    core_responses_request_to_openai,
};
use serde_json::Value;

use crate::http::{execute_request, join_base_url, map_reqwest_error};
use crate::openai_compat::{
    apply_openai_compat_empty_tools_profile, apply_openai_compat_request_profile,
};
use crate::service_tier::{DEFAULT_ONLY, checked_service_tier};
use crate::streaming::normalize_openai_compat_responses_stream;
use crate::token::AccessTokenSource;

pub mod auth;
mod errors;
mod policy;
#[cfg(test)]
mod tests;

pub const DEFAULT_CHATGPT_PLAN_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone)]
pub struct ChatGptPlanConfig {
    pub provider_key: String,
    pub credentials_file: PathBuf,
    pub base_url: String,
    pub strip_unsupported_fields: bool,
    pub default_headers: BTreeMap<String, String>,
    pub request_timeout_ms: u64,
}

#[derive(Clone)]
pub struct ChatGptPlanProvider {
    config: ChatGptPlanConfig,
    client: reqwest::Client,
    token_source: Arc<auth::ChatGptPlanTokenSource>,
}

impl ChatGptPlanProvider {
    pub fn new(config: ChatGptPlanConfig) -> Result<Self, ProviderError> {
        let token_source = Arc::new(auth::ChatGptPlanTokenSource::new(
            config.credentials_file.clone(),
        )?);
        // A redirect must never forward the selected account's bearer credential.
        let client =
            crate::http::provider_http_client_without_redirects(config.request_timeout_ms)?;
        Ok(Self {
            config,
            client,
            token_source,
        })
    }

    fn build_responses_stream_request(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
        token: &str,
    ) -> Result<reqwest::Request, ProviderError> {
        checked_service_tier(context, DEFAULT_ONLY)?;
        let mut body = serde_json::to_value(core_responses_request_to_openai(request))
            .map_err(|error| ProviderError::Transport(error.to_string()))?;
        if let Some(object) = body.as_object_mut() {
            object.insert(
                "model".to_string(),
                Value::String(context.upstream_model.clone()),
            );
            for (key, value) in &context.extra_body {
                object.insert(key.clone(), value.clone());
            }
        }
        crate::replay_id::normalize_openai_responses_replay_ids(&mut body)?;
        apply_openai_compat_request_profile(&mut body, context);
        apply_openai_compat_empty_tools_profile(&mut body, context)?;
        policy::apply_request_policy(&mut body, self.config.strip_unsupported_fields);

        let url = join_base_url(&self.config.base_url, "responses")?;
        let mut builder = self.client.post(url).json(&body);
        for (name, value) in &self.config.default_headers {
            builder = builder.header(name, value);
        }
        for (name, value) in &context.extra_headers {
            if let Some(value) = value.as_str() {
                builder = builder.header(name, value);
            }
        }
        builder = builder
            .header("x-request-id", &context.request_id)
            .header(reqwest::header::ACCEPT, "text/event-stream");
        let mut request = builder.build().map_err(map_reqwest_error)?;
        // SIWC uses the public Responses endpoint with Bearer auth only. Never
        // inherit headers intended for the unrelated ChatGPT backend API.
        for name in [
            "account-id",
            "chatgpt-account-id",
            "originator",
            "openai-beta",
        ] {
            request.headers_mut().remove(name);
        }
        let mut authorization = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| {
                ProviderError::InvalidRequest(
                    "ChatGPT plan credential cannot be encoded as an Authorization header"
                        .to_string(),
                )
            })?;
        authorization.set_sensitive(true);
        request
            .headers_mut()
            .insert(reqwest::header::AUTHORIZATION, authorization);
        Ok(request)
    }
}

#[async_trait]
impl ProviderClient for ChatGptPlanProvider {
    fn provider_key(&self) -> &str {
        &self.config.provider_key
    }

    fn provider_type(&self) -> &str {
        "chatgpt_plan"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            responses: true,
            stream: true,
            tools: true,
            vision: true,
            json_schema: true,
            developer_role: true,
            ..ProviderCapabilities::none()
        }
    }

    async fn chat_completions(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::NotImplemented(
            "chatgpt_plan does not support Chat Completions".to_string(),
        ))
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(ProviderError::NotImplemented(
            "chatgpt_plan does not support Chat Completions streaming".to_string(),
        ))
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::NotImplemented(
            "chatgpt_plan does not support embeddings".to_string(),
        ))
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::InvalidRequest(
            "chatgpt_plan routes require stream: true".to_string(),
        ))
    }

    async fn responses_stream(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        checked_service_tier(context, DEFAULT_ONLY)?;
        let token = self.token_source.fetch_token().await?;
        let request = self.build_responses_stream_request(request, context, &token.token)?;
        let response = execute_request(
            &self.client,
            request,
            self.provider_type(),
            self.provider_key(),
        )
        .await
        .map_err(map_reqwest_error)?;
        let status = response.status();
        let request_id = errors::response_request_id(response.headers());
        if !status.is_success() {
            let body = response.text().await.map_err(map_reqwest_error)?;
            return Err(errors::upstream_error(status.as_u16(), body, request_id));
        }
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value.split(';').next().is_some_and(|media_type| {
                    media_type.trim().eq_ignore_ascii_case("text/event-stream")
                })
            })
        {
            return Err(ProviderError::Transport(
                "chatgpt_plan stream response content-type must be `text/event-stream`".to_string(),
            ));
        }
        Ok(errors::require_terminal(
            normalize_openai_compat_responses_stream(response.bytes_stream()),
            request_id,
        ))
    }
}
