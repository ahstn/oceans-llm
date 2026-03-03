use std::{env, net::SocketAddr};

use admin_ui::{AdminUiConfig, mount_admin_ui};
use axum::{Json, Router, routing::get};
use serde_json::json;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    init_tracing();

    let port = env_u16("PORT", 8080);
    let address = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(address)
        .await
        .expect("gateway listener must bind");

    let app = build_app(load_admin_ui_config());

    axum::serve(listener, app)
        .await
        .expect("gateway server should run");
}

fn build_app(admin_ui: AdminUiConfig) -> Router {
    let router = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/health", get(api_health));

    mount_admin_ui(router, admin_ui)
}

fn load_admin_ui_config() -> AdminUiConfig {
    AdminUiConfig {
        base_path: env::var("ADMIN_UI_BASE_PATH").unwrap_or_else(|_| "/admin".to_string()),
        upstream: env::var("ADMIN_UI_UPSTREAM")
            .unwrap_or_else(|_| "http://127.0.0.1:3001".to_string()),
        connect_timeout_ms: env_u64("ADMIN_UI_CONNECT_TIMEOUT_MS", 750),
        request_timeout_ms: env_u64("ADMIN_UI_REQUEST_TIMEOUT_MS", 10_000),
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_u16(key: &str, default: u16) -> u16 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(default)
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn api_health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "gateway" }))
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        "gateway=info,admin_ui=info"
            .parse()
            .expect("filter should parse")
    });

    tracing_subscriber::fmt().with_env_filter(filter).init();
}

#[cfg(test)]
mod tests {
    use admin_ui::AdminUiConfig;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    use crate::build_app;

    #[tokio::test]
    async fn api_routes_are_not_swallowed_by_ui_proxy() {
        let app = build_app(AdminUiConfig {
            upstream: "http://127.0.0.1:9".to_string(),
            ..AdminUiConfig::default()
        });

        let request = Request::builder()
            .method("GET")
            .uri("/api/v1/health")
            .body(Body::empty())
            .expect("request should build");

        let response = app.oneshot(request).await.expect("response should return");

        assert_eq!(response.status(), StatusCode::OK);
    }
}
