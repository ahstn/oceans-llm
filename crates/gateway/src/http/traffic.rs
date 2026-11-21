//! Per-process response counters for rollout checks.
//!
//! The gateway counts every response except health probes, and splits server
//! errors by cause: an upstream provider failing or timing out, or the
//! gateway itself. Client errors (4xx) are never errors here. `gateway
//! rollout-guard` reads the counts from the management listener.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
    routing::get,
};
use gateway_core::GatewayError;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::shutdown::Shutdown;

/// Paths that probes call; they would hide a change in the error rate.
const UNCOUNTED_PATHS: &[&str] = &["/healthz", "/readyz", "/api/v1/health"];

/// Response extension marking a server error an upstream provider caused.
#[derive(Debug, Clone, Copy)]
pub struct UpstreamFailure;

/// Marks `response` as an upstream failure when `error` is one.
pub fn mark_upstream_failure(response: &mut Response, error: &GatewayError) {
    if is_upstream_failure(error) {
        response.extensions_mut().insert(UpstreamFailure);
    }
}

/// Provider failures, timeouts, and provider error statuses. A request the
/// provider rejected as invalid is a client error, not an upstream failure.
#[must_use]
pub fn is_upstream_failure(error: &GatewayError) -> bool {
    error.error_type() == "upstream_error"
}

/// Counts since this process started.
#[derive(Debug)]
pub struct TrafficCounters {
    instance_id: Uuid,
    started_at: i64,
    requests: AtomicU64,
    gateway_errors: AtomicU64,
    upstream_errors: AtomicU64,
}

/// What `/internal/traffic` returns. Counts are cumulative, so a reader
/// compares two snapshots with the same `instance_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrafficSnapshot {
    pub instance_id: Uuid,
    pub version: String,
    /// Unix seconds.
    pub started_at: i64,
    pub draining: bool,
    pub requests: u64,
    pub gateway_errors: u64,
    pub upstream_errors: u64,
}

impl Default for TrafficCounters {
    fn default() -> Self {
        Self {
            instance_id: Uuid::new_v4(),
            started_at: OffsetDateTime::now_utc().unix_timestamp(),
            requests: AtomicU64::new(0),
            gateway_errors: AtomicU64::new(0),
            upstream_errors: AtomicU64::new(0),
        }
    }
}

impl TrafficCounters {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record(&self, status: StatusCode, upstream_failure: bool) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        if !status.is_server_error() {
            return;
        }
        let counter = if upstream_failure {
            &self.upstream_errors
        } else {
            &self.gateway_errors
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub fn snapshot(&self, draining: bool) -> TrafficSnapshot {
        TrafficSnapshot {
            instance_id: self.instance_id,
            version: env!("CARGO_PKG_VERSION").to_string(),
            started_at: self.started_at,
            draining,
            requests: self.requests.load(Ordering::Relaxed),
            gateway_errors: self.gateway_errors.load(Ordering::Relaxed),
            upstream_errors: self.upstream_errors.load(Ordering::Relaxed),
        }
    }
}

/// Middleware that counts each response by its status and cause.
pub async fn count_responses(
    State(counters): State<Arc<TrafficCounters>>,
    request: Request,
    next: Next,
) -> Response {
    let counted = !UNCOUNTED_PATHS.contains(&request.uri().path());
    let response = next.run(request).await;
    if counted {
        counters.record(
            response.status(),
            response.extensions().get::<UpstreamFailure>().is_some(),
        );
    }
    response
}

/// The management listener's routes. It carries no credentials or request
/// data, but it should stay off public ingress.
pub fn management_router(counters: Arc<TrafficCounters>, shutdown: Shutdown) -> Router {
    Router::new()
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({ "status": "ok" })) }),
        )
        .route(
            "/internal/traffic",
            get(move || async move { Json(counters.snapshot(shutdown.is_draining())) }),
        )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        middleware,
        response::IntoResponse,
        routing::get,
    };
    use gateway_core::{GatewayError, ProviderError};
    use tower::ServiceExt;

    use super::{TrafficCounters, count_responses, management_router, mark_upstream_failure};
    use crate::shutdown::Shutdown;

    fn error_response(error: GatewayError) -> axum::response::Response {
        let status = StatusCode::from_u16(error.http_status_code()).expect("status");
        let mut response = status.into_response();
        mark_upstream_failure(&mut response, &error);
        response
    }

    #[tokio::test]
    async fn management_returns_only_process_metadata_and_aggregate_counters() {
        let counters = Arc::new(TrafficCounters::new());
        counters.record(StatusCode::BAD_GATEWAY, true);
        let expected = counters.snapshot(false);
        let app = management_router(counters.clone(), Shutdown::new());
        let response = app
            .oneshot(
                Request::get("/internal/traffic")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("snapshot JSON");
        assert_eq!(value, serde_json::to_value(expected).expect("snapshot"));
        assert_eq!(value.as_object().expect("object").len(), 7);
        assert_eq!(
            counters.snapshot(false).requests,
            1,
            "management is uncounted"
        );
    }

    #[tokio::test]
    async fn counts_server_errors_by_cause_and_skips_probes_and_client_errors() {
        let counters = Arc::new(TrafficCounters::new());
        let app = Router::new()
            .route("/ok", get(|| async { StatusCode::OK }))
            .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
            .route("/forbidden", get(|| async { StatusCode::FORBIDDEN }))
            .route(
                "/internal",
                get(|| async { error_response(GatewayError::Internal("bug".into())) }),
            )
            .route(
                "/timeout",
                get(|| async { error_response(GatewayError::Provider(ProviderError::Timeout)) }),
            )
            .route(
                "/rejected",
                get(|| async {
                    error_response(GatewayError::Provider(ProviderError::InvalidRequest(
                        "bad".into(),
                    )))
                }),
            )
            .route("/readyz", get(|| async { StatusCode::SERVICE_UNAVAILABLE }))
            .layer(middleware::from_fn_with_state(
                counters.clone(),
                count_responses,
            ));

        for path in [
            "/ok",
            "/missing",
            "/forbidden",
            "/internal",
            "/timeout",
            "/rejected",
            "/readyz",
        ] {
            let request = Request::get(path).body(Body::empty()).expect("request");
            app.clone().oneshot(request).await.expect("response");
        }

        let snapshot = counters.snapshot(false);
        assert_eq!(snapshot.requests, 6, "probe paths are not counted");
        assert_eq!(snapshot.gateway_errors, 1);
        assert_eq!(snapshot.upstream_errors, 1);
    }
}
