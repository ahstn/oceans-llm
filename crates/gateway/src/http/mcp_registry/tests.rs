use std::{collections::BTreeSet, sync::Arc, time::Duration as StdDuration};

use axum::{
    body::to_bytes,
    http::{HeaderValue, StatusCode, header::COOKIE},
    response::IntoResponse,
};
use gateway_core::{
    AuthMode, ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpTransport, GlobalRole, McpRegistryRepository, NewExternalMcpServerRecord,
    ProviderRegistry, SeedHumanBudgetDefaults, UpsertExternalMcpToolRecord, UserStatus,
};
use gateway_guardrails::{GuardrailConfig, GuardrailEngine};
use gateway_service::{GatewayService, McpOauthRuntime, WeightedRoutePlanner};
use gateway_store::{AnyStore, GatewayStore, StoreConnectionOptions, run_migrations_with_options};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::Duration;

use super::*;
use crate::{
    config::{AgentAnalysisRuntimeCapabilities, PermissionsConfig},
    http::response_cache::ResponseCache,
    observability::GatewayMetrics,
};

struct TestContext {
    state: AppState,
    _directory: tempfile::TempDir,
}

impl TestContext {
    async fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary database directory");
        let options = StoreConnectionOptions::Libsql {
            path: directory.path().join("gateway.db"),
        };
        run_migrations_with_options(&options)
            .await
            .expect("migrate database");
        let store = Arc::new(AnyStore::connect(&options).await.expect("connect database"));
        let state = AppState {
            service: Arc::new(GatewayService::new(
                store.clone(),
                Arc::new(WeightedRoutePlanner::default()),
            )),
            store,
            providers: ProviderRegistry::new(),
            copilot_user_provider_keys: Arc::new(Vec::new()),
            guardrail_engine: Arc::new(GuardrailEngine::new(Vec::new(), Default::default())),
            guardrail_config: Arc::new(GuardrailConfig::default()),
            metrics: Arc::new(GatewayMetrics::new()),
            mcp_http_client: reqwest::Client::new(),
            mcp_oauth_runtime: Arc::new(McpOauthRuntime::new(None, Vec::new())),
            identity_token_secret: Arc::new("membership-test-secret".to_string()),
            oidc_public_base_url: Arc::new(None),
            oauth_public_base_url: Arc::new(None),
            client_config_gateway_base_url: Arc::new(None),
            budget_defaults: Arc::new(SeedHumanBudgetDefaults::default()),
            agent_analysis: AgentAnalysisRuntimeCapabilities {
                passive_analysis_enabled: false,
                shadow_diagnostics_visible: false,
                calibrated_score_visible: false,
                team_admin_analytics_enabled: false,
            },
            admin_permissions: Arc::new(
                PermissionsConfig::default().resolve().expect("permissions"),
            ),
            leaderboard_cache: Arc::new(ResponseCache::new(StdDuration::from_secs(1))),
            harness_usage_cache: Arc::new(ResponseCache::new(StdDuration::from_secs(1))),
        };
        Self {
            state,
            _directory: directory,
        }
    }

    async fn session_headers(&self, role: GlobalRole) -> HeaderMap {
        let email = format!("{}@example.test", Uuid::new_v4());
        let user = self
            .state
            .store
            .create_identity_user(
                "Membership reader",
                &email,
                &email,
                role,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("create session user");
        let session_id = Uuid::new_v4();
        // Session validation checks the stored token hash after extracting the UUID.
        let token = format!("{session_id}.membership-test-token");
        let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        let now = OffsetDateTime::now_utc();
        self.state
            .store
            .create_user_session(
                session_id,
                user.user_id,
                &token_hash,
                now + Duration::hours(1),
                now,
            )
            .await
            .expect("create authenticated session");
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("ogw_session={token}")).expect("session cookie"),
        );
        headers
    }

    async fn create_toolset(&self) -> Uuid {
        self.state
            .store
            .create_mcp_toolset(&NewMcpToolsetRecord {
                toolset_key: "engineering".to_string(),
                display_name: "Engineering".to_string(),
                description: None,
                created_at: OffsetDateTime::now_utc(),
            })
            .await
            .expect("create tool set")
            .toolset_id
    }

    async fn get(&self, id: &str, headers: HeaderMap) -> (StatusCode, Value) {
        let response =
            list_mcp_toolset_tools(State(self.state.clone()), headers, Path(id.to_string())).await;
        read_json_response(response).await
    }

    async fn connection_info(&self, headers: HeaderMap) -> (StatusCode, Value) {
        let response = get_mcp_connection_info(State(self.state.clone()), headers).await;
        read_json_response(response).await
    }
}

async fn read_json_response(response: impl IntoResponse) -> (StatusCode, Value) {
    let response = response.into_response();
    let status = response.status();
    let body = to_bytes(response.into_body(), 16_384)
        .await
        .expect("read response body");
    (
        status,
        serde_json::from_slice(&body).expect("JSON response"),
    )
}

#[tokio::test]
async fn connection_info_requires_a_platform_admin_session() {
    let context = TestContext::new().await;
    let (status, body) = context.connection_info(HeaderMap::new()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.get("data").is_none());

    let headers = context.session_headers(GlobalRole::User).await;
    let (status, body) = context.connection_info(headers).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.get("data").is_none());
}

#[tokio::test]
async fn connection_info_uses_the_default_gateway_endpoint() {
    let context = TestContext::new().await;
    let headers = context.session_headers(GlobalRole::PlatformAdmin).await;
    let (status, body) = context.connection_info(headers).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["endpoint"], "http://127.0.0.1:3000/mcp");
}

#[tokio::test]
async fn connection_info_preserves_deployment_prefix_and_removes_api_suffix() {
    let mut context = TestContext::new().await;
    let headers = context.session_headers(GlobalRole::PlatformAdmin).await;
    for (configured, expected) in [
        ("https://gateway.example", "https://gateway.example/mcp"),
        (
            "https://gateway.example/prefix/v1///",
            "https://gateway.example/prefix/mcp",
        ),
        (
            "https://gateway.example/prefix/",
            "https://gateway.example/prefix/mcp",
        ),
    ] {
        context.state.client_config_gateway_base_url = Arc::new(Some(configured.to_string()));
        let (status, body) = context.connection_info(headers.clone()).await;
        assert_eq!(status, StatusCode::OK, "configured base: {configured}");
        assert_eq!(body["data"]["endpoint"], expected);
        let configurations = body["data"]["client_configurations"]
            .as_array()
            .expect("client configurations");
        assert_eq!(configurations.len(), 2);
        for (key, label, token_reference) in [
            ("claude-code", "Claude Code", "Bearer ${OCEANS_LLM_API_KEY}"),
            ("codex", "Codex", "bearer_token_env_var"),
        ] {
            let config = configurations
                .iter()
                .find(|config| config["key"] == key)
                .expect("expected MCP client configuration");
            assert_eq!(config["label"], label);
            assert_eq!(config["model_ids"], json!([]));
            let blocks = config["blocks"].as_array().expect("configuration blocks");
            assert!(blocks.iter().any(|block| {
                let content = block["content"].as_str().expect("configuration content");
                content.contains(expected)
                    && content.contains(token_reference)
                    && content.contains("OCEANS_LLM_API_KEY")
            }));
        }
    }
}

#[tokio::test]
async fn connection_info_rejects_unsafe_config_without_exposing_secrets() {
    let mut context = TestContext::new().await;
    let headers = context.session_headers(GlobalRole::PlatformAdmin).await;
    for configured in [
        "https://private-user:private-secret@gateway.example/prefix",
        "https://gateway.example/prefix?token=private-secret",
        "https://gateway.example/prefix#private-secret",
    ] {
        context.state.client_config_gateway_base_url = Arc::new(Some(configured.to_string()));
        let (status, body) = context.connection_info(headers.clone()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.get("data").is_none());
        assert!(body["error"]["message"].is_string());
        let response = body.to_string();
        assert!(!response.contains("private-secret"));
        assert!(!response.contains("private-user"));
        assert!(!response.contains(configured));
    }
}

#[tokio::test]
async fn membership_requires_a_platform_admin_session() {
    let context = TestContext::new().await;
    let id = context.create_toolset().await.to_string();
    let (status, _) = context.get(&id, HeaderMap::new()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let headers = context.session_headers(GlobalRole::User).await;
    let (status, _) = context.get(&id, headers).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn membership_distinguishes_invalid_and_missing_toolsets() {
    let context = TestContext::new().await;
    let headers = context.session_headers(GlobalRole::PlatformAdmin).await;
    let (status, _) = context.get("not-a-uuid", headers.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = context.get(&Uuid::new_v4().to_string(), headers).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn membership_returns_an_empty_array_for_active_and_disabled_sets() {
    let context = TestContext::new().await;
    let headers = context.session_headers(GlobalRole::PlatformAdmin).await;
    let id = context.create_toolset().await;
    let (status, body) = context.get(&id.to_string(), headers.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"], json!({"tool_ids": []}));

    context
        .state
        .store
        .disable_mcp_toolset(id, OffsetDateTime::now_utc())
        .await
        .expect("disable tool set");
    let (status, body) = context.get(&id.to_string(), headers).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"], json!({"tool_ids": []}));
}

#[tokio::test]
async fn membership_returns_persisted_ids_including_inactive_tools() {
    let context = TestContext::new().await;
    let store = &context.state.store;
    let now = OffsetDateTime::now_utc();
    let server = store
        .create_external_mcp_server(&NewExternalMcpServerRecord {
            server_key: "repository".to_string(),
            display_name: "Repository".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://example.test/mcp".to_string(),
            auth_mode: ExternalMcpAuthMode::None,
            auth_config: Default::default(),
            timeout_ms: 5_000,
            created_at: now,
        })
        .await
        .expect("create server");
    let tools: Vec<_> = ["search", "legacy_search"]
        .into_iter()
        .map(|name| UpsertExternalMcpToolRecord {
            mcp_server_id: server.mcp_server_id,
            upstream_name: name.to_string(),
            display_name: name.to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
            schema_hash: "schema-hash".to_string(),
        })
        .collect();
    let mut run = ExternalMcpDiscoveryRunRecord {
        discovery_run_id: Uuid::new_v4(),
        mcp_server_id: server.mcp_server_id,
        status: ExternalMcpDiscoveryStatus::Success,
        started_at: now,
        finished_at: now,
        discovered_tool_count: 2,
        active_tool_count: 2,
        schema_set_hash: None,
        error_summary: None,
        details: Default::default(),
    };
    let discovered = store
        .record_external_mcp_discovery_success(&run, &tools)
        .await
        .expect("discover tools");
    let tool_ids: Vec<_> = discovered.iter().map(|tool| tool.mcp_tool_id).collect();
    let toolset_id = context.create_toolset().await;
    store
        .replace_mcp_toolset_tools(toolset_id, &tool_ids, now)
        .await
        .expect("save membership");

    run.discovery_run_id = Uuid::new_v4();
    run.discovered_tool_count = 1;
    run.active_tool_count = 1;
    store
        .record_external_mcp_discovery_success(&run, &tools[..1])
        .await
        .expect("mark removed tool inactive");
    assert_eq!(
        store
            .list_external_mcp_tools(server.mcp_server_id, false)
            .await
            .expect("active tools")
            .len(),
        1
    );

    let headers = context.session_headers(GlobalRole::PlatformAdmin).await;
    let (status, body) = context.get(&toolset_id.to_string(), headers).await;
    assert_eq!(status, StatusCode::OK);
    let returned: BTreeSet<String> =
        serde_json::from_value(body["data"]["tool_ids"].clone()).expect("membership IDs");
    assert_eq!(returned, tool_ids.iter().map(Uuid::to_string).collect());
}
