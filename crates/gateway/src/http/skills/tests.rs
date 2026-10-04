use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{
    body::to_bytes,
    http::{Method, Request},
};
use gateway_core::{
    AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode, BudgetCadence,
    BudgetRepository, BudgetScope, BudgetSettings, GlobalRole, Money4, NewApiKeyRecord,
    SkillObjectStore, StoreError, UserStatus,
};
use gateway_service::SkillService;
use gateway_skills::{BundleLimits, pack_directory};
use gateway_store::GatewayStore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;

use super::*;

#[derive(Default)]
struct MemoryObjects(Mutex<HashMap<String, Vec<u8>>>);

#[async_trait::async_trait]
impl SkillObjectStore for MemoryObjects {
    async fn put(&self, key: &str, bytes: &[u8]) -> Result<(), StoreError> {
        self.0
            .lock()
            .unwrap()
            .insert(key.to_owned(), bytes.to_vec());
        Ok(())
    }

    async fn get(&self, key: &str, _max_bytes: u64) -> Result<Vec<u8>, StoreError> {
        self.0
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(key.into()))
    }

    async fn delete(&self, key: &str) -> Result<(), StoreError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

struct TestContext {
    _directory: tempfile::TempDir,
    state: AppState,
    objects: Arc<MemoryObjects>,
}

impl TestContext {
    async fn new() -> Self {
        let (directory, mut state) = crate::http::test_support::app_state().await;
        let objects = Arc::new(MemoryObjects::default());
        state.skills = Some(Arc::new(SkillService::new(
            state.store.clone(),
            objects.clone(),
            BundleLimits::default(),
        )));
        Self {
            _directory: directory,
            state,
            objects,
        }
    }

    async fn user(&self, role: GlobalRole) -> (Uuid, HeaderMap) {
        let email = format!("{}@example.test", Uuid::new_v4());
        let user = self
            .state
            .store
            .create_identity_user(
                "Skill author",
                &email,
                &email,
                role,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .unwrap();
        let session_id = Uuid::new_v4();
        let token = format!("{session_id}.skills-session");
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
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("ogw_session={token}").parse().unwrap(),
        );
        (user.user_id, headers)
    }

    async fn request(
        &self,
        method: Method,
        uri: &str,
        headers: &HeaderMap,
        content_type: &str,
        body: impl Into<Body>,
    ) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .body(body.into())
            .unwrap();
        *request.headers_mut() = headers.clone();
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type.parse().unwrap());
        router()
            .with_state(self.state.clone())
            .oneshot(request)
            .await
            .unwrap()
    }

    async fn json(
        &self,
        method: Method,
        uri: &str,
        headers: &HeaderMap,
        value: Value,
    ) -> (StatusCode, Value) {
        let response = self
            .request(method, uri, headers, "application/json", value.to_string())
            .await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn claim(&self, headers: &HeaderMap, handle: &str) {
        assert_eq!(
            self.json(
                Method::POST,
                "/api/v1/skills/namespace",
                headers,
                json!({"handle":handle})
            )
            .await
            .0,
            StatusCode::CREATED
        );
    }

    async fn upload(
        &self,
        headers: &HeaderMap,
        path: &str,
        name: &str,
        description: &str,
    ) -> (StatusCode, Value) {
        let response = self
            .request(
                Method::POST,
                path,
                headers,
                "application/zip",
                bundle(name, description),
            )
            .await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn api_key(&self, user_id: Option<Uuid>, service: Option<(Uuid, Uuid)>) -> HeaderMap {
        let public_id = Uuid::new_v4().simple().to_string();
        self.state
            .store
            .create_api_key(&NewApiKeyRecord {
                name: "Skills test".into(),
                public_id: public_id.clone(),
                secret_hash: gateway_core::hash_gateway_key_secret("test-secret").unwrap(),
                model_grant_mode: if user_id.is_some() {
                    ApiKeyModelGrantMode::All
                } else {
                    ApiKeyModelGrantMode::Explicit
                },
                owner_kind: if user_id.is_some() {
                    ApiKeyOwnerKind::User
                } else {
                    ApiKeyOwnerKind::ServiceAccount
                },
                owner_user_id: user_id,
                owner_team_id: service.map(|value| value.0),
                owner_service_account_id: service.map(|value| value.1),
                created_at: OffsetDateTime::now_utc(),
            })
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer gwk_{public_id}.test-secret")
                .parse()
                .unwrap(),
        );
        headers
    }
}

fn bundle(name: &str, description: &str) -> Vec<u8> {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join(name);
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\nUse the skill safely.\n"),
    )
    .unwrap();
    std::fs::write(root.join("example.txt"), "example contents").unwrap();
    pack_directory(&root, &BundleLimits::default())
        .unwrap()
        .archive
}

#[tokio::test]
async fn skills_share_names_across_owners_and_keep_versions_immutable() {
    let context = TestContext::new().await;
    let (_, author) = context.user(GlobalRole::User).await;
    let (_, other) = context.user(GlobalRole::PlatformAdmin).await;
    context.claim(&author, "author").await;
    context.claim(&other, "other").await;
    let (status, first) = context
        .upload(&author, "/api/v1/skills", "summarize", "Version one")
        .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    let timestamp = first["skill"]["created_at"]
        .as_str()
        .expect("RFC3339 creation time");
    OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339).unwrap();
    let id = first["skill"]["id"].as_str().unwrap();
    let path = format!("/api/v1/skills/{id}");
    assert_eq!(
        context
            .upload(&other, "/api/v1/skills", "summarize", "Other author")
            .await
            .0,
        StatusCode::CREATED
    );
    let (status, catalog) = context
        .json(Method::GET, "/api/v1/skills", &author, Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(catalog.as_array().unwrap().len(), 2);
    assert_eq!(
        context
            .upload(
                &other,
                &format!("{path}/versions"),
                "summarize",
                "Forbidden change"
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        context
            .json(
                Method::PUT,
                &format!("{path}/default-version"),
                &other,
                json!({"version":1})
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, second) = context
        .upload(
            &author,
            &format!("{path}/versions"),
            "summarize",
            "Version two",
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(second["skill"]["default_version"], 1);
    assert_eq!(second["skill"]["latest_version"], 2);
    let (status, changed) = context
        .json(
            Method::PUT,
            &format!("{path}/default-version"),
            &author,
            json!({"version":2}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(changed["skill"]["description"], "Version two");
    let (_, version_one) = context
        .json(
            Method::GET,
            &format!("{path}/versions/1"),
            &other,
            Value::Null,
        )
        .await;
    assert_eq!(version_one["manifest"]["description"], "Version one");
    assert_eq!(
        context
            .upload(
                &author,
                &format!("{path}/versions"),
                "renamed",
                "Changed name"
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        context
            .json(
                Method::PUT,
                &format!("{path}/default-version"),
                &author,
                json!({"version":99})
            )
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        context
            .json(
                Method::GET,
                "/api/v1/skills/by-name/author/summarize",
                &other,
                Value::Null
            )
            .await
            .1["skill"]["id"],
        id
    );
}

#[tokio::test]
async fn failed_duplicate_upload_cleans_up_and_archive_preview_is_bounded() {
    let context = TestContext::new().await;
    let (_, author) = context.user(GlobalRole::User).await;
    context.claim(&author, "author").await;
    let (_, first) = context
        .upload(&author, "/api/v1/skills", "summarize", "Version one")
        .await;
    let path = format!("/api/v1/skills/{}", first["skill"]["id"].as_str().unwrap());
    let (_, version_one) = context
        .json(
            Method::GET,
            &format!("{path}/versions/1"),
            &author,
            Value::Null,
        )
        .await;
    // Duplicate creation must not retain the staged object, or replace the first bundle.
    let count = context.objects.0.lock().unwrap().len();
    assert_eq!(
        context
            .upload(&author, "/api/v1/skills", "summarize", "Duplicate")
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(context.objects.0.lock().unwrap().len(), count);
    let preview = context
        .json(
            Method::GET,
            &format!("{path}/versions/1/files?path=example.txt"),
            &author,
            Value::Null,
        )
        .await;
    assert_eq!(preview.1["content"], "example contents");
    assert_eq!(
        context
            .json(
                Method::GET,
                &format!("{path}/versions/1/files?path=../secret"),
                &author,
                Value::Null
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let archive = context
        .request(
            Method::GET,
            &format!("{path}/versions/1/archive"),
            &author,
            "application/json",
            Body::empty(),
        )
        .await;
    assert_eq!(archive.status(), StatusCode::OK);
    assert_eq!(
        archive.headers()["x-skill-sha256"],
        version_one["version"]["sha256"].as_str().unwrap()
    );
}

#[tokio::test]
async fn skill_auth_requires_active_users_and_service_accounts_are_read_only() {
    let context = TestContext::new().await;
    let (user_id, session) = context.user(GlobalRole::User).await;
    let headers = context.api_key(Some(user_id), None).await;
    context.claim(&headers, "key-owner").await;
    assert_eq!(
        context
            .upload(&headers, "/api/v1/skills", "key-skill", "Key upload")
            .await
            .0,
        StatusCode::CREATED
    );
    context
        .state
        .store
        .update_user_status(user_id, UserStatus::Disabled, OffsetDateTime::now_utc())
        .await
        .unwrap();
    assert_eq!(
        context
            .json(Method::GET, "/api/v1/skills", &headers, Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        context
            .json(Method::GET, "/api/v1/skills", &session, Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        context
            .json(
                Method::GET,
                "/api/v1/skills",
                &HeaderMap::new(),
                Value::Null
            )
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );

    let team = context
        .state
        .store
        .create_team("skill-team", "Skill team")
        .await
        .unwrap();
    let account = context
        .state
        .store
        .create_service_account(team.team_id, "reader", "Reader", OffsetDateTime::now_utc())
        .await
        .unwrap();
    let headers = context
        .api_key(None, Some((team.team_id, account.service_account_id)))
        .await;
    assert_eq!(
        context
            .json(Method::GET, "/api/v1/skills", &headers, Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    context
        .state
        .store
        .upsert_active_budget(
            &BudgetScope::ServiceAccount {
                service_account_id: account.service_account_id,
            },
            &BudgetSettings {
                cadence: BudgetCadence::Monthly,
                amount_usd: Money4::from_scaled(10_000),
                hard_limit: true,
                timezone: "UTC".into(),
            },
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();
    assert_eq!(
        context
            .json(Method::GET, "/api/v1/skills", &headers, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        context
            .json(
                Method::POST,
                "/api/v1/skills/namespace",
                &headers,
                json!({"handle":"service"})
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        context
            .upload(&headers, "/api/v1/skills", "sa-skill", "Forbidden")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn skill_uploads_bound_input_and_namespace_is_claimed_once() {
    let mut context = TestContext::new().await;
    let (_, author) = context.user(GlobalRole::User).await;
    let (_, other) = context.user(GlobalRole::User).await;
    context.claim(&author, "author").await;
    assert_eq!(
        context
            .json(
                Method::POST,
                "/api/v1/skills/namespace",
                &author,
                json!({"handle":"renamed"})
            )
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        context
            .json(
                Method::POST,
                "/api/v1/skills/namespace",
                &other,
                json!({"handle":"author"})
            )
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        context
            .json(
                Method::POST,
                "/api/v1/skills/namespace",
                &other,
                json!({"handle":"../bad"})
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        context
            .request(
                Method::POST,
                "/api/v1/skills",
                &author,
                "text/plain",
                "not a zip"
            )
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        context
            .request(
                Method::POST,
                "/api/v1/skills",
                &author,
                "application/zip",
                "not a zip"
            )
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let limits = BundleLimits {
        max_archive_bytes: 8,
        ..BundleLimits::default()
    };
    context.state.skills = Some(Arc::new(SkillService::new(
        context.state.store.clone(),
        context.objects.clone(),
        limits,
    )));
    assert_eq!(
        context
            .request(
                Method::POST,
                "/api/v1/skills",
                &author,
                "application/zip",
                vec![0; 9]
            )
            .await
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        context
            .request(
                Method::POST,
                "/api/v1/skills",
                &HeaderMap::new(),
                "application/zip",
                vec![0; 9]
            )
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let (status, configured) = context
        .json(Method::GET, "/api/v1/skills/limits", &author, Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(configured["max_archive_bytes"], 8);
    assert_eq!(
        context
            .json(
                Method::GET,
                "/api/v1/skills/limits",
                &HeaderMap::new(),
                Value::Null
            )
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    context.state.skills = None;
    assert_eq!(
        context
            .json(Method::GET, "/api/v1/skills", &author, Value::Null)
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn concurrent_upload_responses_identify_their_own_versions() {
    let context = TestContext::new().await;
    let (_, author) = context.user(GlobalRole::User).await;
    context.claim(&author, "author").await;
    let (_, first) = context
        .upload(&author, "/api/v1/skills", "summarize", "First")
        .await;
    assert_eq!(first["uploaded_version"], 1);
    let path = format!("/api/v1/skills/{}", first["skill"]["id"].as_str().unwrap());
    let versions = format!("{path}/versions");
    let (left, right) = tokio::join!(
        context.upload(&author, &versions, "summarize", "Left"),
        context.upload(&author, &versions, "summarize", "Right"),
    );
    assert_eq!(left.0, StatusCode::CREATED, "{}", left.1);
    assert_eq!(right.0, StatusCode::CREATED, "{}", right.1);
    assert_ne!(left.1["uploaded_version"], right.1["uploaded_version"]);
    for (response, description) in [(left.1, "Left"), (right.1, "Right")] {
        let version = response["uploaded_version"].as_u64().unwrap();
        let (_, stored) = context
            .json(
                Method::GET,
                &format!("{versions}/{version}"),
                &author,
                Value::Null,
            )
            .await;
        assert_eq!(stored["manifest"]["description"], description);
        assert_eq!(response["skill"]["default_version"], 1);
    }
}
