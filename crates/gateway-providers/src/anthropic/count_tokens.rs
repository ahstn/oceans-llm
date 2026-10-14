//! Shapes Messages requests for upstream token-count APIs.
//!
//! Token-count endpoints accept the prompt-defining subset of a Messages body and
//! reject generation controls such as `max_tokens`, `stream` or `temperature`. The
//! gateway builds the same body it would send for inference, then keeps only the
//! fields that change the prompt.

use gateway_core::ProviderError;
use serde_json::{Value, json};

/// Messages fields that affect the counted prompt.
pub const COUNT_TOKENS_FIELDS: &[&str] = &[
    "model",
    "messages",
    "system",
    "tools",
    "tool_choice",
    "thinking",
    "cache_control",
    "output_config",
    "output_format",
    "mcp_servers",
    "context_management",
];

/// Keeps `fields` and drops every other top-level key of a mapped request body.
pub fn retain_count_tokens_fields(body: Value, fields: &[&str]) -> Value {
    match body {
        Value::Object(mut object) => {
            object.retain(|key, _| fields.contains(&key.as_str()));
            Value::Object(object)
        }
        other => other,
    }
}

/// Reads the upstream count from `field` and returns the Messages shape
/// `{"input_tokens": N}`.
pub fn normalize_count_tokens_response(
    value: &Value,
    field: &str,
    provider: &str,
) -> Result<Value, ProviderError> {
    let tokens = value.get(field).and_then(Value::as_u64).ok_or_else(|| {
        ProviderError::Transport(format!(
            "{provider} token count response is missing `{field}`"
        ))
    })?;
    Ok(json!({ "input_tokens": tokens }))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
    use gateway_core::{CoreChatMessage, CoreChatRequest, ProviderClient, ProviderRequestContext};
    use tokio::{net::TcpListener, sync::Mutex};

    use super::*;
    use crate::anthropic_compat::{
        AnthropicCompatAuth, AnthropicCompatAuthKind, AnthropicCompatConfig,
        AnthropicCompatProvider,
    };

    type Captured = Arc<Mutex<Option<(HeaderMap, Value)>>>;

    fn provider(base_url: String) -> AnthropicCompatProvider {
        let mut config = AnthropicCompatConfig::new("anthropic".to_string(), base_url);
        config.auth = Some(AnthropicCompatAuth {
            kind: AnthropicCompatAuthKind::XApiKey,
            token: "sk-test".to_string(),
        });
        AnthropicCompatProvider::new(config).expect("provider")
    }

    #[test]
    fn count_tokens_endpoint_follows_base_url_version() {
        for (base, expected) in [
            (
                "https://api.example.test",
                "https://api.example.test/v1/messages/count_tokens",
            ),
            (
                "https://api.example.test/v1/",
                "https://api.example.test/v1/messages/count_tokens",
            ),
        ] {
            assert_eq!(
                provider(base.to_string())
                    .count_tokens_endpoint_url()
                    .unwrap(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn anthropic_compat_forwards_prompt_fields_to_upstream_count() {
        let captured: Captured = Arc::default();
        let app = Router::new()
            .route(
                "/v1/messages/count_tokens",
                post(
                    |State(captured): State<Captured>,
                     headers: HeaderMap,
                     Json(body): Json<Value>| async move {
                        *captured.lock().await = Some((headers, body));
                        Json(json!({"input_tokens": 42}))
                    },
                ),
            )
            .with_state(captured.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });

        let message = |role: &str, text: &str| CoreChatMessage {
            role: role.to_string(),
            content: Value::String(text.to_string()),
            name: None,
            extra: BTreeMap::new(),
        };
        let request = CoreChatRequest {
            model: "claude".to_string(),
            messages: vec![message("system", "Be brief."), message("user", "hi")],
            stream: false,
            extra: BTreeMap::from([
                ("max_tokens".to_string(), json!(1024)),
                ("temperature".to_string(), json!(0.5)),
            ]),
        };
        let context = ProviderRequestContext {
            request_id: "req-count".to_string(),
            model_key: "claude".to_string(),
            provider_key: "anthropic".to_string(),
            upstream_model: "claude-sonnet-4-6".to_string(),
            owner_user_id: None,
            expected_provider_credential_id: None,
            extra_headers: serde_json::Map::new(),
            extra_body: serde_json::Map::new(),
            request_headers: BTreeMap::new(),
            compatibility: Default::default(),
        };

        let value = provider(format!("http://{addr}"))
            .count_tokens(&request, &context)
            .await
            .expect("count");

        assert_eq!(value, json!({"input_tokens": 42}));
        let (headers, body) = captured.lock().await.take().expect("upstream call");
        assert_eq!(headers["x-api-key"], "sk-test");
        assert_eq!(headers["anthropic-version"], "2023-06-01");
        assert_eq!(headers["x-request-id"], "req-count");
        assert_eq!(
            body,
            json!({
                "model": "claude-sonnet-4-6",
                "messages": [{"role": "user", "content": "hi"}],
                "system": [{"type": "text", "text": "Be brief."}],
            })
        );
    }

    #[test]
    fn retain_drops_generation_controls() {
        let body = json!({
            "model": "claude",
            "messages": [{"role": "user", "content": "hi"}],
            "system": [{"type": "text", "text": "be brief"}],
            "max_tokens": 4096,
            "stream": false,
            "temperature": 0.2,
            "tools": [],
        });

        let filtered = retain_count_tokens_fields(body, COUNT_TOKENS_FIELDS);

        assert_eq!(
            filtered,
            json!({
                "model": "claude",
                "messages": [{"role": "user", "content": "hi"}],
                "system": [{"type": "text", "text": "be brief"}],
                "tools": [],
            })
        );
    }

    #[test]
    fn normalize_reads_named_field() {
        assert_eq!(
            normalize_count_tokens_response(&json!({"inputTokens": 12}), "inputTokens", "p")
                .unwrap(),
            json!({"input_tokens": 12})
        );
        assert!(matches!(
            normalize_count_tokens_response(&json!({}), "input_tokens", "p"),
            Err(ProviderError::Transport(_))
        ));
    }
}
