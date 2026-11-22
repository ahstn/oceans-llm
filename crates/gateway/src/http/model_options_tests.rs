//! `GET /api/v1/admin/models/options` lists the whole catalog in one response, with the same
//! public fields for every role and none of the route detail only platform admins see.

use axum::{
    body::{Body, to_bytes},
    http::{HeaderValue, Request, StatusCode, header::COOKIE},
};
use gateway_core::{AuthMode, GlobalRole, SeedModel, UserStatus};
use gateway_store::GatewayStore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;
use uuid::Uuid;

use crate::http::{
    build_router,
    state::AppState,
    test_support::{app_state, seed_test_models, test_model},
};

const OPTIONS: &str = "/api/v1/admin/models/options";

/// More models than one page of `/api/v1/admin/models` holds, plus an alias.
const CATALOG_SIZE: usize = 120;

async fn seed_catalog(state: &AppState) {
    let mut models: Vec<SeedModel> = (0..CATALOG_SIZE)
        .map(|index| test_model(&format!("model-{index:03}"), "vertex", "upstream"))
        .collect();
    models.push(SeedModel {
        alias_target_model_key: Some("model-000".to_string()),
        routes: Vec::new(),
        ..test_model("friendly", "vertex", "unused")
    });
    seed_test_models(&state.store, &["vertex"], models).await;
}

/// An active user's session cookie.
async fn session_cookie(state: &AppState, role: GlobalRole) -> HeaderValue {
    let email = format!("{}@example.test", Uuid::new_v4());
    let user = state
        .store
        .create_identity_user(
            "Options tester",
            &email,
            &email,
            role,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("create user");
    let session_id = Uuid::new_v4();
    let token = format!("{session_id}.options-test-token");
    let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let now = OffsetDateTime::now_utc();
    state
        .store
        .create_user_session(
            session_id,
            user.user_id,
            &token_hash,
            now + Duration::hours(1),
            now,
        )
        .await
        .expect("create session");
    HeaderValue::from_str(&format!("ogw_session={token}")).expect("cookie")
}

async fn get(state: &AppState, uri: &str, cookie: Option<&HeaderValue>) -> (StatusCode, Value) {
    let mut request = Request::builder().uri(uri);
    if let Some(cookie) = cookie {
        request = request.header(COOKIE, cookie);
    }
    let response = build_router(state.clone(), admin_ui::AdminUiConfig::default())
        .oneshot(request.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn option<'a>(items: &'a [Value], key: &str) -> &'a Value {
    items
        .iter()
        .find(|item| item["id"] == key)
        .unwrap_or_else(|| panic!("{key} is listed"))
}

#[tokio::test]
async fn options_require_a_session() {
    let (_directory, state) = app_state().await;
    let (status, _) = get(&state, OPTIONS, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn options_list_the_whole_catalog_with_only_public_fields() {
    let (_directory, state) = app_state().await;
    seed_catalog(&state).await;

    for role in [GlobalRole::User, GlobalRole::PlatformAdmin] {
        let cookie = session_cookie(&state, role).await;
        let (status, body) = get(&state, OPTIONS, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK, "{role:?}");
        let items = body["data"]["items"].as_array().expect("items");
        assert_eq!(items.len(), CATALOG_SIZE + 1, "{role:?}");

        for item in items {
            let mut fields: Vec<&str> = item
                .as_object()
                .expect("item object")
                .keys()
                .map(String::as_str)
                .collect();
            fields.sort_unstable();
            assert_eq!(
                fields,
                [
                    "alias_of",
                    "id",
                    "model_id",
                    "provider_label",
                    "resolved_model_key"
                ],
                "{role:?}"
            );
        }

        let alias = option(items, "friendly");
        assert_eq!(alias["alias_of"], "model-000");
        assert_eq!(alias["resolved_model_key"], "model-000");
        let base = option(items, "model-000");
        assert!(base["alias_of"].is_null());
        assert_eq!(base["resolved_model_key"], "model-000");
        // An alias shows the provider of the model it resolves to.
        assert!(base["provider_label"].is_string());
        assert_eq!(alias["provider_label"], base["provider_label"]);
    }
}

#[tokio::test]
async fn options_match_the_paginated_listing() {
    let (_directory, state) = app_state().await;
    seed_catalog(&state).await;
    let cookie = session_cookie(&state, GlobalRole::User).await;

    let (_, options) = get(&state, OPTIONS, Some(&cookie)).await;
    let mut paged = Vec::new();
    for page in 1..=2 {
        let uri = format!("/api/v1/admin/models?page={page}&page_size=100");
        let (status, body) = get(&state, &uri, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);
        paged.extend(body["data"]["items"].as_array().expect("items").clone());
    }

    let project = |item: &Value| {
        [
            "id",
            "model_id",
            "resolved_model_key",
            "alias_of",
            "provider_label",
        ]
        .map(|field| item[field].clone())
    };
    let options: Vec<_> = options["data"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(project)
        .collect();
    let paged: Vec<_> = paged.iter().map(project).collect();
    assert_eq!(options, paged);
}
