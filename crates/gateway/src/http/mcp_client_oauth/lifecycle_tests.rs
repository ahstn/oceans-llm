//! Refresh rotation and revocation through the real router.

use axum::{
    body::Body,
    http::{Request, StatusCode, header::COOKIE},
};
use gateway_core::{
    AuthMode, GatewayOauthClientAuthMethod, GatewayOauthRepository, GlobalRole, UserStatus,
};
use gateway_service::mcp_client_oauth::oauth_secret_hash;
use gateway_store::GatewayStore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::flow_tests::Harness;
use super::tests::send;

impl Harness {
    /// Registers `editor`, runs consent, and returns the token response.
    pub async fn issue_tokens(&self, extra: &[(&str, &str)]) -> Value {
        if self
            .state
            .store
            .get_gateway_oauth_client("editor")
            .await
            .unwrap()
            .is_none()
        {
            self.register_client("editor", GatewayOauthClientAuthMethod::None, None)
                .await;
        }
        let request_id = self.start(&self.authorize_query("editor", extra)).await;
        let code = self.approve(&request_id).await;
        let (status, token) = self.exchange_code("editor", &code).await;
        assert_eq!(status, StatusCode::OK, "{token}");
        token
    }

    pub async fn refresh(
        &self,
        refresh_token: &str,
        extra: &[(&str, &str)],
    ) -> (StatusCode, Value) {
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("client_id", "editor"),
            ("refresh_token", refresh_token),
        ];
        form.extend_from_slice(extra);
        let (status, _, body) = self.token(&form, None).await;
        (status, body)
    }

    pub async fn revoke(&self, form: &[(&str, &str)]) -> StatusCode {
        let mut body = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in form {
            body.append_pair(key, value);
        }
        let (status, _) = send(
            &self.router,
            Request::builder()
                .method("POST")
                .uri("/oauth/revoke")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body.finish()))
                .unwrap(),
        )
        .await;
        status
    }

    pub async fn grant_revoked_reason(&self, token: &str) -> Option<String> {
        self.state
            .store
            .get_gateway_oauth_token_context(&oauth_secret_hash(token))
            .await
            .unwrap()
            .expect("token")
            .grant
            .revoked_reason
    }
}

fn str_field<'a>(body: &'a Value, field: &str) -> &'a str {
    body[field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} missing from {body}"))
}

#[tokio::test]
async fn refresh_rotates_tokens_and_reuse_revokes_the_grant() {
    let harness = Harness::new().await;
    let first = harness.issue_tokens(&[]).await;
    let first_refresh = str_field(&first, "refresh_token");
    assert!(first_refresh.starts_with("ort_"));

    let (status, second) = harness.refresh(first_refresh, &[]).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let second_refresh = str_field(&second, "refresh_token");
    assert_ne!(second_refresh, first_refresh);
    assert_ne!(
        str_field(&second, "access_token"),
        str_field(&first, "access_token")
    );
    assert_eq!(second["scope"], "mcp:aggregate");

    // The rotated-out token is single use; presenting it again ends the family.
    let (status, error) = harness.refresh(first_refresh, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");
    assert_eq!(
        harness
            .grant_revoked_reason(second_refresh)
            .await
            .as_deref(),
        Some("refresh_token_reused")
    );
    let (_, error) = harness.refresh(second_refresh, &[]).await;
    assert_eq!(error["error"], "invalid_grant");
}

#[tokio::test]
async fn refresh_can_narrow_but_never_widen_scope_or_change_resource() {
    let harness = Harness::new().await;
    let tokens = harness
        .issue_tokens(&[("scope", "mcp:aggregate mcp:proxy")])
        .await;
    let refresh = str_field(&tokens, "refresh_token");

    let (status, narrowed) = harness.refresh(refresh, &[("scope", "mcp:proxy")]).await;
    assert_eq!(status, StatusCode::OK, "{narrowed}");
    assert_eq!(narrowed["scope"], "mcp:proxy");
    // The new refresh token keeps the full grant scope.
    let refresh = str_field(&narrowed, "refresh_token");
    let (_, full) = harness.refresh(refresh, &[]).await;
    assert_eq!(full["scope"], "mcp:aggregate mcp:proxy");

    let refresh = str_field(&full, "refresh_token");
    let (_, error) = harness.refresh(refresh, &[("scope", "admin")]).await;
    assert_eq!(error["error"], "invalid_scope");
    let (_, error) = harness
        .refresh(
            refresh,
            &[("resource", "https://oceans.example.com/v1/models")],
        )
        .await;
    assert_eq!(error["error"], "invalid_target");
}

#[tokio::test]
async fn refresh_requires_the_issuing_client_and_a_live_session() {
    let harness = Harness::new().await;
    harness
        .register_client("other", GatewayOauthClientAuthMethod::None, None)
        .await;
    let tokens = harness.issue_tokens(&[]).await;
    let refresh = str_field(&tokens, "refresh_token");

    let (_, _, error) = harness
        .token(
            &[
                ("grant_type", "refresh_token"),
                ("client_id", "other"),
                ("refresh_token", refresh),
            ],
            None,
        )
        .await;
    assert_eq!(error["error"], "invalid_grant");

    harness
        .state
        .store
        .revoke_user_session(harness.session_id, OffsetDateTime::now_utc())
        .await
        .unwrap();
    let (status, error) = harness.refresh(refresh, &[]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");
}

#[tokio::test]
async fn clients_without_the_refresh_grant_get_no_refresh_token() {
    let harness = Harness::new().await;
    let now = OffsetDateTime::now_utc();
    harness
        .state
        .store
        .create_gateway_oauth_client(&gateway_core::GatewayOauthClientRecord {
            client_id: "editor".to_string(),
            registration_kind: "dynamic".to_string(),
            client_name: None,
            client_uri: None,
            logo_uri: None,
            redirect_uris: vec!["http://127.0.0.1/callback".to_string()],
            grant_types: vec!["authorization_code".to_string()],
            token_endpoint_auth_method: GatewayOauthClientAuthMethod::None,
            client_secret_hash: None,
            software_id: None,
            software_version: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
    let tokens = harness.issue_tokens(&[]).await;
    assert!(tokens.get("refresh_token").is_none(), "{tokens}");
    let (_, error) = harness.refresh("ort_anything", &[]).await;
    assert_eq!(error["error"], "unauthorized_client");
}

#[tokio::test]
async fn revocation_ends_the_grant_for_either_token() {
    let harness = Harness::new().await;
    let tokens = harness.issue_tokens(&[]).await;
    let access = str_field(&tokens, "access_token");
    let refresh = str_field(&tokens, "refresh_token");

    assert_eq!(
        harness
            .revoke(&[("client_id", "editor"), ("token", access)])
            .await,
        StatusCode::OK
    );
    assert_eq!(
        harness.grant_revoked_reason(access).await.as_deref(),
        Some("client_revoked")
    );
    let (_, error) = harness.refresh(refresh, &[]).await;
    assert_eq!(error["error"], "invalid_grant");

    // Unknown tokens succeed without effect; bad requests and clients do not.
    assert_eq!(
        harness
            .revoke(&[
                ("client_id", "editor"),
                ("token", "oat_unknown"),
                ("token_type_hint", "access_token"),
            ])
            .await,
        StatusCode::OK
    );
    assert_eq!(
        harness.revoke(&[("client_id", "editor")]).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        harness
            .revoke(&[("client_id", "missing"), ("token", refresh)])
            .await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn another_client_cannot_revoke_a_grant() {
    let harness = Harness::new().await;
    harness
        .register_client("other", GatewayOauthClientAuthMethod::None, None)
        .await;
    let tokens = harness.issue_tokens(&[]).await;
    let access = str_field(&tokens, "access_token");
    assert_eq!(
        harness
            .revoke(&[("client_id", "other"), ("token", access)])
            .await,
        StatusCode::OK
    );
    assert_eq!(harness.grant_revoked_reason(access).await, None);
}

#[tokio::test]
async fn deactivating_the_user_revokes_every_grant() {
    let harness = Harness::new().await;
    let tokens = harness.issue_tokens(&[]).await;
    let access = str_field(&tokens, "access_token");

    let admin = harness
        .state
        .store
        .create_identity_user(
            "Platform admin",
            "platform-admin@example.com",
            "platform-admin@example.com",
            GlobalRole::PlatformAdmin,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .unwrap();
    let session_id = Uuid::new_v4();
    let session_token = format!("{session_id}.admin-signature");
    let now = OffsetDateTime::now_utc();
    harness
        .state
        .store
        .create_user_session(
            session_id,
            admin.user_id,
            &format!("{:x}", Sha256::digest(session_token.as_bytes())),
            now + Duration::hours(1),
            now,
        )
        .await
        .unwrap();
    let (status, body) = send(
        &harness.router,
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/admin/identity/users/{}/deactivate",
                harness.user_id
            ))
            .header(COOKIE, format!("ogw_session={session_token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        harness.grant_revoked_reason(access).await.as_deref(),
        Some("user_deactivated")
    );
}

#[tokio::test]
async fn metadata_advertises_refresh_and_revocation() {
    let harness = Harness::new().await;
    let (_, metadata) = send(
        &harness.router,
        Request::builder()
            .uri("/.well-known/oauth-authorization-server")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        metadata["grant_types_supported"],
        serde_json::json!(["authorization_code", "refresh_token"])
    );
    assert_eq!(
        metadata["revocation_endpoint"],
        "https://oceans.example.com/oauth/revoke"
    );
}
