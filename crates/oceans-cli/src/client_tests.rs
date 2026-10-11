use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderMap, StatusCode, Uri},
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};

use super::Client;

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
        Client::new(url.parse().unwrap(), "gwk_test.secret")
            .unwrap()
            .scoped(&["skills"]),
        Server(task),
    )
}

#[tokio::test]
async fn requests_use_bearer_auth_and_preserve_gateway_path_prefix() {
    let router = Router::new().fallback(|headers: HeaderMap, uri: Uri| async move {
        assert_eq!(headers["authorization"], "Bearer gwk_test.secret");
        assert_eq!(uri.path(), "/gateway/api/v1/skills/by-name/alice/review");
        assert_eq!(uri.query(), Some("limit=100&offset=0"));
        Json(json!({"ok": true}))
    });
    let (client, _server) = server(router, "/gateway/").await;
    let result: Value = client
        .get_query(
            &["by-name", "alice", "review"],
            &[("limit", "100"), ("offset", "0")],
        )
        .await
        .unwrap();
    assert_eq!(result["ok"], true);
}

#[tokio::test]
async fn uploads_are_raw_zip_requests() {
    let router = Router::new().route(
        "/api/v1/skills",
        post(|headers: HeaderMap, body: Bytes| async move {
            assert_eq!(headers["content-type"], "application/zip");
            assert_eq!(body.as_ref(), b"zip bytes");
            Json(json!({"created": true}))
        }),
    );
    let (client, _server) = server(router, "").await;
    let result: Value = client.upload(&[], b"zip bytes".to_vec()).await.unwrap();
    assert_eq!(result["created"], true);
}

#[tokio::test]
async fn only_not_found_is_an_optional_missing_resource() {
    let router = Router::new()
        .route(
            "/api/v1/skills/missing",
            get(|| async { StatusCode::NOT_FOUND }),
        )
        .route(
            "/api/v1/skills/forbidden",
            get(|| async { (StatusCode::FORBIDDEN, "access denied") }),
        );
    let (client, _server) = server(router, "").await;
    assert!(
        client
            .get_optional::<Value>(&["missing"])
            .await
            .unwrap()
            .is_none()
    );
    let error = client
        .get_optional::<Value>(&["forbidden"])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("403"));
    assert!(error.to_string().contains("access denied"));
}

#[tokio::test]
async fn redirects_are_not_followed_and_error_bodies_are_bounded() {
    let hits = Arc::new(AtomicUsize::new(0));
    let observed = hits.clone();
    let router = Router::new()
        .route(
            "/api/v1/skills/redirect",
            get(|| async {
                Response::builder()
                    .status(StatusCode::TEMPORARY_REDIRECT)
                    .header("location", "/api/v1/skills/target")
                    .body(Body::empty())
                    .unwrap()
            }),
        )
        .route(
            "/api/v1/skills/target",
            get(move || async move {
                observed.fetch_add(1, Ordering::Relaxed);
                Json(json!({}))
            }),
        )
        .route(
            "/api/v1/skills/error",
            get(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    format!("gwk_test.secret{}", "x".repeat(100_000)),
                )
            }),
        );
    let (client, _server) = server(router, "").await;
    assert!(
        client
            .get::<Value>(&["redirect"])
            .await
            .unwrap_err()
            .to_string()
            .contains("307")
    );
    assert_eq!(hits.load(Ordering::Relaxed), 0);
    let error = client
        .get::<Value>(&["error"])
        .await
        .unwrap_err()
        .to_string();
    assert!(error.len() < 9_000);
    assert!(!error.contains("gwk_test.secret"));
}

#[tokio::test]
async fn downloads_enforce_streamed_body_limit_without_content_length() {
    let router = Router::new().fallback(|_request: Request| async {
        let stream = futures_stream();
        Response::builder()
            .header("transfer-encoding", "chunked")
            .body(stream)
            .unwrap()
    });
    let (client, _server) = server(router, "").await;
    assert!(
        client
            .download(&["archive"], 3)
            .await
            .unwrap_err()
            .to_string()
            .contains("3-byte limit")
    );
}

fn futures_stream() -> Body {
    // An unknown-size body forces validation of bytes actually read, not just Content-Length.
    Body::from_stream(futures_util::stream::iter([Ok::<_, std::io::Error>(
        Bytes::from_static(b"too large"),
    )]))
}

#[test]
fn gateway_url_rejects_embedded_credentials_and_secret_queries() {
    for url in [
        "https://user:password@example.com",
        "https://example.com?token=secret",
        "file:///tmp/api",
    ] {
        assert!(Client::new(url.parse().unwrap(), "gwk_test.secret").is_err());
    }
    assert!(Client::new("https://example.com".parse().unwrap(), "bad\nvalue").is_err());
}

#[test]
fn plaintext_gateway_urls_require_loopback_ip_literals() {
    for url in [
        "http://gateway.example.com",
        "http://192.0.2.1",
        "http://10.0.0.1",
        "http://0.0.0.0",
        "http://[::]",
        "http://[2001:db8::1]",
        "http://[::ffff:192.0.2.1]",
        "http://localhost",
        "http://localhost.",
        "http://localhost.example.com",
        "http://127.0.0.1.example.com",
        "http://example.localhost",
    ] {
        assert!(
            Client::new(url.parse().unwrap(), "gwk_test.secret").is_err(),
            "must reject {url} before sending credentials"
        );
    }
    for url in [
        "https://gateway.example.com",
        "https://192.0.2.1",
        "http://127.0.0.1:8080",
        "http://127.23.45.67",
        "http://[::1]:8080",
        "http://127.1",
        "http://2130706433",
        "http://0x7f000001",
    ] {
        assert!(
            Client::new(url.parse().unwrap(), "gwk_test.secret").is_ok(),
            "must accept {url} after parsed host validation"
        );
    }
}
