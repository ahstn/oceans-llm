use super::*;

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(previous) => unsafe {
                std::env::set_var(self.key, previous);
            },
            None => unsafe {
                std::env::remove_var(self.key);
            },
        }
    }
}

#[tokio::test]
#[serial]
async fn mcp_gateway_returns_401_before_proxying_when_unauthenticated() {
    let (app, _, _) = build_default_test_app(gateway_core::ProviderRegistry::new()).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/github")
                .header("content-type", "application/json")
                .body(Body::from(json!({"jsonrpc": "2.0"}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get("www-authenticate")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer")
    );
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(payload["error"]["code"], "missing_authorization_header");
}

#[tokio::test]
#[serial]
async fn mcp_gateway_proxies_active_server_and_strips_gateway_credentials() {
    let captured_headers = Arc::new(std::sync::Mutex::new(None::<HeaderMap>));
    let upstream_capture = captured_headers.clone();
    let upstream = Router::new().route(
        "/mcp",
        any(move |headers: HeaderMap| {
            let upstream_capture = upstream_capture.clone();
            async move {
                *upstream_capture.lock().expect("capture lock") = Some(headers);
                Response::builder()
                    .status(StatusCode::ACCEPTED)
                    .header("content-type", "text/event-stream")
                    .header("mcp-session-id", "sess_123")
                    .body(Body::from("event: message\ndata: {}\n\n"))
                    .expect("response")
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("serve upstream");
    });

    let (app, raw_key, db_path) =
        build_default_test_app(gateway_core::ProviderRegistry::new()).await;
    let _env_guard = EnvVarGuard::set("OCEANS_MCP_DISCOVERY_PROXY_TEST_KEY", "upstream-secret");
    insert_mcp_server(
        &db_path,
        "github",
        &format!("http://{addr}/mcp"),
        "gateway_static_header",
        json!({
            "header_name": "X-Upstream-Key",
            "secret_ref": "env/OCEANS_MCP_DISCOVERY_PROXY_TEST_KEY"
        }),
        "active",
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/github")
                .header("authorization", format!("Bearer {raw_key}"))
                .header("x-oceans-api-key", &raw_key)
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-protocol-version", "2025-11-25")
                .body(Body::from(json!({"jsonrpc": "2.0"}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    assert_eq!(
        response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok()),
        Some("sess_123")
    );
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    assert_eq!(
        String::from_utf8(body.to_vec()).expect("utf8"),
        "event: message\ndata: {}\n\n"
    );

    let headers = captured_headers
        .lock()
        .expect("capture lock")
        .clone()
        .expect("upstream captured headers");
    assert!(headers.get("authorization").is_none());
    assert!(headers.get("x-oceans-api-key").is_none());
    assert_eq!(
        headers
            .get("x-upstream-key")
            .and_then(|value| value.to_str().ok()),
        Some("upstream-secret")
    );
    assert_eq!(
        headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok()),
        Some("2025-11-25")
    );
}

#[tokio::test]
#[serial]
async fn mcp_gateway_rejects_query_strings_before_proxying() {
    let (app, raw_key, _) = build_default_test_app(gateway_core::ProviderRegistry::new()).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/github?token=upstream-secret")
                .header("authorization", format!("Bearer {raw_key}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"jsonrpc": "2.0"}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(payload["error"]["code"], "invalid_request");
}

#[tokio::test]
#[serial]
async fn mcp_gateway_rejects_oversized_request_bodies() {
    let (app, raw_key, db_path) =
        build_default_test_app(gateway_core::ProviderRegistry::new()).await;
    insert_mcp_server(
        &db_path,
        "github",
        "http://127.0.0.1:1/mcp",
        "none",
        json!({}),
        "active",
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/github")
                .header("authorization", format!("Bearer {raw_key}"))
                .header("content-type", "application/json")
                .body(Body::from(vec![b'a'; 4 * 1024 * 1024 + 1]))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(payload["error"]["code"], "request_body_too_large");
}

#[tokio::test]
#[serial]
async fn mcp_gateway_get_sse_is_not_capped_by_server_request_timeout() {
    let upstream = Router::new().route(
        "/mcp",
        any(|| async {
            sleep(Duration::from_millis(50)).await;
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(Body::from("event: message\ndata: {}\n\n"))
                .expect("response")
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("serve upstream");
    });

    let (app, raw_key, db_path) =
        build_default_test_app(gateway_core::ProviderRegistry::new()).await;
    insert_mcp_server_with_timeout(
        &db_path,
        "github",
        &format!("http://{addr}/mcp"),
        "none",
        json!({}),
        "active",
        1,
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/mcp/github")
                .header("authorization", format!("Bearer {raw_key}"))
                .header("accept", "text/event-stream")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    assert_eq!(
        String::from_utf8(body.to_vec()).expect("utf8"),
        "event: message\ndata: {}\n\n"
    );
}

#[tokio::test]
#[serial]
async fn mcp_gateway_post_sse_is_not_capped_by_server_request_timeout() {
    let upstream = Router::new().route(
        "/mcp",
        any(|| async {
            sleep(Duration::from_millis(50)).await;
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(Body::from("event: message\ndata: {}\n\n"))
                .expect("response")
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("serve upstream");
    });

    let (app, raw_key, db_path) =
        build_default_test_app(gateway_core::ProviderRegistry::new()).await;
    insert_mcp_server_with_timeout(
        &db_path,
        "github",
        &format!("http://{addr}/mcp"),
        "none",
        json!({}),
        "active",
        1,
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/github")
                .header("authorization", format!("Bearer {raw_key}"))
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .body(Body::from(json!({"jsonrpc": "2.0"}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    assert_eq!(
        String::from_utf8(body.to_vec()).expect("utf8"),
        "event: message\ndata: {}\n\n"
    );
}

#[tokio::test]
#[serial]
async fn mcp_gateway_forwards_configured_bearer_token_to_upstream() {
    let captured_headers = Arc::new(std::sync::Mutex::new(None::<HeaderMap>));
    let upstream_capture = captured_headers.clone();
    let upstream = Router::new().route(
        "/mcp",
        any(move |headers: HeaderMap| {
            let upstream_capture = upstream_capture.clone();
            async move {
                *upstream_capture.lock().expect("capture lock") = Some(headers);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("response")
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("serve upstream");
    });

    let (app, raw_key, db_path) =
        build_default_test_app(gateway_core::ProviderRegistry::new()).await;
    let _env_guard = EnvVarGuard::set("OCEANS_MCP_DISCOVERY_BEARER_TEST_TOKEN", "upstream-token");
    insert_mcp_server(
        &db_path,
        "github",
        &format!("http://{addr}/mcp"),
        "gateway_bearer_token",
        json!({
            "secret_ref": "env/OCEANS_MCP_DISCOVERY_BEARER_TEST_TOKEN"
        }),
        "active",
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/github")
                .header("authorization", format!("Bearer {raw_key}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"jsonrpc": "2.0"}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let headers = captured_headers
        .lock()
        .expect("capture lock")
        .clone()
        .expect("upstream captured headers");
    assert_eq!(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer upstream-token")
    );
}

#[tokio::test]
#[serial]
async fn mcp_gateway_proxies_user_owned_key_to_active_server() {
    let upstream = Router::new().route(
        "/mcp",
        any(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("response")
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    tokio::spawn(async move {
        axum::serve(listener, upstream)
            .await
            .expect("serve upstream");
    });

    let (app, raw_key, db_path) =
        build_default_test_app(gateway_core::ProviderRegistry::new()).await;
    set_api_key_owner_to_user(&db_path, &raw_key, false).await;
    insert_mcp_server(
        &db_path,
        "github",
        &format!("http://{addr}/mcp"),
        "none",
        json!({}),
        "active",
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/github")
                .header("authorization", format!("Bearer {raw_key}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"jsonrpc": "2.0"}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn mcp_gateway_requires_user_scoped_upstream_auth_for_obo_modes() {
    let (app, raw_key, db_path) =
        build_default_test_app(gateway_core::ProviderRegistry::new()).await;
    insert_mcp_server(
        &db_path,
        "github",
        "https://example.test/mcp",
        "oauth_obo",
        json!({}),
        "active",
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp/github")
                .header("authorization", format!("Bearer {raw_key}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"jsonrpc": "2.0"}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(payload["error"]["code"], "mcp_upstream_auth_required");
}
