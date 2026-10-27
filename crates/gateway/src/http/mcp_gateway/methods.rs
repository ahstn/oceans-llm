//! Per-server proxy handling for MCP methods outside `tools/*`.
//!
//! Resources, prompts, and completions expose server data just as tools do, so
//! they need at least one effective grant on the server, and then go through
//! the same governance steps as `tools/call` (see `governance`). Unknown
//! methods are denied rather than forwarded.

use std::time::Instant;

use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, Method, Response, StatusCode},
};
use gateway_core::{
    AuthenticatedApiKey, ExternalMcpServerRecord, GatewayError, McpToolInvocationStatus,
    McpToolPolicyResult,
};
use gateway_mcp::server::{JSON_RPC_METHOD_NOT_FOUND, JSON_RPC_POLICY_DENIED};
use gateway_service::{McpAccess, McpGatewayService};
use serde_json::Value;
use uuid::Uuid;

use super::{
    credentials::upstream_auth_error_response,
    finish_proxied_call,
    governance::GovernedCall,
    guardrails::enforce_direct_mcp_result,
    json_rpc::{
        GovernedMcpMethod, mcp_jsonrpc_error_response, mcp_notification_error_response,
        mcp_request_id,
    },
    mcp_error_response, tool_invocation_log_input,
    upstream::{proxy_initialize, proxy_upstream},
};
use crate::http::{request_policy::RequestPolicyContext, state::AppState};

/// A parsed resources, prompts, or completion request.
pub(super) struct GovernedRequest {
    pub id: Option<Value>,
    pub method: GovernedMcpMethod,
    pub target: Option<String>,
    pub params: Option<Value>,
}

async fn server_has_grant(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    server: &ExternalMcpServerRecord,
) -> Result<bool, GatewayError> {
    McpAccess::new(state.store.clone())
        .has_server_grant(auth, server.mcp_server_id)
        .await
}

fn server_not_granted_response(id: Option<&Value>) -> Response<Body> {
    const MESSAGE: &str = "MCP server is not granted for this API key";
    match id {
        Some(id) => mcp_jsonrpc_error_response(
            StatusCode::FORBIDDEN,
            Some(id),
            JSON_RPC_POLICY_DENIED,
            MESSAGE,
        ),
        None => {
            mcp_notification_error_response(StatusCode::FORBIDDEN, JSON_RPC_POLICY_DENIED, MESSAGE)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_governed_request(
    state: &AppState,
    policy: &RequestPolicyContext,
    auth: &AuthenticatedApiKey,
    http_method: &Method,
    headers: &HeaderMap,
    body: Bytes,
    server: ExternalMcpServerRecord,
    started_at: Instant,
    request: GovernedRequest,
) -> Response<Body> {
    let mcp_method = request.method.as_str();
    let display_key = request.target.as_deref().unwrap_or(mcp_method);
    let call = GovernedCall::new(
        state,
        policy,
        auth,
        &server,
        Uuid::new_v4(),
        mcp_request_id(&request.id),
    );
    let log_input = |status, policy_result, error_code: Option<String>| {
        tool_invocation_log_input(
            &server,
            mcp_method,
            &request.id,
            Some(call.invocation_id),
            None,
            display_key,
            display_key,
            status,
            policy_result,
            error_code,
            request.params.clone(),
            None,
            started_at,
        )
    };

    match server_has_grant(state, auth, &server).await {
        Ok(true) => {}
        Ok(false) => {
            let _ = state
                .service
                .log_mcp_tool_invocation(
                    auth,
                    log_input(
                        McpToolInvocationStatus::PolicyDenied,
                        McpToolPolicyResult::Denied,
                        Some("mcp_server_not_granted".to_string()),
                    ),
                )
                .await;
            return server_not_granted_response(request.id.as_ref());
        }
        Err(error) => return mcp_error_response(error),
    }

    let upstream = match call.admit(None).await {
        Ok(admitted) => admitted.upstream,
        Err(refused) => {
            let refusal = refused.refusal;
            let refused = refusal.log();
            let mut entry = log_input(
                refused.status,
                refused.policy_result,
                Some(refused.error_code),
            );
            entry.metadata = refused.metadata;
            let _ = state.service.log_mcp_tool_invocation(auth, entry).await;
            return refusal.direct_response(state, auth, &server, request.id.as_ref(), false);
        }
    };

    let outcome = match proxy_upstream(
        &state.mcp_http_clients,
        http_method,
        headers,
        body,
        &upstream,
    )
    .await
    {
        Ok(response) if request.method.guards_result() => {
            enforce_direct_mcp_result(
                state,
                &upstream.server.server_key,
                mcp_method,
                request.id.as_ref(),
                call.invocation_id,
                response,
            )
            .await
        }
        Ok(response) => Ok((response, None, false)),
        Err(error) => Err(error),
    };
    finish_proxied_call(state, auth, outcome, log_input).await
}

/// `GET` opens the server-to-client SSE stream. It can carry resource update
/// notifications and server requests, so it needs the same server grant as
/// resources and prompts. The stream is long-lived and is not logged per call.
pub(super) async fn handle_event_stream(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    http_method: &Method,
    headers: &HeaderMap,
    body: Bytes,
    server: ExternalMcpServerRecord,
) -> Response<Body> {
    match server_has_grant(state, auth, &server).await {
        Ok(true) => {}
        Ok(false) => return server_not_granted_response(None),
        Err(error) => return mcp_error_response(error),
    }
    // `dispatch_mcp_request` binds the stream to the caller's reach.
    proxy_lifecycle(state, auth, http_method, headers, body, server, false).await
}

/// Forwards session plumbing that carries no server data. `initialize` is
/// buffered so the gateway can drop capabilities it does not proxy.
pub(super) async fn proxy_lifecycle(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    http_method: &Method,
    headers: &HeaderMap,
    body: Bytes,
    server: ExternalMcpServerRecord,
    is_initialize: bool,
) -> Response<Body> {
    let gateway = McpGatewayService::new(
        state.store.clone(),
        state.mcp_http_clients.url_policy().clone(),
    )
    .with_oauth_runtime(state.mcp_oauth_runtime.clone());
    let upstream = match gateway
        .prepare_upstream_for_auth(auth, server.clone())
        .await
    {
        Ok(upstream) => upstream,
        Err(error) => return upstream_auth_error_response(state, auth, &server, error),
    };
    let client = &state.mcp_http_clients;
    let response = if is_initialize {
        proxy_initialize(client, http_method, headers, body, &upstream).await
    } else {
        proxy_upstream(client, http_method, headers, body, &upstream).await
    };
    response.unwrap_or_else(mcp_error_response)
}

pub(super) async fn deny_unknown_method(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    server: &ExternalMcpServerRecord,
    started_at: Instant,
    id: Option<Value>,
    method: String,
) -> Response<Body> {
    let _ = state
        .service
        .log_mcp_tool_invocation(
            auth,
            tool_invocation_log_input(
                server,
                &method,
                &id,
                None,
                None,
                &method,
                &method,
                McpToolInvocationStatus::PolicyDenied,
                McpToolPolicyResult::Denied,
                Some("mcp_method_not_allowed".to_string()),
                None,
                None,
                started_at,
            ),
        )
        .await;
    let message = format!("MCP method `{method}` is not allowed through the gateway");
    match id {
        Some(id) => mcp_jsonrpc_error_response(
            StatusCode::FORBIDDEN,
            Some(&id),
            JSON_RPC_METHOD_NOT_FOUND,
            &message,
        ),
        None => mcp_notification_error_response(
            StatusCode::FORBIDDEN,
            JSON_RPC_METHOD_NOT_FOUND,
            &message,
        ),
    }
}
