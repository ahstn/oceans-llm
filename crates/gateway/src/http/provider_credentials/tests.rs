use std::{ffi::OsString, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    http::{HeaderValue, Method, Request, header},
};
use gateway_core::{
    AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode, BudgetCadence,
    BudgetRepository, BudgetScope, BudgetSettings, GlobalRole, Money4, NewApiKeyRecord,
    ProviderUserCredentialRepository, ProviderUserTokenResolver, SeedProvider,
};
use gateway_service::PROVIDER_CREDENTIAL_KEY_ENV;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use tower::ServiceExt;

use super::*;

const COLLECTION: &str = "/api/v1/me/provider-credentials";
const PROVIDER_KEY: &str = "personal-copilot";
const CREDENTIAL: &str = "/api/v1/me/provider-credentials/personal-copilot";

struct EncryptionKeyGuard(Option<OsString>);

impl EncryptionKeyGuard {
    fn new() -> Self {
        let previous = std::env::var_os(PROVIDER_CREDENTIAL_KEY_ENV);
        unsafe {
            std::env::set_var(
                PROVIDER_CREDENTIAL_KEY_ENV,
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            );
        }
        Self(previous)
    }
}

impl Drop for EncryptionKeyGuard {
    fn drop(&mut self) {
        unsafe {
            match self.0.take() {
                Some(value) => std::env::set_var(PROVIDER_CREDENTIAL_KEY_ENV, value),
                None => std::env::remove_var(PROVIDER_CREDENTIAL_KEY_ENV),
            }
        }
    }
}

struct TestContext {
    _directory: tempfile::TempDir,
    state: AppState,
}

impl TestContext {
    async fn new() -> Self {
        let (directory, mut state) = crate::http::test_support::app_state().await;
        state
            .store
            .seed_from_inputs(
                &[
                    SeedProvider {
                        provider_key: PROVIDER_KEY.into(),
                        provider_type: "github_copilot".into(),
                        config: json!({"auth_mode": "github_user"}),
                        secrets: None,
                    },
                    SeedProvider {
                        provider_key: "app-copilot".into(),
                        provider_type: "github_copilot".into(),
                        config: json!({"auth_mode": "github_app"}),
                        secrets: None,
                    },
                ],
                &[],
                &[],
                &[],
                &[],
                &[],
                &[],
                &[],
            )
            .await
            .unwrap();
        state.copilot_user_provider_keys = Arc::new(vec![PROVIDER_KEY.into()]);
        Self {
            _directory: directory,
            state,
        }
    }

    async fn user(&self, role: GlobalRole) -> (Uuid, Uuid, HeaderMap) {
        let email = format!("{}@example.test", Uuid::new_v4());
        let user = self
            .state
            .store
            .create_identity_user(
                "Credential owner",
                &email,
                &email,
                role,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .unwrap();
        let (key_id, headers) = self.key(Some(user.user_id), None).await;
        (user.user_id, key_id, headers)
    }

    async fn key(&self, user_id: Option<Uuid>, service: Option<(Uuid, Uuid)>) -> (Uuid, HeaderMap) {
        let public_id = Uuid::new_v4().simple().to_string();
        let key = self
            .state
            .store
            .create_api_key(&NewApiKeyRecord {
                name: "Credential test".into(),
                public_id: public_id.clone(),
                secret_hash: gateway_core::hash_gateway_key_secret("test-secret").unwrap(),
                model_grant_mode: ApiKeyModelGrantMode::Explicit,
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
            AUTHORIZATION,
            format!("Bearer gwk_{public_id}.test-secret")
                .parse()
                .unwrap(),
        );
        (key.id, headers)
    }

    async fn session(&self, user_id: Uuid) -> HeaderValue {
        let session_id = Uuid::new_v4();
        let token = format!("{session_id}.credential-session");
        let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
        let now = OffsetDateTime::now_utc();
        self.state
            .store
            .create_user_session(
                session_id,
                user_id,
                &token_hash,
                now + Duration::hours(1),
                now,
            )
            .await
            .unwrap();
        format!("ogw_session={token}").parse().unwrap()
    }

    async fn service_account_key(&self) -> HeaderMap {
        let team = self
            .state
            .store
            .create_team("credential-team", "Credential team")
            .await
            .unwrap();
        let account = self
            .state
            .store
            .create_service_account(
                team.team_id,
                "credential-service",
                "Credential service",
                OffsetDateTime::now_utc(),
            )
            .await
            .unwrap();
        self.state
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
        self.key(None, Some((team.team_id, account.service_account_id)))
            .await
            .1
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        headers: &HeaderMap,
        body: impl Into<Body>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .body(body.into())
            .unwrap();
        *request.headers_mut() = headers.clone();
        request.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        let response =
            crate::http::build_router(self.state.clone(), admin_ui::AdminUiConfig::default())
                .oneshot(request)
                .await
                .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn put(&self, headers: &HeaderMap, token: &str) -> (StatusCode, Value) {
        self.request(
            Method::PUT,
            CREDENTIAL,
            headers,
            json!({"token": token}).to_string(),
        )
        .await
    }

    async fn list(&self, headers: &HeaderMap) -> (StatusCode, Value) {
        self.request(Method::GET, COLLECTION, headers, Body::empty())
            .await
    }
}

#[tokio::test]
#[serial_test::serial]
async fn saves_and_replaces_only_the_bearer_owners_token() {
    let _key = EncryptionKeyGuard::new();
    let context = TestContext::new().await;
    let (alice, _, mut alice_key) = context.user(GlobalRole::User).await;
    let (bob, _, bob_key) = context.user(GlobalRole::PlatformAdmin).await;
    alice_key.insert(header::COOKIE, context.session(bob).await);
    assert_eq!(
        context.put(&bob_key, "bob-private-token").await.0,
        StatusCode::OK
    );
    assert_eq!(
        context.list(&alice_key).await.1,
        json!([{
            "provider_key": PROVIDER_KEY, "configured": false, "updated_at": null, "last_used_at": null,
        }])
    );

    let (status, saved) = context.put(&alice_key, " alice-first-token\n").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["provider_key"], PROVIDER_KEY);
    assert_eq!(saved["configured"], true);
    OffsetDateTime::parse(saved["updated_at"].as_str().unwrap(), &Rfc3339).unwrap();
    assert!(saved["last_used_at"].is_null());
    assert_eq!(saved.as_object().unwrap().len(), 4);
    assert!(!saved.to_string().contains("alice-first-token"));
    let service = ProviderCredentialService::new(context.state.store.clone());
    assert_eq!(
        service
            .resolve_provider_user_token(PROVIDER_KEY, alice, None)
            .await
            .unwrap(),
        "alice-first-token"
    );
    let used = context.list(&alice_key).await.1;
    OffsetDateTime::parse(used[0]["last_used_at"].as_str().unwrap(), &Rfc3339).unwrap();

    let before = context
        .state
        .store
        .get_provider_user_credential(PROVIDER_KEY, alice)
        .await
        .unwrap()
        .unwrap();
    let (_, second_key) = context.key(Some(alice), None).await;
    let (status, replaced) = context.put(&second_key, "alice-replacement-token").await;
    assert_eq!(status, StatusCode::OK);
    assert!(replaced["last_used_at"].is_null());
    assert_eq!(context.list(&alice_key).await.1, json!([replaced]));
    let after = context
        .state
        .store
        .get_provider_user_credential(PROVIDER_KEY, alice)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(before.credential_id, after.credential_id);
    assert_ne!(after.secret_ciphertext, "alice-replacement-token");
    assert_eq!(
        service
            .resolve_provider_user_token(PROVIDER_KEY, alice, None)
            .await
            .unwrap(),
        "alice-replacement-token"
    );
    assert_eq!(
        service
            .resolve_provider_user_token(PROVIDER_KEY, bob, None)
            .await
            .unwrap(),
        "bob-private-token"
    );

    let admin_path =
        format!("/api/v1/admin/identity/users/{alice}/provider-credentials/{PROVIDER_KEY}");
    assert_eq!(
        context
            .request(
                Method::PUT,
                &admin_path,
                &bob_key,
                json!({"token": "ignored"}).to_string()
            )
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let mut admin_session = HeaderMap::new();
    admin_session.insert(header::COOKIE, context.session(bob).await);
    let (status, response) = context
        .request(
            Method::PUT,
            &admin_path,
            &admin_session,
            json!({"token": "admin-replacement"}).to_string(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["data"]["user_id"], alice.to_string());
    assert_eq!(
        service
            .resolve_provider_user_token(PROVIDER_KEY, alice, None)
            .await
            .unwrap(),
        "admin-replacement"
    );
}

#[tokio::test]
async fn rejects_cookies_invalid_keys_service_accounts_and_inactive_users() {
    let context = TestContext::new().await;
    let (admin, _, _) = context.user(GlobalRole::PlatformAdmin).await;
    let mut session_only = HeaderMap::new();
    session_only.insert(header::COOKIE, context.session(admin).await);
    let mut invalid = session_only.clone();
    invalid.insert(AUTHORIZATION, HeaderValue::from_static("Bearer invalid"));
    let mut service = context.service_account_key().await;
    service.insert(header::COOKIE, context.session(admin).await);
    let (inactive_user, _, inactive) = context.user(GlobalRole::User).await;
    context
        .state
        .store
        .update_user_status(
            inactive_user,
            UserStatus::Disabled,
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();
    let (_, revoked_id, revoked) = context.user(GlobalRole::User).await;
    context
        .state
        .store
        .revoke_api_key(revoked_id, OffsetDateTime::now_utc())
        .await
        .unwrap();
    let mut explicit_only = HeaderMap::new();
    explicit_only.insert("x-oceans-api-key", HeaderValue::from_static("not-a-bearer"));

    for (headers, expected) in [
        (HeaderMap::new(), StatusCode::UNAUTHORIZED),
        (session_only, StatusCode::UNAUTHORIZED),
        (invalid, StatusCode::UNAUTHORIZED),
        (revoked, StatusCode::UNAUTHORIZED),
        (explicit_only, StatusCode::UNAUTHORIZED),
        (service, StatusCode::FORBIDDEN),
        (inactive, StatusCode::FORBIDDEN),
    ] {
        assert_eq!(context.list(&headers).await.0, expected);
        assert_eq!(context.put(&headers, "must-not-be-saved").await.0, expected);
    }
    assert!(
        context
            .state
            .store
            .list_provider_user_credential_statuses(PROVIDER_KEY)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn rejects_unknown_providers_and_other_auth_modes() {
    let mut context = TestContext::new().await;
    let (_, _, key) = context.user(GlobalRole::User).await;
    for provider in ["app-copilot", "missing-provider"] {
        let path = format!("{COLLECTION}/{provider}");
        let (status, response) = context
            .request(
                Method::PUT,
                &path,
                &key,
                json!({"token": "not-stored"}).to_string(),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!response.to_string().contains("not-stored"));
    }
    context.state.copilot_user_provider_keys = Arc::new(Vec::new());
    assert_eq!(context.list(&key).await, (StatusCode::OK, json!([])));
}

#[tokio::test]
async fn rejects_bad_tokens_and_json_without_echoing_secret_input() {
    let context = TestContext::new().await;
    let (_, _, key) = context.user(GlobalRole::User).await;
    for token in [
        "".to_owned(),
        " ".to_owned(),
        "a".repeat(4097),
        "secret\ninside".into(),
        "secreté".into(),
    ] {
        let (status, response) = context.put(&key, &token).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!response.to_string().contains("secret"));
    }
    for body in [
        json!("private-secret").to_string(),
        json!({"token": {"private-secret": "value"}}).to_string(),
        json!({"token": "private-secret", "user_id": Uuid::new_v4()}).to_string(),
        "{\"token\":\"private-secret\"".into(),
    ] {
        let (status, response) = context.request(Method::PUT, CREDENTIAL, &key, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!response.to_string().contains("private-secret"));
    }
    assert!(
        context
            .state
            .store
            .list_provider_user_credential_statuses(PROVIDER_KEY)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn bounds_token_bodies_after_authentication() {
    let context = TestContext::new().await;
    let (_, _, key) = context.user(GlobalRole::User).await;
    let body = json!({"token": "a".repeat(PROVIDER_CREDENTIAL_BODY_LIMIT)}).to_string();
    let (status, response) = context
        .request(Method::PUT, CREDENTIAL, &key, body.clone())
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!response.to_string().contains(&"a".repeat(32)));
    assert_eq!(
        context
            .request(Method::PUT, CREDENTIAL, &HeaderMap::new(), body)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
