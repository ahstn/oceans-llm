//! Rate limits on `tools/call`, shared by the per-server proxy and the
//! aggregate `call_tool`. A rejection is a JSON-RPC error with HTTP 429; the
//! request-policy middleware adds `Retry-After` and `X-RateLimit-*`.

use axum::{
    Json,
    body::Body,
    http::{HeaderValue, Response, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
};
use gateway_core::{
    AuthenticatedApiKey, ExternalMcpServerRecord, GatewayError, RateLimitOperation,
};
use gateway_service::{RateLimitStatus, RateLimitSubject};
use serde_json::{Map, Value, json};

use crate::http::{
    request_policy::{RequestPolicyContext, check_rate_limits},
    state::AppState,
};

/// Implementation-defined JSON-RPC server error for "rate limited".
pub(super) const JSON_RPC_RATE_LIMITED: i64 = -32029;
/// The invocation log's `error_code` for a rate-limited call.
pub(super) const RATE_LIMITED_ERROR_CODE: &str = "rate_limited";

pub(super) async fn check_tool_call(
    state: &AppState,
    policy: &RequestPolicyContext,
    auth: &AuthenticatedApiKey,
    server: &ExternalMcpServerRecord,
) -> Result<Option<RateLimitStatus>, GatewayError> {
    let subject = RateLimitSubject::new(RateLimitOperation::McpToolCall, auth)
        .with_mcp_server(server.mcp_server_id, &server.server_key);
    check_rate_limits(state, policy, &subject).await
}

/// The JSON-RPC error body. `data` repeats the retry delay for clients that
/// do not read HTTP headers.
pub(super) fn rejection_body(id: Value, status: &RateLimitStatus) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": JSON_RPC_RATE_LIMITED,
            "message": status.rejection_message(),
            "data": {
                "error_code": RATE_LIMITED_ERROR_CODE,
                "retry_after_seconds": status.retry_after_seconds,
            },
        }
    })
}

pub(super) fn rejection_response(id: Option<&Value>, status: &RateLimitStatus) -> Response<Body> {
    let body = rejection_body(id.cloned().unwrap_or(Value::Null), status);
    let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

/// Invocation-log metadata naming the policy that rejected the call.
pub(super) fn rejection_metadata(status: &RateLimitStatus) -> Map<String, Value> {
    Map::from_iter([(
        "rate_limit".to_string(),
        json!({
            "policy": status.policy_name,
            "scope_kind": status.scope_kind.as_str(),
            "metric": status.metric.as_str(),
            "period": status.period.as_str(),
            "limit": status.limit,
            "retry_after_seconds": status.retry_after_seconds,
        }),
    )])
}
