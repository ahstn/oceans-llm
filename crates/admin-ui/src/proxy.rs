use std::time::Duration;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::State,
    http::{
        HeaderMap, HeaderName, Request, StatusCode, Uri,
        header::{CONNECTION, HOST},
    },
    response::{IntoResponse, Response},
    routing::any,
};
use serde_json::json;
use tracing::warn;

use crate::AdminUiConfig;

#[derive(Clone)]
struct ProxyState {
    upstream: String,
    client: reqwest::Client,
}

impl ProxyState {
    fn new(config: &AdminUiConfig) -> Self {
        let upstream = config.upstream.trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(config.connect_timeout_ms))
            .timeout(Duration::from_millis(config.request_timeout_ms))
            .build()
            .expect("failed to build reqwest proxy client");

        Self { upstream, client }
    }
}

pub fn mount_admin_ui(router: Router, config: AdminUiConfig) -> Router {
    let base_path = normalize_base_path(&config.base_path);
    let wildcard_path = if base_path == "/" {
        "/{*path}".to_string()
    } else {
        format!("{base_path}/{{*path}}")
    };

    let admin_ui_router = Router::new()
        .route(&base_path, any(proxy_request))
        .route(&wildcard_path, any(proxy_request))
        .with_state(ProxyState::new(&config));

    router.merge(admin_ui_router)
}

async fn proxy_request(State(state): State<ProxyState>, req: Request<Body>) -> Response {
    let (parts, body) = req.into_parts();
    let method = parts.method;
    let uri = parts.uri;
    let inbound_headers = parts.headers;

    let body_bytes = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(error) => {
            warn!(%error, "failed to read incoming admin UI proxy request body");
            return ui_unavailable_response();
        }
    };

    let target_url = build_target_url(&state, &uri);
    let mut outbound = state.client.request(method, target_url);
    let forwarded_proto = inbound_headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    if let Some(host) = inbound_headers.get(HOST) {
        outbound = outbound
            .header("x-forwarded-host", host)
            .header("x-forwarded-proto", forwarded_proto)
            .header(
                "x-forwarded-origin",
                format!(
                    "{forwarded_proto}://{}",
                    host.to_str().unwrap_or("localhost")
                ),
            );
    }

    let connection_tokens = parse_connection_tokens(&inbound_headers);
    for (name, value) in &inbound_headers {
        if should_forward_request_header(name, &connection_tokens) {
            outbound = outbound.header(name, value);
        }
    }

    if !body_bytes.is_empty() {
        outbound = outbound.body(body_bytes);
    }

    let upstream_response = match outbound.send().await {
        Ok(response) => response,
        Err(error) => {
            warn!(%error, "admin UI upstream request failed");
            return ui_unavailable_response();
        }
    };

    let status = upstream_response.status();
    let upstream_headers = upstream_response.headers().clone();
    let upstream_body = match upstream_response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            warn!(%error, "failed reading admin UI upstream response body");
            return ui_unavailable_response();
        }
    };

    let connection_tokens = parse_connection_tokens(&upstream_headers);
    let mut response = Response::new(Body::from(upstream_body));
    *response.status_mut() = status;

    for (name, value) in &upstream_headers {
        if should_forward_response_header(name, &connection_tokens) {
            response.headers_mut().append(name, value.clone());
        }
    }

    response
}

fn build_target_url(state: &ProxyState, uri: &Uri) -> String {
    let path = uri.path();
    let query = uri
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();

    format!("{}{}{}", state.upstream, path, query)
}

fn normalize_base_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }

    let with_leading = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };

    with_leading.trim_end_matches('/').to_string()
}

fn parse_connection_tokens(headers: &HeaderMap) -> Vec<HeaderName> {
    headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|raw| raw.split(','))
        .filter_map(|token| HeaderName::from_bytes(token.trim().as_bytes()).ok())
        .collect()
}

fn should_forward_request_header(name: &HeaderName, connection_tokens: &[HeaderName]) -> bool {
    if name == HOST {
        return false;
    }

    !is_hop_by_hop_header(name) && !connection_tokens.iter().any(|token| token == name)
}

fn should_forward_response_header(name: &HeaderName, connection_tokens: &[HeaderName]) -> bool {
    !is_hop_by_hop_header(name) && !connection_tokens.iter().any(|token| token == name)
}

fn is_hop_by_hop_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn ui_unavailable_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": {
                "type": "ui_unavailable",
                "code": "admin_ui_upstream_unavailable",
                "message": "Admin UI upstream is unavailable"
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{HeaderMap, HeaderValue, Request, StatusCode, header::CONNECTION},
        response::Response,
        routing::any,
    };
    use serde::Serialize;
    use tokio::net::TcpListener;
    use tower::ServiceExt;

    use super::{AdminUiConfig, mount_admin_ui};

    #[derive(Debug, Clone, Serialize)]
    struct CapturedRequest {
        path_and_query: String,
        headers: Vec<(String, String)>,
    }

    #[tokio::test]
    async fn proxy_preserves_path_and_query() {
        let captured = Arc::new(Mutex::new(Vec::<CapturedRequest>::new()));
        let upstream = start_upstream(captured.clone()).await;

        let app = mount_admin_ui(
            Router::new(),
            AdminUiConfig {
                upstream,
                ..AdminUiConfig::default()
            },
        );

        let request = Request::builder()
            .uri("/admin/models?x=1")
            .method("GET")
            .body(Body::empty())
            .expect("request must build");

        let response = app
            .oneshot(request)
            .await
            .expect("proxy request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        let captured = captured.lock().expect("capture lock");
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].path_and_query, "/admin/models?x=1");
    }

    #[tokio::test]
    async fn proxy_strips_hop_by_hop_request_headers() {
        let captured = Arc::new(Mutex::new(Vec::<CapturedRequest>::new()));
        let upstream = start_upstream(captured.clone()).await;

        let app = mount_admin_ui(
            Router::new(),
            AdminUiConfig {
                upstream,
                ..AdminUiConfig::default()
            },
        );

        let mut request = Request::builder()
            .uri("/admin/models")
            .method("GET")
            .body(Body::empty())
            .expect("request must build");

        request.headers_mut().insert(
            CONNECTION,
            HeaderValue::from_static("keep-alive, x-drop-me"),
        );
        request
            .headers_mut()
            .insert("x-request-id", HeaderValue::from_static("req-123"));
        request
            .headers_mut()
            .insert("x-drop-me", HeaderValue::from_static("drop"));

        let response = app
            .oneshot(request)
            .await
            .expect("proxy request should succeed");
        assert_eq!(response.status(), StatusCode::OK);

        let captured = captured.lock().expect("capture lock");
        let header_map = to_header_map(&captured[0].headers);

        assert_eq!(
            header_map
                .get("x-request-id")
                .and_then(|value| value.to_str().ok()),
            Some("req-123")
        );
        assert!(header_map.get("connection").is_none());
        assert!(header_map.get("x-drop-me").is_none());
    }

    #[tokio::test]
    async fn proxy_returns_deterministic_503_when_upstream_unavailable() {
        let app = mount_admin_ui(
            Router::new(),
            AdminUiConfig {
                upstream: "http://127.0.0.1:9".to_string(),
                ..AdminUiConfig::default()
            },
        );

        let request = Request::builder()
            .uri("/admin/models")
            .method("GET")
            .body(Body::empty())
            .expect("request must build");

        let response = app
            .oneshot(request)
            .await
            .expect("proxy request should return 503 response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body to read");
        let body_string = String::from_utf8_lossy(&body);
        assert!(body_string.contains("admin_ui_upstream_unavailable"));
    }

    async fn start_upstream(captured: Arc<Mutex<Vec<CapturedRequest>>>) -> String {
        let app = Router::new().fallback(any(move |request: Request<Body>| {
            let captured = captured.clone();
            async move { record_request(captured, request).await }
        }));

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener must have local addr");

        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("upstream server should run");
        });

        format!("http://{address}")
    }

    async fn record_request(
        captured: Arc<Mutex<Vec<CapturedRequest>>>,
        request: Request<Body>,
    ) -> Response {
        let (parts, body) = request.into_parts();
        let _ = to_bytes(body, usize::MAX).await.expect("body should read");

        let path_and_query = parts
            .uri
            .path_and_query()
            .map(|value| value.as_str().to_string())
            .unwrap_or_else(|| parts.uri.path().to_string());

        let headers = parts
            .headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.to_string(), value.to_string()))
            })
            .collect::<Vec<_>>();

        captured
            .lock()
            .expect("capture lock")
            .push(CapturedRequest {
                path_and_query,
                headers,
            });

        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from("ok"))
            .expect("response should build")
    }

    fn to_header_map(entries: &[(String, String)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in entries {
            if let (Ok(name), Ok(value)) = (
                name.parse::<axum::http::HeaderName>(),
                value.parse::<HeaderValue>(),
            ) {
                headers.insert(name, value);
            }
        }

        headers
    }
}
