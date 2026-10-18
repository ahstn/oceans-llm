//! Rate limits on governed MCP calls: `tools/call` on the per-server proxy,
//! aggregate `call_tool`, and the governed resources, prompts and completion
//! methods. They share the server's `mcp_tool_call` policies; the persisted
//! operation keeps that name. A rejection is a JSON-RPC error with HTTP 429;
//! the request-policy middleware adds `Retry-After` and `X-RateLimit-*`.

use axum::{
    body::Body,
    http::{Response, StatusCode},
};
use gateway_core::{
    AuthenticatedApiKey, ExternalMcpServerRecord, GatewayError, RateLimitOperation,
};
use gateway_mcp::server::json_rpc_error_with_id;
use gateway_service::{RateLimitStatus, RateLimitSubject};
use serde_json::{Map, Value, json};

use super::json_rpc::mcp_refusal_response;
use crate::http::{
    request_policy::{RequestPolicyContext, check_rate_limits},
    state::AppState,
};

pub(super) use gateway_mcp::server::{JSON_RPC_INTERNAL_ERROR, JSON_RPC_RATE_LIMITED};
/// The invocation log's `error_code` for a rate-limited call.
pub(super) const RATE_LIMITED_ERROR_CODE: &str = "rate_limited";

/// Checks the server's `mcp_tool_call` policies for one governed MCP call.
pub(super) async fn check_mcp_call(
    state: &AppState,
    policy: &RequestPolicyContext,
    auth: &AuthenticatedApiKey,
    server: &ExternalMcpServerRecord,
) -> Result<Option<RateLimitStatus>, GatewayError> {
    let subject = RateLimitSubject::new(RateLimitOperation::McpToolCall, auth)
        .with_mcp_server(server.mcp_server_id, &server.server_key);
    check_rate_limits(state, policy, &subject).await
}

/// The rejection's JSON-RPC error `data`, the same on both routes. It
/// repeats the retry delay for clients that do not read HTTP headers.
fn rejection_data(status: &RateLimitStatus) -> Value {
    json!({
        "error_code": RATE_LIMITED_ERROR_CODE,
        "retry_after_seconds": status.retry_after_seconds,
    })
}

/// The aggregate route's JSON-RPC error body.
pub(super) fn rejection_body(id: Value, status: &RateLimitStatus) -> Value {
    json_rpc_error_with_id(
        Some(id),
        JSON_RPC_RATE_LIMITED,
        status.rejection_message(),
        Some(rejection_data(status)),
    )
}

/// The per-server proxy's response, with HTTP 429.
pub(super) fn rejection_response(id: Option<&Value>, status: &RateLimitStatus) -> Response<Body> {
    mcp_refusal_response(
        StatusCode::TOO_MANY_REQUESTS,
        id,
        JSON_RPC_RATE_LIMITED,
        &status.rejection_message(),
        Some(rejection_data(status)),
    )
}

/// The JSON-RPC error for a limiter that cannot check a call in `closed`
/// mode, with its HTTP status (503).
pub(super) fn unavailable_response(id: Option<&Value>, error: &GatewayError) -> Response<Body> {
    let status =
        StatusCode::from_u16(error.http_status_code()).unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
    mcp_refusal_response(
        status,
        id,
        JSON_RPC_INTERNAL_ERROR,
        &error.to_string(),
        None,
    )
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
