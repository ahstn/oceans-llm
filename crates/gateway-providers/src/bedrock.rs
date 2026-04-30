use std::{collections::BTreeMap, time::Duration};

use async_stream::stream;
use async_trait::async_trait;
use bytes::{Buf, Bytes};
use futures_util::StreamExt;
use gateway_core::{
    CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, ProviderCapabilities,
    ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;

use crate::http::map_reqwest_error;
use crate::streaming::{done_sse_chunk, openai_sse_error_chunk, render_sse_event_chunk};

#[derive(Debug, Clone)]
pub enum BedrockAuthConfig {
    DefaultChain,
    Bearer {
        token: String,
    },
    StaticCredentials {
        access_key_id: String,
        secret_access_key: String,
        session_token: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct BedrockProviderConfig {
    pub provider_key: String,
    pub region: String,
    pub endpoint_url: String,
    pub auth: BedrockAuthConfig,
    pub default_headers: BTreeMap<String, String>,
    pub request_timeout_ms: u64,
}

impl BedrockProviderConfig {
    #[must_use]
    pub fn default_endpoint_url(region: &str) -> String {
        format!("https://bedrock-runtime.{region}.amazonaws.com")
    }

    pub fn resolved_endpoint_url(
        region: &str,
        endpoint_url: Option<&str>,
    ) -> Result<String, url::ParseError> {
        let url = match endpoint_url {
            Some(endpoint_url) => Url::parse(endpoint_url)?,
            None => Url::parse(&Self::default_endpoint_url(region))?,
        };

        Ok(url.to_string().trim_end_matches('/').to_string())
    }
}

#[derive(Clone)]
pub struct BedrockProvider {
    config: BedrockProviderConfig,
    client: reqwest::Client,
}

impl BedrockProvider {
    pub fn new(config: BedrockProviderConfig) -> Result<Self, ProviderError> {
        let _ = Url::parse(&config.endpoint_url).map_err(|error| {
            ProviderError::InvalidRequest(format!(
                "aws_bedrock provider `{}` endpoint_url is invalid: {error}",
                config.provider_key
            ))
        })?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .build()
            .map_err(map_reqwest_error)?;

        Ok(Self { config, client })
    }

    fn unsupported(method: &str) -> ProviderError {
        ProviderError::NotImplemented(format!(
            "aws_bedrock {method} execution is not implemented yet"
        ))
    }

    fn converse_endpoint(&self, upstream_model: &str) -> String {
        self.bedrock_model_endpoint(upstream_model, "converse")
    }

    fn converse_stream_endpoint(&self, upstream_model: &str) -> String {
        self.bedrock_model_endpoint(upstream_model, "converse-stream")
    }

    fn bedrock_model_endpoint(&self, upstream_model: &str, operation: &str) -> String {
        let encoded_model_id: String =
            url::form_urlencoded::byte_serialize(upstream_model.as_bytes()).collect();
        format!(
            "{}/model/{encoded_model_id}/{operation}",
            self.config.endpoint_url
        )
    }

    fn invoke_endpoint(&self, upstream_model: &str) -> String {
        let encoded_model_id: String =
            url::form_urlencoded::byte_serialize(upstream_model.as_bytes()).collect();
        format!(
            "{}/model/{encoded_model_id}/invoke",
            self.config.endpoint_url
        )
    }

    fn build_chat_request(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let (body, url) = if is_anthropic_claude_model(&context.upstream_model) {
            (
                map_chat_request_to_anthropic_messages(request, context)?,
                self.invoke_endpoint(&context.upstream_model),
            )
        } else {
            (
                map_chat_request_to_converse(request, context)?,
                self.converse_endpoint(&context.upstream_model),
            )
        };

        let mut request = self.client.post(url).json(&body);
        request = request.header("content-type", "application/json");
        request = request.header("accept", "application/json");
        request = request.header("x-request-id", &context.request_id);

        for (name, value) in &self.config.default_headers {
            request = request.header(name, value);
        }

        for (name, value) in &context.extra_headers {
            if let Some(value) = value.as_str() {
                request = request.header(name, value);
            }
        }

        match &self.config.auth {
            BedrockAuthConfig::Bearer { token } => {
                request = request.bearer_auth(token);
            }
            BedrockAuthConfig::DefaultChain | BedrockAuthConfig::StaticCredentials { .. } => {
                return Err(ProviderError::NotImplemented(
                    "aws_bedrock IAM SigV4 request signing is owned by the provider auth foundation"
                        .to_string(),
                ));
            }
        }

        request.build().map_err(map_reqwest_error)
    }

    fn build_converse_stream_request(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let mut stream_request = request.clone();
        stream_request.stream = true;
        let body = map_chat_request_to_converse(&stream_request, context)?;
        let url = self.converse_stream_endpoint(&context.upstream_model);

        let mut request = self.client.post(url).json(&body);
        request = request.header("content-type", "application/json");
        request = request.header("accept", "application/vnd.amazon.eventstream");
        request = request.header("x-request-id", &context.request_id);

        for (name, value) in &self.config.default_headers {
            request = request.header(name, value);
        }

        for (name, value) in &context.extra_headers {
            if let Some(value) = value.as_str() {
                request = request.header(name, value);
            }
        }

        match &self.config.auth {
            BedrockAuthConfig::Bearer { token } => {
                request = request.bearer_auth(token);
            }
            BedrockAuthConfig::DefaultChain | BedrockAuthConfig::StaticCredentials { .. } => {
                return Err(ProviderError::NotImplemented(
                    "aws_bedrock IAM SigV4 request signing is owned by the provider auth foundation"
                        .to_string(),
                ));
            }
        }

        request.build().map_err(map_reqwest_error)
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
impl ProviderClient for BedrockProvider {
    fn provider_key(&self) -> &str {
        &self.config.provider_key
    }

    fn provider_type(&self) -> &str {
        "aws_bedrock"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::with_dimensions(true, true, false, true, true, false, true)
    }

    async fn chat_completions(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let is_anthropic_claude = is_anthropic_claude_model(&context.upstream_model);
        let request = self.build_chat_request(request, context)?;
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
            ProviderError::Transport(format!("invalid JSON from aws_bedrock chat: {error}"))
        })?;
        if is_anthropic_claude {
            Ok(normalize_anthropic_messages_response(&value, context))
        } else {
            Ok(normalize_converse_response(&value, context))
        }
    }

    async fn chat_completions_stream(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        let request = self.build_converse_stream_request(request, context)?;
        let response = self.execute_stream_request(request).await?;

        Ok(normalize_bedrock_converse_stream(
            response.bytes_stream(),
            context.clone(),
        ))
    }

    async fn embeddings(
        &self,
        _request: &CoreEmbeddingsRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(Self::unsupported("embeddings"))
    }

    async fn responses(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        Err(Self::unsupported("responses"))
    }

    async fn responses_stream(
        &self,
        _request: &CoreResponsesRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(Self::unsupported("responses streaming"))
    }
}

fn map_chat_request_to_converse(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
) -> Result<Value, ProviderError> {
    let mut body = Map::new();
    let mut system = Vec::new();
    let mut messages = Vec::new();

    for message in &request.messages {
        match message.role.as_str() {
            "system" | "developer" => {
                system.push(json!({ "text": message_content_as_text(&message.content)? }));
            }
            "user" => {
                messages.push(json!({
                    "role": "user",
                    "content": map_bedrock_content_blocks(&message.content)?
                }));
            }
            "assistant" => {
                let mut content = map_bedrock_content_blocks(&message.content)?;
                content.extend(map_assistant_tool_uses(message)?);
                messages.push(json!({
                    "role": "assistant",
                    "content": content
                }));
            }
            "tool" => {
                messages.push(json!({
                    "role": "user",
                    "content": [map_tool_result(message)?]
                }));
            }
            other => {
                return Err(ProviderError::InvalidRequest(format!(
                    "unsupported message role `{other}` for aws_bedrock Converse mapping"
                )));
            }
        }
    }

    if messages.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Converse requires at least one user, assistant, or tool message"
                .to_string(),
        ));
    }

    if !system.is_empty() {
        body.insert("system".to_string(), Value::Array(system));
    }
    body.insert("messages".to_string(), Value::Array(messages));

    let mut passthrough = request.extra.clone();
    passthrough.remove("model");
    passthrough.remove("messages");
    passthrough.remove("stream");

    let inference_config = extract_inference_config(&mut passthrough)?;
    if !inference_config.is_empty() {
        body.insert(
            "inferenceConfig".to_string(),
            Value::Object(inference_config),
        );
    }

    if let Some(tool_config) = extract_tool_config(&mut passthrough)? {
        body.insert("toolConfig".to_string(), tool_config);
    }

    if let Some(additional) = passthrough.remove("additionalModelRequestFields") {
        body.insert("additionalModelRequestFields".to_string(), additional);
    }
    if let Some(additional) = passthrough.remove("additional_model_request_fields") {
        body.insert("additionalModelRequestFields".to_string(), additional);
    }

    reject_openai_only_fields(&passthrough)?;
    merge_object_overrides(&mut body, &context.extra_body);
    Ok(Value::Object(body))
}

fn is_anthropic_claude_model(upstream_model: &str) -> bool {
    upstream_model
        .to_ascii_lowercase()
        .contains("anthropic.claude")
}

fn map_chat_request_to_anthropic_messages(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
) -> Result<Value, ProviderError> {
    if request.stream {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Anthropic Claude Messages streaming is gated until native InvokeModelWithResponseStream mapping lands"
                .to_string(),
        ));
    }

    let mut body = Map::new();
    body.insert(
        "anthropic_version".to_string(),
        Value::String("bedrock-2023-05-31".to_string()),
    );

    let mut system = Vec::new();
    let mut messages = Vec::new();

    for message in &request.messages {
        match message.role.as_str() {
            "system" | "developer" => {
                let text = message_content_as_text(&message.content)?;
                if !text.is_empty() {
                    system.push(text);
                }
            }
            "user" => {
                messages.push(json!({
                    "role": "user",
                    "content": map_anthropic_content_blocks(&message.content)?
                }));
            }
            "assistant" => {
                let mut content = map_anthropic_content_blocks(&message.content)?;
                content.extend(map_anthropic_assistant_tool_uses(message)?);
                messages.push(json!({
                    "role": "assistant",
                    "content": content
                }));
            }
            "tool" => {
                messages.push(json!({
                    "role": "user",
                    "content": [map_anthropic_tool_result(message)?]
                }));
            }
            other => {
                return Err(ProviderError::InvalidRequest(format!(
                    "unsupported message role `{other}` for aws_bedrock Anthropic Claude Messages mapping"
                )));
            }
        }
    }

    if messages.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Anthropic Claude Messages requires at least one user, assistant, or tool message"
                .to_string(),
        ));
    }

    if !system.is_empty() {
        body.insert("system".to_string(), Value::String(system.join("\n")));
    }
    body.insert("messages".to_string(), Value::Array(messages));

    let mut passthrough = request.extra.clone();
    passthrough.remove("model");
    passthrough.remove("messages");
    passthrough.remove("stream");

    extract_anthropic_inference_fields(&mut body, &mut passthrough)?;
    if let Some(tools) = extract_anthropic_tools(&mut passthrough)? {
        body.extend(tools);
    }
    extract_anthropic_passthrough_fields(&mut body, &mut passthrough);
    reject_openai_only_fields(&passthrough)?;

    merge_object_overrides(&mut body, &context.extra_body);
    if !body.contains_key("max_tokens") {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Anthropic Claude Messages requires `max_tokens` or `max_completion_tokens`"
                .to_string(),
        ));
    }

    Ok(Value::Object(body))
}

fn message_content_as_text(content: &Value) -> Result<String, ProviderError> {
    match content {
        Value::Null => Ok(String::new()),
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
                match kind {
                    "text" | "input_text" => {
                        let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                            ProviderError::InvalidRequest(
                                "text content entries must include a string `text`".to_string(),
                            )
                        })?;
                        lines.push(text.to_string());
                    }
                    other => {
                        return Err(ProviderError::InvalidRequest(format!(
                            "unsupported content type `{other}` for aws_bedrock instruction text"
                        )));
                    }
                }
            }
            Ok(lines.join("\n"))
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

fn map_bedrock_content_blocks(content: &Value) -> Result<Vec<Value>, ProviderError> {
    match content {
        Value::Null => Ok(Vec::new()),
        Value::String(text) => Ok(vec![json!({ "text": text })]),
        Value::Array(items) => {
            let mut blocks = Vec::new();
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
                        blocks.push(json!({ "text": text }));
                    }
                    "tool_result" => {
                        blocks.push(map_tool_result_content_block(object)?);
                    }
                    other => {
                        return Err(ProviderError::InvalidRequest(format!(
                            "unsupported content type `{other}` for aws_bedrock Converse mapping"
                        )));
                    }
                }
            }
            Ok(blocks)
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

fn map_anthropic_content_blocks(content: &Value) -> Result<Vec<Value>, ProviderError> {
    match content {
        Value::Null => Ok(Vec::new()),
        Value::String(text) => Ok(vec![json!({ "type": "text", "text": text })]),
        Value::Array(items) => {
            let mut blocks = Vec::new();
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
                        blocks.push(json!({ "type": "text", "text": text }));
                    }
                    "image" | "image_url" | "input_image" => {
                        blocks.push(map_anthropic_image_block(object)?);
                    }
                    "tool_result" => {
                        blocks.push(map_anthropic_tool_result_content_block(object)?);
                    }
                    other => {
                        return Err(ProviderError::InvalidRequest(format!(
                            "unsupported content type `{other}` for aws_bedrock Anthropic Claude Messages mapping"
                        )));
                    }
                }
            }
            Ok(blocks)
        }
        _ => Err(ProviderError::InvalidRequest(
            "message content must be a string or typed content array".to_string(),
        )),
    }
}

fn map_anthropic_image_block(object: &Map<String, Value>) -> Result<Value, ProviderError> {
    let image_url = object
        .get("image_url")
        .or_else(|| object.get("source"))
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "image content entries must include `image_url` or `source`".to_string(),
            )
        })?;

    match image_url {
        Value::Object(image_object) => {
            if image_object.get("type").and_then(Value::as_str) == Some("base64") {
                return Ok(json!({ "type": "image", "source": image_object }));
            }
            if let Some(source) = image_object.get("source").and_then(Value::as_object)
                && source.get("type").and_then(Value::as_str) == Some("base64")
            {
                return Ok(json!({ "type": "image", "source": source }));
            }

            let url = image_object
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ProviderError::InvalidRequest("image_url.url must be a string".to_string())
                })?;
            map_anthropic_data_url_image(url, image_object)
        }
        Value::String(url) => map_anthropic_data_url_image(url, object),
        _ => Err(ProviderError::InvalidRequest(
            "image_url must be a string or object for aws_bedrock Anthropic Claude Messages"
                .to_string(),
        )),
    }
}

fn map_anthropic_data_url_image(
    url: &str,
    metadata: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let Some((media_type, data)) = url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,"))
    else {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Anthropic Claude Messages only supports base64 image data URLs; remote image URLs are not supported"
                .to_string(),
        ));
    };
    let media_type = metadata
        .get("mime_type")
        .and_then(Value::as_str)
        .unwrap_or(media_type);

    match media_type {
        "image/jpeg" | "image/png" | "image/webp" | "image/gif" => Ok(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data
            }
        })),
        other => Err(ProviderError::InvalidRequest(format!(
            "unsupported image media type `{other}` for aws_bedrock Anthropic Claude Messages"
        ))),
    }
}

fn map_assistant_tool_uses(
    message: &gateway_core::CoreChatMessage,
) -> Result<Vec<Value>, ProviderError> {
    let Some(tool_calls) = message.extra.get("tool_calls") else {
        return Ok(Vec::new());
    };
    let calls = tool_calls.as_array().ok_or_else(|| {
        ProviderError::InvalidRequest("assistant tool_calls must be an array".to_string())
    })?;

    calls
        .iter()
        .map(|call| {
            let object = call.as_object().ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "assistant tool_calls entries must be objects".to_string(),
                )
            })?;
            if object.get("type").and_then(Value::as_str) != Some("function") {
                return Err(ProviderError::InvalidRequest(
                    "only function tool_calls are supported for aws_bedrock Converse".to_string(),
                ));
            }
            let tool_use_id = object.get("id").and_then(Value::as_str).ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "assistant tool_calls entries must include `id`".to_string(),
                )
            })?;
            let function = object
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "assistant function tool_calls must include `function`".to_string(),
                    )
                })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "assistant function tool_calls must include function.name".to_string(),
                    )
                })?;
            let input = match function.get("arguments") {
                Some(Value::String(arguments)) => {
                    serde_json::from_str(arguments).map_err(|error| {
                        ProviderError::InvalidRequest(format!(
                            "assistant function tool_call arguments must be JSON: {error}"
                        ))
                    })?
                }
                Some(value) => value.clone(),
                None => Value::Object(Map::new()),
            };

            Ok(json!({
                "toolUse": {
                    "toolUseId": tool_use_id,
                    "name": name,
                    "input": input
                }
            }))
        })
        .collect()
}

fn map_anthropic_assistant_tool_uses(
    message: &gateway_core::CoreChatMessage,
) -> Result<Vec<Value>, ProviderError> {
    let Some(tool_calls) = message.extra.get("tool_calls") else {
        return Ok(Vec::new());
    };
    let calls = tool_calls.as_array().ok_or_else(|| {
        ProviderError::InvalidRequest("assistant tool_calls must be an array".to_string())
    })?;

    calls
        .iter()
        .map(|call| {
            let object = call.as_object().ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "assistant tool_calls entries must be objects".to_string(),
                )
            })?;
            if object.get("type").and_then(Value::as_str) != Some("function") {
                return Err(ProviderError::InvalidRequest(
                    "only function tool_calls are supported for aws_bedrock Anthropic Claude Messages"
                        .to_string(),
                ));
            }
            let id = object.get("id").and_then(Value::as_str).ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "assistant tool_calls entries must include `id`".to_string(),
                )
            })?;
            let function = object
                .get("function")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "assistant function tool_calls must include `function`".to_string(),
                    )
                })?;
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "assistant function tool_calls must include function.name".to_string(),
                    )
                })?;
            let input = match function.get("arguments") {
                Some(Value::String(arguments)) => {
                    serde_json::from_str(arguments).map_err(|error| {
                        ProviderError::InvalidRequest(format!(
                            "assistant function tool_call arguments must be JSON: {error}"
                        ))
                    })?
                }
                Some(value) => value.clone(),
                None => Value::Object(Map::new()),
            };

            Ok(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input
            }))
        })
        .collect()
}

fn map_tool_result(message: &gateway_core::CoreChatMessage) -> Result<Value, ProviderError> {
    let tool_call_id = message
        .extra
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest("tool messages must include `tool_call_id`".to_string())
        })?;
    let content = match &message.content {
        Value::String(text) => vec![json!({ "text": text })],
        Value::Array(items) => items
            .iter()
            .map(|item| {
                let object = item.as_object().ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "tool message content array entries must be objects".to_string(),
                    )
                })?;
                let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "tool message content entries must include string `text`".to_string(),
                    )
                })?;
                Ok(json!({ "text": text }))
            })
            .collect::<Result<Vec<_>, ProviderError>>()?,
        _ => {
            return Err(ProviderError::InvalidRequest(
                "tool message content must be a string or text content array".to_string(),
            ));
        }
    };

    Ok(json!({
        "toolResult": {
            "toolUseId": tool_call_id,
            "content": content
        }
    }))
}

fn map_anthropic_tool_result(
    message: &gateway_core::CoreChatMessage,
) -> Result<Value, ProviderError> {
    let tool_use_id = message
        .extra
        .get("tool_call_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest("tool messages must include `tool_call_id`".to_string())
        })?;
    let content = match &message.content {
        Value::String(text) => Value::String(text.clone()),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| {
                    let object = item.as_object().ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "tool message content array entries must be objects".to_string(),
                        )
                    })?;
                    let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "tool message content entries must include string `text`".to_string(),
                        )
                    })?;
                    Ok(json!({ "type": "text", "text": text }))
                })
                .collect::<Result<Vec<_>, ProviderError>>()?,
        ),
        _ => {
            return Err(ProviderError::InvalidRequest(
                "tool message content must be a string or text content array".to_string(),
            ));
        }
    };

    Ok(json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": content
    }))
}

fn map_tool_result_content_block(object: &Map<String, Value>) -> Result<Value, ProviderError> {
    let tool_use_id = object
        .get("tool_use_id")
        .or_else(|| object.get("toolUseId"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "tool_result content must include tool_use_id".to_string(),
            )
        })?;
    let text = object.get("text").and_then(Value::as_str).ok_or_else(|| {
        ProviderError::InvalidRequest("tool_result content must include string `text`".to_string())
    })?;

    Ok(json!({
        "toolResult": {
            "toolUseId": tool_use_id,
            "content": [{ "text": text }]
        }
    }))
}

fn map_anthropic_tool_result_content_block(
    object: &Map<String, Value>,
) -> Result<Value, ProviderError> {
    let tool_use_id = object
        .get("tool_use_id")
        .or_else(|| object.get("toolUseId"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "tool_result content must include tool_use_id".to_string(),
            )
        })?;
    let content = object
        .get("content")
        .cloned()
        .or_else(|| {
            object
                .get("text")
                .and_then(Value::as_str)
                .map(|text| Value::String(text.to_string()))
        })
        .ok_or_else(|| {
            ProviderError::InvalidRequest(
                "tool_result content must include `content` or string `text`".to_string(),
            )
        })?;

    Ok(json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": content
    }))
}

fn extract_inference_config(
    extra: &mut BTreeMap<String, Value>,
) -> Result<Map<String, Value>, ProviderError> {
    let mut config = Map::new();
    if let Some(value) = extra
        .remove("max_completion_tokens")
        .or_else(|| extra.remove("max_tokens"))
    {
        config.insert("maxTokens".to_string(), value);
    }
    if let Some(value) = extra.remove("temperature") {
        config.insert("temperature".to_string(), value);
    }
    if let Some(value) = extra.remove("top_p") {
        config.insert("topP".to_string(), value);
    }
    if let Some(value) = extra.remove("stop") {
        config.insert(
            "stopSequences".to_string(),
            normalize_stop_sequences(value)?,
        );
    }
    Ok(config)
}

fn extract_anthropic_inference_fields(
    body: &mut Map<String, Value>,
    extra: &mut BTreeMap<String, Value>,
) -> Result<(), ProviderError> {
    if let Some(value) = extra
        .remove("max_completion_tokens")
        .or_else(|| extra.remove("max_tokens"))
    {
        body.insert("max_tokens".to_string(), value);
    }
    for field in ["temperature", "top_p", "top_k"] {
        if let Some(value) = extra.remove(field) {
            body.insert(field.to_string(), value);
        }
    }
    if let Some(value) = extra.remove("stop") {
        body.insert(
            "stop_sequences".to_string(),
            normalize_stop_sequences(value)?,
        );
    }
    if let Some(value) = extra.remove("stop_sequences") {
        body.insert(
            "stop_sequences".to_string(),
            normalize_stop_sequences(value)?,
        );
    }
    Ok(())
}

fn normalize_stop_sequences(value: Value) -> Result<Value, ProviderError> {
    match value {
        Value::String(sequence) => Ok(Value::Array(vec![Value::String(sequence)])),
        Value::Array(values) if values.iter().all(Value::is_string) => Ok(Value::Array(values)),
        Value::Null => Ok(Value::Array(Vec::new())),
        _ => Err(ProviderError::InvalidRequest(
            "`stop` must be a string or array of strings for aws_bedrock Converse".to_string(),
        )),
    }
}

fn extract_tool_config(
    extra: &mut BTreeMap<String, Value>,
) -> Result<Option<Value>, ProviderError> {
    let Some(tools) = extra.remove("tools") else {
        if let Some(tool_choice) = extra.remove("tool_choice")
            && !tool_choice_is_none_or_auto(&tool_choice)
        {
            return Err(ProviderError::InvalidRequest(
                "tool_choice requires non-empty tools for aws_bedrock Converse".to_string(),
            ));
        }
        return Ok(None);
    };

    let tools_array = tools.as_array().ok_or_else(|| {
        ProviderError::InvalidRequest("tools must be an array for aws_bedrock Converse".to_string())
    })?;
    if tools_array.is_empty() {
        return Ok(None);
    }

    let mut bedrock_tools = Vec::new();
    for tool in tools_array {
        let object = tool.as_object().ok_or_else(|| {
            ProviderError::InvalidRequest("tool entries must be objects".to_string())
        })?;
        if object.get("type").and_then(Value::as_str) != Some("function") {
            return Err(ProviderError::InvalidRequest(
                "only OpenAI function tools are supported for aws_bedrock Converse".to_string(),
            ));
        }
        let function = object
            .get("function")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ProviderError::InvalidRequest("function tools must include `function`".to_string())
            })?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "function tools must include function.name".to_string(),
                )
            })?;
        let schema = function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
        let mut spec = Map::new();
        spec.insert("name".to_string(), Value::String(name.to_string()));
        if let Some(description) = function
            .get("description")
            .and_then(Value::as_str)
            .filter(|description| !description.trim().is_empty())
        {
            spec.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
        spec.insert("inputSchema".to_string(), json!({ "json": schema }));
        if let Some(strict) = function.get("strict").and_then(Value::as_bool) {
            spec.insert("strict".to_string(), Value::Bool(strict));
        }
        bedrock_tools.push(json!({ "toolSpec": spec }));
    }

    let mut tool_config = Map::new();
    tool_config.insert("tools".to_string(), Value::Array(bedrock_tools));
    if let Some(tool_choice) = extra.remove("tool_choice")
        && let Some(mapped) = map_tool_choice(&tool_choice)?
    {
        tool_config.insert("toolChoice".to_string(), mapped);
    }

    Ok(Some(Value::Object(tool_config)))
}

fn extract_anthropic_tools(
    extra: &mut BTreeMap<String, Value>,
) -> Result<Option<Map<String, Value>>, ProviderError> {
    let tool_choice = extra.remove("tool_choice");
    let Some(tools) = extra.remove("tools") else {
        if let Some(tool_choice) = tool_choice
            && !tool_choice_is_none_or_auto(&tool_choice)
        {
            return Err(ProviderError::InvalidRequest(
                "tool_choice requires non-empty tools for aws_bedrock Anthropic Claude Messages"
                    .to_string(),
            ));
        }
        return Ok(None);
    };

    if tool_choice.as_ref().is_some_and(tool_choice_is_none) {
        return Ok(None);
    }

    let tools_array = tools.as_array().ok_or_else(|| {
        ProviderError::InvalidRequest(
            "tools must be an array for aws_bedrock Anthropic Claude Messages".to_string(),
        )
    })?;
    if tools_array.is_empty() {
        return Ok(None);
    }

    let mut anthropic_tools = Vec::new();
    for tool in tools_array {
        let object = tool.as_object().ok_or_else(|| {
            ProviderError::InvalidRequest("tool entries must be objects".to_string())
        })?;
        if object.get("type").and_then(Value::as_str) != Some("function") {
            return Err(ProviderError::InvalidRequest(
                "only OpenAI function tools are supported for aws_bedrock Anthropic Claude Messages"
                    .to_string(),
            ));
        }
        let function = object
            .get("function")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ProviderError::InvalidRequest("function tools must include `function`".to_string())
            })?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "function tools must include function.name".to_string(),
                )
            })?;
        let schema = function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
        let mut spec = Map::new();
        spec.insert("name".to_string(), Value::String(name.to_string()));
        if let Some(description) = function
            .get("description")
            .and_then(Value::as_str)
            .filter(|description| !description.trim().is_empty())
        {
            spec.insert(
                "description".to_string(),
                Value::String(description.to_string()),
            );
        }
        spec.insert("input_schema".to_string(), schema);
        anthropic_tools.push(Value::Object(spec));
    }

    let mut mapped = Map::new();
    mapped.insert("tools".to_string(), Value::Array(anthropic_tools));
    if let Some(tool_choice) = tool_choice
        && let Some(choice) = map_anthropic_tool_choice(&tool_choice)?
    {
        mapped.insert("tool_choice".to_string(), choice);
    }
    Ok(Some(mapped))
}

fn extract_anthropic_passthrough_fields(
    body: &mut Map<String, Value>,
    extra: &mut BTreeMap<String, Value>,
) {
    for field in [
        "anthropic_beta",
        "thinking",
        "output_config",
        "container",
        "context_management",
        "metadata",
    ] {
        if let Some(value) = extra.remove(field) {
            body.insert(field.to_string(), value);
        }
    }
}

fn tool_choice_is_none_or_auto(value: &Value) -> bool {
    matches!(value.as_str(), Some("none" | "auto"))
        || value
            .as_object()
            .and_then(|object| object.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| matches!(kind, "none" | "auto"))
}

fn tool_choice_is_none(value: &Value) -> bool {
    value.as_str() == Some("none")
        || value
            .as_object()
            .and_then(|object| object.get("type"))
            .and_then(Value::as_str)
            == Some("none")
}

fn map_tool_choice(value: &Value) -> Result<Option<Value>, ProviderError> {
    match value {
        Value::String(choice) => match choice.as_str() {
            "auto" => Ok(Some(json!({ "auto": {} }))),
            "required" => Ok(Some(json!({ "any": {} }))),
            "none" => Ok(None),
            other => Err(ProviderError::InvalidRequest(format!(
                "unsupported tool_choice `{other}` for aws_bedrock Converse"
            ))),
        },
        Value::Object(object) => match object.get("type").and_then(Value::as_str) {
            Some("auto") => Ok(Some(json!({ "auto": {} }))),
            Some("required") => Ok(Some(json!({ "any": {} }))),
            Some("none") => Ok(None),
            Some("function") => {
                let function = object
                    .get("function")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "function tool_choice must include `function`".to_string(),
                        )
                    })?;
                let name = function
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "function tool_choice must include function.name".to_string(),
                        )
                    })?;
                Ok(Some(json!({ "tool": { "name": name } })))
            }
            Some(other) => Err(ProviderError::InvalidRequest(format!(
                "unsupported tool_choice type `{other}` for aws_bedrock Converse"
            ))),
            None => Err(ProviderError::InvalidRequest(
                "tool_choice object must include `type`".to_string(),
            )),
        },
        Value::Null => Ok(None),
        _ => Err(ProviderError::InvalidRequest(
            "tool_choice must be a string or object for aws_bedrock Converse".to_string(),
        )),
    }
}

fn map_anthropic_tool_choice(value: &Value) -> Result<Option<Value>, ProviderError> {
    match value {
        Value::String(choice) => match choice.as_str() {
            "auto" => Ok(Some(json!({ "type": "auto" }))),
            "required" => Ok(Some(json!({ "type": "any" }))),
            "none" => Ok(None),
            other => Err(ProviderError::InvalidRequest(format!(
                "unsupported tool_choice `{other}` for aws_bedrock Anthropic Claude Messages"
            ))),
        },
        Value::Object(object) => match object.get("type").and_then(Value::as_str) {
            Some("auto") => Ok(Some(json!({ "type": "auto" }))),
            Some("required") => Ok(Some(json!({ "type": "any" }))),
            Some("none") => Ok(None),
            Some("function") => {
                let function = object
                    .get("function")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "function tool_choice must include `function`".to_string(),
                        )
                    })?;
                let name = function
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        ProviderError::InvalidRequest(
                            "function tool_choice must include function.name".to_string(),
                        )
                    })?;
                Ok(Some(json!({ "type": "tool", "name": name })))
            }
            Some("tool") => {
                let name = object.get("name").and_then(Value::as_str).ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "tool tool_choice must include `name`".to_string(),
                    )
                })?;
                Ok(Some(json!({ "type": "tool", "name": name })))
            }
            Some(other) => Err(ProviderError::InvalidRequest(format!(
                "unsupported tool_choice type `{other}` for aws_bedrock Anthropic Claude Messages"
            ))),
            None => Err(ProviderError::InvalidRequest(
                "tool_choice object must include `type`".to_string(),
            )),
        },
        Value::Null => Ok(None),
        _ => Err(ProviderError::InvalidRequest(
            "tool_choice must be a string or object for aws_bedrock Anthropic Claude Messages"
                .to_string(),
        )),
    }
}

fn reject_openai_only_fields(extra: &BTreeMap<String, Value>) -> Result<(), ProviderError> {
    const UNSUPPORTED: &[&str] = &[
        "frequency_penalty",
        "presence_penalty",
        "logit_bias",
        "logprobs",
        "top_logprobs",
        "n",
        "response_format",
        "seed",
        "store",
        "metadata",
        "parallel_tool_calls",
        "user",
    ];

    if let Some(field) = UNSUPPORTED.iter().find(|field| extra.contains_key(**field)) {
        return Err(ProviderError::InvalidRequest(format!(
            "`{field}` is not supported for aws_bedrock chat in this slice"
        )));
    }

    Ok(())
}

fn normalize_converse_response(value: &Value, context: &ProviderRequestContext) -> Value {
    let id = value
        .get("responseId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("chatcmpl-{}", Uuid::new_v4().simple()));
    let created = OffsetDateTime::now_utc().unix_timestamp();
    let blocks = value
        .get("output")
        .and_then(|output| output.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let content = blocks
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("");
    let tool_calls = extract_tool_calls(blocks);
    let finish_reason = value
        .get("stopReason")
        .and_then(Value::as_str)
        .map(map_stop_reason)
        .unwrap_or("stop");

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert("content".to_string(), Value::String(content));
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
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
        "provider_model".to_string(),
        Value::String(context.upstream_model.clone()),
    );
    completion.insert(
        "choices".to_string(),
        Value::Array(vec![json!({
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        })]),
    );

    if let Some(usage) = map_usage(value) {
        completion.insert("usage".to_string(), usage);
    }

    Value::Object(completion)
}

fn normalize_anthropic_messages_response(value: &Value, context: &ProviderRequestContext) -> Value {
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
    let content = blocks
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
    let tool_calls = extract_anthropic_tool_calls(blocks);
    let finish_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(map_stop_reason)
        .unwrap_or("stop");

    let mut message = Map::new();
    message.insert("role".to_string(), Value::String("assistant".to_string()));
    message.insert("content".to_string(), Value::String(content));
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
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
        "provider_model".to_string(),
        Value::String(context.upstream_model.clone()),
    );
    completion.insert(
        "choices".to_string(),
        Value::Array(vec![json!({
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        })]),
    );

    if let Some(usage) = map_usage(value) {
        completion.insert("usage".to_string(), usage);
    }

    Value::Object(completion)
}

fn extract_tool_calls(blocks: &[Value]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| {
            let tool_use = block.get("toolUse")?;
            let id = tool_use.get("toolUseId").and_then(Value::as_str)?;
            let name = tool_use.get("name").and_then(Value::as_str)?;
            let arguments = tool_use
                .get("input")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            Some(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments.to_string()
                }
            }))
        })
        .collect()
}

fn extract_anthropic_tool_calls(blocks: &[Value]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                return None;
            }
            let id = block.get("id").and_then(Value::as_str)?;
            let name = block.get("name").and_then(Value::as_str)?;
            let arguments = block
                .get("input")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            Some(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments.to_string()
                }
            }))
        })
        .collect()
}

fn map_stop_reason(reason: &str) -> &'static str {
    match reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" | "model_context_window_exceeded" => "length",
        "tool_use" => "tool_calls",
        "guardrail_intervened" | "content_filtered" | "refusal" => "content_filter",
        "malformed_model_output" | "malformed_tool_use" => "stop",
        _ => "stop",
    }
}

fn map_usage(value: &Value) -> Option<Value> {
    let usage = value.get("usage")?.as_object()?;
    let prompt = usage
        .get("inputTokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion = usage
        .get("outputTokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = usage
        .get("totalTokens")
        .or_else(|| usage.get("total_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(prompt + completion);

    let mut mapped = Map::new();
    mapped.insert("prompt_tokens".to_string(), Value::Number(prompt.into()));
    mapped.insert(
        "completion_tokens".to_string(),
        Value::Number(completion.into()),
    );
    mapped.insert("total_tokens".to_string(), Value::Number(total.into()));
    mapped.insert("provider_usage".to_string(), Value::Object(usage.clone()));
    Some(Value::Object(mapped))
}

#[derive(Debug, Clone)]
struct BedrockEvent {
    message_type: Option<String>,
    event_type: Option<String>,
    exception_type: Option<String>,
    payload: Bytes,
}

#[derive(Debug, Default)]
struct BedrockEventStreamDecoder {
    buffer: Vec<u8>,
}

impl BedrockEventStreamDecoder {
    const PRELUDE_LEN: usize = 12;
    const MESSAGE_CRC_LEN: usize = 4;
    const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

    fn push_bytes(&mut self, chunk: &[u8]) -> Result<Vec<BedrockEvent>, ProviderError> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        loop {
            if self.buffer.len() < Self::PRELUDE_LEN {
                break;
            }

            let total_len = u32::from_be_bytes(self.buffer[0..4].try_into().unwrap()) as usize;
            let headers_len = u32::from_be_bytes(self.buffer[4..8].try_into().unwrap()) as usize;

            if total_len < Self::PRELUDE_LEN + Self::MESSAGE_CRC_LEN {
                return Err(ProviderError::Transport(format!(
                    "invalid aws_bedrock EventStream frame length `{total_len}`"
                )));
            }
            if total_len > Self::MAX_FRAME_LEN {
                return Err(ProviderError::Transport(format!(
                    "aws_bedrock EventStream frame length `{total_len}` exceeds limit"
                )));
            }
            let payload_start = Self::PRELUDE_LEN.checked_add(headers_len).ok_or_else(|| {
                ProviderError::Transport(
                    "aws_bedrock EventStream headers length overflow".to_string(),
                )
            })?;
            let payload_end = total_len
                .checked_sub(Self::MESSAGE_CRC_LEN)
                .ok_or_else(|| {
                    ProviderError::Transport(
                        "aws_bedrock EventStream payload length underflow".to_string(),
                    )
                })?;
            if payload_start > payload_end {
                return Err(ProviderError::Transport(format!(
                    "aws_bedrock EventStream headers length `{headers_len}` exceeds frame payload"
                )));
            }
            if self.buffer.len() < total_len {
                break;
            }

            let frame = self.buffer.drain(..total_len).collect::<Vec<_>>();
            let headers = parse_eventstream_headers(&frame[Self::PRELUDE_LEN..payload_start])?;
            let payload = Bytes::copy_from_slice(&frame[payload_start..payload_end]);
            events.push(BedrockEvent {
                message_type: headers.get(":message-type").cloned(),
                event_type: headers.get(":event-type").cloned(),
                exception_type: headers.get(":exception-type").cloned(),
                payload,
            });
        }

        Ok(events)
    }

    fn finish(&self) -> Result<(), ProviderError> {
        if self.buffer.is_empty() {
            Ok(())
        } else {
            Err(ProviderError::Transport(format!(
                "stream ended with incomplete aws_bedrock EventStream frame ({} bytes buffered)",
                self.buffer.len()
            )))
        }
    }
}

fn parse_eventstream_headers(headers: &[u8]) -> Result<BTreeMap<String, String>, ProviderError> {
    let mut cursor = headers;
    let mut parsed = BTreeMap::new();

    while cursor.has_remaining() {
        if cursor.remaining() < 1 {
            return Err(ProviderError::Transport(
                "malformed aws_bedrock EventStream header name length".to_string(),
            ));
        }
        let name_len = cursor.get_u8() as usize;
        if cursor.remaining() < name_len + 1 {
            return Err(ProviderError::Transport(
                "malformed aws_bedrock EventStream header".to_string(),
            ));
        }
        let name = std::str::from_utf8(&cursor[..name_len]).map_err(|error| {
            ProviderError::Transport(format!(
                "aws_bedrock EventStream header name was not utf8: {error}"
            ))
        })?;
        cursor.advance(name_len);
        let value_type = cursor.get_u8();

        let value = match value_type {
            7 => {
                if cursor.remaining() < 2 {
                    return Err(ProviderError::Transport(
                        "malformed aws_bedrock EventStream string header length".to_string(),
                    ));
                }
                let value_len = cursor.get_u16() as usize;
                if cursor.remaining() < value_len {
                    return Err(ProviderError::Transport(
                        "malformed aws_bedrock EventStream string header value".to_string(),
                    ));
                }
                let value = std::str::from_utf8(&cursor[..value_len]).map_err(|error| {
                    ProviderError::Transport(format!(
                        "aws_bedrock EventStream string header was not utf8: {error}"
                    ))
                })?;
                cursor.advance(value_len);
                value.to_string()
            }
            0 => "true".to_string(),
            1 => "false".to_string(),
            other => {
                return Err(ProviderError::Transport(format!(
                    "unsupported aws_bedrock EventStream header value type `{other}`"
                )));
            }
        };
        parsed.insert(name.to_string(), value);
    }

    Ok(parsed)
}

#[derive(Debug)]
enum BedrockStreamAction {
    Chunk(Value),
    Error { code: String, message: String },
}

#[derive(Debug)]
struct BedrockConverseStreamNormalizer {
    id: String,
    created: i64,
    model: String,
    provider_model: String,
    role_sent: bool,
    saw_payload: bool,
    saw_terminal: bool,
}

impl BedrockConverseStreamNormalizer {
    fn new(context: &ProviderRequestContext) -> Self {
        Self {
            id: format!("chatcmpl-{}", Uuid::new_v4().simple()),
            created: OffsetDateTime::now_utc().unix_timestamp(),
            model: context.model_key.clone(),
            provider_model: context.upstream_model.clone(),
            role_sent: false,
            saw_payload: false,
            saw_terminal: false,
        }
    }

    fn process_event(
        &mut self,
        event: BedrockEvent,
    ) -> Result<Vec<BedrockStreamAction>, ProviderError> {
        if event.message_type.as_deref() == Some("exception") || event.exception_type.is_some() {
            let code = event
                .exception_type
                .or(event.event_type)
                .unwrap_or_else(|| "bedrock_eventstream_exception".to_string());
            let message = bedrock_event_payload_message(&event.payload)
                .unwrap_or_else(|| "aws_bedrock EventStream exception".to_string());
            return Ok(vec![BedrockStreamAction::Error { code, message }]);
        }

        let event_type = event.event_type.as_deref().ok_or_else(|| {
            ProviderError::Transport(
                "aws_bedrock EventStream event is missing :event-type".to_string(),
            )
        })?;

        let payload: Value = serde_json::from_slice(&event.payload).map_err(|error| {
            ProviderError::Transport(format!(
                "invalid JSON payload in aws_bedrock `{event_type}` EventStream event: {error}"
            ))
        })?;

        self.saw_payload = true;
        let mut actions = Vec::new();
        match event_type {
            "messageStart" => {
                if let Some(conversation_id) = payload.get("conversationId").and_then(Value::as_str)
                {
                    self.id = format!("chatcmpl-{conversation_id}");
                }
                if !self.role_sent {
                    let role = payload
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or("assistant");
                    actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                        json!({
                            "role": role
                        }),
                        Value::Null,
                    )));
                    self.role_sent = true;
                }
            }
            "contentBlockStart" => {
                if let Some(tool_use) = payload
                    .get("start")
                    .and_then(|start| start.get("toolUse"))
                    .and_then(Value::as_object)
                {
                    let index = payload
                        .get("contentBlockIndex")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let id = tool_use
                        .get("toolUseId")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let name = tool_use
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                        json!({
                            "tool_calls": [{
                                "index": index,
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": ""
                                }
                            }]
                        }),
                        Value::Null,
                    )));
                }
            }
            "contentBlockDelta" => {
                let delta = payload.get("delta").ok_or_else(|| {
                    ProviderError::Transport(
                        "aws_bedrock contentBlockDelta event is missing `delta`".to_string(),
                    )
                })?;
                if let Some(text) = delta.get("text").and_then(Value::as_str) {
                    actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                        json!({
                            "content": text
                        }),
                        Value::Null,
                    )));
                } else if let Some(tool_use) = delta.get("toolUse").and_then(Value::as_object) {
                    let index = payload
                        .get("contentBlockIndex")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let input = tool_use
                        .get("input")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    actions.push(BedrockStreamAction::Chunk(self.delta_chunk(
                        json!({
                            "tool_calls": [{
                                "index": index,
                                "function": {
                                    "arguments": input
                                }
                            }]
                        }),
                        Value::Null,
                    )));
                }
            }
            "contentBlockStop" => {}
            "messageStop" => {
                let finish_reason = payload
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .map(map_stop_reason)
                    .unwrap_or("stop");
                actions.push(BedrockStreamAction::Chunk(
                    self.delta_chunk(json!({}), Value::String(finish_reason.to_string())),
                ));
                self.saw_terminal = true;
            }
            "metadata" => {
                if let Some(usage) = map_stream_usage(&payload) {
                    actions.push(BedrockStreamAction::Chunk(self.usage_chunk(usage)));
                }
            }
            other => {
                return Err(ProviderError::Transport(format!(
                    "unsupported aws_bedrock ConverseStream event `{other}`"
                )));
            }
        }

        Ok(actions)
    }

    fn delta_chunk(&self, delta: Value, finish_reason: Value) -> Value {
        json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "provider_model": self.provider_model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason
            }]
        })
    }

    fn usage_chunk(&self, usage: Value) -> Value {
        json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "provider_model": self.provider_model,
            "choices": [],
            "usage": usage
        })
    }
}

fn bedrock_event_payload_message(payload: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(payload)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .or_else(|| value.get("Message"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| value.as_str().map(str::to_string))
        })
}

fn map_stream_usage(value: &Value) -> Option<Value> {
    let usage = value.get("usage")?.as_object()?;
    let prompt = usage
        .get("inputTokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion = usage
        .get("outputTokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = usage
        .get("totalTokens")
        .or_else(|| usage.get("total_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(prompt + completion);

    let mut mapped = Map::new();
    mapped.insert("prompt_tokens".to_string(), Value::Number(prompt.into()));
    mapped.insert(
        "completion_tokens".to_string(),
        Value::Number(completion.into()),
    );
    mapped.insert("total_tokens".to_string(), Value::Number(total.into()));
    mapped.insert("provider_usage".to_string(), Value::Object(usage.clone()));
    Some(Value::Object(mapped))
}

fn normalize_bedrock_converse_stream<S>(
    upstream: S,
    context: ProviderRequestContext,
) -> ProviderStream
where
    S: futures_util::stream::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(stream! {
        let mut decoder = BedrockEventStreamDecoder::default();
        let mut normalizer = BedrockConverseStreamNormalizer::new(&context);
        let mut stream_failed = false;
        futures_util::pin_mut!(upstream);

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "upstream_bedrock_eventstream_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            let events = match decoder.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Ok(openai_sse_error_chunk(
                        "bedrock_eventstream_parse_error",
                        &error.to_string(),
                    ));
                    stream_failed = true;
                    break;
                }
            };

            for event in events {
                let actions = match normalizer.process_event(event) {
                    Ok(actions) => actions,
                    Err(error) => {
                        yield Ok(openai_sse_error_chunk(
                            "bedrock_conversestream_normalization_error",
                            &error.to_string(),
                        ));
                        stream_failed = true;
                        break;
                    }
                };

                for action in actions {
                    match action {
                        BedrockStreamAction::Chunk(value) => {
                            yield Ok(render_sse_event_chunk(None, &value.to_string()));
                        }
                        BedrockStreamAction::Error { code, message } => {
                            yield Ok(openai_sse_error_chunk(&code, &message));
                            stream_failed = true;
                            break;
                        }
                    }
                }

                if stream_failed {
                    break;
                }
            }

            if stream_failed {
                break;
            }
        }

        if !stream_failed && let Err(error) = decoder.finish() {
            yield Ok(openai_sse_error_chunk(
                "bedrock_eventstream_finalization_error",
                &error.to_string(),
            ));
            stream_failed = true;
        }

        if !stream_failed && !normalizer.saw_payload {
            yield Ok(openai_sse_error_chunk(
                "bedrock_eventstream_empty_stream",
                "upstream stream ended without Bedrock EventStream payload events",
            ));
            stream_failed = true;
        }

        if !stream_failed && !normalizer.saw_terminal {
            yield Ok(openai_sse_error_chunk(
                "bedrock_eventstream_missing_terminal_event",
                "upstream Bedrock ConverseStream ended without messageStop",
            ));
            stream_failed = true;
        }

        if !stream_failed {
            yield Ok(done_sse_chunk());
        }
    })
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bytes::Bytes;
    use futures_util::StreamExt;
    use gateway_core::{CoreChatMessage, CoreChatRequest, ProviderRequestContext};
    use serde_json::{Map, Value, json};

    use super::{
        BedrockAuthConfig, BedrockEventStreamDecoder, BedrockProvider, BedrockProviderConfig,
        map_chat_request_to_anthropic_messages, map_chat_request_to_converse,
        normalize_anthropic_messages_response, normalize_bedrock_converse_stream,
        normalize_converse_response,
    };

    #[test]
    fn resolves_default_endpoint_from_region() {
        let endpoint =
            BedrockProviderConfig::resolved_endpoint_url("us-east-1", None).expect("endpoint");
        assert_eq!(endpoint, "https://bedrock-runtime.us-east-1.amazonaws.com");
    }

    #[test]
    fn normalizes_custom_endpoint_trailing_slash() {
        let endpoint = BedrockProviderConfig::resolved_endpoint_url(
            "us-east-1",
            Some("https://bedrock-runtime.us-west-2.amazonaws.com/"),
        )
        .expect("endpoint");
        assert_eq!(endpoint, "https://bedrock-runtime.us-west-2.amazonaws.com");
    }

    #[test]
    fn maps_text_chat_request_to_converse_body() {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![
                message("system", "Be terse."),
                message("developer", "Prefer SI units."),
                message("user", "Hello"),
            ],
            stream: false,
            extra: BTreeMap::from([
                ("max_completion_tokens".to_string(), json!(128)),
                ("temperature".to_string(), json!(0.2)),
                ("top_p".to_string(), json!(0.9)),
                ("stop".to_string(), json!(["END"])),
            ]),
        };

        let body = map_chat_request_to_converse(
            &request,
            &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
        )
        .expect("mapped");

        assert_eq!(
            body,
            json!({
                "system": [{"text":"Be terse."},{"text":"Prefer SI units."}],
                "messages": [{
                    "role": "user",
                    "content": [{"text": "Hello"}]
                }],
                "inferenceConfig": {
                    "maxTokens": 128,
                    "temperature": 0.2,
                    "topP": 0.9,
                    "stopSequences": ["END"]
                }
            })
        );
    }

    #[test]
    fn maps_text_chat_request_to_anthropic_messages_invoke_body() {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![
                message("system", "Be terse."),
                message("developer", "Prefer SI units."),
                message("user", "Hello"),
            ],
            stream: false,
            extra: BTreeMap::from([
                ("max_completion_tokens".to_string(), json!(128)),
                ("temperature".to_string(), json!(0.2)),
                ("top_p".to_string(), json!(0.9)),
                ("stop".to_string(), json!(["END"])),
                (
                    "anthropic_beta".to_string(),
                    json!(["token-efficient-tools-2025-02-19"]),
                ),
            ]),
        };

        let body = map_chat_request_to_anthropic_messages(
            &request,
            &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
        )
        .expect("mapped");

        assert_eq!(
            body,
            json!({
                "anthropic_version": "bedrock-2023-05-31",
                "anthropic_beta": ["token-efficient-tools-2025-02-19"],
                "system": "Be terse.\nPrefer SI units.",
                "messages": [{
                    "role": "user",
                    "content": [{"type": "text", "text": "Hello"}]
                }],
                "max_tokens": 128,
                "temperature": 0.2,
                "top_p": 0.9,
                "stop_sequences": ["END"]
            })
        );
    }

    #[test]
    fn builds_bearer_converse_request_with_encoded_model_path_and_headers() {
        let provider = BedrockProvider::new(BedrockProviderConfig {
            provider_key: "bedrock".to_string(),
            region: "us-east-1".to_string(),
            endpoint_url: "https://bedrock-runtime.us-east-1.amazonaws.com".to_string(),
            auth: BedrockAuthConfig::Bearer {
                token: "test-token".to_string(),
            },
            default_headers: BTreeMap::from([(
                "x-amzn-bedrock-trace".to_string(),
                "ENABLED".to_string(),
            )]),
            request_timeout_ms: 1_000,
        })
        .expect("provider");
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Hello")],
            stream: false,
            extra: BTreeMap::new(),
        };

        let built = provider
            .build_chat_request(&request, &context("amazon.nova-pro-v1:0"))
            .expect("request");
        let body: Value =
            serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

        assert_eq!(
            built.url().as_str(),
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/amazon.nova-pro-v1%3A0/converse"
        );
        assert_eq!(
            built.headers().get("authorization").unwrap(),
            "Bearer test-token"
        );
        assert_eq!(built.headers().get("x-request-id").unwrap(), "req-test");
        assert_eq!(
            built.headers().get("x-amzn-bedrock-trace").unwrap(),
            "ENABLED"
        );
        assert_eq!(
            body,
            json!({
                "messages": [{
                    "role": "user",
                    "content": [{"text": "Hello"}]
                }]
            })
        );
    }

    #[test]
    fn builds_bearer_anthropic_invoke_request_with_encoded_model_path() {
        let provider = BedrockProvider::new(BedrockProviderConfig {
            provider_key: "bedrock".to_string(),
            region: "us-east-1".to_string(),
            endpoint_url: "https://bedrock-runtime.us-east-1.amazonaws.com".to_string(),
            auth: BedrockAuthConfig::Bearer {
                token: "test-token".to_string(),
            },
            default_headers: BTreeMap::new(),
            request_timeout_ms: 1_000,
        })
        .expect("provider");
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Hello")],
            stream: false,
            extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
        };

        let built = provider
            .build_chat_request(
                &request,
                &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
            )
            .expect("request");
        let body: Value =
            serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

        assert_eq!(
            built.url().as_str(),
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/us.anthropic.claude-3-5-sonnet-20241022-v2%3A0/invoke"
        );
        assert_eq!(
            built.headers().get("authorization").unwrap(),
            "Bearer test-token"
        );
        assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
        assert_eq!(body["max_tokens"], 64);
    }

    #[test]
    fn builds_bearer_converse_stream_request_with_eventstream_accept_header() {
        let provider = BedrockProvider::new(BedrockProviderConfig {
            provider_key: "bedrock".to_string(),
            region: "us-east-1".to_string(),
            endpoint_url: "https://bedrock-runtime.us-east-1.amazonaws.com".to_string(),
            auth: BedrockAuthConfig::Bearer {
                token: "test-token".to_string(),
            },
            default_headers: BTreeMap::new(),
            request_timeout_ms: 1_000,
        })
        .expect("provider");
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Hello")],
            stream: true,
            extra: BTreeMap::new(),
        };

        let built = provider
            .build_converse_stream_request(
                &request,
                &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
            )
            .expect("request");

        assert_eq!(
            built.url().as_str(),
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/us.anthropic.claude-3-5-sonnet-20241022-v2%3A0/converse-stream"
        );
        assert_eq!(
            built.headers().get("accept").unwrap(),
            "application/vnd.amazon.eventstream"
        );
    }

    #[test]
    fn maps_function_tools_and_tool_choice() {
        let request = CoreChatRequest {
            model: "nova".to_string(),
            messages: vec![message("user", "Check weather")],
            stream: false,
            extra: BTreeMap::from([
                (
                    "tools".to_string(),
                    json!([{
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "description": "Get weather",
                            "parameters": {
                                "type": "object",
                                "properties": {"city": {"type": "string"}},
                                "required": ["city"]
                            }
                        }
                    }]),
                ),
                (
                    "tool_choice".to_string(),
                    json!({"type":"function","function":{"name":"get_weather"}}),
                ),
            ]),
        };

        let body = map_chat_request_to_converse(&request, &context("amazon.nova-pro-v1:0"))
            .expect("mapped");

        assert_eq!(
            body["toolConfig"],
            json!({
                "tools": [{
                    "toolSpec": {
                        "name": "get_weather",
                        "description": "Get weather",
                        "inputSchema": {
                            "json": {
                                "type": "object",
                                "properties": {"city": {"type": "string"}},
                                "required": ["city"]
                            }
                        }
                    }
                }],
                "toolChoice": {"tool": {"name": "get_weather"}}
            })
        );
    }

    #[test]
    fn maps_anthropic_function_tools_tool_choice_and_tool_results() {
        let mut assistant = message("assistant", "Calling tool");
        assistant.extra.insert(
            "tool_calls".to_string(),
            json!([{
                "id": "toolu_123",
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "arguments": "{\"city\":\"London\"}"
                }
            }]),
        );
        let mut tool = message("tool", "12 C");
        tool.extra
            .insert("tool_call_id".to_string(), json!("toolu_123"));
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Check weather"), assistant, tool],
            stream: false,
            extra: BTreeMap::from([
                ("max_tokens".to_string(), json!(256)),
                (
                    "tools".to_string(),
                    json!([{
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "description": "Get weather",
                            "parameters": {
                                "type": "object",
                                "properties": {"city": {"type": "string"}},
                                "required": ["city"]
                            }
                        }
                    }]),
                ),
                (
                    "tool_choice".to_string(),
                    json!({"type":"function","function":{"name":"get_weather"}}),
                ),
            ]),
        };

        let body = map_chat_request_to_anthropic_messages(
            &request,
            &context("anthropic.claude-3-haiku-20240307-v1:0"),
        )
        .expect("mapped");

        assert_eq!(
            body["tools"],
            json!([{
                "name": "get_weather",
                "description": "Get weather",
                "input_schema": {
                    "type": "object",
                    "properties": {"city": {"type": "string"}},
                    "required": ["city"]
                }
            }])
        );
        assert_eq!(
            body["tool_choice"],
            json!({"type": "tool", "name": "get_weather"})
        );
        assert_eq!(
            body["messages"][1]["content"][1],
            json!({
                "type": "tool_use",
                "id": "toolu_123",
                "name": "get_weather",
                "input": {"city": "London"}
            })
        );
        assert_eq!(
            body["messages"][2]["content"][0],
            json!({
                "type": "tool_result",
                "tool_use_id": "toolu_123",
                "content": "12 C"
            })
        );
    }

    #[test]
    fn maps_anthropic_base64_image_blocks_and_rejects_remote_urls() {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![CoreChatMessage {
                role: "user".to_string(),
                content: json!([
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": "aW1hZ2U="
                        }
                    },
                    {"type": "text", "text": "Describe it"}
                ]),
                name: None,
                extra: BTreeMap::new(),
            }],
            stream: false,
            extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
        };

        let body = map_chat_request_to_anthropic_messages(
            &request,
            &context("anthropic.claude-3-haiku-20240307-v1:0"),
        )
        .expect("mapped");
        assert_eq!(
            body["messages"][0]["content"][0],
            json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": "aW1hZ2U="
                }
            })
        );

        let remote = CoreChatRequest {
            messages: vec![CoreChatMessage {
                content: json!([{
                    "type": "image_url",
                    "image_url": {"url": "https://example.test/image.png"}
                }]),
                ..message("user", "")
            }],
            ..request
        };
        let error = map_chat_request_to_anthropic_messages(
            &remote,
            &context("anthropic.claude-3-haiku-20240307-v1:0"),
        )
        .expect_err("remote image rejected")
        .to_string();
        assert!(error.contains("remote image URLs are not supported"));
    }

    #[test]
    fn rejects_anthropic_messages_without_max_tokens() {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Hello")],
            stream: false,
            extra: BTreeMap::new(),
        };

        let error = map_chat_request_to_anthropic_messages(
            &request,
            &context("anthropic.claude-3-haiku-20240307-v1:0"),
        )
        .expect_err("max tokens rejected")
        .to_string();
        assert!(error.contains("requires `max_tokens` or `max_completion_tokens`"));
    }

    #[test]
    fn gates_anthropic_messages_streaming_mapping() {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("user", "Hello")],
            stream: true,
            extra: BTreeMap::from([("max_tokens".to_string(), json!(64))]),
        };

        let error = map_chat_request_to_anthropic_messages(
            &request,
            &context("anthropic.claude-3-haiku-20240307-v1:0"),
        )
        .expect_err("streaming gated")
        .to_string();
        assert!(error.contains("streaming is gated"));
    }

    #[test]
    fn rejects_unsupported_role_deterministically() {
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("critic", "Nope")],
            stream: false,
            extra: BTreeMap::new(),
        };

        let error = map_chat_request_to_converse(
            &request,
            &context("anthropic.claude-3-haiku-20240307-v1:0"),
        )
        .expect_err("role rejected")
        .to_string();
        assert!(error.contains("unsupported message role `critic`"));
    }

    #[test]
    fn normalizes_text_response_with_usage() {
        let response = json!({
            "responseId": "bedrock-response-id",
            "output": {
                "message": {
                    "role": "assistant",
                    "content": [{"text": "Hello from Bedrock."}]
                }
            },
            "stopReason": "end_turn",
            "usage": {
                "inputTokens": 12,
                "outputTokens": 5,
                "totalTokens": 17,
                "cacheReadInputTokens": 2
            }
        });

        let normalized = normalize_converse_response(
            &response,
            &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
        );

        assert_eq!(normalized["id"], "bedrock-response-id");
        assert_eq!(normalized["object"], "chat.completion");
        assert_eq!(normalized["model"], "gateway-model");
        assert_eq!(normalized["choices"][0]["message"]["role"], "assistant");
        assert_eq!(
            normalized["choices"][0]["message"]["content"],
            "Hello from Bedrock."
        );
        assert_eq!(normalized["choices"][0]["finish_reason"], "stop");
        assert_eq!(normalized["usage"]["prompt_tokens"], 12);
        assert_eq!(normalized["usage"]["completion_tokens"], 5);
        assert_eq!(normalized["usage"]["total_tokens"], 17);
        assert_eq!(
            normalized["usage"]["provider_usage"]["cacheReadInputTokens"],
            2
        );
    }

    #[test]
    fn normalizes_tool_use_response() {
        let response = json!({
            "output": {
                "message": {
                    "role": "assistant",
                    "content": [{
                        "toolUse": {
                            "toolUseId": "tooluse_123",
                            "name": "get_weather",
                            "input": {"city": "London"}
                        }
                    }]
                }
            },
            "stopReason": "tool_use",
            "usage": {
                "inputTokens": 30,
                "outputTokens": 8,
                "totalTokens": 38
            }
        });

        let normalized = normalize_converse_response(&response, &context("amazon.nova-pro-v1:0"));

        assert_eq!(normalized["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            normalized["choices"][0]["message"]["tool_calls"][0],
            json!({
                "id": "tooluse_123",
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "arguments": "{\"city\":\"London\"}"
                }
            })
        );
    }

    #[test]
    fn normalizes_anthropic_messages_response_with_usage_and_cache_tokens() {
        let response = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-5-sonnet",
            "content": [{"type": "text", "text": "Hello from Claude."}],
            "stop_reason": "stop_sequence",
            "usage": {
                "input_tokens": 12,
                "output_tokens": 5,
                "cache_read_input_tokens": 2,
                "cache_creation_input_tokens": 3
            }
        });

        let normalized = normalize_anthropic_messages_response(
            &response,
            &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
        );

        assert_eq!(normalized["id"], "msg_123");
        assert_eq!(
            normalized["choices"][0]["message"]["content"],
            "Hello from Claude."
        );
        assert_eq!(normalized["choices"][0]["finish_reason"], "stop");
        assert_eq!(normalized["usage"]["prompt_tokens"], 12);
        assert_eq!(normalized["usage"]["completion_tokens"], 5);
        assert_eq!(normalized["usage"]["total_tokens"], 17);
        assert_eq!(
            normalized["usage"]["provider_usage"]["cache_read_input_tokens"],
            2
        );
        assert_eq!(
            normalized["usage"]["provider_usage"]["cache_creation_input_tokens"],
            3
        );
    }

    #[test]
    fn normalizes_anthropic_tool_use_response() {
        let response = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": "toolu_123",
                "name": "get_weather",
                "input": {"city": "London"}
            }],
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 30,
                "output_tokens": 8
            }
        });

        let normalized = normalize_anthropic_messages_response(
            &response,
            &context("anthropic.claude-3-haiku-20240307-v1:0"),
        );

        assert_eq!(normalized["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            normalized["choices"][0]["message"]["tool_calls"][0],
            json!({
                "id": "toolu_123",
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "arguments": "{\"city\":\"London\"}"
                }
            })
        );
    }

    #[test]
    fn decodes_fragmented_eventstream_frames() {
        let frame = eventstream_frame(
            &[
                (":message-type", "event"),
                (":event-type", "contentBlockDelta"),
            ],
            br#"{"delta":{"text":"hel"}}"#,
        );
        let mut decoder = BedrockEventStreamDecoder::default();

        assert!(decoder.push_bytes(&frame[..7]).expect("first").is_empty());
        let events = decoder.push_bytes(&frame[7..]).expect("second");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message_type.as_deref(), Some("event"));
        assert_eq!(events[0].event_type.as_deref(), Some("contentBlockDelta"));
        assert_eq!(
            events[0].payload,
            Bytes::from_static(br#"{"delta":{"text":"hel"}}"#)
        );
        decoder.finish().expect("complete");
    }

    #[test]
    fn rejects_malformed_eventstream_lengths() {
        let mut decoder = BedrockEventStreamDecoder::default();
        let error = decoder
            .push_bytes(&[0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0])
            .expect_err("malformed")
            .to_string();

        assert!(error.contains("invalid aws_bedrock EventStream frame length"));
    }

    #[tokio::test]
    async fn normalizes_text_finish_usage_and_done_from_converse_stream() {
        let frames = vec![
            eventstream_frame(
                &[(":message-type", "event"), (":event-type", "messageStart")],
                br#"{"role":"assistant","conversationId":"conv-123"}"#,
            ),
            eventstream_frame(
                &[
                    (":message-type", "event"),
                    (":event-type", "contentBlockDelta"),
                ],
                br#"{"contentBlockIndex":0,"delta":{"text":"Hello "}}"#,
            ),
            eventstream_frame(
                &[
                    (":message-type", "event"),
                    (":event-type", "contentBlockDelta"),
                ],
                br#"{"contentBlockIndex":0,"delta":{"text":"world"}}"#,
            ),
            eventstream_frame(
                &[(":message-type", "event"), (":event-type", "messageStop")],
                br#"{"stopReason":"end_turn"}"#,
            ),
            eventstream_frame(
                &[(":message-type", "event"), (":event-type", "metadata")],
                br#"{"usage":{"inputTokens":11,"outputTokens":4,"totalTokens":15},"metrics":{"latencyMs":10}}"#,
            ),
        ];

        let transcript = collect_bedrock_stream(frames).await;

        assert!(transcript.contains(r#""id":"chatcmpl-conv-123""#));
        assert!(transcript.contains(r#""delta":{"role":"assistant"}"#));
        assert!(transcript.contains(r#""delta":{"content":"Hello "}"#));
        assert!(transcript.contains(r#""delta":{"content":"world"}"#));
        assert!(transcript.contains(r#""finish_reason":"stop""#));
        assert!(transcript.contains(r#""prompt_tokens":11"#));
        assert!(transcript.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn normalizes_tool_deltas_from_converse_stream() {
        let frames = vec![
            eventstream_frame(
                &[(":message-type", "event"), (":event-type", "messageStart")],
                br#"{"role":"assistant"}"#,
            ),
            eventstream_frame(
                &[
                    (":message-type", "event"),
                    (":event-type", "contentBlockStart"),
                ],
                br#"{"contentBlockIndex":1,"start":{"toolUse":{"toolUseId":"tool_123","name":"get_weather"}}}"#,
            ),
            eventstream_frame(
                &[
                    (":message-type", "event"),
                    (":event-type", "contentBlockDelta"),
                ],
                br#"{"contentBlockIndex":1,"delta":{"toolUse":{"input":"{\"city\":"}}}"#,
            ),
            eventstream_frame(
                &[(":message-type", "event"), (":event-type", "messageStop")],
                br#"{"stopReason":"tool_use"}"#,
            ),
        ];

        let transcript = collect_bedrock_stream(frames).await;

        assert!(transcript.contains(r#""tool_calls":[{"function":{"arguments":"","name":"get_weather"},"id":"tool_123","index":1,"type":"function"}]"#));
        assert!(
            transcript
                .contains(r#""tool_calls":[{"function":{"arguments":"{\"city\":"},"index":1}]"#)
        );
        assert!(transcript.contains(r#""finish_reason":"tool_calls""#));
        assert!(transcript.ends_with("data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn emits_structured_error_for_exception_event_without_done() {
        let frames = vec![eventstream_frame(
            &[
                (":message-type", "exception"),
                (":exception-type", "throttlingException"),
            ],
            br#"{"message":"rate limited"}"#,
        )];

        let transcript = collect_bedrock_stream(frames).await;

        assert!(transcript.contains(r#""code":"throttlingException""#));
        assert!(transcript.contains(r#""message":"rate limited""#));
        assert!(!transcript.contains("[DONE]"));
    }

    #[tokio::test]
    async fn emits_structured_error_for_incomplete_frame_without_done() {
        let frame = eventstream_frame(
            &[(":message-type", "event"), (":event-type", "messageStart")],
            br#"{"role":"assistant"}"#,
        );
        let truncated = frame[..frame.len() - 3].to_vec();

        let transcript = collect_bedrock_stream(vec![truncated]).await;

        assert!(transcript.contains(r#""code":"bedrock_eventstream_finalization_error""#));
        assert!(transcript.contains("incomplete aws_bedrock EventStream frame"));
        assert!(!transcript.contains("[DONE]"));
    }

    fn message(role: &str, content: &str) -> CoreChatMessage {
        CoreChatMessage {
            role: role.to_string(),
            content: Value::String(content.to_string()),
            name: None,
            extra: BTreeMap::new(),
        }
    }

    fn context(upstream_model: &str) -> ProviderRequestContext {
        ProviderRequestContext {
            request_id: "req-test".to_string(),
            model_key: "gateway-model".to_string(),
            provider_key: "bedrock".to_string(),
            upstream_model: upstream_model.to_string(),
            extra_headers: Map::new(),
            extra_body: Map::new(),
            request_headers: BTreeMap::new(),
            compatibility: Default::default(),
        }
    }

    fn eventstream_frame(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
        let mut encoded_headers = Vec::new();
        for (name, value) in headers {
            encoded_headers.push(name.len() as u8);
            encoded_headers.extend_from_slice(name.as_bytes());
            encoded_headers.push(7);
            encoded_headers.extend_from_slice(&(value.len() as u16).to_be_bytes());
            encoded_headers.extend_from_slice(value.as_bytes());
        }

        let total_len = 12 + encoded_headers.len() + payload.len() + 4;
        let mut frame = Vec::with_capacity(total_len);
        frame.extend_from_slice(&(total_len as u32).to_be_bytes());
        frame.extend_from_slice(&(encoded_headers.len() as u32).to_be_bytes());
        frame.extend_from_slice(&0_u32.to_be_bytes());
        frame.extend_from_slice(&encoded_headers);
        frame.extend_from_slice(payload);
        frame.extend_from_slice(&0_u32.to_be_bytes());
        frame
    }

    async fn collect_bedrock_stream(frames: Vec<Vec<u8>>) -> String {
        let chunks = frames.into_iter().map(|frame| Ok(Bytes::from(frame)));
        let mut stream =
            normalize_bedrock_converse_stream(futures_util::stream::iter(chunks), context("nova"));
        let mut transcript = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("stream chunk");
            transcript.push_str(std::str::from_utf8(&chunk).expect("utf8"));
        }

        transcript
    }
}
