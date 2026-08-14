use std::collections::BTreeMap;

use axum::Router;
use axum::extract::Json;
use axum::routing::post;
use gateway_core::{CoreChatMessage, CoreChatRequest, ProviderClient, ProviderRequestContext};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use tokio::net::TcpListener as TokioTcpListener;
use tokio::sync::mpsc;

use super::*;
use crate::copilot::auth::{CopilotAuthConfig, GitHubAppInstallationTokenSource};
use crate::token::AccessTokenSource;

// Note: This RSA private key is a throwaway fixture used strictly for unit testing
// JWT signature generation and is not used in any production environment.
const TEST_RSA_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCo1WHWzHdosbKK
WTlCf4nJS2wANN4n/lXEc+7E/2OEoS8co9upp4NVgH0wcLjfSYXz1bmrCdnj7ppW
Vbg+mZpW4r9JKncCTtKjXq2Qt/a4tYj6WxakcLk42pnhAx8PNbmYHp2bZhPv4eJf
BqZKu5nXyZ6BFwoIWkj/3MGIq+e7pA3bGEa/vW3U+1YRAA3+0WsE5abPcJKMLgF6
1PFfjKuw9x//yenMHkL4gjZOINac1nHyTQI/4Km/84IyztNBwyxncGwW4g/x0Neb
nSgOKs8Nndg3Rh82R6IkOEbH8Eopgs2y0/7rBPrqSHuaPwoDbo9ryOx/bBhhF40g
yDHRzteFAgMBAAECggEADt8a2uWQSBXM9QBGeavLyoIc/Yiqn+m4itEucUZQsQxU
nsh1LyS8/hFPFa78LdjnVnLXQ7Bes8Pe7udmjwcPMAORl2OI75hbV/4dOj/mGN+O
tQTEghAW1KH2x3nzqK6SDkr2FYvgijMCsl2e1LrhIn+VOWg630D6qKT8nCoOQ7ob
G2aCWsrdFLkGnF/OyzN9HvA1cIi2QSloLYX0cxfoa5nILevbzPL5JFYphzXF7V3T
OSzueatw54K9o7Pywn6zb5pG/fri9jxFugojZlSG08vnamaFJjdjW/k76DVBgLi+
hlmvOmQ08hdIk4q05L9OzEbSZgw0bOPFCe/PECVRAQKBgQDlf1zRI0uI6YKxhDX3
B2hNWQuiqk2CXy5qb8EH+3omFTmDrShnhjdvYOtxUs8Sys+/W8h8ONOfnR1moBtI
ysoNno6E1AtpL0563CJhRo9H+XT8spwMMFolgQy37Eg/Jh/6be9/B4t5HUUVl9C4
m9IHV1DGrYLnW7UF5mcMISEghQKBgQC8VJwKM76m9ADQvtZ6H9SvimCzhhq/Hcqi
9uTzPqaBPWg7S5ErJCGDydHd68vBZbiqlVuYqNogdJq+WTxZzuy00/Wk87U6XGno
MS2pNpZdi5Lpzm/vIvNe18K9ZBSdlCS9mgzibDJ13pvMUvkw9NWXlhlqktW2rTGS
tzW+SagLAQKBgCNcdn67A357DGoxxubjO0z/tW1A9GRsKgi4Y3PJac7IYm5Jlfot
kgkVU/HIIqPwoAYKLGAHmYP0f306mjmjFXL3xVnuGjwA0ATaOmnmp1kdtMri8mxm
Xt18fusv+wnP5AmAOvDFxtXIjsZ+9+gaCkibSZTzU0I2vTPFhoc165bJAoGBALGZ
rKkmUPWKhzZTsVjrqZt9CGJj5dczFgQGhrQo8cZRDXlVcunXIc/xQ/tewQB5l+Mu
BHn7SfBvZfp5lqMusyR3+l/6/32w5qLztZashrJizEG2zvIZ6J4ZJGmL9rD/ooI2
w03HMPLc4dmWqa6URNS11PQe0nF59JTiN0lilpkBAoGBAJSUsh5qGGyAE+gHZXYn
yPy48bBninSJZBa7aUm5PxbZLLG5FQoyBDZPUyOvsKJc7UBjpwDe0jMkJmjpvW+r
GgkfTd4qdOaEI8ljZxJM7plf5ZHfJND9xz+SJ3PqpNejzDeD4xQkwKAzeMQyl1z6
UQ2sSTSfuLHz2F1jr5+pRNL2
-----END PRIVATE KEY-----"#;

fn dummy_context(upstream_model: &str) -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: "test-req-123".to_string(),
        model_key: "test-model".to_string(),
        provider_key: "github_copilot".to_string(),
        upstream_model: upstream_model.to_string(),
        extra_headers: Map::new(),
        extra_body: Map::new(),
        request_headers: BTreeMap::new(),
        compatibility: Default::default(),
    }
}

#[test]
fn endpoint_routing_rules() {
    assert_eq!(
        CopilotProvider::resolve_chat_endpoint_suffix("gpt-4o"),
        "chat/completions"
    );
    assert_eq!(
        CopilotProvider::resolve_chat_endpoint_suffix("gemini-2.5-flash"),
        "chat/completions"
    );
    assert_eq!(
        CopilotProvider::resolve_chat_endpoint_suffix("claude-3-7-sonnet"),
        "v1/messages"
    );
    assert_eq!(
        CopilotProvider::resolve_chat_endpoint_suffix("claude-opus-4-5"),
        "v1/messages"
    );
    assert_eq!(
        CopilotProvider::resolve_chat_endpoint_suffix("gpt-5.4"),
        "chat/completions"
    );
    assert_eq!(
        CopilotProvider::resolve_chat_endpoint_suffix("gpt-5-mini"),
        "chat/completions"
    );
    assert_eq!(
        CopilotProvider::resolve_chat_endpoint_suffix("gpt-5-codex"),
        "chat/completions"
    );
}

#[tokio::test]
async fn builds_chat_completions_with_copilot_headers() {
    let (tx, mut rx) = mpsc::channel(1);

    let app = Router::new().route(
        "/chat/completions",
        post(
            move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                let tx = tx.clone();
                async move {
                    let editor_version = headers
                        .get("editor-version")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let integration_id = headers
                        .get("copilot-integration-id")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let api_version = headers
                        .get("x-github-api-version")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let req_id = headers
                        .get("x-request-id")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();

                    let _ = tx
                        .send((
                            editor_version,
                            integration_id,
                            auth,
                            api_version,
                            req_id,
                            body,
                        ))
                        .await;

                    Json(json!({
                        "id": "chatcmpl-test",
                        "object": "chat.completion",
                        "choices": [{
                            "index": 0,
                            "message": { "role": "assistant", "content": "Hello world" },
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": 10,
                            "completion_tokens": 5,
                            "total_tokens": 15
                        }
                    }))
                }
            },
        ),
    );

    let listener = TokioTcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let mut config = CopilotProviderConfig::new(
        "github_copilot".to_string(),
        CopilotAuthConfig::Bearer {
            token: "test-ghs-token".to_string(),
        },
    );
    config.base_url = format!("http://{addr}");

    let provider = CopilotProvider::new(config).unwrap();

    let request = CoreChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!("Hi"),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: false,
        extra: BTreeMap::new(),
    };

    let context = dummy_context("gpt-4o");
    let response = provider.chat_completions(&request, &context).await.unwrap();

    assert_eq!(response["choices"][0]["message"]["content"], "Hello world");

    let (editor_version, integration_id, auth, api_version, req_id, body) =
        rx.recv().await.unwrap();
    assert_eq!(editor_version, "vscode/1.126.0");
    assert_eq!(integration_id, "vscode-chat");
    assert_eq!(auth, "Bearer test-ghs-token");
    assert_eq!(api_version, "2026-06-01");
    assert_eq!(req_id, "test-req-123");
    assert_eq!(body["model"], "gpt-4o");
}

#[tokio::test]
async fn builds_claude_messages_request() {
    let (tx, mut rx) = mpsc::channel(1);

    let app = Router::new().route(
        "/v1/messages",
        post(
            move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                let tx = tx.clone();
                async move {
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let _ = tx.send((auth, body)).await;

                    Json(json!({
                        "id": "msg-test",
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "text", "text": "Claude response" }],
                        "usage": {
                            "input_tokens": 12,
                            "output_tokens": 4
                        }
                    }))
                }
            },
        ),
    );

    let listener = TokioTcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let mut config = CopilotProviderConfig::new(
        "github_copilot".to_string(),
        CopilotAuthConfig::Bearer {
            token: "ghs_bearer".to_string(),
        },
    );
    config.base_url = format!("http://{addr}");

    let provider = CopilotProvider::new(config).unwrap();

    let request = CoreChatRequest {
        model: "claude-3-7-sonnet".to_string(),
        messages: vec![
            CoreChatMessage {
                role: "system".to_string(),
                content: json!("System prompt"),
                name: None,
                extra: BTreeMap::new(),
            },
            CoreChatMessage {
                role: "user".to_string(),
                content: json!("Hello Claude"),
                name: None,
                extra: BTreeMap::new(),
            },
        ],
        stream: false,
        extra: BTreeMap::new(),
    };

    let context = dummy_context("claude-3-7-sonnet");
    let response = provider.chat_completions(&request, &context).await.unwrap();

    assert_eq!(
        response["choices"][0]["message"]["content"],
        "Claude response"
    );

    let (auth, body) = rx.recv().await.unwrap();
    assert_eq!(auth, "Bearer ghs_bearer");
    assert_eq!(body["model"], "claude-3-7-sonnet");
    assert_eq!(body["system"], "System prompt");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"][0]["text"], "Hello Claude");
    assert_eq!(body["max_tokens"], 4096);
}

#[tokio::test]
async fn github_app_installation_token_source_mints_token() {
    let (tx, mut rx) = mpsc::channel(1);

    let app = Router::new().route(
        "/app/installations/12345/access_tokens",
        post(
            move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                let tx = tx.clone();
                async move {
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let _ = tx.send((auth, body)).await;

                    let expires_at = (OffsetDateTime::now_utc() + time::Duration::hours(1))
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap();

                    Json(json!({
                        "token": "ghs_installation_token_abc",
                        "expires_at": expires_at
                    }))
                }
            },
        ),
    );

    let listener = TokioTcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let source =
        GitHubAppInstallationTokenSource::new(99999, TEST_RSA_PRIVATE_KEY, 12345, Some(67890))
            .unwrap()
            .with_api_url(format!("http://{addr}"));

    let token = source.fetch_token().await.unwrap();
    assert_eq!(token.token, "ghs_installation_token_abc");

    let (auth, body) = rx.recv().await.unwrap();
    assert!(auth.starts_with("Bearer ey"));
    assert_eq!(body["permissions"]["copilot_requests"], "write");
    assert_eq!(body["repository_ids"][0], 67890);
}
