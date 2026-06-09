use std::collections::BTreeMap;

use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{
    AwsBedrockApiStyle, AwsBedrockRouteCompatibility, CoreChatMessage, CoreChatRequest,
    CoreResponsesRequest, ProviderClient, ProviderError, ProviderRequestContext,
    RouteCompatibility,
};
use serde_json::{Map, Value, json};
use serial_test::serial;

use super::{
    AnthropicMessagesTarget, BedrockAuthConfig, BedrockEndpointKind, BedrockEventStreamDecoder,
    BedrockProvider, BedrockProviderConfig,
    map_chat_request_to_anthropic_messages as map_chat_request_to_anthropic_messages_target,
    map_chat_request_to_converse, normalize_anthropic_messages_response,
    normalize_anthropic_messages_stream, normalize_bedrock_converse_stream,
    normalize_converse_response,
};

mod endpoint;
mod eventstream;
mod provider_requests;
mod request_content;
mod request_thinking;
mod request_tools;
mod response;

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

fn context_with_api_style(
    upstream_model: &str,
    api_style: AwsBedrockApiStyle,
    openai_base_path: Option<&str>,
) -> ProviderRequestContext {
    ProviderRequestContext {
        compatibility: RouteCompatibility {
            aws_bedrock: Some(AwsBedrockRouteCompatibility {
                api_style,
                openai_base_path: openai_base_path.map(ToString::to_string),
            }),
            ..Default::default()
        },
        ..context(upstream_model)
    }
}

fn map_chat_request_to_anthropic_messages(
    request: &CoreChatRequest,
    context: &ProviderRequestContext,
) -> Result<Value, ProviderError> {
    map_chat_request_to_anthropic_messages_target(
        request,
        context,
        AnthropicMessagesTarget::RuntimeInvoke,
    )
}

fn responses_request(stream: bool) -> CoreResponsesRequest {
    CoreResponsesRequest {
        model: "gpt".to_string(),
        input: json!([
            {"type":"message","role":"user","content":"hello"}
        ]),
        stream,
        instructions: Some(json!("Answer briefly.")),
        tools: None,
        tool_choice: None,
        reasoning: Some(json!({"effort":"medium"})),
        text: None,
        extra: BTreeMap::new(),
    }
}

fn mantle_bearer_provider() -> BedrockProvider {
    BedrockProvider::new(BedrockProviderConfig {
        provider_key: "bedrock-mantle".to_string(),
        region: "us-east-2".to_string(),
        endpoint_kind: BedrockEndpointKind::BedrockMantle,
        endpoint_url: "https://bedrock-mantle.us-east-2.api.aws".to_string(),
        auth: BedrockAuthConfig::Bearer {
            token: "mantle-token".to_string(),
        },
        default_headers: BTreeMap::new(),
        request_timeout_ms: 1_000,
    })
    .expect("provider")
}

fn static_credentials_provider(session_token: Option<&str>) -> BedrockProvider {
    BedrockProvider::new(BedrockProviderConfig {
        provider_key: "bedrock".to_string(),
        region: "us-east-1".to_string(),
        endpoint_kind: BedrockEndpointKind::BedrockRuntime,
        endpoint_url: "https://bedrock-runtime.us-east-1.amazonaws.com".to_string(),
        auth: BedrockAuthConfig::StaticCredentials {
            access_key_id: "test-access-key".to_string(),
            secret_access_key: "test-secret-key".to_string(),
            session_token: session_token.map(ToString::to_string),
        },
        default_headers: BTreeMap::new(),
        request_timeout_ms: 1_000,
    })
    .expect("provider")
}

struct AwsCredentialEnvGuard {
    previous: Vec<(&'static str, Option<String>)>,
}

impl AwsCredentialEnvGuard {
    fn set() -> Self {
        let keys = [
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "AWS_PROFILE",
        ];
        let previous = keys
            .into_iter()
            .map(|key| (key, std::env::var(key).ok()))
            .collect::<Vec<_>>();
        unsafe {
            std::env::set_var("AWS_ACCESS_KEY_ID", "chain-access-key");
            std::env::set_var("AWS_SECRET_ACCESS_KEY", "chain-secret-key");
            std::env::set_var("AWS_SESSION_TOKEN", "chain-session-token");
            std::env::remove_var("AWS_PROFILE");
        }
        Self { previous }
    }
}

impl Drop for AwsCredentialEnvGuard {
    fn drop(&mut self) {
        unsafe {
            for (key, value) in &self.previous {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
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
