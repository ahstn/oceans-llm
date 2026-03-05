use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};

use async_stream::stream;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{
    ChatCompletionsRequest, EmbeddingsRequest, ProviderCapabilities, ProviderClient, ProviderError,
    ProviderRequestContext, ProviderStream,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    http::map_reqwest_error,
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
        if let Some(idempotency_key) = &context.idempotency_key {
            request = request.header("Idempotency-Key", idempotency_key);
        }

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
            return Err(ProviderError::NotImplemented(format!(
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
        request: &ChatCompletionsRequest,
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
        request: &ChatCompletionsRequest,
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
        _request: &EmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::NotImplemented(
            "vertex embeddings are not implemented in this slice".to_string(),
        ))
    }
}

fn map_google_request(
    request: &ChatCompletionsRequest,
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
    Ok(Value::Object(body))
}

fn map_anthropic_request(
    request: &ChatCompletionsRequest,
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
    Ok(Value::Object(body))
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
    let text = value
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| {
                    let object = block.as_object()?;
                    if object.get("type").and_then(Value::as_str) == Some("text") {
                        object
                            .get("text")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    let finish_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(map_anthropic_finish_reason)
        .unwrap_or("stop");

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
            "message": {"role":"assistant","content": text},
            "finish_reason": finish_reason
        })]),
    );

    if let Some(usage) = map_anthropic_usage(value) {
        completion.insert("usage".to_string(), usage);
    }

    Value::Object(completion)
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
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("upstream_google_stream_error", &error.to_string()));
                    break;
                }
            };

            let objects = match parser.push_bytes(&chunk) {
                Ok(objects) => objects,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("google_stream_parse_error", &error.to_string()));
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

        if !finish_emitted {
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
        yield Ok(Bytes::from("data: [DONE]\n\n"));
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
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("upstream_anthropic_stream_error", &error.to_string()));
                    break;
                }
            };

            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk("anthropic_sse_parse_error", &error.to_string()));
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
                    "message_start" => {
                        if !role_emitted {
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
                    }
                    "content_block_delta" => {
                        let delta = payload.get("delta").and_then(Value::as_object);
                        let delta_type = delta
                            .and_then(|delta| delta.get("type"))
                            .and_then(Value::as_str);
                        if delta_type == Some("text_delta") {
                            if let Some(text) = delta
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
                            }
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
                    "message_stop" => {
                        if !finish_emitted {
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
                    }
                    "error" => {
                        let message = payload
                            .get("error")
                            .and_then(Value::as_object)
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("anthropic stream error");
                        yield Ok(openai_sse_error_chunk("anthropic_stream_error", message));
                    }
                    _ => {}
                }
            }
        }

        if !finish_emitted {
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
        yield Ok(Bytes::from("data: [DONE]\n\n"));
    })
}

#[derive(Debug, Clone, Default)]
struct JsonObjectParser {
    buffer: String,
}

impl JsonObjectParser {
    fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<Value>, ProviderError> {
        let text = std::str::from_utf8(chunk).map_err(|error| {
            ProviderError::Transport(format!("stream chunk was not utf8: {error}"))
        })?;
        self.buffer.push_str(text);

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
                b'}' => {
                    if depth > 0 {
                        depth -= 1;
                        if depth == 0 {
                            if let Some(start) = object_start.take() {
                                let end = index + 1;
                                let object_json = &self.buffer[start..end];
                                let value: Value =
                                    serde_json::from_str(object_json).map_err(|error| {
                                        ProviderError::Transport(format!(
                                            "failed parsing streamed google JSON object: {error}"
                                        ))
                                    })?;
                                parsed.push(value);
                                consumed_until = end;
                            }
                        }
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

#[derive(Debug, Clone)]
struct ParsedSseEvent {
    event: Option<String>,
    data: String,
}

#[derive(Debug, Clone, Default)]
struct SseEventParser {
    buffer: String,
}

impl SseEventParser {
    fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<ParsedSseEvent>, ProviderError> {
        let text = std::str::from_utf8(chunk).map_err(|error| {
            ProviderError::Transport(format!("stream chunk was not utf8: {error}"))
        })?;
        self.buffer.push_str(text);

        let mut events = Vec::new();
        while let Some((delimiter_index, delimiter_len)) = find_sse_delimiter(&self.buffer) {
            let block = self.buffer[..delimiter_index]
                .replace("\r\n", "\n")
                .replace('\r', "\n");
            self.buffer.drain(..delimiter_index + delimiter_len);

            let mut event_type = None;
            let mut data_lines = Vec::new();
            for line in block.lines() {
                if let Some(rest) = line.strip_prefix("event:") {
                    event_type = Some(rest.trim().to_string());
                } else if let Some(rest) = line.strip_prefix("data:") {
                    data_lines.push(rest.trim_start().to_string());
                }
            }

            events.push(ParsedSseEvent {
                event: event_type,
                data: data_lines.join("\n"),
            });
        }

        Ok(events)
    }
}

fn find_sse_delimiter(input: &str) -> Option<(usize, usize)> {
    [
        input.find("\r\n\r\n").map(|index| (index, 4)),
        input.find("\n\n").map(|index| (index, 2)),
        input.find("\r\r").map(|index| (index, 2)),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|(index, _)| *index)
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

fn openai_sse_chunk(value: &Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}

fn openai_sse_error_chunk(kind: &str, message: &str) -> Bytes {
    Bytes::from(format!(
        "data: {}\n\n",
        json!({
            "error": {
                "message": message,
                "type": "upstream_error",
                "code": kind
            }
        })
    ))
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
    use gateway_core::{ChatCompletionsRequest, ProviderClient, ProviderRequestContext};
    use serde_json::{Map, Value, json};
    use tokio::{net::TcpListener, sync::Mutex};

    use super::{
        JsonObjectParser, SseEventParser, VertexAuthConfig, VertexProvider, VertexProviderConfig,
        map_anthropic_request, map_google_request, normalize_anthropic_response,
        normalize_google_response, parse_upstream_model,
    };

    fn context(upstream_model: &str) -> ProviderRequestContext {
        ProviderRequestContext {
            request_id: "req-1".to_string(),
            model_key: "fast".to_string(),
            provider_key: "vertex-prod".to_string(),
            upstream_model: upstream_model.to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            idempotency_key: None,
            request_headers: std::collections::BTreeMap::new(),
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
        let request = gateway_core::ChatCompletionsRequest {
            model: "fast".to_string(),
            messages: vec![gateway_core::protocol::openai::ChatMessage {
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
    fn maps_openai_request_to_anthropic_payload_with_default_version() {
        let request = gateway_core::ChatCompletionsRequest {
            model: "fast".to_string(),
            messages: vec![
                gateway_core::protocol::openai::ChatMessage {
                    role: "system".to_string(),
                    content: Value::String("be concise".to_string()),
                    name: None,
                    extra: std::collections::BTreeMap::new(),
                },
                gateway_core::protocol::openai::ChatMessage {
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

    fn chat_request(
        messages: Vec<gateway_core::protocol::openai::ChatMessage>,
    ) -> ChatCompletionsRequest {
        ChatCompletionsRequest {
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

        let mut request = chat_request(vec![gateway_core::protocol::openai::ChatMessage {
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
                        assert!(path.ends_with(":streamRawPredict"));
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
        let mut request = chat_request(vec![gateway_core::protocol::openai::ChatMessage {
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
        assert_eq!(request_payload["stream"], Value::Bool(true));
    }
}
