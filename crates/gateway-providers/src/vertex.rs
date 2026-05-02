use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use async_stream::stream;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{
    CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, ProviderCapabilities,
    ProviderClient, ProviderError, ProviderRequestContext, ProviderStream, SseEventParser,
    Utf8ChunkDecoder,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    http::map_reqwest_error,
    streaming::{done_sse_chunk, openai_sse_error_chunk},
    token::{
        AccessTokenSource, AdcTokenSource, CLOUD_PLATFORM_SCOPE, CachedAccessTokenSource,
        ServiceAccountTokenSource, StaticBearerTokenSource,
    },
};

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
        let base = if self.config.api_host.starts_with("http://")
            || self.config.api_host.starts_with("https://")
        {
            self.config.api_host.trim_end_matches('/').to_string()
        } else {
            format!("https://{}", self.config.api_host)
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

#[async_trait]
impl ProviderClient for VertexProvider {
    fn provider_key(&self) -> &str {
        &self.config.provider_key
    }

    fn provider_type(&self) -> &str {
        "gcp_vertex"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::chat_only_streaming()
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
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::InvalidRequest(
            "vertex embeddings are not supported in this v1 runtime".to_string(),
        ))
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

fn map_google_request(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    stream: bool,
) -> Result<Value, ProviderError> {
    let mut body = Map::new();
    let mut contents = Vec::new();
    let mut system_lines = Vec::new();

    for message in &request.messages {
        match message.role.as_str() {
            "system" | "developer" => {
                system_lines.push(message_content_as_text(&message.content)?);
            }
            "user" | "assistant" => {
                let role = if message.role == "assistant" {
                    "model"
                } else {
                    "user"
                };
                let parts = map_google_parts(&message.content)?;
                contents.push(json!({
                    "role": role,
                    "parts": parts
                }));
            }
            other => {
                return Err(ProviderError::InvalidRequest(format!(
                    "unsupported message role `{other}` for google vertex mapping"
                )));
            }
        }
    }

    if contents.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "google vertex request requires at least one user/assistant message".to_string(),
        ));
    }
    body.insert("contents".to_string(), Value::Array(contents));

    if !system_lines.is_empty() {
        body.insert(
            "systemInstruction".to_string(),
            json!({
                "parts": [{"text": system_lines.join("\n\n")}]
            }),
        );
    }

    let mut passthrough = request.extra.clone();
    passthrough.remove("model");
    passthrough.remove("messages");
    passthrough.remove("stream");

    let generation_config = extract_google_generation_config(&mut passthrough);
    if !generation_config.is_empty() {
        body.insert(
            "generationConfig".to_string(),
            Value::Object(generation_config),
        );
    }

    for (key, value) in passthrough {
        body.insert(key, value);
    }

    if stream {
        body.remove("stream");
    }

    merge_object_overrides(&mut body, &context.extra_body);
    validate_google_stream_candidate_count(&body, stream)?;
    Ok(Value::Object(body))
}

fn map_anthropic_request(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
    stream: bool,
) -> Result<Value, ProviderError> {
    let mut body: Map<String, Value> = request
        .extra
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    body.remove("model");
    body.remove("messages");
    body.remove("stream");

    let mut messages = Vec::new();
    let mut instructions = Vec::new();
    for message in &request.messages {
        match message.role.as_str() {
            "system" | "developer" => {
                instructions.push(message_content_as_text(&message.content)?);
            }
            "user" | "assistant" => {
                let content = map_anthropic_content(&message.content)?;
                messages.push(json!({
                    "role": message.role,
                    "content": content
                }));
            }
            other => {
                return Err(ProviderError::InvalidRequest(format!(
                    "unsupported message role `{other}` for anthropic vertex mapping"
                )));
            }
        }
    }

    if messages.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "anthropic vertex request requires at least one user/assistant message".to_string(),
        ));
    }

    body.insert("messages".to_string(), Value::Array(messages));
    body.insert("stream".to_string(), Value::Bool(stream));
    if !body.contains_key("max_tokens") {
        body.insert("max_tokens".to_string(), Value::Number(1024.into()));
    }
    if !body.contains_key("anthropic_version")
        && !context.extra_body.contains_key("anthropic_version")
    {
        body.insert(
            "anthropic_version".to_string(),
            Value::String("vertex-2023-10-16".to_string()),
        );
    }
    if !instructions.is_empty()
        && !body.contains_key("system")
        && !context.extra_body.contains_key("system")
    {
        body.insert(
            "system".to_string(),
            Value::String(instructions.join("\n\n")),
        );
    }

    merge_object_overrides(&mut body, &context.extra_body);
    apply_vertex_anthropic_thinking_compatibility(&mut body, &context.upstream_model)?;
    validate_vertex_anthropic_sampling_fields(&mut body, &context.upstream_model)?;
    Ok(Value::Object(body))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeThinkingPolicy {
    AdaptiveOnly,
    AdaptivePreferred,
    ManualWithEffort,
    ManualOnly,
    MythosPreview,
}

fn claude_thinking_policy(upstream_model: &str) -> ClaudeThinkingPolicy {
    let model = upstream_model.to_ascii_lowercase();
    if model.contains("claude-mythos-preview") {
        ClaudeThinkingPolicy::MythosPreview
    } else if is_opus_4_7_or_later(&model) {
        ClaudeThinkingPolicy::AdaptiveOnly
    } else if model.contains("claude-opus-4-6") || model.contains("claude-sonnet-4-6") {
        ClaudeThinkingPolicy::AdaptivePreferred
    } else if model.contains("claude-opus-4-5") {
        ClaudeThinkingPolicy::ManualWithEffort
    } else {
        ClaudeThinkingPolicy::ManualOnly
    }
}

fn is_opus_4_7_or_later(model: &str) -> bool {
    let Some(rest) = model.split("claude-opus-4-").nth(1) else {
        return false;
    };
    rest.split(|ch: char| !ch.is_ascii_digit())
        .next()
        .and_then(|minor| minor.parse::<u16>().ok())
        .is_some_and(|minor| minor >= 7)
}

fn apply_vertex_anthropic_thinking_compatibility(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    let reasoning_effort = extract_anthropic_reasoning_effort(body)?;
    let native_effort = extract_existing_anthropic_output_effort(body)?;
    let has_native_effort = native_effort.is_some();
    let effort = merge_optional_efforts(reasoning_effort, native_effort, upstream_model)?;
    let budget_tokens = extract_anthropic_reasoning_budget_tokens(body);
    let policy = claude_thinking_policy(upstream_model);

    validate_caller_thinking_for_policy(body, policy, upstream_model)?;

    if let Some(effort) = effort {
        match policy {
            ClaudeThinkingPolicy::AdaptiveOnly
            | ClaudeThinkingPolicy::AdaptivePreferred
            | ClaudeThinkingPolicy::MythosPreview => {
                ensure_anthropic_adaptive_thinking(body, upstream_model)?;
                merge_anthropic_output_effort(body, effort, upstream_model)?;
            }
            ClaudeThinkingPolicy::ManualWithEffort => {
                let budget_tokens = budget_tokens
                    .or_else(|| existing_manual_thinking_budget(body))
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(format!(
                            "`reasoning_effort` requires an explicit manual thinking budget for `{upstream_model}` because this Claude model does not support adaptive thinking"
                        ))
                    })?;
                ensure_anthropic_manual_thinking(body, budget_tokens, upstream_model)?;
                merge_anthropic_output_effort(body, effort, upstream_model)?;
            }
            ClaudeThinkingPolicy::ManualOnly => {
                if has_native_effort {
                    return Err(ProviderError::InvalidRequest(format!(
                        "`output_config.effort` is not supported for `{upstream_model}`"
                    )));
                }
                let budget_tokens = budget_tokens
                    .or_else(|| existing_manual_thinking_budget(body))
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(format!(
                            "`reasoning_effort` requires an explicit manual thinking budget for `{upstream_model}` because this Claude model does not support adaptive thinking or effort"
                        ))
                    })?;
                ensure_anthropic_manual_thinking(body, budget_tokens, upstream_model)?;
            }
        }
    } else if let Some(budget_tokens) = budget_tokens {
        match policy {
            ClaudeThinkingPolicy::AdaptiveOnly => {
                return Err(ProviderError::InvalidRequest(format!(
                    "`reasoning.budget_tokens` is not supported for `{upstream_model}`; use adaptive thinking with `reasoning_effort` or `output_config.effort`"
                )));
            }
            ClaudeThinkingPolicy::AdaptivePreferred
            | ClaudeThinkingPolicy::ManualWithEffort
            | ClaudeThinkingPolicy::ManualOnly
            | ClaudeThinkingPolicy::MythosPreview => {
                ensure_anthropic_manual_thinking(body, budget_tokens, upstream_model)?;
            }
        }
    }

    Ok(())
}

fn extract_anthropic_reasoning_effort(
    body: &mut Map<String, Value>,
) -> Result<Option<Value>, ProviderError> {
    let reasoning_effort = body
        .remove("reasoning_effort")
        .filter(|value| !value.is_null());
    let reasoning = body.remove("reasoning");

    match (reasoning_effort, reasoning) {
        (Some(effort), None) => Ok(Some(effort)),
        (None, Some(Value::Object(mut reasoning))) => {
            if let Some(budget_tokens) = reasoning.remove("budget_tokens") {
                body.insert("reasoning_budget_tokens".to_string(), budget_tokens);
            }
            Ok(reasoning.remove("effort").filter(|value| !value.is_null()))
        }
        (Some(effort), Some(Value::Object(mut reasoning))) => {
            if let Some(reasoning_effort) =
                reasoning.remove("effort").filter(|value| !value.is_null())
                && reasoning_effort != effort
            {
                return Err(ProviderError::InvalidRequest(
                    "`reasoning_effort` conflicts with `reasoning.effort` for Anthropic Vertex mapping"
                        .to_string(),
                ));
            }
            if let Some(budget_tokens) = reasoning.remove("budget_tokens") {
                body.insert("reasoning_budget_tokens".to_string(), budget_tokens);
            }
            Ok(Some(effort))
        }
        (None, Some(Value::Null)) => Ok(None),
        (Some(effort), Some(Value::Null)) => Ok(Some(effort)),
        (_, Some(_)) => Err(ProviderError::InvalidRequest(
            "`reasoning` must be an object for Anthropic Vertex mapping".to_string(),
        )),
        (None, None) => Ok(None),
    }
}

fn extract_existing_anthropic_output_effort(
    body: &mut Map<String, Value>,
) -> Result<Option<Value>, ProviderError> {
    let (effort, remove_output_config) = {
        let Some(output_config) = body.get_mut("output_config") else {
            return Ok(None);
        };
        let output_config = output_config.as_object_mut().ok_or_else(|| {
            ProviderError::InvalidRequest(
                "`output_config` must be an object for Anthropic Vertex mapping".to_string(),
            )
        })?;

        let effort = output_config.get("effort").cloned();
        if effort.as_ref().is_some_and(Value::is_null) {
            output_config.remove("effort");
            (None, output_config.is_empty())
        } else {
            (effort, false)
        }
    };
    if remove_output_config {
        body.remove("output_config");
    }

    Ok(effort)
}

fn merge_optional_efforts(
    reasoning_effort: Option<Value>,
    native_effort: Option<Value>,
    upstream_model: &str,
) -> Result<Option<Value>, ProviderError> {
    match (reasoning_effort, native_effort) {
        (Some(reasoning_effort), Some(native_effort)) if reasoning_effort != native_effort => {
            Err(ProviderError::InvalidRequest(format!(
                "`reasoning_effort` conflicts with `output_config.effort` for `{upstream_model}`"
            )))
        }
        (Some(reasoning_effort), _) => Ok(Some(reasoning_effort)),
        (None, Some(native_effort)) => Ok(Some(native_effort)),
        (None, None) => Ok(None),
    }
}

fn extract_anthropic_reasoning_budget_tokens(body: &mut Map<String, Value>) -> Option<Value> {
    if let Some(value) = body.remove("thinking_budget_tokens") {
        return Some(value);
    }
    if let Some(value) = body.remove("reasoning_budget_tokens") {
        return Some(value);
    }
    None
}

fn validate_caller_thinking_for_policy(
    body: &Map<String, Value>,
    policy: ClaudeThinkingPolicy,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    let Some(thinking) = body.get("thinking") else {
        return Ok(());
    };
    let thinking = thinking.as_object().ok_or_else(|| {
        ProviderError::InvalidRequest(
            "`thinking` must be an object for Anthropic Vertex mapping".to_string(),
        )
    })?;
    let thinking_type = thinking.get("type").and_then(Value::as_str);

    match policy {
        ClaudeThinkingPolicy::AdaptiveOnly => {
            if thinking_type == Some("enabled") {
                return Err(ProviderError::InvalidRequest(format!(
                    "`thinking.type: enabled` with manual `budget_tokens` is not supported for `{upstream_model}`; use `thinking.type: adaptive` and `output_config.effort`"
                )));
            }
        }
        ClaudeThinkingPolicy::ManualOnly | ClaudeThinkingPolicy::ManualWithEffort => {
            if thinking_type == Some("adaptive") {
                return Err(ProviderError::InvalidRequest(format!(
                    "`thinking.type: adaptive` is not supported for `{upstream_model}`; use `thinking.type: enabled` with `budget_tokens`"
                )));
            }
        }
        ClaudeThinkingPolicy::MythosPreview => {
            if thinking_type == Some("disabled") {
                return Err(ProviderError::InvalidRequest(
                    "`thinking.type: disabled` is not supported for Claude Mythos Preview"
                        .to_string(),
                ));
            }
        }
        ClaudeThinkingPolicy::AdaptivePreferred => {}
    }

    if thinking_type == Some("enabled")
        && thinking
            .get("budget_tokens")
            .is_none_or(|value| value.is_null())
    {
        return Err(ProviderError::InvalidRequest(format!(
            "`thinking.type: enabled` for `{upstream_model}` must include `budget_tokens`"
        )));
    }

    Ok(())
}

fn ensure_anthropic_adaptive_thinking(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    match body.get("thinking") {
        None => {
            body.insert("thinking".to_string(), json!({ "type": "adaptive" }));
            Ok(())
        }
        Some(Value::Object(object))
            if object.get("type").and_then(Value::as_str) == Some("adaptive") =>
        {
            Ok(())
        }
        Some(_) => Err(ProviderError::InvalidRequest(format!(
            "`reasoning_effort` requires `thinking.type: adaptive` for `{upstream_model}` and conflicts with caller-supplied `thinking`"
        ))),
    }
}

fn ensure_anthropic_manual_thinking(
    body: &mut Map<String, Value>,
    budget_tokens: Value,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    match body.get("thinking") {
        None => {
            body.insert(
                "thinking".to_string(),
                json!({ "type": "enabled", "budget_tokens": budget_tokens }),
            );
            Ok(())
        }
        Some(Value::Object(object))
            if object.get("type").and_then(Value::as_str) == Some("enabled") =>
        {
            match object.get("budget_tokens") {
                Some(existing) if existing == &budget_tokens => Ok(()),
                Some(_) => Err(ProviderError::InvalidRequest(format!(
                    "manual Anthropic thinking budget for `{upstream_model}` conflicts with caller-supplied `thinking.budget_tokens`"
                ))),
                None => Err(ProviderError::InvalidRequest(format!(
                    "`thinking.type: enabled` for `{upstream_model}` must include `budget_tokens`"
                ))),
            }
        }
        Some(_) => Err(ProviderError::InvalidRequest(format!(
            "manual Anthropic thinking budget for `{upstream_model}` conflicts with caller-supplied `thinking`"
        ))),
    }
}

fn existing_manual_thinking_budget(body: &Map<String, Value>) -> Option<Value> {
    let thinking = body.get("thinking")?.as_object()?;
    if thinking.get("type").and_then(Value::as_str) == Some("enabled") {
        thinking.get("budget_tokens").cloned()
    } else {
        None
    }
}

fn merge_anthropic_output_effort(
    body: &mut Map<String, Value>,
    effort: Value,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    match body.get_mut("output_config") {
        None => {
            body.insert("output_config".to_string(), json!({ "effort": effort }));
            Ok(())
        }
        Some(Value::Object(output_config)) => match output_config.get("effort") {
            Some(existing) if existing != &effort => Err(ProviderError::InvalidRequest(format!(
                "`reasoning_effort` conflicts with `output_config.effort` for `{upstream_model}`"
            ))),
            Some(_) => Ok(()),
            None => {
                output_config.insert("effort".to_string(), effort);
                Ok(())
            }
        },
        Some(_) => Err(ProviderError::InvalidRequest(
            "`output_config` must be an object for Anthropic Vertex mapping".to_string(),
        )),
    }
}

fn validate_vertex_anthropic_sampling_fields(
    body: &mut Map<String, Value>,
    upstream_model: &str,
) -> Result<(), ProviderError> {
    if claude_thinking_policy(upstream_model) != ClaudeThinkingPolicy::AdaptiveOnly {
        return Ok(());
    }

    for field in ["temperature", "top_p", "top_k"] {
        let Some(value) = body.get(field) else {
            continue;
        };
        if value.is_null() || is_default_anthropic_sampling_value(field, value) {
            body.remove(field);
            continue;
        }
        return Err(ProviderError::InvalidRequest(format!(
            "`{field}` is not supported with non-default values for `{upstream_model}`; omit the field for Claude Opus 4.7+"
        )));
    }

    Ok(())
}

fn is_default_anthropic_sampling_value(field: &str, value: &Value) -> bool {
    match field {
        "temperature" | "top_p" => value
            .as_f64()
            .is_some_and(|number| (number - 1.0).abs() < f64::EPSILON),
        "top_k" => false,
        _ => false,
    }
}

fn message_content_as_text(content: &Value) -> Result<String, ProviderError> {
    match content {
        Value::String(value) => Ok(value.clone()),
        Value::Array(items) => {
            let mut lines = Vec::new();
            for item in items {
                let object = item.as_object().ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "content array entries must be objects".to_string(),
                    )
                })?;
                let kind = object.get("type").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "content array entries must include `type`".to_string(),
                    )
                })?;
                if kind != "text" && kind != "input_text" {
                    return Err(ProviderError::InvalidRequest(format!(
                        "unsupported content type `{kind}` for instruction text"
                    )));
                }
                let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "text content entries must include a string `text`".to_string(),
                    )
                })?;
                lines.push(text.to_string());
            }
            Ok(lines.join("\n"))
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

fn map_google_parts(content: &Value) -> Result<Vec<Value>, ProviderError> {
    match content {
        Value::String(text) => Ok(vec![json!({ "text": text })]),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                let object = item.as_object().ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "content array entries must be objects".to_string(),
                    )
                })?;
                let kind = object.get("type").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "content array entries must include `type`".to_string(),
                    )
                })?;

                match kind {
                    "text" | "input_text" => {
                        let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "text content entries must include a string `text`".to_string(),
                            )
                        })?;
                        parts.push(json!({ "text": text }));
                    }
                    "image_url" | "input_image" => {
                        let image_url_object = object
                            .get("image_url")
                            .and_then(Value::as_object)
                            .ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "image_url content entries must include an `image_url` object"
                                    .to_string(),
                            )
                        })?;
                        let uri = image_url_object
                            .get("url")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                ProviderError::InvalidRequest(
                                    "image_url.url must be a string".to_string(),
                                )
                            })?;
                        if !uri.starts_with("gs://") {
                            return Err(ProviderError::InvalidRequest(
                                "only gs:// image/file URIs are supported for google vertex in this slice"
                                    .to_string(),
                            ));
                        }
                        let mime_type = image_url_object
                            .get("mime_type")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .or_else(|| guess_mime_type(uri))
                            .ok_or_else(|| {
                                ProviderError::InvalidRequest(
                                    "could not infer MIME type for gs:// URI; set image_url.mime_type"
                                        .to_string(),
                                )
                            })?;
                        parts.push(json!({
                            "fileData": {
                                "fileUri": uri,
                                "mimeType": mime_type
                            }
                        }));
                    }
                    "file" => {
                        let file =
                            object
                                .get("file")
                                .and_then(Value::as_object)
                                .ok_or_else(|| {
                                    ProviderError::InvalidRequest(
                                        "file content entries must include a `file` object"
                                            .to_string(),
                                    )
                                })?;
                        let uri = file.get("url").and_then(Value::as_str).ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "file.url must be provided for file content".to_string(),
                            )
                        })?;
                        if !uri.starts_with("gs://") {
                            return Err(ProviderError::InvalidRequest(
                                "only gs:// file URIs are supported for google vertex in this slice"
                                    .to_string(),
                            ));
                        }
                        let mime_type = file
                            .get("mime_type")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .or_else(|| guess_mime_type(uri))
                            .ok_or_else(|| {
                                ProviderError::InvalidRequest(
                                    "could not infer MIME type for file URI; set file.mime_type"
                                        .to_string(),
                                )
                            })?;
                        parts.push(json!({
                            "fileData": {
                                "fileUri": uri,
                                "mimeType": mime_type
                            }
                        }));
                    }
                    other => {
                        return Err(ProviderError::InvalidRequest(format!(
                            "unsupported content type `{other}` for google vertex mapping"
                        )));
                    }
                }
            }
            Ok(parts)
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

fn map_anthropic_content(content: &Value) -> Result<Value, ProviderError> {
    match content {
        Value::String(value) => Ok(Value::String(value.clone())),
        Value::Array(items) => {
            let mut mapped = Vec::new();
            for item in items {
                let object = item.as_object().ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "content array entries must be objects".to_string(),
                    )
                })?;
                let kind = object.get("type").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "content array entries must include `type`".to_string(),
                    )
                })?;
                match kind {
                    "text" | "input_text" => {
                        let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "text content entries must include a string `text`".to_string(),
                            )
                        })?;
                        mapped.push(json!({"type":"text","text":text}));
                    }
                    other => {
                        return Err(ProviderError::InvalidRequest(format!(
                            "unsupported content type `{other}` for anthropic vertex mapping"
                        )));
                    }
                }
            }
            Ok(Value::Array(mapped))
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

fn extract_google_generation_config(extra: &mut BTreeMap<String, Value>) -> Map<String, Value> {
    let mut generation_config = Map::new();

    if let Some(value) = extra.remove("temperature") {
        generation_config.insert("temperature".to_string(), value);
    }
    if let Some(value) = extra.remove("top_p") {
        generation_config.insert("topP".to_string(), value);
    }
    if let Some(value) = extra.remove("top_k") {
        generation_config.insert("topK".to_string(), value);
    }
    if let Some(value) = extra.remove("max_tokens") {
        generation_config.insert("maxOutputTokens".to_string(), value);
    }
    if let Some(value) = extra.remove("presence_penalty") {
        generation_config.insert("presencePenalty".to_string(), value);
    }
    if let Some(value) = extra.remove("frequency_penalty") {
        generation_config.insert("frequencyPenalty".to_string(), value);
    }
    if let Some(value) = extra.remove("seed") {
        generation_config.insert("seed".to_string(), value);
    }
    if let Some(value) = extra.remove("n") {
        generation_config.insert("candidateCount".to_string(), value);
    }
    if let Some(value) = extra.remove("stop") {
        let normalized = match value {
            Value::String(sequence) => Value::Array(vec![Value::String(sequence)]),
            Value::Array(values) => Value::Array(values),
            other => other,
        };
        generation_config.insert("stopSequences".to_string(), normalized);
    }

    generation_config
}

fn validate_google_stream_candidate_count(
    body: &Map<String, Value>,
    stream: bool,
) -> Result<(), ProviderError> {
    if !stream {
        return Ok(());
    }

    let candidate_count = body
        .get("generationConfig")
        .and_then(Value::as_object)
        .and_then(|config| config.get("candidateCount"))
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().and_then(|count| u64::try_from(count).ok()))
        });

    if candidate_count.is_some_and(|count| count > 1) {
        return Err(ProviderError::InvalidRequest(
            "google vertex streaming supports only a single candidate in this slice; remove `n`/`candidateCount` or use non-streaming".to_string(),
        ));
    }

    Ok(())
}

fn normalize_google_response(value: &Value, context: &ProviderRequestContext) -> Value {
    let id = value
        .get("responseId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("chatcmpl-{}", Uuid::new_v4().simple()));
    let created = OffsetDateTime::now_utc().unix_timestamp();

    let mut choices = Vec::new();
    for (index, candidate) in value
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let text = extract_google_candidate_text(candidate);
        let finish_reason = candidate
            .get("finishReason")
            .and_then(Value::as_str)
            .map(map_google_finish_reason)
            .unwrap_or("stop");

        choices.push(json!({
            "index": candidate.get("index").and_then(Value::as_i64).unwrap_or(index as i64),
            "message": {
                "role": "assistant",
                "content": text
            },
            "finish_reason": finish_reason
        }));
    }

    if choices.is_empty() {
        choices.push(json!({
            "index": 0,
            "message": {"role":"assistant","content":""},
            "finish_reason": "stop"
        }));
    }

    let mut completion = Map::new();
    completion.insert("id".to_string(), Value::String(id));
    completion.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    completion.insert("created".to_string(), Value::Number(created.into()));
    completion.insert(
        "model".to_string(),
        Value::String(context.model_key.clone()),
    );
    completion.insert("choices".to_string(), Value::Array(choices));

    if let Some(usage) = map_google_usage(value) {
        completion.insert("usage".to_string(), usage);
    }

    Value::Object(completion)
}

fn normalize_anthropic_response(value: &Value, context: &ProviderRequestContext) -> Value {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("chatcmpl-{}", Uuid::new_v4().simple()));
    let created = OffsetDateTime::now_utc().unix_timestamp();
    let blocks = value
        .get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let text = blocks
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(Value::as_str) == Some("text") {
                block.get("text").and_then(Value::as_str)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("");
    let thinking_blocks = extract_anthropic_thinking_blocks(blocks);
    let finish_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(map_anthropic_finish_reason)
        .unwrap_or("stop");

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert("content".to_string(), Value::String(text));
    if !thinking_blocks.is_empty() {
        message.insert(
            "provider_metadata".to_string(),
            vertex_reasoning_metadata("anthropic_messages", thinking_blocks),
        );
    }

    let mut completion = Map::new();
    completion.insert("id".to_string(), Value::String(id));
    completion.insert(
        "object".to_string(),
        Value::String("chat.completion".to_string()),
    );
    completion.insert("created".to_string(), Value::Number(created.into()));
    completion.insert(
        "model".to_string(),
        Value::String(context.model_key.clone()),
    );
    completion.insert(
        "choices".to_string(),
        Value::Array(vec![json!({
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        })]),
    );

    if let Some(usage) = map_anthropic_usage(value) {
        completion.insert("usage".to_string(), usage);
    }

    Value::Object(completion)
}

fn extract_anthropic_thinking_blocks(blocks: &[Value]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("thinking") => {
                let mut normalized = Map::new();
                normalized.insert("type".to_string(), Value::String("thinking".to_string()));
                normalized.insert(
                    "thinking".to_string(),
                    block
                        .get("thinking")
                        .cloned()
                        .unwrap_or_else(|| Value::String(String::new())),
                );
                if let Some(signature) = block.get("signature").cloned() {
                    normalized.insert("signature".to_string(), signature);
                }
                Some(Value::Object(normalized))
            }
            Some("redacted_thinking") => {
                let mut normalized = Map::new();
                normalized.insert(
                    "type".to_string(),
                    Value::String("redacted_thinking".to_string()),
                );
                if let Some(data) = block.get("data").cloned() {
                    normalized.insert("data".to_string(), data);
                }
                Some(Value::Object(normalized))
            }
            _ => None,
        })
        .collect()
}

fn normalize_anthropic_thinking_delta(delta: &Map<String, Value>) -> Option<Value> {
    match delta.get("type").and_then(Value::as_str) {
        Some("thinking_delta") => {
            let mut normalized = Map::new();
            normalized.insert(
                "type".to_string(),
                Value::String("thinking_delta".to_string()),
            );
            normalized.insert(
                "thinking".to_string(),
                delta
                    .get("thinking")
                    .cloned()
                    .unwrap_or_else(|| Value::String(String::new())),
            );
            Some(Value::Object(normalized))
        }
        Some("signature_delta") => {
            let mut normalized = Map::new();
            normalized.insert(
                "type".to_string(),
                Value::String("signature_delta".to_string()),
            );
            if let Some(signature) = delta.get("signature").cloned() {
                normalized.insert("signature".to_string(), signature);
            }
            Some(Value::Object(normalized))
        }
        _ => None,
    }
}

fn vertex_reasoning_metadata(source: &str, blocks: Vec<Value>) -> Value {
    json!({
        "gcp_vertex": {
            "reasoning": {
                "source": source,
                "blocks": blocks
            }
        }
    })
}

fn map_google_usage(value: &Value) -> Option<Value> {
    let usage = value.get("usageMetadata")?.as_object()?;
    Some(json!({
        "prompt_tokens": usage.get("promptTokenCount").and_then(Value::as_i64).unwrap_or(0),
        "completion_tokens": usage.get("candidatesTokenCount").and_then(Value::as_i64).unwrap_or(0),
        "total_tokens": usage.get("totalTokenCount").and_then(Value::as_i64).unwrap_or(0)
    }))
}

fn map_anthropic_usage(value: &Value) -> Option<Value> {
    let usage = value.get("usage")?.as_object()?;
    let prompt = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion = usage
        .get("output_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Some(json!({
        "prompt_tokens": prompt,
        "completion_tokens": completion,
        "total_tokens": prompt + completion
    }))
}

fn extract_google_candidate_text(candidate: &Value) -> String {
    candidate
        .get("content")
        .and_then(Value::as_object)
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn map_google_finish_reason(reason: &str) -> &'static str {
    match reason {
        "STOP" => "stop",
        "MAX_TOKENS" => "length",
        "SAFETY" | "BLOCKLIST" | "PROHIBITED_CONTENT" => "content_filter",
        _ => "stop",
    }
}

fn map_anthropic_finish_reason(reason: &str) -> &'static str {
    match reason {
        "end_turn" => "stop",
        "max_tokens" => "length",
        "stop_sequence" => "stop",
        "tool_use" => "tool_calls",
        "refusal" => "content_filter",
        _ => "stop",
    }
}

fn merge_object_overrides(base: &mut Map<String, Value>, overrides: &Map<String, Value>) {
    for (key, value) in overrides {
        match (base.get_mut(key), value) {
            (Some(base_value), Value::Object(override_object)) => {
                if let Some(base_object) = base_value.as_object_mut() {
                    merge_object_overrides(base_object, override_object);
                } else {
                    *base_value = Value::Object(override_object.clone());
                }
            }
            (Some(base_value), override_value) => {
                *base_value = override_value.clone();
            }
            (None, override_value) => {
                base.insert(key.clone(), override_value.clone());
            }
        }
    }
}

fn guess_mime_type(uri: &str) -> Option<String> {
    let lowercase = uri.to_ascii_lowercase();
    if lowercase.ends_with(".png") {
        Some("image/png".to_string())
    } else if lowercase.ends_with(".jpg") || lowercase.ends_with(".jpeg") {
        Some("image/jpeg".to_string())
    } else if lowercase.ends_with(".webp") {
        Some("image/webp".to_string())
    } else if lowercase.ends_with(".gif") {
        Some("image/gif".to_string())
    } else if lowercase.ends_with(".pdf") {
        Some("application/pdf".to_string())
    } else if lowercase.ends_with(".mp3") {
        Some("audio/mpeg".to_string())
    } else if lowercase.ends_with(".wav") {
        Some("audio/wav".to_string())
    } else if lowercase.ends_with(".mp4") {
        Some("video/mp4".to_string())
    } else {
        None
    }
}

fn normalize_google_stream<S>(
    upstream: S,
    stream_id: String,
    created: i64,
    model: String,
) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(stream! {
        let mut parser = JsonObjectParser::default();
        let mut role_emitted = false;
        let mut finish_emitted = false;
        let mut stream_failed = false;
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("upstream_google_stream_error", &error.to_string()));
                    stream_failed = true;
                    break;
                }
            };

            let objects = match parser.push_bytes(&chunk) {
                Ok(objects) => objects,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("google_stream_parse_error", &error.to_string()));
                    stream_failed = true;
                    break;
                }
            };

            for object in objects {
                let text = object
                    .get("candidates")
                    .and_then(Value::as_array)
                    .and_then(|candidates| candidates.first())
                    .map(extract_google_candidate_text)
                    .unwrap_or_default();
                let finish_reason = object
                    .get("candidates")
                    .and_then(Value::as_array)
                    .and_then(|candidates| candidates.first())
                    .and_then(|candidate| candidate.get("finishReason"))
                    .and_then(Value::as_str)
                    .map(map_google_finish_reason);

                if !text.is_empty() {
                    let delta = openai_chunk(
                        &stream_id,
                        created,
                        &model,
                        Some("assistant").filter(|_| !role_emitted),
                        Some(&text),
                        None,
                    );
                    yield Ok(openai_sse_chunk(&delta));
                    role_emitted = true;
                }

                if let Some(finish_reason) = finish_reason {
                    let finish = openai_chunk(
                        &stream_id,
                        created,
                        &model,
                        None,
                        None,
                        Some(finish_reason),
                    );
                    yield Ok(openai_sse_chunk(&finish));
                    finish_emitted = true;
                }
            }
        }

        if !stream_failed && !finish_emitted {
            let finish = openai_chunk(
                &stream_id,
                created,
                &model,
                None,
                None,
                Some("stop"),
            );
            yield Ok(openai_sse_chunk(&finish));
        }
        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

fn normalize_anthropic_stream<S>(
    upstream: S,
    stream_id: String,
    created: i64,
    model: String,
) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(stream! {
        let mut parser = SseEventParser::default();
        let mut role_emitted = false;
        let mut finish_emitted = false;
        let mut stream_failed = false;
        futures_util::pin_mut!(upstream);

        'stream_loop: while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("upstream_anthropic_stream_error", &error.to_string()));
                    stream_failed = true;
                    break;
                }
            };

            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("anthropic_sse_parse_error", &error.to_string()));
                    stream_failed = true;
                    break;
                }
            };

            for event in events {
                if event.data.trim().is_empty() || event.data.trim() == "[DONE]" {
                    continue;
                }

                let payload: Value = match serde_json::from_str(&event.data) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let kind = payload
                    .get("type")
                    .and_then(Value::as_str)
                    .or(event.event.as_deref())
                    .unwrap_or_default();

                match kind {
                    "message_start" if !role_emitted => {
                        let delta = openai_chunk(
                            &stream_id,
                            created,
                            &model,
                            Some("assistant"),
                            None,
                            None,
                        );
                        yield Ok(openai_sse_chunk(&delta));
                        role_emitted = true;
                    }
                    "content_block_delta" => {
                        let delta = payload.get("delta").and_then(Value::as_object);
                        let delta_type = delta
                            .and_then(|delta| delta.get("type"))
                            .and_then(Value::as_str);
                        if delta_type == Some("text_delta")
                            && let Some(text) = delta
                                .and_then(|delta| delta.get("text"))
                                .and_then(Value::as_str)
                                .filter(|text| !text.is_empty())
                        {
                            let chunk = openai_chunk(
                                &stream_id,
                                created,
                                &model,
                                Some("assistant").filter(|_| !role_emitted),
                                Some(text),
                                None,
                            );
                            yield Ok(openai_sse_chunk(&chunk));
                            role_emitted = true;
                        } else if let Some(delta) = delta
                            && let Some(block) = normalize_anthropic_thinking_delta(delta)
                        {
                            let mut outbound_delta = Map::new();
                            if !role_emitted {
                                outbound_delta.insert(
                                    "role".to_string(),
                                    Value::String("assistant".to_string()),
                                );
                            }
                            outbound_delta.insert(
                                "provider_metadata".to_string(),
                                vertex_reasoning_metadata(
                                    "anthropic_messages_stream",
                                    vec![block],
                                ),
                            );
                            let chunk = openai_delta_chunk(
                                &stream_id,
                                created,
                                &model,
                                Value::Object(outbound_delta),
                                None,
                            );
                            yield Ok(openai_sse_chunk(&chunk));
                            role_emitted = true;
                        }
                    }
                    "message_delta" => {
                        if let Some(reason) = payload
                            .get("delta")
                            .and_then(Value::as_object)
                            .and_then(|delta| delta.get("stop_reason"))
                            .and_then(Value::as_str)
                        {
                            let finish = openai_chunk(
                                &stream_id,
                                created,
                                &model,
                                None,
                                None,
                                Some(map_anthropic_finish_reason(reason)),
                            );
                            yield Ok(openai_sse_chunk(&finish));
                            finish_emitted = true;
                        }
                    }
                    "message_stop" if !finish_emitted => {
                        let finish = openai_chunk(
                            &stream_id,
                            created,
                            &model,
                            None,
                            None,
                            Some("stop"),
                        );
                        yield Ok(openai_sse_chunk(&finish));
                        finish_emitted = true;
                    }
                    "error" => {
                        let message = payload
                            .get("error")
                            .and_then(Value::as_object)
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("anthropic stream error");
                        yield Ok(openai_sse_error_chunk("anthropic_stream_error", message));
                        stream_failed = true;
                        break 'stream_loop;
                    }
                    _ => {}
                }
            }
        }

        if !stream_failed && !finish_emitted {
            let finish = openai_chunk(
                &stream_id,
                created,
                &model,
                None,
                None,
                Some("stop"),
            );
            yield Ok(openai_sse_chunk(&finish));
        }
        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
}

#[derive(Debug, Clone, Default)]
struct JsonObjectParser {
    utf8: Utf8ChunkDecoder,
    buffer: String,
}

impl JsonObjectParser {
    fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<Value>, ProviderError> {
        let text = self.utf8.push_bytes(chunk)?;
        self.buffer.push_str(&text);

        let mut parsed = Vec::new();
        let mut consumed_until = 0usize;
        let mut object_start = None;
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        let bytes = self.buffer.as_bytes();

        let mut index = 0usize;
        while index < bytes.len() {
            let byte = bytes[index];
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                index += 1;
                continue;
            }

            match byte {
                b'"' => in_string = true,
                b'{' => {
                    if depth == 0 {
                        object_start = Some(index);
                    }
                    depth += 1;
                }
                b'}' if depth > 0 => {
                    depth -= 1;
                    if depth == 0
                        && let Some(start) = object_start.take()
                    {
                        let end = index + 1;
                        let object_json = &self.buffer[start..end];
                        let value: Value = serde_json::from_str(object_json).map_err(|error| {
                            ProviderError::Transport(format!(
                                "failed parsing streamed google JSON object: {error}"
                            ))
                        })?;
                        parsed.push(value);
                        consumed_until = end;
                    }
                }
                _ => {}
            }

            index += 1;
        }

        if consumed_until > 0 {
            self.buffer.drain(..consumed_until);
        }

        Ok(parsed)
    }
}

fn openai_chunk(
    id: &str,
    created: i64,
    model: &str,
    role: Option<&str>,
    content: Option<&str>,
    finish_reason: Option<&str>,
) -> Value {
    let mut delta = Map::new();
    if let Some(role) = role {
        delta.insert("role".to_string(), Value::String(role.to_string()));
    }
    if let Some(content) = content {
        delta.insert("content".to_string(), Value::String(content.to_string()));
    }

    let mut choice = Map::new();
    choice.insert("index".to_string(), Value::Number(0.into()));
    choice.insert("delta".to_string(), Value::Object(delta));
    choice.insert(
        "finish_reason".to_string(),
        finish_reason
            .map(|reason| Value::String(reason.to_string()))
            .unwrap_or(Value::Null),
    );

    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [Value::Object(choice)]
    })
}

fn openai_delta_chunk(
    id: &str,
    created: i64,
    model: &str,
    delta: Value,
    finish_reason: Option<&str>,
) -> Value {
    json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": delta,
            "finish_reason": finish_reason
        }]
    })
}

fn openai_sse_chunk(value: &Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, convert::Infallible, sync::Arc};

    use axum::{
        Json, Router,
        body::Body,
        extract::{Path, State},
        http::{HeaderMap, StatusCode},
        response::Response,
        routing::post,
    };
    use bytes::Bytes;
    use futures_util::StreamExt;
    use futures_util::stream;
    use gateway_core::{CoreChatMessage, CoreChatRequest, ProviderClient, ProviderRequestContext};
    use serde_json::{Map, Value, json};
    use tokio::{net::TcpListener, sync::Mutex};

    use super::{
        JsonObjectParser, SseEventParser, VertexAuthConfig, VertexProvider, VertexProviderConfig,
        map_anthropic_request, map_google_request, normalize_anthropic_response,
        normalize_google_response, parse_upstream_model,
    };
    use gateway_core::ProviderError;

    fn context(upstream_model: &str) -> ProviderRequestContext {
        ProviderRequestContext {
            request_id: "req-1".to_string(),
            model_key: "fast".to_string(),
            provider_key: "vertex-prod".to_string(),
            upstream_model: upstream_model.to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: std::collections::BTreeMap::new(),
            compatibility: Default::default(),
        }
    }

    #[test]
    fn parses_upstream_model_family() {
        let (family, _, _) = parse_upstream_model("google/gemini-2.0-flash").expect("parse");
        assert!(matches!(family, super::PublisherFamily::Google));
        parse_upstream_model("bad-format").expect_err("invalid format");
        parse_upstream_model("meta/llama").expect_err("unsupported family");
    }

    #[test]
    fn model_endpoint_defaults_to_https_but_honors_explicit_scheme() {
        let default_host = vertex_provider_for_test("aiplatform.googleapis.com".to_string());
        let default_url =
            default_host.model_endpoint("google", "gemini-2.0-flash", "generateContent");
        assert!(default_url.starts_with("https://aiplatform.googleapis.com/"));

        let explicit_host = vertex_provider_for_test("http://127.0.0.1:8080/".to_string());
        let explicit_url =
            explicit_host.model_endpoint("google", "gemini-2.0-flash", "generateContent");
        assert!(explicit_url.starts_with("http://127.0.0.1:8080/"));
    }

    #[test]
    fn maps_openai_request_to_google_payload() {
        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![CoreChatMessage {
                role: "user".to_string(),
                content: json!([
                    {"type":"text","text":"Describe this"},
                    {"type":"image_url","image_url":{"url":"gs://bucket/pic.png","mime_type":"image/png"}}
                ]),
                name: None,
                extra: std::collections::BTreeMap::new(),
            }],
            stream: false,
            extra: std::collections::BTreeMap::new(),
        };
        let mapped = map_google_request(&request, &context("google/gemini-2.0-flash"), false)
            .expect("mapped");
        assert_eq!(mapped["contents"][0]["role"], "user");
        assert_eq!(mapped["contents"][0]["parts"][0]["text"], "Describe this");
        assert_eq!(
            mapped["contents"][0]["parts"][1]["fileData"]["fileUri"],
            "gs://bucket/pic.png"
        );
    }

    #[test]
    fn rejects_google_streaming_multiple_candidates_from_n() {
        let mut request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("ping".to_string()),
                name: None,
                extra: std::collections::BTreeMap::new(),
            }],
            stream: true,
            extra: std::collections::BTreeMap::new(),
        };
        request.extra.insert("n".to_string(), json!(2));

        let error = map_google_request(&request, &context("google/gemini-2.0-flash"), true)
            .expect_err("streaming n>1 should be rejected");

        match error {
            ProviderError::InvalidRequest(message) => {
                assert!(message.contains("single candidate"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn rejects_google_streaming_multiple_candidates_from_route_override() {
        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("ping".to_string()),
                name: None,
                extra: std::collections::BTreeMap::new(),
            }],
            stream: true,
            extra: std::collections::BTreeMap::new(),
        };
        let mut context = context("google/gemini-2.0-flash");
        context.extra_body.insert(
            "generationConfig".to_string(),
            json!({ "candidateCount": 2 }),
        );

        let error =
            map_google_request(&request, &context, true).expect_err("route override should win");

        match error {
            ProviderError::InvalidRequest(message) => {
                assert!(message.contains("single candidate"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn allows_google_non_streaming_multiple_candidates() {
        let mut request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("ping".to_string()),
                name: None,
                extra: std::collections::BTreeMap::new(),
            }],
            stream: false,
            extra: std::collections::BTreeMap::new(),
        };
        request.extra.insert("n".to_string(), json!(2));

        let mapped = map_google_request(&request, &context("google/gemini-2.0-flash"), false)
            .expect("non-stream n>1 remains allowed");

        assert_eq!(mapped["generationConfig"]["candidateCount"], json!(2));
    }

    #[test]
    fn maps_openai_request_to_anthropic_payload_with_default_version() {
        let request = CoreChatRequest {
            model: "fast".to_string(),
            messages: vec![
                CoreChatMessage {
                    role: "system".to_string(),
                    content: Value::String("be concise".to_string()),
                    name: None,
                    extra: std::collections::BTreeMap::new(),
                },
                CoreChatMessage {
                    role: "user".to_string(),
                    content: Value::String("ping".to_string()),
                    name: None,
                    extra: std::collections::BTreeMap::new(),
                },
            ],
            stream: false,
            extra: std::collections::BTreeMap::new(),
        };
        let mapped =
            map_anthropic_request(&request, &context("anthropic/claude-sonnet-4-6"), false)
                .expect("mapped");
        assert_eq!(mapped["anthropic_version"], "vertex-2023-10-16");
        assert_eq!(mapped["messages"][0]["role"], "user");
        assert_eq!(mapped["system"], "be concise");
    }

    #[test]
    fn maps_vertex_opus_4_7_reasoning_effort_to_adaptive_thinking() {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request.extra.insert("model".to_string(), json!("fast"));
        request
            .extra
            .insert("reasoning_effort".to_string(), json!("xhigh"));
        request.extra.insert("temperature".to_string(), json!(1.0));
        request.extra.insert("top_p".to_string(), json!(1.0));

        let mapped = map_anthropic_request(&request, &context("anthropic/claude-opus-4-7"), false)
            .expect("mapped");

        assert_eq!(mapped["anthropic_version"], "vertex-2023-10-16");
        assert_eq!(mapped["thinking"], json!({ "type": "adaptive" }));
        assert_eq!(mapped["output_config"], json!({ "effort": "xhigh" }));
        assert!(mapped.get("reasoning_effort").is_none());
        assert!(mapped.get("model").is_none());
        assert!(mapped.get("temperature").is_none());
        assert!(mapped.get("top_p").is_none());
    }

    #[test]
    fn ignores_null_reasoning_effort_for_vertex_anthropic_mapping() {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("hello".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request
            .extra
            .insert("reasoning_effort".to_string(), Value::Null);
        request
            .extra
            .insert("reasoning".to_string(), json!({ "effort": null }));
        request
            .extra
            .insert("output_config".to_string(), json!({ "effort": null }));

        let mapped = map_anthropic_request(&request, &context("anthropic/claude-opus-4-7"), false)
            .expect("mapped");

        assert!(mapped.get("thinking").is_none());
        assert!(mapped.get("output_config").is_none());
        assert!(mapped.get("reasoning_effort").is_none());
        assert!(mapped.get("reasoning").is_none());
    }

    #[test]
    fn validates_native_output_config_effort_for_vertex_anthropic_mapping() {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request
            .extra
            .insert("output_config".to_string(), json!({ "effort": "xhigh" }));

        let mapped = map_anthropic_request(&request, &context("anthropic/claude-opus-4-7"), false)
            .expect("mapped");

        assert_eq!(mapped["thinking"], json!({ "type": "adaptive" }));
        assert_eq!(mapped["output_config"], json!({ "effort": "xhigh" }));
    }

    #[test]
    fn rejects_native_output_config_effort_for_vertex_manual_only_models() {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request
            .extra
            .insert("output_config".to_string(), json!({ "effort": "medium" }));
        request
            .extra
            .insert("reasoning_budget_tokens".to_string(), json!(1024));

        let error = map_anthropic_request(
            &request,
            &context("anthropic/claude-sonnet-4-5@20250929"),
            false,
        )
        .expect_err("manual-only effort rejected")
        .to_string();

        assert!(error.contains("output_config.effort"));
    }

    #[test]
    fn maps_vertex_opus_and_sonnet_4_6_reasoning_effort_to_adaptive_thinking() {
        for model in ["anthropic/claude-opus-4-6", "anthropic/claude-sonnet-4-6"] {
            let mut request = chat_request(vec![CoreChatMessage {
                role: "user".to_string(),
                content: Value::String("think carefully".to_string()),
                name: None,
                extra: BTreeMap::new(),
            }]);
            request
                .extra
                .insert("reasoning_effort".to_string(), json!("high"));

            let mapped = map_anthropic_request(&request, &context(model), false).expect("mapped");

            assert_eq!(mapped["thinking"], json!({ "type": "adaptive" }));
            assert_eq!(mapped["output_config"], json!({ "effort": "high" }));
            assert!(mapped.get("reasoning_effort").is_none());
        }
    }

    #[test]
    fn maps_vertex_opus_4_5_reasoning_effort_with_manual_budget() {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request.extra.insert(
            "reasoning".to_string(),
            json!({ "effort": "medium", "budget_tokens": 2048 }),
        );

        let mapped = map_anthropic_request(
            &request,
            &context("anthropic/claude-opus-4-5@20251101"),
            false,
        )
        .expect("mapped");

        assert_eq!(
            mapped["thinking"],
            json!({ "type": "enabled", "budget_tokens": 2048 })
        );
        assert_eq!(mapped["output_config"], json!({ "effort": "medium" }));
        assert!(mapped.get("reasoning").is_none());
    }

    #[test]
    fn maps_vertex_older_claude_reasoning_budget_to_manual_thinking() {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request.extra.insert(
            "reasoning".to_string(),
            json!({ "effort": "medium", "budget_tokens": 1024 }),
        );

        let mapped = map_anthropic_request(
            &request,
            &context("anthropic/claude-sonnet-4-5@20250929"),
            false,
        )
        .expect("mapped");

        assert_eq!(
            mapped["thinking"],
            json!({ "type": "enabled", "budget_tokens": 1024 })
        );
        assert!(mapped.get("output_config").is_none());
        assert!(mapped.get("reasoning").is_none());
    }

    #[test]
    fn rejects_vertex_opus_4_7_manual_thinking_budget() {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request.extra.insert(
            "thinking".to_string(),
            json!({ "type": "enabled", "budget_tokens": 4096 }),
        );

        let error = map_anthropic_request(&request, &context("anthropic/claude-opus-4-7"), false)
            .expect_err("manual thinking should be rejected");

        match error {
            ProviderError::InvalidRequest(message) => {
                assert!(message.contains("thinking.type: enabled"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn rejects_vertex_extra_body_that_bypasses_anthropic_validation() {
        let request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        let mut context = context("anthropic/claude-opus-4-7");
        context
            .extra_body
            .insert("temperature".to_string(), json!(0.2));

        let error = map_anthropic_request(&request, &context, false)
            .expect_err("route extra_body should be validated after merge");

        match error {
            ProviderError::InvalidRequest(message) => {
                assert!(message.contains("temperature"));
                assert!(message.contains("Claude Opus 4.7+"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn rejects_vertex_native_manual_thinking_without_budget() {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request
            .extra
            .insert("thinking".to_string(), json!({ "type": "enabled" }));

        let error = map_anthropic_request(
            &request,
            &context("anthropic/claude-sonnet-4-5@20250929"),
            false,
        )
        .expect_err("native manual thinking requires a budget");

        match error {
            ProviderError::InvalidRequest(message) => {
                assert!(message.contains("thinking.type: enabled"));
                assert!(message.contains("budget_tokens"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn rejects_vertex_older_claude_adaptive_thinking() {
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("think carefully".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request
            .extra
            .insert("thinking".to_string(), json!({ "type": "adaptive" }));

        let error = map_anthropic_request(
            &request,
            &context("anthropic/claude-haiku-4-5@20251001"),
            false,
        )
        .expect_err("adaptive thinking should be rejected");

        match error {
            ProviderError::InvalidRequest(message) => {
                assert!(message.contains("thinking.type: adaptive"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn normalizes_google_response_into_openai_shape() {
        let response = json!({
            "responseId": "resp-123",
            "candidates":[
                {"index":0, "content":{"parts":[{"text":"hello"}]}, "finishReason":"STOP"}
            ],
            "usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":3,"totalTokenCount":13}
        });
        let normalized = normalize_google_response(&response, &context("google/gemini-2.0-flash"));
        assert_eq!(normalized["object"], "chat.completion");
        assert_eq!(normalized["choices"][0]["message"]["content"], "hello");
        assert_eq!(normalized["usage"]["total_tokens"], 13);
    }

    #[test]
    fn normalizes_anthropic_response_into_openai_shape() {
        let response = json!({
            "id":"msg_123",
            "content":[{"type":"text","text":"hello"}],
            "stop_reason":"end_turn",
            "usage":{"input_tokens":5,"output_tokens":7}
        });
        let normalized =
            normalize_anthropic_response(&response, &context("anthropic/claude-sonnet-4-6"));
        assert_eq!(normalized["choices"][0]["message"]["content"], "hello");
        assert_eq!(normalized["usage"]["prompt_tokens"], 5);
        assert_eq!(normalized["usage"]["completion_tokens"], 7);
    }

    #[test]
    fn normalizes_anthropic_thinking_metadata_without_leaking_into_content() {
        let response = json!({
            "id":"msg_123",
            "content":[
                {"type":"thinking","thinking":"summarized hidden reasoning","signature":"sig-thinking"},
                {"type":"redacted_thinking","data":"encrypted-redacted"},
                {"type":"text","text":"visible answer"}
            ],
            "stop_reason":"end_turn",
            "usage":{"input_tokens":5,"output_tokens":7}
        });
        let normalized =
            normalize_anthropic_response(&response, &context("anthropic/claude-opus-4-7"));
        let message = &normalized["choices"][0]["message"];

        assert_eq!(message["content"], "visible answer");
        assert_eq!(
            message["provider_metadata"]["gcp_vertex"]["reasoning"]["source"],
            "anthropic_messages"
        );
        assert_eq!(
            message["provider_metadata"]["gcp_vertex"]["reasoning"]["blocks"],
            json!([
                {"type":"thinking","thinking":"summarized hidden reasoning","signature":"sig-thinking"},
                {"type":"redacted_thinking","data":"encrypted-redacted"}
            ])
        );
    }

    #[test]
    fn parses_google_streamed_json_objects() {
        let mut parser = JsonObjectParser::default();
        let part_a = br#"{"candidates":[{"content":{"parts":[{"text":"he"}]}}]}
{"candidates":[{"content":{"parts":[{"text":"ll"#;
        let part_b = br#"o"}]},"finishReason":"STOP"}]}"#;
        let first = parser.push_bytes(part_a).expect("first");
        assert_eq!(first.len(), 1);
        let second = parser.push_bytes(part_b).expect("second");
        assert_eq!(second.len(), 1);
    }

    #[test]
    fn parses_google_streamed_json_objects_with_split_utf8_codepoint() {
        let mut parser = JsonObjectParser::default();
        let payload = format!(
            r#"{{"candidates":[{{"content":{{"parts":[{{"text":"{}"}}]}}}}]}}"#,
            "👋"
        );
        let split = payload.find('👋').expect("emoji position") + 2;
        let first = parser
            .push_bytes(&payload.as_bytes()[..split])
            .expect("first chunk");
        assert!(first.is_empty());
        let second = parser
            .push_bytes(&payload.as_bytes()[split..])
            .expect("second chunk");
        assert_eq!(second.len(), 1);
        assert_eq!(
            super::extract_google_candidate_text(&second[0]["candidates"][0]),
            "👋"
        );
    }

    #[test]
    fn parses_anthropic_sse_events() {
        let mut parser = SseEventParser::default();
        let input = br#"event: message_start
data: {"type":"message_start","message":{"role":"assistant"}}

event: content_block_delta
data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"hello"}}

event: vertex_event
data: {"type":"vertex_event"}

"#;
        let events = parser.push_bytes(input).expect("events");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event.as_deref(), Some("message_start"));
        assert_eq!(events[1].event.as_deref(), Some("content_block_delta"));
        assert_eq!(events[2].event.as_deref(), Some("vertex_event"));
    }

    #[test]
    fn parses_anthropic_sse_events_with_crlf_and_chunked_boundaries() {
        let mut parser = SseEventParser::default();
        let part_a = b"event: message_start\r\ndata: {\"type\":\"message_start\",\"message\":{\"role\":\"assistant\"}}\r\n\r";
        let part_b = b"\nevent: message_stop\r\ndata: {\"type\":\"message_stop\"}\r\n\r\n";

        let first = parser.push_bytes(part_a).expect("events a");
        assert!(first.is_empty());
        let second = parser.push_bytes(part_b).expect("events b");
        assert_eq!(second.len(), 2);
        assert_eq!(second[0].event.as_deref(), Some("message_start"));
        assert_eq!(second[1].event.as_deref(), Some("message_stop"));
    }

    #[test]
    fn parses_anthropic_sse_events_with_split_utf8_codepoint() {
        let mut parser = SseEventParser::default();
        let payload = format!(
            "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"{}\"}}}}\n\n",
            "👋"
        );
        let split = payload.find('👋').expect("emoji position") + 2;
        let first = parser
            .push_bytes(&payload.as_bytes()[..split])
            .expect("first chunk");
        assert!(first.is_empty());
        let second = parser
            .push_bytes(&payload.as_bytes()[split..])
            .expect("second chunk");
        assert_eq!(second.len(), 1);
        let payload: Value = serde_json::from_str(&second[0].data).expect("event payload");
        assert_eq!(payload["delta"]["text"], "👋");
    }

    #[tokio::test]
    async fn google_stream_normalization_emits_done() {
        let upstream = stream::iter(vec![
            Ok(Bytes::from(
                r#"{"candidates":[{"content":{"parts":[{"text":"hello"}]}}]}"#,
            )),
            Ok(Bytes::from(r#"{"candidates":[{"finishReason":"STOP"}]}"#)),
        ]);
        let stream = super::normalize_google_stream(
            upstream,
            "chatcmpl-test".to_string(),
            1,
            "fast".to_string(),
        );
        let bytes: Vec<_> = stream.collect().await;
        let rendered = bytes
            .into_iter()
            .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
            .collect::<String>();
        assert!(rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn google_stream_normalization_stops_after_parse_error() {
        let upstream = stream::iter(vec![Ok(Bytes::from_static(&[0x80]))]);
        let stream = super::normalize_google_stream(
            upstream,
            "chatcmpl-test".to_string(),
            1,
            "fast".to_string(),
        );
        let bytes: Vec<_> = stream.collect().await;
        let rendered = bytes
            .into_iter()
            .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
            .collect::<String>();
        assert!(rendered.contains("google_stream_parse_error"));
        assert!(!rendered.contains(r#""finish_reason":"stop""#));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn anthropic_stream_normalization_stops_after_parse_error() {
        let upstream = stream::iter(vec![Ok(Bytes::from_static(&[0x80]))]);
        let stream = super::normalize_anthropic_stream(
            upstream,
            "chatcmpl-test".to_string(),
            1,
            "fast".to_string(),
        );
        let bytes: Vec<_> = stream.collect().await;
        let rendered = bytes
            .into_iter()
            .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
            .collect::<String>();
        assert!(rendered.contains("anthropic_sse_parse_error"));
        assert!(!rendered.contains(r#""finish_reason":"stop""#));
        assert!(!rendered.contains("data: [DONE]"));
    }

    #[tokio::test]
    async fn anthropic_stream_preserves_thinking_and_signature_metadata() {
        let upstream = stream::iter(vec![Ok(Bytes::from(concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\"}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-stream\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"visible\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n"
        )))]);
        let stream = super::normalize_anthropic_stream(
            upstream,
            "chatcmpl-test".to_string(),
            1,
            "fast".to_string(),
        );
        let bytes: Vec<_> = stream.collect().await;
        let rendered = bytes
            .into_iter()
            .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
            .collect::<String>();

        assert!(rendered.contains("\"content\":\"visible\""));
        assert!(rendered.contains("\"provider_metadata\""));
        assert!(rendered.contains("\"gcp_vertex\""));
        assert!(rendered.contains("\"thinking_delta\""));
        assert!(rendered.contains("\"signature_delta\""));
        assert!(rendered.contains("\"sig-stream\""));
        assert_eq!(rendered.matches("\"role\":\"assistant\"").count(), 1);
        assert!(rendered.contains("data: [DONE]"));
    }

    fn vertex_provider_for_test(api_host: String) -> VertexProvider {
        VertexProvider::new(VertexProviderConfig {
            provider_key: "vertex-prod".to_string(),
            project_id: "proj-123".to_string(),
            location: "global".to_string(),
            api_host,
            auth: VertexAuthConfig::Bearer {
                token: "test-token".to_string(),
            },
            default_headers: BTreeMap::new(),
            request_timeout_ms: 5_000,
        })
        .expect("provider")
    }

    fn chat_request(messages: Vec<CoreChatMessage>) -> CoreChatRequest {
        CoreChatRequest {
            model: "fast".to_string(),
            messages,
            stream: false,
            extra: BTreeMap::new(),
        }
    }

    async fn start_router(app: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        addr.to_string()
    }

    #[tokio::test]
    async fn vertex_provider_google_non_stream_executes_real_http_mapping() {
        let captured = Arc::new(Mutex::new(None::<Value>));
        let state = captured.clone();
        let app = Router::new()
            .route(
                "/v1/{*path}",
                post(
                    |Path(path): Path<String>,
                     State(captured): State<Arc<Mutex<Option<Value>>>>,
                     headers: HeaderMap,
                     Json(payload): Json<Value>| async move {
                        assert!(path.ends_with(":generateContent"));
                        assert_eq!(
                            headers
                                .get("authorization")
                                .and_then(|value| value.to_str().ok()),
                            Some("Bearer test-token")
                        );
                        *captured.lock().await = Some(payload);
                        Json(json!({
                            "responseId": "resp-google-1",
                            "candidates": [{
                                "index": 0,
                                "content": {"parts": [{"text":"pong"}]},
                                "finishReason":"STOP"
                            }]
                        }))
                    },
                ),
            )
            .with_state(state);

        let host = start_router(app).await;
        let provider = vertex_provider_for_test(format!("http://{host}"));

        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("ping".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request.extra.insert("temperature".to_string(), json!(0.2));

        let response = provider
            .chat_completions(&request, &context("google/gemini-2.0-flash"))
            .await
            .expect("chat completion");

        assert_eq!(response["choices"][0]["message"]["content"], "pong");

        let request_payload = captured.lock().await.clone().expect("captured request");
        assert_eq!(request_payload["contents"][0]["parts"][0]["text"], "ping");
        assert_eq!(
            request_payload["generationConfig"]["temperature"],
            json!(0.2)
        );
    }

    #[tokio::test]
    async fn vertex_provider_anthropic_stream_handles_fragmented_crlf_events() {
        let captured = Arc::new(Mutex::new(None::<Value>));
        let state = captured.clone();
        let app = Router::new()
            .route(
                "/v1/{*path}",
                post(
                    |Path(path): Path<String>,
                     State(captured): State<Arc<Mutex<Option<Value>>>>,
                     headers: HeaderMap,
                     Json(payload): Json<Value>| async move {
                        assert!(path.ends_with(
                            "publishers/anthropic/models/claude-sonnet-4-6:streamRawPredict"
                        ));
                        assert_eq!(
                            headers
                                .get("authorization")
                                .and_then(|value| value.to_str().ok()),
                            Some("Bearer test-token")
                        );
                        *captured.lock().await = Some(payload);

                        let chunks = vec![
                            "event: message_start\r\n",
                            "data: {\"type\":\"message_start\"}\r\n",
                            "\r\nevent: content_block_delta\r\n",
                            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hel\"}}\r\n\r\n",
                            "event: content_block_delta\r\n",
                            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\r\n\r\n",
                            "event: message_delta\r\n",
                            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\r\n\r\n",
                            "event: message_stop\r\n",
                            "data: {\"type\":\"message_stop\"}\r\n\r\n",
                        ];

                        let body = Body::from_stream(stream::iter(chunks.into_iter().map(
                            |chunk| Ok::<_, Infallible>(Bytes::from(chunk)),
                        )));
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "text/event-stream")
                            .body(body)
                            .expect("stream response")
                    },
                ),
            )
            .with_state(state);

        let host = start_router(app).await;
        let provider = vertex_provider_for_test(format!("http://{host}"));
        let mut request = chat_request(vec![CoreChatMessage {
            role: "user".to_string(),
            content: Value::String("ping".to_string()),
            name: None,
            extra: BTreeMap::new(),
        }]);
        request.stream = true;

        let stream = provider
            .chat_completions_stream(&request, &context("anthropic/claude-sonnet-4-6"))
            .await
            .expect("stream");

        let bytes: Vec<_> = stream.collect().await;
        let rendered = bytes
            .into_iter()
            .map(|item| String::from_utf8(item.expect("chunk").to_vec()).expect("utf8"))
            .collect::<String>();

        assert!(rendered.contains("\"content\":\"hel\""));
        assert!(rendered.contains("\"content\":\"lo\""));
        assert_eq!(rendered.matches("\"role\":\"assistant\"").count(), 1);
        assert!(rendered.contains("\"finish_reason\":\"stop\""));
        assert!(rendered.contains("data: [DONE]"));

        let request_payload = captured.lock().await.clone().expect("captured request");
        assert_eq!(
            request_payload["anthropic_version"],
            Value::String("vertex-2023-10-16".to_string())
        );
        assert!(request_payload.get("model").is_none());
        assert_eq!(request_payload["stream"], Value::Bool(true));
    }
}
