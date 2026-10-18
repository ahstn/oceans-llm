//! RFC 7591 Dynamic Client Registration through the real router.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode, header::CACHE_CONTROL},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use gateway_service::McpClientOauthSettings;
use serde_json::{Value, json};

use super::flow_tests::{CODE_VERIFIER, Harness, REDIRECT_URI};
use super::tests::{ISSUER, router, send, send_with_headers};

impl Harness {
    pub async fn register(&self, body: Value) -> (StatusCode, axum::http::HeaderMap, Value) {
        send_with_headers(
            &self.router,
            Request::builder()
                .method("POST")
                .uri("/oauth/register")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }
}

#[tokio::test]
async fn metadata_advertises_registration_only_when_enabled() {
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
        metadata["registration_endpoint"],
        "https://oceans.example.com/oauth/register"
    );

    let mut state = harness.state.clone();
    state.mcp_client_oauth = Some(Arc::new(
        McpClientOauthSettings::new(ISSUER, 3600, 86400, false).unwrap(),
    ));
    let router = router(&state);
    let (_, metadata) = send(
        &router,
        Request::builder()
            .uri("/.well-known/oauth-authorization-server")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(metadata.get("registration_endpoint").is_none());
    let (status, _) = send(
        &router,
        Request::builder()
            .method("POST")
            .uri("/oauth/register")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"redirect_uris":["https://a.example/cb"]}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn public_client_registers_and_completes_the_code_flow() {
    let harness = Harness::new().await;
    let (status, headers, body) = harness
        .register(json!({
            "client_name": "Registered Editor",
            "redirect_uris": ["http://127.0.0.1/callback"],
            "grant_types": ["authorization_code", "refresh_token"],
            "token_endpoint_auth_method": "none",
            "software_version": "2.1.0",
            "unknown_member": true
        }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(headers[CACHE_CONTROL], "no-store");
    assert!(body.get("client_secret").is_none());
    assert_eq!(body["token_endpoint_auth_method"], "none");
    assert_eq!(body["response_types"], json!(["code"]));
    assert_eq!(body["software_version"], "2.1.0");
    let client_id = body["client_id"].as_str().unwrap();

    let request_id = harness
        .start(&harness.authorize_query(client_id, &[]))
        .await;
    let (_, view) = harness.consent(&request_id).await;
    assert_eq!(view["client_name"], "Registered Editor");
    assert_eq!(view["redirect_is_loopback"], true);
    let code = harness.approve(&request_id).await;
    let (status, token) = harness.exchange_code(client_id, &code).await;
    assert_eq!(status, StatusCode::OK, "{token}");
}

#[tokio::test]
async fn omitted_auth_method_registers_a_confidential_client() {
    let harness = Harness::new().await;
    let (status, _, body) = harness
        .register(json!({ "redirect_uris": [REDIRECT_URI] }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["token_endpoint_auth_method"], "client_secret_basic");
    assert_eq!(body["grant_types"], json!(["authorization_code"]));
    assert_eq!(body["client_secret_expires_at"], 0);
    let client_id = body["client_id"].as_str().unwrap();
    let secret = body["client_secret"].as_str().unwrap();
    assert!(secret.starts_with("ocs_"));

    let request_id = harness
        .start(&harness.authorize_query(client_id, &[]))
        .await;
    let code = harness.approve(&request_id).await;
    let basic = format!("Basic {}", STANDARD.encode(format!("{client_id}:{secret}")));
    let (status, _, token) = harness
        .token(
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT_URI),
                ("code_verifier", CODE_VERIFIER),
            ],
            Some(&basic),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{token}");
}

#[tokio::test]
async fn registration_rejects_unsafe_or_unsupported_metadata() {
    let harness = Harness::new().await;
    let cases = [
        (json!({}), "invalid_redirect_uri"),
        (
            json!({ "redirect_uris": ["http://editor.example.com/callback"] }),
            "invalid_redirect_uri",
        ),
        (
            json!({ "redirect_uris": ["editor-app://callback"] }),
            "invalid_redirect_uri",
        ),
        (
            json!({ "redirect_uris": [REDIRECT_URI], "grant_types": ["client_credentials"] }),
            "invalid_client_metadata",
        ),
        (
            json!({ "redirect_uris": [REDIRECT_URI], "token_endpoint_auth_method": "private_key_jwt" }),
            "invalid_client_metadata",
        ),
        (
            json!({ "redirect_uris": [REDIRECT_URI], "logo_uri": "http://editor.example.com/logo.png" }),
            "invalid_client_metadata",
        ),
        (json!(["not", "an", "object"]), "invalid_client_metadata"),
    ];
    for (body, expected) in cases {
        let (status, _, error) = harness.register(body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(error["error"], expected, "{body}");
    }

    let (status, _) = send(
        &harness.router,
        Request::builder()
            .method("POST")
            .uri("/oauth/register")
            .header("content-type", "application/json")
            .body(Body::from(format!(
                r#"{{"redirect_uris":["{REDIRECT_URI}"],"client_name":"{}"}}"#,
                "x".repeat(20 * 1024)
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

fn from_address(mut request: Request<Body>, address: [u8; 4]) -> Request<Body> {
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            address, 40_000,
        ))));
    request
}

/// Open registration runs no Argon2, so a flood cannot take the slots that
/// token requests from older confidential clients still need, and each
/// source is limited before anything is parsed or stored.
#[tokio::test]
async fn registration_is_limited_per_source_and_hashes_secrets_cheaply() {
    use gateway_core::GatewayOauthRepository;
    let harness = Harness::new().await;
    let register = |address| {
        let request = Request::builder()
            .method("POST")
            .uri("/oauth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "redirect_uris": ["https://editor.example.com/cb"],
                    "token_endpoint_auth_method": "client_secret_basic",
                })
                .to_string(),
            ))
            .unwrap();
        send_with_headers(&harness.router, from_address(request, address))
    };

    let (status, _, body) = register([192, 0, 2, 7]).await;
    assert_eq!(status, StatusCode::CREATED);
    let stored = harness
        .state
        .store
        .get_gateway_oauth_client(body["client_id"].as_str().unwrap())
        .await
        .expect("client lookup")
        .expect("client");
    assert!(
        stored
            .client_secret_hash
            .as_deref()
            .is_some_and(|hash| hash.starts_with("sha256:")),
        "a generated secret is a random token, hashed with SHA-256"
    );
    for _ in 1..10 {
        let (status, _, _) = register([192, 0, 2, 7]).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, headers, body) = register([192, 0, 2, 7]).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.contains_key("retry-after"));
    assert_eq!(body["error"], "temporarily_unavailable");
    let (status, _, _) = register([198, 51, 100, 7]).await;
    assert_eq!(status, StatusCode::CREATED, "other sources are unaffected");
}

/// The gateway-wide limit holds callers whose address is unknown, or who
/// spread requests over many addresses.
#[tokio::test]
async fn registration_has_a_gateway_wide_limit() {
    let settings = super::tests::oauth_settings()
        .with_registration_rate_limits(gateway_service::SourceRateLimits {
            per_source_per_minute: 10,
            per_minute: 2,
        })
        .unwrap();
    let harness = Harness::with_settings(settings).await;
    let body = json!({
        "redirect_uris": ["https://editor.example.com/cb"],
        "token_endpoint_auth_method": "none",
    });
    for _ in 0..2 {
        let (status, _, _) = harness.register(body.clone()).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, headers, _) = harness.register(body).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.contains_key("retry-after"));

    // Over the limit, the body is never read: one that fails to arrive
    // still gets the 429.
    let unreadable = Body::from_stream(futures_util::stream::iter([Err::<
        axum::body::Bytes,
        std::io::Error,
    >(std::io::Error::other(
        "the client stalled",
    ))]));
    let (status, _, _) = send_with_headers(
        &harness.router,
        Request::builder()
            .method("POST")
            .uri("/oauth/register")
            .header("content-type", "application/json")
            .body(unreadable)
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}
