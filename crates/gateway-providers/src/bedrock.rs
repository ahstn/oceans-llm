use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use gateway_core::{
    CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest, ProviderCapabilities,
    ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;

use crate::http::map_reqwest_error;

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
        let encoded_model_id: String =
            url::form_urlencoded::byte_serialize(upstream_model.as_bytes()).collect();
        format!(
            "{}/model/{encoded_model_id}/converse",
            self.config.endpoint_url
        )
    }

    fn build_converse_request(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<reqwest::Request, ProviderError> {
        let body = map_chat_request_to_converse(request, context)?;
        let url = self.converse_endpoint(&context.upstream_model);

        let mut request = self.client.post(url).json(&body);
        request = request.header("content-type", "application/json");
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
        ProviderCapabilities::with_dimensions(true, false, false, true, false, false, true)
    }

    async fn chat_completions(
        &self,
        request: &CoreChatRequest,
        context: &ProviderRequestContext,
    ) -> Result<Value, ProviderError> {
        let request = self.build_converse_request(request, context)?;
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
            ProviderError::Transport(format!("invalid JSON from aws_bedrock converse: {error}"))
        })?;
        Ok(normalize_converse_response(&value, context))
    }

    async fn chat_completions_stream(
        &self,
        _request: &CoreChatRequest,
        _context: &ProviderRequestContext,
    ) -> Result<ProviderStream, ProviderError> {
        Err(Self::unsupported("chat completions streaming"))
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
    if request.stream {
        return Err(ProviderError::InvalidRequest(
            "aws_bedrock Converse streaming is not supported in this slice".to_string(),
        ));
    }

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

fn tool_choice_is_none_or_auto(value: &Value) -> bool {
    matches!(value.as_str(), Some("none" | "auto"))
        || value
            .as_object()
            .and_then(|object| object.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|kind| matches!(kind, "none" | "auto"))
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
            "`{field}` is not supported for aws_bedrock Converse in this slice"
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

fn map_stop_reason(reason: &str) -> &'static str {
    match reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" | "model_context_window_exceeded" => "length",
        "tool_use" => "tool_calls",
        "guardrail_intervened" | "content_filtered" => "content_filter",
        "malformed_model_output" | "malformed_tool_use" => "stop",
        _ => "stop",
    }
}

fn map_usage(value: &Value) -> Option<Value> {
    let usage = value.get("usage")?.as_object()?;
    let prompt = usage
        .get("inputTokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let completion = usage
        .get("outputTokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = usage
        .get("totalTokens")
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

    use gateway_core::{CoreChatMessage, CoreChatRequest, ProviderRequestContext};
    use serde_json::{Map, Value, json};

    use super::{
        BedrockAuthConfig, BedrockProvider, BedrockProviderConfig, map_chat_request_to_converse,
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
            .build_converse_request(
                &request,
                &context("us.anthropic.claude-3-5-sonnet-20241022-v2:0"),
            )
            .expect("request");
        let body: Value =
            serde_json::from_slice(built.body().unwrap().as_bytes().unwrap()).expect("json body");

        assert_eq!(
            built.url().as_str(),
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/us.anthropic.claude-3-5-sonnet-20241022-v2%3A0/converse"
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
}
