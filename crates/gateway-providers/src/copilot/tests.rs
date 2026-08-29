use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use axum::extract::Json;
use axum::routing::post;
use gateway_core::{
    CoreChatMessage, CoreChatRequest, CoreEmbeddingsRequest, CoreResponsesRequest,
    GitHubCopilotChatApi, GitHubCopilotRouteCompatibility, GitHubCopilotUpstreamSupports,
    OpenAiCompatDeveloperRole, OpenAiCompatMaxTokensField, OpenAiCompatRouteCompatibility,
    ProviderClient, ProviderError, ProviderRequestContext, ProviderUserTokenResolver,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use tokio::net::TcpListener as TokioTcpListener;
use tokio::sync::mpsc;

use super::*;
use crate::copilot::auth::{CopilotAuthConfig, GitHubAppInstallationTokenSource};
use crate::token::AccessTokenSource;

struct TestUserTokenResolver {
    tokens: HashMap<uuid::Uuid, String>,
}

#[async_trait]
impl ProviderUserTokenResolver for TestUserTokenResolver {
    async fn resolve_provider_user_token(
        &self,
        _provider_key: &str,
        user_id: uuid::Uuid,
    ) -> Result<String, ProviderError> {
        self.tokens
            .get(&user_id)
            .cloned()
            .ok_or_else(|| ProviderError::InvalidRequest("missing test token".to_string()))
    }
}

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
    let mut context = ProviderRequestContext {
        request_id: "test-req-123".to_string(),
        model_key: "test-model".to_string(),
        provider_key: "github_copilot".to_string(),
        upstream_model: upstream_model.to_string(),
        owner_user_id: None,
        extra_headers: Map::new(),
        extra_body: Map::new(),
        request_headers: BTreeMap::new(),
        compatibility: Default::default(),
    };
    context.compatibility.github_copilot = Some(GitHubCopilotRouteCompatibility {
        chat_api: Some(GitHubCopilotChatApi::ChatCompletions),
        supports_responses: true,
        supports_embeddings: true,
        upstream_supports: GitHubCopilotUpstreamSupports {
            streaming: true,
            tool_calls: true,
            vision: true,
            structured_outputs: true,
        },
    });
    context
}

fn messages_context(upstream_model: &str) -> ProviderRequestContext {
    let mut context = dummy_context(upstream_model);
    context
        .compatibility
        .github_copilot
        .as_mut()
        .expect("Copilot compatibility")
        .chat_api = Some(GitHubCopilotChatApi::AnthropicMessages);
    context
}

#[tokio::test]
async fn github_user_tokens_are_selected_by_trusted_user_id() {
    let user_a = uuid::Uuid::new_v4();
    let user_b = uuid::Uuid::new_v4();
    let resolver = Arc::new(TestUserTokenResolver {
        tokens: HashMap::from([
            (user_a, "token-a".to_string()),
            (user_b, "token-b".to_string()),
        ]),
    });
    let provider = CopilotProvider::new_with_user_token_resolver(
        CopilotProviderConfig::new(
            "github-copilot-user".to_string(),
            CopilotAuthConfig::GitHubUser,
        ),
        resolver,
    )
    .expect("user-token provider");

    let mut context = dummy_context("gpt-5.6-luna");
    context.owner_user_id = Some(user_a);
    assert_eq!(provider.token(&context).await.unwrap(), "token-a");
    context.owner_user_id = Some(user_b);
    assert_eq!(provider.token(&context).await.unwrap(), "token-b");
    context.owner_user_id = None;
    assert!(
        provider
            .token(&context)
            .await
            .unwrap_err()
            .to_string()
            .contains("user-owned gateway API key")
    );
}

#[test]
fn endpoint_routing_rules() {
    let chat_context = dummy_context("claude-3-7-sonnet");
    assert_eq!(
        CopilotProvider::resolve_chat_api(&chat_context).unwrap(),
        GitHubCopilotChatApi::ChatCompletions,
    );
    let messages_context = messages_context("gpt-4o");
    assert_eq!(
        CopilotProvider::resolve_chat_api(&messages_context).unwrap(),
        GitHubCopilotChatApi::AnthropicMessages,
    );

    let mut unknown_context = dummy_context("claude-3-7-sonnet");
    unknown_context.compatibility.github_copilot = None;
    let error = CopilotProvider::resolve_chat_api(&unknown_context).unwrap_err();
    assert!(error.to_string().contains("does not configure a chat API"));
}

#[test]
fn compatibility_profile_matches_canary_contract() {
    let contract: Value = serde_json::from_str(include_str!(
        "../../../../scripts/copilot-compatibility-profile.json"
    ))
    .expect("valid canary compatibility profile");
    let config = CopilotProviderConfig::new(
        "github_copilot".to_string(),
        CopilotAuthConfig::Bearer {
            token: "test-token".to_string(),
        },
    );
    let profile = VSCODE_CHAT_2026_06_01_PROFILE;

    assert_eq!(contract["name"], "vscode_chat_2026_06_01");
    assert_eq!(contract["editor_version"], config.editor_version);
    assert_eq!(contract["plugin_version"], profile.plugin_version);
    assert_eq!(contract["integration_id"], config.integration_id);
    assert_eq!(contract["intent"], profile.openai_intent);
    assert_eq!(contract["interaction_type"], profile.interaction_type);
    assert_eq!(contract["api_version"], profile.github_api_version);
    assert_eq!(contract["anthropic_version"], profile.anthropic_version);
}

#[test]
fn initiator_follows_the_last_conversation_item() {
    let mut chat = CoreChatRequest {
        model: "test".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!("new turn"),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: false,
        extra: BTreeMap::new(),
    };
    assert_eq!(CopilotInitiator::for_chat(&chat), CopilotInitiator::User);

    chat.messages[0].role = "assistant".to_string();
    assert_eq!(CopilotInitiator::for_chat(&chat), CopilotInitiator::Agent);

    chat.messages[0].role = "tool".to_string();
    assert_eq!(CopilotInitiator::for_chat(&chat), CopilotInitiator::Agent);

    chat.messages[0].role = "user".to_string();
    chat.messages[0].content = json!([{"type": "tool_result", "tool_use_id": "call-1"}]);
    assert_eq!(CopilotInitiator::for_chat(&chat), CopilotInitiator::Agent);

    let mut responses = CoreResponsesRequest {
        model: "test".to_string(),
        input: json!("new turn"),
        stream: false,
        instructions: None,
        tools: None,
        tool_choice: None,
        reasoning: None,
        text: None,
        extra: BTreeMap::new(),
    };
    assert_eq!(
        CopilotInitiator::for_responses(&responses),
        CopilotInitiator::User
    );
    responses.input = json!([{"type": "function_call_output", "call_id": "call-1"}]);
    assert_eq!(
        CopilotInitiator::for_responses(&responses),
        CopilotInitiator::Agent
    );

    responses.input = json!([{"type": "message", "role": "user", "content": "hello"}]);
    assert_eq!(
        CopilotInitiator::for_responses(&responses),
        CopilotInitiator::User
    );

    responses.input = json!([{
        "type": "message",
        "role": "user",
        "content": [{"type": "tool_result", "tool_use_id": "call-1"}]
    }]);
    assert_eq!(
        CopilotInitiator::for_responses(&responses),
        CopilotInitiator::Agent
    );
}

#[test]
fn embedding_response_normalization_preserves_upstream_metadata() {
    let response = normalize_embeddings_response(
        json!({
            "object": "custom.list",
            "model": "upstream-model",
            "data": []
        }),
        "requested-model",
    );

    assert_eq!(response["object"], "custom.list");
    assert_eq!(response["model"], "upstream-model");
}

#[tokio::test]
async fn embeddings_add_missing_openai_response_metadata() {
    let app = Router::new().route(
        "/embeddings",
        post(|| async {
            Json(json!({
                "data": [{
                    "object": "embedding",
                    "embedding": [0.25, 0.75],
                    "index": 0
                }],
                "usage": { "prompt_tokens": 1, "total_tokens": 1 }
            }))
        }),
    );

    let listener = TokioTcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let mut config = CopilotProviderConfig::new(
        "github_copilot".to_string(),
        CopilotAuthConfig::Bearer {
            token: "test-token".to_string(),
        },
    );
    config.base_url = format!("http://{addr}");
    let provider = CopilotProvider::new(config).unwrap();
    let request = CoreEmbeddingsRequest {
        model: "public-model-key".to_string(),
        input: json!("hello"),
        extra: BTreeMap::new(),
    };

    let response = provider
        .embeddings(&request, &dummy_context("copilot-embedding-model"))
        .await
        .unwrap();

    assert_eq!(response["object"], "list");
    assert_eq!(response["model"], "copilot-embedding-model");
    assert_eq!(response["data"][0]["embedding"], json!([0.25, 0.75]));
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
                    let profile_headers = [
                        "editor-plugin-version",
                        "openai-intent",
                        "x-interaction-type",
                        "x-initiator",
                    ]
                    .map(|name| {
                        headers
                            .get(name)
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or_default()
                            .to_string()
                    });

                    tx.send((
                        editor_version,
                        integration_id,
                        auth,
                        api_version,
                        req_id,
                        profile_headers,
                        body,
                    ))
                    .await
                    .expect("failed to send request data to test channel");

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

    let mut context = dummy_context("gpt-4o");
    context
        .extra_headers
        .insert("x-initiator".to_string(), json!("agent"));
    let response = provider.chat_completions(&request, &context).await.unwrap();

    assert_eq!(response["choices"][0]["message"]["content"], "Hello world");

    let (editor_version, integration_id, auth, api_version, req_id, profile_headers, body) =
        rx.recv().await.unwrap();
    assert_eq!(editor_version, "vscode/1.126.0");
    assert_eq!(integration_id, "vscode-chat");
    assert_eq!(auth, "Bearer test-ghs-token");
    assert_eq!(api_version, "2026-06-01");
    assert_eq!(req_id, "test-req-123");
    assert_eq!(profile_headers[0], DEFAULT_COPILOT_PLUGIN_VERSION);
    assert_eq!(profile_headers[1], "conversation-agent");
    assert_eq!(profile_headers[2], "conversation-agent");
    assert_eq!(profile_headers[3], "user");
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
                    let anthropic_version = headers
                        .get("anthropic-version")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    let plugin_version = headers
                        .get("editor-plugin-version")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    tx.send((auth, anthropic_version, plugin_version, body))
                        .await
                        .expect("failed to send message request data to test channel");

                    Json(json!({
                        "id": "msg-test",
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "thinking",
                                "thinking": "hidden reasoning",
                                "signature": "sig-test"
                            },
                            { "type": "text", "text": "Claude response" }
                        ],
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

    let context = messages_context("claude-3-7-sonnet");
    let response = provider.chat_completions(&request, &context).await.unwrap();

    assert_eq!(
        response["choices"][0]["message"]["content"],
        "Claude response"
    );

    let (auth, anthropic_version, plugin_version, body) = rx.recv().await.unwrap();
    assert_eq!(auth, "Bearer ghs_bearer");
    assert_eq!(anthropic_version, "2023-06-01");
    assert_eq!(plugin_version, DEFAULT_COPILOT_PLUGIN_VERSION);
    assert_eq!(body["model"], "claude-3-7-sonnet");
    assert_eq!(body["system"], "System prompt");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"][0]["text"], "Hello Claude");
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(
        response["choices"][0]["message"]["provider_metadata"]["github_copilot"]["reasoning"]["source"],
        "anthropic_messages"
    );
    assert!(response["choices"][0]["message"]["provider_metadata"]["aws_bedrock"].is_null());
}
#[tokio::test]
async fn applies_openai_compatibility_to_chat_requests() {
    let provider = CopilotProvider::new(CopilotProviderConfig::new(
        "github_copilot".to_string(),
        CopilotAuthConfig::Bearer {
            token: "test-token".to_string(),
        },
    ))
    .unwrap();
    let mut request = CoreChatRequest {
        model: "gpt-4o".to_string(),
        messages: vec![CoreChatMessage {
            role: "developer".to_string(),
            content: json!("Follow policy"),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: false,
        extra: BTreeMap::from([("max_completion_tokens".to_string(), json!(128))]),
    };
    request.extra.insert("store".to_string(), json!(true));
    let mut context = dummy_context("gpt-4o");
    context.compatibility.openai_compat = Some(OpenAiCompatRouteCompatibility {
        supports_store: false,
        max_tokens_field: OpenAiCompatMaxTokensField::MaxTokens,
        developer_role: OpenAiCompatDeveloperRole::System,
        ..Default::default()
    });

    let built = provider
        .build_chat_request(
            &request,
            &context,
            GitHubCopilotChatApi::ChatCompletions,
            false,
        )
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(
        built
            .body()
            .and_then(reqwest::Body::as_bytes)
            .expect("JSON request body"),
    )
    .unwrap();

    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["max_tokens"], 128);
    assert!(body.get("max_completion_tokens").is_none());
    assert!(body.get("store").is_none());
}

#[tokio::test]
async fn normalizes_responses_replay_ids() {
    let provider = CopilotProvider::new(CopilotProviderConfig::new(
        "github_copilot".to_string(),
        CopilotAuthConfig::Bearer {
            token: "test-token".to_string(),
        },
    ))
    .unwrap();
    let request = CoreResponsesRequest {
        model: "gpt-5.4".to_string(),
        input: json!([
            {
                "type": "function_call",
                "id": "foreign item id",
                "call_id": "foreign call id",
                "name": "lookup",
                "arguments": "{}"
            },
            {
                "type": "function_call_output",
                "call_id": "foreign call id",
                "output": "done"
            }
        ]),
        stream: false,
        instructions: None,
        tools: None,
        tool_choice: None,
        reasoning: None,
        text: None,
        extra: BTreeMap::new(),
    };

    let built = provider
        .build_responses_request(&request, &dummy_context("gpt-5.4"), false)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(
        built
            .body()
            .and_then(reqwest::Body::as_bytes)
            .expect("JSON request body"),
    )
    .unwrap();

    let function_call_id = body["input"][0]["id"].as_str().unwrap();
    let call_id = body["input"][0]["call_id"].as_str().unwrap();
    assert!(function_call_id.starts_with("fc_"));
    assert!(call_id.starts_with("call_"));
    assert_eq!(body["input"][1]["call_id"], call_id);
}

#[tokio::test]
async fn rejects_operations_missing_from_route_metadata() {
    let provider = CopilotProvider::new(CopilotProviderConfig::new(
        "github_copilot".to_string(),
        CopilotAuthConfig::Bearer {
            token: "test-token".to_string(),
        },
    ))
    .unwrap();
    let mut context = dummy_context("gpt-5");
    let compatibility = context
        .compatibility
        .github_copilot
        .as_mut()
        .expect("Copilot compatibility");
    compatibility.supports_responses = false;
    compatibility.supports_embeddings = false;
    compatibility.upstream_supports.streaming = false;

    let chat = CoreChatRequest {
        model: "gpt-5".to_string(),
        messages: vec![CoreChatMessage {
            role: "user".to_string(),
            content: json!("hello"),
            name: None,
            extra: BTreeMap::new(),
        }],
        stream: true,
        extra: BTreeMap::new(),
    };
    let error = provider
        .build_chat_request(&chat, &context, GitHubCopilotChatApi::ChatCompletions, true)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not support streaming"));

    let responses = CoreResponsesRequest {
        model: "gpt-5".to_string(),
        input: json!("hello"),
        stream: false,
        instructions: None,
        tools: None,
        tool_choice: None,
        reasoning: None,
        text: None,
        extra: BTreeMap::new(),
    };
    let error = provider
        .build_responses_request(&responses, &context, false)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not support responses"));

    let embeddings = CoreEmbeddingsRequest {
        model: "embedding".to_string(),
        input: json!("hello"),
        extra: BTreeMap::new(),
    };
    let error = provider
        .build_embeddings_request(&embeddings, &context)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not support embeddings"));
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
                    tx.send((auth, body))
                        .await
                        .expect("failed to send token request data to test channel");

                    let expires_at = (OffsetDateTime::now_utc() + time::Duration::hours(1))
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap();

                    Json(json!({
                        "token": "ghs_installation_token_abc",
                        "expires_at": expires_at,
                        "permissions": {
                            "copilot_requests": "write"
                        },
                        "repository_selection": "selected",
                        "repositories": [{ "id": 67890 }]
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

    let source = GitHubAppInstallationTokenSource::new(99999, TEST_RSA_PRIVATE_KEY, 12345, 67890)
        .unwrap()
        .with_api_url(format!("http://{addr}"));

    let token = source.fetch_token().await.unwrap();
    assert_eq!(token.token, "ghs_installation_token_abc");

    let (auth, body) = rx.recv().await.unwrap();
    assert!(auth.starts_with("Bearer ey"));
    assert_eq!(body["permissions"]["copilot_requests"], "write");
    assert_eq!(body["repository_ids"][0], 67890);
}

async fn token_source_with_response(response: Value) -> GitHubAppInstallationTokenSource {
    let app = Router::new().route(
        "/app/installations/12345/access_tokens",
        post(move || {
            let response = response.clone();
            async move { Json(response) }
        }),
    );

    let listener = TokioTcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    GitHubAppInstallationTokenSource::new(99999, TEST_RSA_PRIVATE_KEY, 12345, 67890)
        .unwrap()
        .with_api_url(format!("http://{addr}"))
}

fn installation_token_response() -> Value {
    let expires_at = (OffsetDateTime::now_utc() + time::Duration::hours(1))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();

    json!({
        "token": "ghs_installation_token_abc",
        "expires_at": expires_at,
        "permissions": {
            "copilot_requests": "write"
        },
        "repository_selection": "selected",
        "repositories": [{ "id": 67890 }]
    })
}

#[test]
fn github_app_installation_token_source_rejects_zero_repository_id() {
    let error = match GitHubAppInstallationTokenSource::new(99999, TEST_RSA_PRIVATE_KEY, 12345, 0) {
        Ok(_) => panic!("zero repository ID should fail"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("repository ID cannot be 0"));
}

#[tokio::test]
async fn github_app_installation_token_source_rejects_wrong_permission() {
    let mut response = installation_token_response();
    response["permissions"]["copilot_requests"] = json!("read");
    let source = token_source_with_response(response).await;

    let error = source
        .fetch_token()
        .await
        .expect_err("wrong Copilot permission should fail");

    assert!(matches!(&error, ProviderError::Transport(_)));
    assert!(error.to_string().contains("copilot_requests: write"));
}

#[tokio::test]
async fn github_app_installation_token_source_rejects_all_repository_selection() {
    let mut response = installation_token_response();
    response["repository_selection"] = json!("all");
    let source = token_source_with_response(response).await;

    let error = source
        .fetch_token()
        .await
        .expect_err("all-repository token selection should fail");

    assert!(matches!(&error, ProviderError::Transport(_)));
    assert!(
        error
            .to_string()
            .contains("unexpected repository selection")
    );
}

#[tokio::test]
async fn github_app_installation_token_source_accepts_lightweight_response() {
    let expires_at = (OffsetDateTime::now_utc() + time::Duration::hours(1))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let source = token_source_with_response(json!({
        "token": "ghs_installation_token_abc",
        "expires_at": expires_at
    }))
    .await;

    let token = source
        .fetch_token()
        .await
        .expect("GitHub can omit optional scope metadata from the response");

    assert_eq!(token.token, "ghs_installation_token_abc");
}

#[tokio::test]
async fn github_app_installation_token_source_rejects_wrong_repository_scope() {
    let mut response = installation_token_response();
    response["repositories"][0]["id"] = json!(98765);
    let source = token_source_with_response(response).await;

    let error = source
        .fetch_token()
        .await
        .expect_err("wrong repository scope should fail");

    assert!(matches!(&error, ProviderError::Transport(_)));
    assert!(
        error
            .to_string()
            .contains("does not match requested repository `67890`")
    );
}
