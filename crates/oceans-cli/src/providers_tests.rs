use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Json, Router,
    body::Body,
    http::{HeaderMap, StatusCode, Uri},
    response::Response,
    routing::{get, put},
};
use serde_json::{Value, json};

use super::*;

const TOKEN: &str = "dummy_github_token";
const GATEWAY_KEY: &str = "gwk_dummy.secret";

struct Server(tokio::task::JoinHandle<()>);

impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn server(router: Router, prefix: &str) -> (Client, Server) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}{prefix}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (
        Client::new(url.parse().unwrap(), GATEWAY_KEY).unwrap(),
        Server(task),
    )
}

fn args(token: Option<&str>, token_stdin: bool) -> CopilotTokenArgs {
    CopilotTokenArgs {
        token: token.map(str::to_owned),
        token_stdin,
        provider: None,
    }
}

fn assert_private_error(error: anyhow::Error) {
    let error = format!("{error:?}");
    assert!(!error.contains(TOKEN), "{error}");
    assert!(!error.contains(GATEWAY_KEY), "{error}");
}

#[test]
fn token_input_accepts_one_source_and_normalizes_trailing_newlines() {
    assert_eq!(
        read_token(&args(Some(TOKEN), false), io::empty(), false).unwrap(),
        TOKEN
    );
    assert_eq!(
        read_token(&args(None, true), format!("{TOKEN}\r\n").as_bytes(), false).unwrap(),
        TOKEN
    );
    let maximum = format!("{}\r\n", "x".repeat(MAX_TOKEN_BYTES));
    assert_eq!(
        read_token(&args(None, true), maximum.as_bytes(), false)
            .unwrap()
            .len(),
        MAX_TOKEN_BYTES
    );
    assert!(read_token(&args(Some(TOKEN), true), io::empty(), false).is_err());
    assert!(
        read_token(&args(None, false), io::empty(), false)
            .unwrap_err()
            .to_string()
            .contains("--token-stdin")
    );
}

#[test]
fn invalid_token_input_has_bounded_secret_free_errors() {
    for token in [
        String::new(),
        format!("{TOKEN} extra"),
        format!("{TOKEN}\nextra"),
        format!("{TOKEN}\u{1b}"),
        format!("{TOKEN}é"),
        TOKEN.repeat(MAX_TOKEN_BYTES),
    ] {
        assert_private_error(
            read_token(&args(Some(&token), false), io::empty(), false).unwrap_err(),
        );
        assert_private_error(read_token(&args(None, true), token.as_bytes(), false).unwrap_err());
    }
    assert_private_error(read_token(&args(None, true), &[0xff_u8][..], false).unwrap_err());
}

#[tokio::test]
async fn sole_provider_uses_authenticated_discovery_and_put_under_gateway_prefix() {
    let router = Router::new()
        .route("/gateway/api/v1/me/provider-credentials", get(|headers: HeaderMap| async move {
            assert_eq!(headers["authorization"], format!("Bearer {GATEWAY_KEY}"));
            Json(json!([{ "provider_key": "copilot-main", "configured": false, "updated_at": null, "last_used_at": null }]))
        }))
        .route("/gateway/api/v1/me/provider-credentials/copilot-main", put(|headers: HeaderMap, uri: Uri, Json(body): Json<Value>| async move {
            assert_eq!(headers["authorization"], format!("Bearer {GATEWAY_KEY}"));
            assert_eq!(headers["content-type"], "application/json");
            assert!(uri.query().is_none());
            assert!(!uri.path().contains(TOKEN));
            assert_eq!(body, json!({ "token": TOKEN }));
            Json(json!({ "provider_key": "copilot-main", "configured": true, "updated_at": TOKEN, "last_used_at": TOKEN, "token": TOKEN }))
        }));
    let (client, _server) = server(router, "/gateway/").await;
    let provider = store_token(&client, None, TOKEN).await.unwrap();
    assert_eq!(provider, "copilot-main");
    let result = StoredCredential {
        gateway: "https://gateway.example.com/gateway/",
        provider_key: &provider,
        configured: true,
        status: "stored",
    };
    for as_json in [true, false] {
        let output = render_result(&result, as_json).unwrap();
        assert!(!output.contains(TOKEN));
        assert!(!output.contains("validated"));
        assert!(output.contains("copilot-main"));
        assert!(output.contains("gateway.example.com"));
    }
    assert!(
        render_result(&result, false)
            .unwrap()
            .contains("Token stored")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&render_result(&result, true).unwrap()).unwrap(),
        json!({ "gateway": result.gateway, "provider_key": "copilot-main", "configured": true, "status": "stored" })
    );
}

#[tokio::test]
async fn explicit_provider_selects_one_of_multiple_candidates() {
    let router = Router::new()
        .route(
            "/api/v1/me/provider-credentials",
            get(|| async {
                Json(json!([
                    { "provider_key": "copilot-first", "configured": false },
                    { "provider_key": "copilot-second", "configured": true }
                ]))
            }),
        )
        .route(
            "/api/v1/me/provider-credentials/copilot-second",
            put(|| async { Json(json!({ "provider_key": "copilot-second", "configured": true })) }),
        );
    let (client, _server) = server(router, "").await;
    assert_eq!(
        store_token(&client, Some("copilot-second"), TOKEN)
            .await
            .unwrap(),
        "copilot-second"
    );
}

#[tokio::test]
async fn missing_ambiguous_or_unknown_provider_does_not_submit_token() {
    for (providers, requested, message) in [
        (json!([]), None, "ask a platform admin"),
        (
            json!([{ "provider_key": "one", "configured": false }, { "provider_key": "two", "configured": false }]),
            None,
            "--provider",
        ),
        (
            json!([{ "provider_key": "one", "configured": false }]),
            Some(TOKEN),
            "not configured for GitHub user authentication",
        ),
    ] {
        let writes = Arc::new(AtomicUsize::new(0));
        let observed = writes.clone();
        let router = Router::new()
            .route(
                "/api/v1/me/provider-credentials",
                get(move || {
                    let providers = providers.clone();
                    async { Json(providers) }
                }),
            )
            .fallback(move || {
                observed.fetch_add(1, Ordering::Relaxed);
                async { StatusCode::INTERNAL_SERVER_ERROR }
            });
        let (client, _server) = server(router, "").await;
        let error = store_token(&client, requested, TOKEN).await.unwrap_err();
        assert!(error.to_string().contains(message));
        assert_private_error(error);
        assert_eq!(writes.load(Ordering::Relaxed), 0);
    }
}

#[tokio::test]
async fn secret_request_errors_never_display_response_bodies_or_decoder_values() {
    for (status, body) in [
        (StatusCode::UNAUTHORIZED, format!("{TOKEN} {GATEWAY_KEY}")),
        (StatusCode::FORBIDDEN, format!("{TOKEN} {GATEWAY_KEY}")),
        (StatusCode::NOT_FOUND, format!("{TOKEN} {GATEWAY_KEY}")),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("{TOKEN} {GATEWAY_KEY}"),
        ),
        (
            StatusCode::OK,
            format!(r#"{{"provider_key":"copilot-main","configured":"{TOKEN}"}}"#),
        ),
        (StatusCode::OK, TOKEN.to_owned()),
        (StatusCode::OK, TOKEN.repeat(400_000)),
    ] {
        let router = Router::new().fallback(move || {
            let body = body.clone();
            async move { (status, body) }
        });
        let (client, _server) = server(router, "").await;
        let error = client
            .put_sensitive::<ProviderCredentialStatus>(
                CREDENTIAL_PATH,
                &SetTokenRequest { token: TOKEN },
            )
            .await
            .err()
            .unwrap();
        assert_private_error(error);
        let error = client
            .get_sensitive::<Vec<ProviderCredentialStatus>>(CREDENTIAL_PATH)
            .await
            .err()
            .unwrap();
        assert_private_error(error);
    }
}

#[tokio::test]
async fn mismatched_provider_or_unconfigured_response_cannot_report_success() {
    for (provider, configured) in [(TOKEN, true), ("copilot-main", false)] {
        let router = Router::new()
            .route(
                "/api/v1/me/provider-credentials",
                get(|| async {
                    Json(json!([{ "provider_key": "copilot-main", "configured": false }]))
                }),
            )
            .route(
                "/api/v1/me/provider-credentials/copilot-main",
                put(move || async move {
                    Json(json!({ "provider_key": provider, "configured": configured }))
                }),
            );
        let (client, _server) = server(router, "").await;
        assert_private_error(store_token(&client, None, TOKEN).await.unwrap_err());
    }
}

#[tokio::test]
async fn credential_redirects_are_not_followed() {
    let hits = Arc::new(AtomicUsize::new(0));
    let observed = hits.clone();
    let router = Router::new()
        .route(
            "/api/v1/me/provider-credentials",
            get(|| async {
                Json(json!([{ "provider_key": "copilot-main", "configured": false }]))
            }),
        )
        .route(
            "/api/v1/me/provider-credentials/copilot-main",
            put(|| async {
                Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header("location", "/target")
                    .body(Body::from(TOKEN))
                    .unwrap()
            }),
        )
        .route(
            "/target",
            put(move || {
                observed.fetch_add(1, Ordering::Relaxed);
                async { Json(json!({})) }
            }),
        );
    let (client, _server) = server(router, "").await;
    let error = store_token(&client, None, TOKEN).await.unwrap_err();
    assert!(error.to_string().contains("307"));
    assert_private_error(error);
    assert_eq!(hits.load(Ordering::Relaxed), 0);
}
