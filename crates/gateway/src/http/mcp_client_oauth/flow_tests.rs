//! The authorization-code flow through the real router and store.

use axum::{
    Router,
    body::Body,
    http::{
        Request, StatusCode,
        header::{CACHE_CONTROL, COOKIE, LOCATION, WWW_AUTHENTICATE},
    },
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use gateway_core::{
    AuthMode, GatewayOauthClientAuthMethod, GatewayOauthClientRecord, GatewayOauthRepository,
    GlobalRole, UserStatus, hash_gateway_key_secret,
};
use gateway_service::mcp_client_oauth::oauth_token_hash;
use gateway_store::GatewayStore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use url::Url;
use uuid::Uuid;

use super::tests::{
    ISSUER, oauth_settings, oauth_state_with, router, seed_server, send, send_with_headers,
};
use crate::http::state::AppState;

pub(super) const REDIRECT_URI: &str = "http://127.0.0.1:43123/callback";
pub(super) const CODE_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

pub(super) struct Harness {
    _directory: tempfile::TempDir,
    pub state: AppState,
    pub router: Router,
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub cookie: String,
}

pub(super) fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl Harness {
    pub async fn new() -> Self {
        Self::with_settings(oauth_settings()).await
    }

    pub async fn with_settings(settings: gateway_service::McpClientOauthSettings) -> Self {
        let (directory, state) = oauth_state_with(settings).await;
        let user = state
            .store
            .create_identity_user(
                "Client user",
                "client-user@example.com",
                "client-user@example.com",
                GlobalRole::User,
                AuthMode::Password,
                UserStatus::Active,
            )
            .await
            .expect("user");
        let session_id = Uuid::new_v4();
        let session_token = format!("{session_id}.test-signature");
        let now = OffsetDateTime::now_utc();
        state
            .store
            .create_user_session(
                session_id,
                user.user_id,
                &format!("{:x}", Sha256::digest(session_token.as_bytes())),
                now + Duration::days(1),
                now,
            )
            .await
            .expect("session");
        Self {
            router: router(&state),
            _directory: directory,
            state,
            user_id: user.user_id,
            session_id,
            cookie: format!("ogw_session={session_token}"),
        }
    }

    pub async fn register_client(
        &self,
        client_id: &str,
        auth_method: GatewayOauthClientAuthMethod,
        secret: Option<&str>,
    ) {
        let now = OffsetDateTime::now_utc();
        self.state
            .store
            .create_gateway_oauth_client(&GatewayOauthClientRecord {
                client_id: client_id.to_string(),
                registration_kind: "dynamic".to_string(),
                client_name: Some("Test Editor".to_string()),
                client_uri: Some("https://editor.example.com".to_string()),
                logo_uri: None,
                redirect_uris: vec![
                    "http://127.0.0.1/callback".to_string(),
                    "https://editor.example.com/oauth/callback".to_string(),
                ],
                grant_types: vec![
                    "authorization_code".to_string(),
                    "refresh_token".to_string(),
                ],
                token_endpoint_auth_method: auth_method,
                client_secret_hash: secret.map(|secret| hash_gateway_key_secret(secret).unwrap()),
                software_id: None,
                software_version: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .expect("client");
    }

    pub fn authorize_query(&self, client_id: &str, extra: &[(&str, &str)]) -> String {
        let challenge = code_challenge(CODE_VERIFIER);
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("state", "client-state")
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        for (key, value) in extra {
            query.append_pair(key, value);
        }
        query.finish()
    }

    /// Sends `/oauth/authorize` and returns the status and `Location`.
    pub async fn authorize(&self, query: &str) -> (StatusCode, Option<String>) {
        let (status, headers, _) = send_with_headers(
            &self.router,
            Request::builder()
                .uri(format!("/oauth/authorize?{query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        let location = headers
            .get(LOCATION)
            .map(|value| value.to_str().unwrap().to_string());
        (status, location)
    }

    /// Runs `/oauth/authorize` and returns the pending request ID.
    pub async fn start(&self, query: &str) -> String {
        let (status, location) = self.authorize(query).await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        let location = location.expect("consent redirect");
        location
            .strip_prefix("/admin/oauth/consent?request_id=")
            .unwrap_or_else(|| panic!("unexpected redirect {location}"))
            .to_string()
    }

    pub async fn consent(&self, request_id: &str) -> (StatusCode, Value) {
        send(
            &self.router,
            Request::builder()
                .uri(format!("/api/v1/oauth/authorization-requests/{request_id}"))
                .header(COOKIE, &self.cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    pub async fn decide(&self, request_id: &str, approve: bool) -> (StatusCode, Value) {
        send(
            &self.router,
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/oauth/authorization-requests/{request_id}/decision"
                ))
                .header(COOKIE, &self.cookie)
                .header("content-type", "application/json")
                .body(Body::from(json!({ "approve": approve }).to_string()))
                .unwrap(),
        )
        .await
    }

    /// Audit entries for `action`, newest first, each read in full (listing
    /// leaves out summaries and metadata).
    pub async fn audit_entries(&self, action: &str) -> Vec<gateway_core::AuditLogRecord> {
        use gateway_core::AuditLogRepository;
        let store = &self.state.store;
        let page = store
            .list_audit_logs(&gateway_core::AuditLogQuery {
                action: Some(action.to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        let mut entries = Vec::new();
        for item in page.items {
            entries.push(store.get_audit_log(item.id).await.unwrap().unwrap());
        }
        entries
    }

    /// Approves `request_id` and returns the authorization code.
    pub async fn approve(&self, request_id: &str) -> String {
        let (status, body) = self.decide(request_id, true).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let redirect = Url::parse(body["redirect_to"].as_str().unwrap()).unwrap();
        let params: std::collections::HashMap<_, _> = redirect.query_pairs().into_owned().collect();
        assert_eq!(params["state"], "client-state");
        assert_eq!(params["iss"], ISSUER);
        params["code"].clone()
    }

    pub async fn token(
        &self,
        form: &[(&str, &str)],
        authorization: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut body = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in form {
            body.append_pair(key, value);
        }
        let mut request = Request::builder()
            .method("POST")
            .uri("/oauth/token")
            .header("content-type", "application/x-www-form-urlencoded");
        if let Some(authorization) = authorization {
            request = request.header("authorization", authorization);
        }
        send_with_headers(
            &self.router,
            request.body(Body::from(body.finish())).unwrap(),
        )
        .await
    }

    pub async fn exchange_code(&self, client_id: &str, code: &str) -> (StatusCode, Value) {
        let (status, _, body) = self
            .token(
                &[
                    ("grant_type", "authorization_code"),
                    ("client_id", client_id),
                    ("code", code),
                    ("redirect_uri", REDIRECT_URI),
                    ("code_verifier", CODE_VERIFIER),
                ],
                None,
            )
            .await;
        (status, body)
    }
}

fn query_params(location: &str) -> std::collections::HashMap<String, String> {
    Url::parse(location)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}

#[tokio::test]
async fn code_flow_issues_an_audience_bound_access_token() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    let request_id = harness.start(&harness.authorize_query("editor", &[])).await;

    let (status, view) = harness.consent(&request_id).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["client_name"], "Test Editor");
    assert_eq!(view["redirect_host"], "127.0.0.1:43123");
    assert_eq!(view["redirect_is_loopback"], true);
    assert_eq!(view["resource"], "https://oceans.example.com/mcp");
    assert_eq!(view["resource_kind"], "aggregate");
    assert_eq!(view["scopes"][0]["scope"], "mcp:aggregate");

    let code = harness.approve(&request_id).await;
    // The approval is audited against the request, without the code.
    let approved = harness.audit_entries("mcp_client_grant.authorized").await;
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].actor.user_id, Some(harness.user_id));
    assert_eq!(approved[0].target_id.as_deref(), Some(request_id.as_str()));
    assert_eq!(approved[0].target_label.as_deref(), Some("Test Editor"));
    let summary = approved[0].after.as_ref().unwrap();
    assert_eq!(summary["redirect_host"], "127.0.0.1:43123");
    assert_eq!(summary["scopes"], json!(["mcp:aggregate"]));
    assert!(!serde_json::to_string(&approved[0]).unwrap().contains(&code));
    let (status, headers, token) = harness
        .token(
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "editor"),
                ("code", &code),
                ("redirect_uri", REDIRECT_URI),
                ("code_verifier", CODE_VERIFIER),
                ("resource", "https://oceans.example.com/mcp"),
            ],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{token}");
    assert_eq!(headers[CACHE_CONTROL], "no-store");
    assert_eq!(token["token_type"], "Bearer");
    assert_eq!(token["scope"], "mcp:aggregate");
    let access_token = token["access_token"].as_str().unwrap();
    assert!(access_token.starts_with("oat_"));
    let expires_in = token["expires_in"].as_i64().unwrap();
    assert!((3590..=3600).contains(&expires_in), "{expires_in}");

    let context = harness
        .state
        .store
        .get_gateway_oauth_token_context(&oauth_token_hash(access_token))
        .await
        .unwrap()
        .expect("stored token");
    assert_eq!(context.grant.user_id, harness.user_id);
    assert_eq!(context.grant.user_session_id, harness.session_id);
    assert_eq!(context.grant.resource, "https://oceans.example.com/mcp");
}

#[tokio::test]
async fn per_server_resource_defaults_to_the_proxy_scope() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    seed_server(&harness.state, "repository", "http://127.0.0.1:9/mcp").await;
    let request_id = harness
        .start(&harness.authorize_query(
            "editor",
            &[("resource", "https://oceans.example.com/mcp/repository")],
        ))
        .await;
    let (_, view) = harness.consent(&request_id).await;
    assert_eq!(view["resource_kind"], "server");
    assert_eq!(view["server_key"], "repository");
    assert_eq!(view["server_display_name"], "Repository");
    assert_eq!(view["scopes"][0]["scope"], "mcp:proxy");
}

#[tokio::test]
async fn reused_code_fails_and_revokes_the_grant_it_created() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    let request_id = harness.start(&harness.authorize_query("editor", &[])).await;
    let code = harness.approve(&request_id).await;
    let (status, token) = harness.exchange_code("editor", &code).await;
    assert_eq!(status, StatusCode::OK);

    let (status, error) = harness.exchange_code("editor", &code).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");
    let context = harness
        .state
        .store
        .get_gateway_oauth_token_context(&oauth_token_hash(token["access_token"].as_str().unwrap()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        context.grant.revoked_reason.as_deref(),
        Some("authorization_code_replayed")
    );
}

#[tokio::test]
async fn token_exchange_checks_pkce_client_resource_and_session() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    harness
        .register_client("other", GatewayOauthClientAuthMethod::None, None)
        .await;

    let request_id = harness.start(&harness.authorize_query("editor", &[])).await;
    let code = harness.approve(&request_id).await;
    let (status, _, error) = harness
        .token(
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "editor"),
                ("code", &code),
                (
                    "code_verifier",
                    "wrong-verifier-wrong-verifier-wrong-verifier-x",
                ),
            ],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");
    let (_, error) = harness.exchange_code("other", &code).await;
    assert_eq!(error["error"], "invalid_grant");
    let (_, _, error) = harness
        .token(
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "editor"),
                ("code", &code),
                ("redirect_uri", REDIRECT_URI),
                ("code_verifier", CODE_VERIFIER),
                ("resource", "https://oceans.example.com/v1/chat/completions"),
            ],
            None,
        )
        .await;
    assert_eq!(error["error"], "invalid_target");

    // Signing out before the exchange ends the authorization.
    harness
        .state
        .store
        .revoke_user_session(harness.session_id, OffsetDateTime::now_utc())
        .await
        .unwrap();
    let (status, error) = harness.exchange_code("editor", &code).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");
    assert_eq!(
        error["error_description"],
        "the approving session has ended"
    );
}

#[tokio::test]
async fn confidential_clients_must_authenticate_with_their_registered_method() {
    let harness = Harness::new().await;
    harness
        .register_client(
            "server-app",
            GatewayOauthClientAuthMethod::ClientSecretBasic,
            Some("s3cret"),
        )
        .await;
    let request_id = harness
        .start(&harness.authorize_query("server-app", &[]))
        .await;
    let code = harness.approve(&request_id).await;
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", CODE_VERIFIER),
    ];

    let wrong = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("server-app:nope")
    );
    let (status, headers, error) = harness.token(&form, Some(&wrong)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"], "invalid_client");
    assert_eq!(headers[WWW_AUTHENTICATE], r#"Basic realm="oceans""#);

    let mut post_form = form.to_vec();
    post_form.push(("client_id", "server-app"));
    post_form.push(("client_secret", "s3cret"));
    let (status, _, error) = harness.token(&post_form, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{error}");

    let right = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("server-app:s3cret")
    );
    let (status, _, token) = harness.token(&form, Some(&right)).await;
    assert_eq!(status, StatusCode::OK, "{token}");
}

#[tokio::test]
async fn authorize_never_redirects_to_an_unverified_client() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;

    let unknown = harness.authorize_query("unknown", &[]);
    let (status, location) = harness.authorize(&unknown).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(location.is_none());

    let query = harness
        .authorize_query("editor", &[])
        .replace("43123%2Fcallback", "43123%2Fevil");
    let (status, location) = harness.authorize(&query).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(location.is_none());

    // A repeated client_id or redirect_uri never redirects, even after an
    // earlier repeat of another parameter.
    for suffix in [
        "&client_id=editor",
        "&code_challenge_method=S256&client_id=another-client",
        "&code_challenge_method=S256&redirect_uri=https%3A%2F%2Fevil.example.com%2Fcallback",
    ] {
        let repeated = format!("{}{suffix}", harness.authorize_query("editor", &[]));
        let (status, location) = harness.authorize(&repeated).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{suffix}");
        assert!(location.is_none(), "{suffix}");
    }
}

#[tokio::test]
async fn authorize_redirects_parameter_errors_with_state_and_issuer() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    let cases = [
        (
            harness
                .authorize_query("editor", &[])
                .replace("code_challenge_method=S256", "code_challenge_method=plain"),
            "invalid_request",
        ),
        (
            harness
                .authorize_query("editor", &[])
                .replace("response_type=code", "response_type=token"),
            "unsupported_response_type",
        ),
        (
            harness.authorize_query("editor", &[("scope", "mcp:aggregate admin")]),
            "invalid_scope",
        ),
        (
            harness.authorize_query("editor", &[("resource", "https://other.example.com/mcp")]),
            "invalid_target",
        ),
        (
            harness.authorize_query(
                "editor",
                &[("resource", "https://oceans.example.com/mcp/missing")],
            ),
            "invalid_target",
        ),
        (
            format!(
                "{}&code_challenge_method=S256",
                harness.authorize_query("editor", &[])
            ),
            "invalid_request",
        ),
    ];
    for (query, expected) in cases {
        let (status, location) = harness.authorize(&query).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{expected}");
        let location = location.expect("error redirect");
        assert!(location.starts_with(REDIRECT_URI), "{location}");
        let params = query_params(&location);
        assert_eq!(params["error"], expected, "{location}");
        assert_eq!(params["state"], "client-state");
        assert_eq!(params["iss"], ISSUER);
    }
}

#[tokio::test]
async fn denying_consent_returns_access_denied_and_the_request_cannot_be_reused() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    let request_id = harness.start(&harness.authorize_query("editor", &[])).await;
    let (status, body) = harness.decide(&request_id, false).await;
    assert_eq!(status, StatusCode::OK);
    let params = query_params(body["redirect_to"].as_str().unwrap());
    assert_eq!(params["error"], "access_denied");
    assert_eq!(params["iss"], ISSUER);

    let (status, _) = harness.decide(&request_id, true).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = harness.consent(&request_id).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Only the denial is audited; the refused second decision records nothing.
    let denied = harness.audit_entries("mcp_client_grant.denied").await;
    assert_eq!(denied.len(), 1);
    assert_eq!(denied[0].target_id.as_deref(), Some(request_id.as_str()));
    assert!(
        harness
            .audit_entries("mcp_client_grant.authorized")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn consent_requires_an_active_admin_session() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    let request_id = harness.start(&harness.authorize_query("editor", &[])).await;
    let (status, _) = send(
        &harness.router,
        Request::builder()
            .uri(format!("/api/v1/oauth/authorization-requests/{request_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = harness.consent(&Uuid::new_v4().to_string()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unsupported_grant_types_are_rejected() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    let (status, _, error) = harness
        .token(
            &[
                ("grant_type", "client_credentials"),
                ("client_id", "editor"),
            ],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "unsupported_grant_type");
}

#[tokio::test]
async fn token_exchange_requires_the_redirect_uri_sent_at_authorize() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    let request_id = harness.start(&harness.authorize_query("editor", &[])).await;
    let code = harness.approve(&request_id).await;
    let (status, _, error) = harness
        .token(
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "editor"),
                ("code", &code),
                ("code_verifier", CODE_VERIFIER),
            ],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");
    assert_eq!(
        error["error_description"],
        "redirect_uri does not match the authorization request"
    );
    // The failed attempt did not consume the code.
    let (status, token) = harness.exchange_code("editor", &code).await;
    assert_eq!(status, StatusCode::OK, "{token}");
}

#[tokio::test]
async fn omitted_redirect_uri_may_stay_omitted_at_the_token_endpoint() {
    let harness = Harness::new().await;
    let now = OffsetDateTime::now_utc();
    harness
        .state
        .store
        .create_gateway_oauth_client(&GatewayOauthClientRecord {
            client_id: "web-app".to_string(),
            registration_kind: "dynamic".to_string(),
            client_name: Some("Web app".to_string()),
            client_uri: None,
            logo_uri: None,
            redirect_uris: vec!["https://web.example.com/callback".to_string()],
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
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("response_type", "code")
        .append_pair("client_id", "web-app")
        .append_pair("state", "client-state")
        .append_pair("code_challenge", &code_challenge(CODE_VERIFIER))
        .append_pair("code_challenge_method", "S256")
        .finish();
    let request_id = harness.start(&query).await;
    let (_, view) = harness.consent(&request_id).await;
    assert_eq!(view["redirect_is_loopback"], false);
    let code = harness.approve(&request_id).await;
    let (status, _, token) = harness
        .token(
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "web-app"),
                ("code", &code),
                ("code_verifier", CODE_VERIFIER),
            ],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{token}");
}

#[tokio::test]
async fn only_the_issuing_client_with_its_verifier_can_trigger_replay_revocation() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    harness
        .register_client("attacker", GatewayOauthClientAuthMethod::None, None)
        .await;
    let request_id = harness.start(&harness.authorize_query("editor", &[])).await;
    let code = harness.approve(&request_id).await;
    let (status, token) = harness.exchange_code("editor", &code).await;
    assert_eq!(status, StatusCode::OK);
    let access = oauth_token_hash(token["access_token"].as_str().unwrap());

    // A leaked code replayed by another client, or without the verifier.
    let (_, error) = harness.exchange_code("attacker", &code).await;
    assert_eq!(error["error"], "invalid_grant");
    let (_, _, error) = harness
        .token(
            &[
                ("grant_type", "authorization_code"),
                ("client_id", "editor"),
                ("code", &code),
                ("redirect_uri", REDIRECT_URI),
                (
                    "code_verifier",
                    "wrong-verifier-wrong-verifier-wrong-verifier-x",
                ),
            ],
            None,
        )
        .await;
    assert_eq!(error["error"], "invalid_grant");
    let context = harness
        .state
        .store
        .get_gateway_oauth_token_context(&access)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        context.grant.revoked_at, None,
        "the victim's grant survives"
    );
}

#[tokio::test]
async fn concurrent_redemption_leaves_no_live_grant() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    let request_id = harness.start(&harness.authorize_query("editor", &[])).await;
    let code = harness.approve(&request_id).await;
    let (first, second) = tokio::join!(
        harness.exchange_code("editor", &code),
        harness.exchange_code("editor", &code)
    );
    let outcomes = [first, second];
    let issued: Vec<&Value> = outcomes
        .iter()
        .filter(|(status, _)| *status == StatusCode::OK)
        .map(|(_, body)| body)
        .collect();
    assert!(issued.len() <= 1, "a code redeems at most once");
    assert!(
        outcomes
            .iter()
            .any(|(_, body)| body["error"] == "invalid_grant")
    );
    // Whichever request won, the replay revoked its grant.
    for token in issued {
        let context = harness
            .state
            .store
            .get_gateway_oauth_token_context(&oauth_token_hash(
                token["access_token"].as_str().unwrap(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            context.grant.revoked_reason.as_deref(),
            Some("authorization_code_replayed")
        );
    }
}

#[tokio::test]
async fn loopback_warning_follows_the_selected_redirect_uri() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    // The client registers both an HTTPS and a loopback URI.
    let https = harness.authorize_query("editor", &[]).replace(
        &url::form_urlencoded::byte_serialize(REDIRECT_URI.as_bytes()).collect::<String>(),
        &url::form_urlencoded::byte_serialize(b"https://editor.example.com/oauth/callback")
            .collect::<String>(),
    );
    let request_id = harness.start(&https).await;
    let (_, view) = harness.consent(&request_id).await;
    assert_eq!(view["redirect_is_loopback"], false);
    assert_eq!(view["redirect_host"], "editor.example.com");

    let request_id = harness.start(&harness.authorize_query("editor", &[])).await;
    let (_, view) = harness.consent(&request_id).await;
    assert_eq!(view["redirect_is_loopback"], true);
}

#[tokio::test]
async fn omitted_token_resource_keeps_the_authorized_per_server_audience() {
    let harness = Harness::new().await;
    harness
        .register_client("editor", GatewayOauthClientAuthMethod::None, None)
        .await;
    seed_server(&harness.state, "repository", "http://127.0.0.1:9/mcp").await;
    let request_id = harness
        .start(&harness.authorize_query(
            "editor",
            &[("resource", "https://oceans.example.com/mcp/repository")],
        ))
        .await;
    let code = harness.approve(&request_id).await;
    let (status, token) = harness.exchange_code("editor", &code).await;
    assert_eq!(status, StatusCode::OK, "{token}");
    assert_eq!(token["scope"], "mcp:proxy");
    let context = harness
        .state
        .store
        .get_gateway_oauth_token_context(&oauth_token_hash(token["access_token"].as_str().unwrap()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        context.grant.resource,
        "https://oceans.example.com/mcp/repository"
    );
}

#[tokio::test]
async fn token_requests_over_the_body_limit_are_refused() {
    let harness = Harness::new().await;
    let scope = "x ".repeat(10 * 1024);
    let (status, _, _) = harness
        .token(
            &[("grant_type", "refresh_token"), ("scope", scope.as_str())],
            None,
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}
