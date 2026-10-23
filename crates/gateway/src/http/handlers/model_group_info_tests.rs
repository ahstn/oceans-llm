//! LiteLLM's `GET /model_group/info` through the real router: the same
//! authentication and scope as `/v1/models`, and the LiteLLM row shape.

use std::collections::BTreeSet;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use gateway_core::{AdminApiKeyRepository, ApiKeyAccess, ApiKeyRepository};
use serde_json::Value;
use tower::ServiceExt;

use crate::http::{
    build_router,
    state::AppState,
    test_support::{app_state, seed_stream_cancellation_test},
};

const PATH: &str = "/model_group/info";
const KEY: &str = "gwk_streamtest.cancel-secret";

const REQUIRED_FIELDS: [&str; 10] = [
    "model_group",
    "providers",
    "mode",
    "max_input_tokens",
    "max_output_tokens",
    "supports_function_calling",
    "supports_vision",
    "supports_reasoning",
    "supports_response_schema",
    "supported_openai_params",
];

const COST_FIELDS: [&str; 4] = [
    "input_cost_per_token",
    "output_cost_per_token",
    "cache_read_input_token_cost",
    "cache_creation_input_token_cost",
];

async fn seeded_router() -> (tempfile::TempDir, AppState, Router) {
    let (directory, state) = app_state().await;
    seed_stream_cancellation_test(&state.store).await;
    let router = build_router(state.clone(), admin_ui::AdminUiConfig::default());
    (directory, state, router)
}

async fn get(router: &Router, path: &str, key: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder().uri(path);
    if let Some(key) = key {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn model_group_info_requires_an_inference_key() {
    let (_directory, state, router) = seeded_router().await;
    let (status, body) = get(&router, PATH, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    let key = state
        .store
        .get_api_key_by_public_id("streamtest")
        .await
        .unwrap()
        .expect("seeded key");
    state
        .store
        .replace_api_key_access(
            key.id,
            &ApiKeyAccess {
                scopes: Some(vec![gateway_core::SCOPE_MCP_PROXY.to_string()]),
                mcp_server_keys: None,
            },
        )
        .await
        .unwrap();
    for path in [PATH, "/v1/models"] {
        let (status, body) = get(&router, path, Some(KEY)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        assert_eq!(
            body["error"]["code"], "insufficient_scope",
            "{path}: {body}"
        );
    }
}

#[tokio::test]
async fn model_group_info_lists_the_visible_models_in_litellm_shape() {
    let (_directory, _state, router) = seeded_router().await;
    let (status, models) = get(&router, "/v1/models", Some(KEY)).await;
    assert_eq!(status, StatusCode::OK, "{models}");
    let (status, info) = get(&router, PATH, Some(KEY)).await;
    assert_eq!(status, StatusCode::OK, "{info}");

    let ids = |body: &Value, field: &str| -> Vec<String> {
        body["data"]
            .as_array()
            .unwrap_or_else(|| panic!("no data in {body}"))
            .iter()
            .map(|row| row[field].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(ids(&info, "model_group"), ids(&models, "id"));
    assert_eq!(ids(&info, "model_group"), ["fast"]);

    let row = info["data"][0].as_object().unwrap();
    let keys: BTreeSet<&str> = row.keys().map(String::as_str).collect();
    let required: BTreeSet<&str> = REQUIRED_FIELDS.into_iter().collect();
    let costs: BTreeSet<&str> = COST_FIELDS.into_iter().collect();
    assert!(keys.is_superset(&required), "{info}");
    let extra: BTreeSet<&str> = keys.difference(&required).copied().collect();
    // Only cost fields are optional. Input and output come together, and a cache cost
    // only comes with them.
    assert!(extra.is_subset(&costs), "{info}");
    let priced = extra.contains("input_cost_per_token");
    assert_eq!(priced, extra.contains("output_cost_per_token"), "{info}");
    assert!(priced || extra.is_empty(), "{info}");
    assert!(
        extra
            .iter()
            .all(|field| row[*field].as_f64().is_some_and(|cost| cost >= 0.0)),
        "{info}"
    );

    let providers = row["providers"].as_array().unwrap();
    assert!(!providers.is_empty(), "{info}");
    assert!(providers.iter().all(|provider| {
        provider
            .as_str()
            .is_some_and(|name| !name.is_empty() && name == name.to_lowercase())
    }));
    assert_eq!(row["mode"], "chat");
    for field in ["max_input_tokens", "max_output_tokens"] {
        assert!(
            row[field].is_null() || row[field].is_u64(),
            "{field}: {info}"
        );
    }
    for field in [
        "supports_function_calling",
        "supports_vision",
        "supports_reasoning",
        "supports_response_schema",
    ] {
        assert!(row[field].is_boolean(), "{field}: {info}");
    }
    assert!(
        row["supported_openai_params"]
            .as_array()
            .unwrap()
            .iter()
            .all(Value::is_string),
        "{info}"
    );
}
