mod config;
mod http;
mod observability;

use std::{env, net::SocketAddr, path::Path, sync::Arc};

use admin_ui::AdminUiConfig;
use anyhow::Context;
use gateway_core::ProviderRegistry;
use gateway_providers::{OpenAiCompatProvider, VertexProvider};
use gateway_service::{GatewayService, WeightedRoutePlanner};
use gateway_store::{LibsqlStore, run_migrations};
use http::{build_router, state::AppState};
use tokio::net::TcpListener;

use crate::config::GatewayConfig;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config_path = env::var("GATEWAY_CONFIG").unwrap_or_else(|_| "./gateway.yaml".to_string());
    let config = GatewayConfig::from_path(Path::new(&config_path))
        .with_context(|| format!("failed to load gateway configuration from `{config_path}`"))?;

    observability::init_tracing(&config.server)?;

    run_migrations(&config.database.path)
        .await
        .context("failed to run database migrations")?;
    let store = Arc::new(
        LibsqlStore::new_local(&config.database.path)
            .await
            .context("failed to initialize local libsql store")?,
    );

    let providers_seed = config.seed_providers()?;
    let models_seed = config.seed_models()?;
    let api_keys_seed = config.seed_api_keys()?;

    store
        .seed_from_inputs(&providers_seed, &models_seed, &api_keys_seed)
        .await
        .context("failed to seed foundational config data")?;

    let planner = Arc::new(WeightedRoutePlanner::default());
    let service = Arc::new(GatewayService::new(store, planner));
    let providers = build_provider_registry(&config)?;

    let bind_address: SocketAddr = config
        .server
        .bind
        .parse()
        .with_context(|| format!("invalid bind address `{}`", config.server.bind))?;

    let app = build_router(AppState { service, providers }, load_admin_ui_config());

    let listener = TcpListener::bind(bind_address)
        .await
        .with_context(|| format!("failed binding gateway listener at `{bind_address}`"))?;

    tracing::info!(address = %bind_address, "gateway started");

    axum::serve(listener, app)
        .await
        .context("gateway server stopped unexpectedly")?;

    Ok(())
}

fn build_provider_registry(config: &GatewayConfig) -> anyhow::Result<ProviderRegistry> {
    let mut providers = ProviderRegistry::new();

    for provider_config in config.openai_compat_provider_configs()? {
        let provider = OpenAiCompatProvider::new(provider_config)
            .map_err(|error| anyhow::anyhow!("failed building openai_compat provider: {error}"))?;
        providers.register(Arc::new(provider));
    }

    for provider_config in config.vertex_provider_configs()? {
        let provider = VertexProvider::new(provider_config)
            .map_err(|error| anyhow::anyhow!("failed building gcp_vertex provider: {error}"))?;
        providers.register(Arc::new(provider));
    }

    Ok(providers)
}

fn load_admin_ui_config() -> AdminUiConfig {
    AdminUiConfig {
        base_path: env::var("ADMIN_UI_BASE_PATH").unwrap_or_else(|_| "/admin".to_string()),
        upstream: env::var("ADMIN_UI_UPSTREAM")
            .unwrap_or_else(|_| "http://localhost:3001".to_string()),
        connect_timeout_ms: env_u64("ADMIN_UI_CONNECT_TIMEOUT_MS", 750),
        request_timeout_ms: env_u64("ADMIN_UI_REQUEST_TIMEOUT_MS", 10_000),
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use admin_ui::AdminUiConfig;
    use async_trait::async_trait;
    use axum::{
        Router,
        body::{Body, Bytes, to_bytes},
        http::{Request, StatusCode},
    };
    use gateway_core::ChatCompletionsRequest;
    use gateway_core::{
        EmbeddingsRequest, ProviderCapabilities, ProviderClient, ProviderError,
        ProviderRequestContext, ProviderStream, SeedApiKey, SeedModel, SeedModelRoute,
        SeedProvider, parse_gateway_api_key,
    };
    use gateway_service::{GatewayService, WeightedRoutePlanner, hash_gateway_key_secret};
    use gateway_store::{LibsqlStore, run_migrations};
    use serde_json::{Map, Value, json};
    use serial_test::serial;
    use tempfile::tempdir;
    use tower::ServiceExt;

    use crate::http::{build_router, state::AppState};

    #[derive(Clone)]
    enum MockChatResult {
        Value(Value),
        Error(MockError),
    }

    #[derive(Clone)]
    enum MockError {
        UpstreamHttp(u16, String),
    }

    impl MockError {
        fn into_provider_error(self) -> ProviderError {
            match self {
                Self::UpstreamHttp(status, body) => ProviderError::UpstreamHttp { status, body },
            }
        }
    }

    #[derive(Clone)]
    struct MockProvider {
        key: String,
        provider_type: &'static str,
        caps: ProviderCapabilities,
        chat_result: MockChatResult,
        stream_chunks: Vec<String>,
        chat_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ProviderClient for MockProvider {
        fn provider_key(&self) -> &str {
            &self.key
        }

        fn provider_type(&self) -> &str {
            self.provider_type
        }

        fn capabilities(&self) -> ProviderCapabilities {
            self.caps
        }

        async fn chat_completions(
            &self,
            _request: &ChatCompletionsRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            self.chat_calls.fetch_add(1, Ordering::SeqCst);
            match self.chat_result.clone() {
                MockChatResult::Value(value) => Ok(value),
                MockChatResult::Error(error) => Err(error.into_provider_error()),
            }
        }

        async fn chat_completions_stream(
            &self,
            _request: &ChatCompletionsRequest,
            _context: &ProviderRequestContext,
        ) -> Result<ProviderStream, ProviderError> {
            let stream = futures_util::stream::iter(
                self.stream_chunks
                    .clone()
                    .into_iter()
                    .map(|chunk| Ok(Bytes::from(chunk))),
            );
            Ok(Box::pin(stream))
        }

        async fn embeddings(
            &self,
            _request: &EmbeddingsRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            Err(ProviderError::NotImplemented(
                "mock embeddings not implemented".to_string(),
            ))
        }
    }

    fn make_chat_provider(
        key: &str,
        chat_result: MockChatResult,
        stream_chunks: Vec<String>,
        caps: ProviderCapabilities,
    ) -> (Arc<AtomicUsize>, MockProvider) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            calls.clone(),
            MockProvider {
                key: key.to_string(),
                provider_type: "mock",
                caps,
                chat_result,
                stream_chunks,
                chat_calls: calls,
            },
        )
    }

    async fn build_test_app(
        seed_providers: Vec<SeedProvider>,
        models: Vec<SeedModel>,
        provider_registry: gateway_core::ProviderRegistry,
    ) -> (Router, String) {
        let tmp = tempdir().expect("tempdir");
        let tmp_path = tmp.keep();
        let db_path = tmp_path.join("gateway.db");

        run_migrations(&db_path).await.expect("migrations");

        let store = Arc::new(
            LibsqlStore::new_local(db_path.to_str().expect("db path"))
                .await
                .expect("store"),
        );

        let raw_key = "gwk_dev123.super-secret".to_string();
        let parsed = parse_gateway_api_key(&raw_key).expect("parse key");
        let api_keys = vec![SeedApiKey {
            name: "dev".to_string(),
            public_id: parsed.public_id,
            secret_hash: hash_gateway_key_secret(&parsed.secret).expect("hash"),
            allowed_models: vec!["fast".to_string()],
        }];

        store
            .seed_from_inputs(&seed_providers, &models, &api_keys)
            .await
            .expect("seed data");

        let service = Arc::new(GatewayService::new(
            store,
            Arc::new(WeightedRoutePlanner::seeded(11)),
        ));

        let app = build_router(
            AppState {
                service,
                providers: provider_registry,
            },
            AdminUiConfig::default(),
        );

        (app, raw_key)
    }

    async fn build_default_test_app(providers: gateway_core::ProviderRegistry) -> (Router, String) {
        let seed_providers = vec![SeedProvider {
            provider_key: "openai-prod".to_string(),
            provider_type: "openai_compat".to_string(),
            config: serde_json::json!({"base_url": "https://api.openai.com/v1"}),
            secrets: None,
        }];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            description: Some("Fast tier".to_string()),
            tags: vec!["fast".to_string()],
            rank: 10,
            routes: vec![SeedModelRoute {
                provider_key: "openai-prod".to_string(),
                upstream_model: "gpt-4o-mini".to_string(),
                priority: 10,
                weight: 1.0,
                enabled: true,
                extra_headers: Map::<String, Value>::new(),
                extra_body: Map::<String, Value>::new(),
            }],
        }];

        build_test_app(seed_providers, models, providers).await
    }

    #[tokio::test]
    #[serial]
    async fn api_routes_are_not_swallowed_by_ui_proxy() {
        let (app, _) = build_default_test_app(gateway_core::ProviderRegistry::new()).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[serial]
    async fn readyz_returns_ok() {
        let (app, _) = build_default_test_app(gateway_core::ProviderRegistry::new()).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[serial]
    async fn v1_models_are_auth_filtered() {
        let (app, raw_key) = build_default_test_app(gateway_core::ProviderRegistry::new()).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/models")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["object"], "list");
        assert_eq!(json["data"][0]["id"], "fast");
    }

    #[tokio::test]
    #[serial]
    async fn chat_completions_executes_resolved_provider() {
        let (calls, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_123",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}, "finish_reason":"stop"}]
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key) = build_default_test_app(registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        serde_json::json!({
                            "model": "fast",
                            "messages": [{"role": "user", "content": "ping"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["choices"][0]["message"]["content"], "pong");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    #[serial]
    async fn fallback_retries_when_idempotency_key_is_present() {
        let (primary_calls, primary_provider) = make_chat_provider(
            "primary",
            MockChatResult::Error(MockError::UpstreamHttp(503, "unavailable".to_string())),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let (fallback_calls, fallback_provider) = make_chat_provider(
            "fallback",
            MockChatResult::Value(json!({
                "id": "chatcmpl_fallback",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "from-fallback"}, "finish_reason":"stop"}]
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );

        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(primary_provider));
        registry.register(Arc::new(fallback_provider));

        let seed_providers = vec![
            SeedProvider {
                provider_key: "primary".to_string(),
                provider_type: "openai_compat".to_string(),
                config: serde_json::json!({"base_url":"https://example.invalid/v1"}),
                secrets: None,
            },
            SeedProvider {
                provider_key: "fallback".to_string(),
                provider_type: "openai_compat".to_string(),
                config: serde_json::json!({"base_url":"https://example.invalid/v1"}),
                secrets: None,
            },
        ];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            description: None,
            tags: vec![],
            rank: 10,
            routes: vec![
                SeedModelRoute {
                    provider_key: "primary".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                },
                SeedModelRoute {
                    provider_key: "fallback".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 20,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                },
            ],
        }];

        let (app, raw_key) = build_test_app(seed_providers, models, registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .header("idempotency-key", "idem-123")
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "messages": [{"role": "user", "content": "ping"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload["choices"][0]["message"]["content"], "from-fallback");
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    #[serial]
    async fn no_retry_without_idempotency_key() {
        let (primary_calls, primary_provider) = make_chat_provider(
            "primary",
            MockChatResult::Error(MockError::UpstreamHttp(503, "unavailable".to_string())),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );
        let (fallback_calls, fallback_provider) = make_chat_provider(
            "fallback",
            MockChatResult::Value(json!({
                "id": "chatcmpl_fallback",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "from-fallback"}, "finish_reason":"stop"}]
            })),
            vec![],
            ProviderCapabilities::openai_compat_baseline(),
        );

        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(primary_provider));
        registry.register(Arc::new(fallback_provider));

        let seed_providers = vec![
            SeedProvider {
                provider_key: "primary".to_string(),
                provider_type: "openai_compat".to_string(),
                config: serde_json::json!({"base_url":"https://example.invalid/v1"}),
                secrets: None,
            },
            SeedProvider {
                provider_key: "fallback".to_string(),
                provider_type: "openai_compat".to_string(),
                config: serde_json::json!({"base_url":"https://example.invalid/v1"}),
                secrets: None,
            },
        ];
        let models = vec![SeedModel {
            model_key: "fast".to_string(),
            description: None,
            tags: vec![],
            rank: 10,
            routes: vec![
                SeedModelRoute {
                    provider_key: "primary".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                },
                SeedModelRoute {
                    provider_key: "fallback".to_string(),
                    upstream_model: "gpt-4o-mini".to_string(),
                    priority: 20,
                    weight: 1.0,
                    enabled: true,
                    extra_headers: Map::new(),
                    extra_body: Map::new(),
                },
            ],
        }];

        let (app, raw_key) = build_test_app(seed_providers, models, registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "messages": [{"role": "user", "content": "ping"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    #[serial]
    async fn streaming_response_emits_done_terminator() {
        let (_, provider) = make_chat_provider(
            "openai-prod",
            MockChatResult::Value(json!({
                "id": "chatcmpl_123",
                "object": "chat.completion",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "unused"}, "finish_reason":"stop"}]
            })),
            vec![
                "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n".to_string(),
                "data: [DONE]\n\n".to_string(),
            ],
            ProviderCapabilities::new(true, true, true),
        );
        let mut registry = gateway_core::ProviderRegistry::new();
        registry.register(Arc::new(provider));

        let (app, raw_key) = build_default_test_app(registry).await;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {raw_key}"))
                    .body(Body::from(
                        json!({
                            "model": "fast",
                            "stream": true,
                            "messages": [{"role":"user","content":"ping"}]
                        })
                        .to_string(),
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let text = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(text.contains("data: [DONE]"));
    }
}
