use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use gateway_core::{
    CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, GitHubCopilotChatApi,
    ProviderCapabilities, ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
    ProviderUserTokenResolver, core_chat_request_to_openai, core_embeddings_request_to_openai,
    core_responses_request_to_openai,
};
use serde_json::Value;

use crate::bedrock::{
    AnthropicMessagesTarget, map_chat_request_to_anthropic_messages, merge_object_overrides,
    normalize_anthropic_messages_response, normalize_anthropic_messages_stream,
};
use crate::http::{
    TracedResponse, execute_request as execute_http_request, join_base_url, map_reqwest_error,
};
use crate::openai_compat::{
    apply_openai_compat_empty_tools_profile, apply_openai_compat_request_profile,
};
use crate::streaming::{normalize_openai_compat_responses_stream, normalize_openai_compat_stream};
use crate::token::CachedAccessTokenSource;

pub mod auth;
pub use auth::CopilotAuthConfig;

#[cfg(test)]
mod tests;

pub const DEFAULT_COPILOT_API_URL: &str = "https://api.githubcopilot.com";
pub const DEFAULT_COPILOT_EDITOR_VERSION: &str = "vscode/1.126.0";
pub const DEFAULT_COPILOT_PLUGIN_VERSION: &str = "copilot-chat/0.35.0";
pub const DEFAULT_COPILOT_INTEGRATION_ID: &str = "vscode-chat";
pub const DEFAULT_COPILOT_API_VERSION: &str = "2026-06-01";
#[derive(Debug, Clone, Copy)]
struct CopilotCompatibilityProfile {
    plugin_version: &'static str,
    openai_intent: &'static str,
    interaction_type: &'static str,
    github_api_version: &'static str,
    anthropic_version: &'static str,
}

const VSCODE_CHAT_2026_06_01_PROFILE: CopilotCompatibilityProfile = CopilotCompatibilityProfile {
    plugin_version: DEFAULT_COPILOT_PLUGIN_VERSION,
    openai_intent: "conversation-agent",
    interaction_type: "conversation-agent",
    github_api_version: DEFAULT_COPILOT_API_VERSION,
    anthropic_version: "2023-06-01",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CopilotInitiator {
    User,
    Agent,
}

impl CopilotInitiator {
    const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
        }
    }

    fn for_chat(request: &CoreChatRequest) -> Self {
        let Some(message) = request.messages.last() else {
            return Self::Agent;
        };

        match message.role.to_ascii_lowercase().as_str() {
            "user" if !content_is_only_tool_results(&message.content) => Self::User,
            _ => Self::Agent,
        }
    }

    fn for_responses(request: &CoreResponsesRequest) -> Self {
        let last_input = match &request.input {
            Value::Array(items) => items.last(),
            value => Some(value),
        };

        match last_input {
            Some(Value::String(_)) => Self::User,
            Some(value) if response_input_is_user_turn(value) => Self::User,
            _ => Self::Agent,
        }
    }
}

fn content_is_only_tool_results(content: &Value) -> bool {
    let Value::Array(parts) = content else {
        return false;
    };
    !parts.is_empty()
        && parts.iter().all(|part| {
            part.get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "tool_result" || kind.ends_with("_call_output"))
        })
}

fn response_input_is_user_turn(input: &Value) -> bool {
    let Some(object) = input.as_object() else {
        return false;
    };
    if object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.ends_with("_call_output"))
    {
        return false;
    }
    object
        .get("role")
        .and_then(Value::as_str)
        .is_some_and(|role| role.eq_ignore_ascii_case("user"))
        && !object
            .get("content")
            .is_some_and(content_is_only_tool_results)
}

fn normalize_embeddings_response(mut response: Value, requested_model: &str) -> Value {
    if let Some(object) = response.as_object_mut() {
        object
            .entry("object")
            .or_insert_with(|| Value::String("list".to_string()));
        object
            .entry("model")
            .or_insert_with(|| Value::String(requested_model.to_string()));
    }
    response
}

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
    token_source: CopilotTokenSource,
}

#[derive(Clone)]
enum CopilotTokenSource {
    Shared(CachedAccessTokenSource),
    PerUser(Arc<dyn ProviderUserTokenResolver>),
}

impl CopilotProvider {
    pub fn new(config: CopilotProviderConfig) -> Result<Self, ProviderError> {
        if matches!(config.auth, CopilotAuthConfig::GitHubUser) {
            return Err(ProviderError::InvalidRequest(
                "GitHub user authentication requires a per-user token resolver".to_string(),
            ));
        }
        let token_source = CopilotTokenSource::Shared(CachedAccessTokenSource::new(
            config.auth.build_source(config.github_api_url.as_deref())?,
        ));
        Self::build(config, token_source)
    }

    pub fn new_with_user_token_resolver(
        config: CopilotProviderConfig,
        user_token_resolver: Arc<dyn ProviderUserTokenResolver>,
    ) -> Result<Self, ProviderError> {
        if !matches!(config.auth, CopilotAuthConfig::GitHubUser) {
            return Err(ProviderError::InvalidRequest(
                "a per-user token resolver requires GitHub user authentication".to_string(),
            ));
        }
        Self::build(config, CopilotTokenSource::PerUser(user_token_resolver))
    }

    fn build(
        config: CopilotProviderConfig,
        token_source: CopilotTokenSource,
    ) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .build()
            .map_err(map_reqwest_error)?;
        Ok(Self {
            config,
            client,
            token_source,
        })
    }

    async fn token(&self, context: &ProviderRequestContext) -> Result<String, ProviderError> {
        match &self.token_source {
            CopilotTokenSource::Shared(source) => source.token().await,
            CopilotTokenSource::PerUser(resolver) => {
                let user_id = context.owner_user_id.ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "GitHub user authentication requires a user-owned gateway API key"
                            .to_string(),
                    )
                })?;
                resolver
                    .resolve_provider_user_token(&self.config.provider_key, user_id)
                    .await
            }
        }
    }

    /// Selects the configured chat API and fails closed when metadata is absent.
    fn resolve_chat_api(
        context: &ProviderRequestContext,
    ) -> Result<GitHubCopilotChatApi, ProviderError> {
        context
            .compatibility
            .github_copilot
            .as_ref()
            .and_then(|compatibility| compatibility.chat_api)
            .ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "github_copilot route does not configure a chat API".to_string(),
                )
            })
    }

    const fn chat_endpoint_suffix(chat_api: GitHubCopilotChatApi) -> &'static str {
        match chat_api {
            GitHubCopilotChatApi::AnthropicMessages => "v1/messages",
            GitHubCopilotChatApi::ChatCompletions => "chat/completions",
        }
    }

    fn apply_copilot_headers(
        &self,
        mut request: reqwest::RequestBuilder,
        context: &ProviderRequestContext,
    ) -> reqwest::RequestBuilder {
        let profile = VSCODE_CHAT_2026_06_01_PROFILE;
        request = request
            .header("editor-version", &self.config.editor_version)
            .header("editor-plugin-version", profile.plugin_version)
            .header("copilot-integration-id", &self.config.integration_id)
            .header("openai-intent", profile.openai_intent)
            .header("x-interaction-type", profile.interaction_type)
            .header("x-github-api-version", profile.github_api_version)
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

    #[tracing::instrument(name = "gateway.provider.prepare_request", skip_all)]
    async fn build_copilot_request(
        &self,
        endpoint_suffix: &str,
        body: Value,
        context: &ProviderRequestContext,
        chat_api: Option<GitHubCopilotChatApi>,
        initiator: CopilotInitiator,
    ) -> Result<reqwest::Request, ProviderError> {
        let token = self.token(context).await?;
        let url = join_base_url(&self.config.base_url, endpoint_suffix)?;
        let req_builder = self.client.post(url).json(&body);
        let req_builder = self.apply_copilot_headers(req_builder, context);
        let mut request = req_builder.build().map_err(map_reqwest_error)?;
        if chat_api == Some(GitHubCopilotChatApi::AnthropicMessages) {
            request.headers_mut().insert(
                "anthropic-version",
                reqwest::header::HeaderValue::from_static(
                    VSCODE_CHAT_2026_06_01_PROFILE.anthropic_version,
                ),
            );
        }
        let mut authorization =
            reqwest::header::HeaderValue::from_bytes(format!("Bearer {token}").as_bytes())
                .map_err(|_| {
                    ProviderError::InvalidRequest(
                        "GitHub Copilot credential cannot be encoded as an Authorization header"
                            .to_string(),
                    )
                })?;
        authorization.set_sensitive(true);
        // Insert after configurable headers so one resolved credential is authoritative.
        request
            .headers_mut()
            .insert(reqwest::header::AUTHORIZATION, authorization);
        request.headers_mut().insert(
            "x-initiator",
            reqwest::header::HeaderValue::from_static(initiator.as_str()),
        );
        Ok(request)
    }

    async fn build_chat_request(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
        chat_api: GitHubCopilotChatApi,
        stream: bool,
    ) -> Result<reqwest::Request, ProviderError> {
        let endpoint_suffix = Self::chat_endpoint_suffix(chat_api);
        if stream
            && !context
                .compatibility
                .github_copilot
                .as_ref()
                .is_some_and(|compatibility| compatibility.upstream_supports.streaming)
        {
            return Err(ProviderError::InvalidRequest(
                "github_copilot route does not support streaming".to_string(),
            ));
        }

        let body = if chat_api == GitHubCopilotChatApi::AnthropicMessages {
            let mut stream_request = request.clone();
            stream_request.stream = stream;
            // Anthropic Messages API requires `max_tokens`; inject a sensible default if unspecified.
            if !stream_request.extra.contains_key("max_tokens")
                && !stream_request.extra.contains_key("max_completion_tokens")
            {
                stream_request
                    .extra
                    .insert("max_tokens".to_string(), serde_json::json!(4096));
            }
            map_chat_request_to_anthropic_messages(
                &stream_request,
                context,
                AnthropicMessagesTarget::MantleMessages,
            )?
        } else {
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
                merge_object_overrides(object, &context.extra_body);
            }
            apply_openai_compat_empty_tools_profile(&mut body, context)?;
            apply_openai_compat_request_profile(&mut body, context);
            body
        };

        self.build_copilot_request(
            endpoint_suffix,
            body,
            context,
            Some(chat_api),
            CopilotInitiator::for_chat(request),
        )
        .await
    }

    async fn build_embeddings_request(
        &self,
        request: &CoreEmbeddingsRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let wire_request = core_embeddings_request_to_openai(request);
        let mut body = serde_json::to_value(wire_request)
            .map_err(|error| ProviderError::Transport(error.to_string()))?;

        if let Some(object) = body.as_object_mut() {
            object.insert(
                "model".to_string(),
                Value::String(context.upstream_model.clone()),
            );
            merge_object_overrides(object, &context.extra_body);
        }

        if !context
            .compatibility
            .github_copilot
            .as_ref()
            .is_some_and(|compatibility| compatibility.supports_embeddings)
        {
            return Err(ProviderError::InvalidRequest(
                "github_copilot route does not support embeddings".to_string(),
            ));
        }
        self.build_copilot_request("embeddings", body, context, None, CopilotInitiator::Agent)
            .await
    }

    async fn build_responses_request(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
        stream: bool,
    ) -> Result<reqwest::Request, ProviderError> {
        if !context
            .compatibility
            .github_copilot
            .as_ref()
            .is_some_and(|compatibility| compatibility.supports_responses)
        {
            return Err(ProviderError::InvalidRequest(
                "github_copilot route does not support responses".to_string(),
            ));
        }
        if stream
            && !context
                .compatibility
                .github_copilot
                .as_ref()
                .is_some_and(|compatibility| compatibility.upstream_supports.streaming)
        {
            return Err(ProviderError::InvalidRequest(
                "github_copilot route does not support streaming".to_string(),
            ));
        }

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
            merge_object_overrides(object, &context.extra_body);
        }
        crate::replay_id::normalize_openai_responses_replay_ids(&mut body)?;

        self.build_copilot_request(
            "responses",
            body,
            context,
            None,
            CopilotInitiator::for_responses(request),
        )
        .await
    }
    async fn execute_request(
        &self,
        request: reqwest::Request,
    ) -> Result<TracedResponse, ProviderError> {
        let response = execute_http_request(
            &self.client,
            request,
            "github_copilot",
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
        Ok(response)
    }

    async fn execute_json_request(
        &self,
        request: reqwest::Request,
    ) -> Result<Value, ProviderError> {
        let response = self.execute_request(request).await?;
        let text = response.text().await.map_err(map_reqwest_error)?;
        serde_json::from_str(&text).map_err(|error| ProviderError::Transport(error.to_string()))
    }

    async fn execute_stream_request(
        &self,
        request: reqwest::Request,
    ) -> Result<TracedResponse, ProviderError> {
        self.execute_request(request).await
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
        ProviderCapabilities::none()
    }

    async fn chat_completions(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let chat_api = Self::resolve_chat_api(context)?;
        let request = self
            .build_chat_request(request, context, chat_api, false)
            .await?;
        let value = self.execute_json_request(request).await?;
        if chat_api == GitHubCopilotChatApi::AnthropicMessages {
            Ok(normalize_anthropic_messages_response(
                &value,
                context,
                "github_copilot",
            ))
        } else {
            Ok(value)
        }
    }

    async fn chat_completions_stream(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let chat_api = Self::resolve_chat_api(context)?;
        let request = self
            .build_chat_request(request, context, chat_api, true)
            .await?;
        let response = self.execute_stream_request(request).await?;

        if chat_api == GitHubCopilotChatApi::AnthropicMessages {
            Ok(normalize_anthropic_messages_stream(
                response.bytes_stream(),
                context.clone(),
                "github_copilot",
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
        let response = self.execute_json_request(request).await?;
        Ok(normalize_embeddings_response(
            response,
            &context.upstream_model,
        ))
    }

    async fn responses(
        &self,
        request: &CoreResponsesRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let request = self
            .build_responses_request(request, context, false)
            .await?;
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
