mod aggregate;
mod code_mode;
#[cfg(test)]
mod code_mode_tests;
mod json_rpc;
mod session;
mod upstream;

use std::{error::Error as _, time::Instant};

use axum::{
    Json,
    body::{Body, Bytes, to_bytes},
    extract::{Path, State},
    http::{
        HeaderMap, HeaderValue, Request, Response, StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
    },
    response::IntoResponse,
};
use gateway_core::{
    ApiKeyOwnerKind, AuthError, GatewayError, McpToolInvocationStatus, McpToolPolicyResult,
    OpenAiErrorEnvelope, ProviderError, auth::extract_bearer_token,
};
use gateway_service::{McpAccess, McpGatewayService, McpInvocationLogInput, McpInvocationLogging};
use json_rpc::{McpRpcRequest, mcp_jsonrpc_error_response, mcp_request_id, parse_mcp_rpc_request};
use serde_json::{Map, json};
use time::OffsetDateTime;
use upstream::{proxy_tools_list, proxy_upstream};
use uuid::Uuid;

use crate::http::state::AppState;

const X_OCEANS_API_KEY: &str = "x-oceans-api-key";
const MCP_PROTOCOL_VERSION: &str = "mcp-protocol-version";
const MCP_SESSION_ID: &str = "mcp-session-id";
const LAST_EVENT_ID: &str = "last-event-id";
const MAX_MCP_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_MCP_REWRITE_BODY_BYTES: u64 = 4 * 1024 * 1024;

#[tracing::instrument(
    skip(state, request),
    fields(
        server_key = %server_key,
        mcp_server_id = tracing::field::Empty,
        upstream_auth_mode = tracing::field::Empty,
        owner_kind = tracing::field::Empty,
        status_code = tracing::field::Empty,
    )
)]
pub async fn mcp_streamable_http_proxy(
    State(state): State<AppState>,
    Path(server_key): Path<String>,
    request: Request<Body>,
) -> Response<Body> {
    let started_at = Instant::now();
    let method = request.method().clone();
    let has_query = request.uri().query().is_some();
    let headers = request.headers().clone();

    let bearer_token = match extract_mcp_gateway_api_key(&headers) {
        Ok(token) => token,
        Err(error) => return mcp_error_response(error.into()),
    };

    let auth = match state.service.authenticate_bearer_token(&bearer_token).await {
        Ok(auth) => auth,
        Err(error) => return mcp_error_response(error),
    };
    tracing::Span::current().record("owner_kind", auth.owner_kind.as_str());
    if !matches!(
        auth.owner_kind,
        ApiKeyOwnerKind::User | ApiKeyOwnerKind::ServiceAccount
    ) {
        return mcp_error_response(AuthError::InsufficientPrivileges.into());
    }
    if has_query {
        return mcp_error_response(GatewayError::InvalidRequest(
            "query strings are not accepted on MCP gateway routes".to_string(),
        ));
    }

    let gateway = McpGatewayService::new(state.store.clone());
    let server = match gateway.load_active_server(&server_key).await {
        Ok(server) => server,
        Err(error) => return mcp_error_response(error),
    };
    tracing::Span::current().record("mcp_server_id", server.mcp_server_id.to_string());
    tracing::Span::current().record("upstream_auth_mode", server.auth_mode.as_str());

    let body = match to_bytes(request.into_body(), MAX_MCP_REQUEST_BODY_BYTES).await {
        Ok(body) => body,
        Err(error) if body_read_exceeded_limit(&error) => {
            return mcp_error_response(GatewayError::PayloadTooLarge {
                limit_bytes: MAX_MCP_REQUEST_BODY_BYTES,
            });
        }
        Err(error) => {
            return mcp_error_response(GatewayError::InvalidRequest(format!(
                "failed reading MCP request body: {error}"
            )));
        }
    };

    let rpc_request = parse_mcp_rpc_request(&body);
    let access = McpAccess::new(state.store.clone());
    let invocation_logger = McpInvocationLogging::new(state.store.clone());

    let response_result = match rpc_request {
        Ok(McpRpcRequest::ToolsList { id }) => {
            let access_resolution = match access
                .effective_tools_for_api_key(&auth, Some(server.mcp_server_id))
                .await
            {
                Ok(resolution) => resolution,
                Err(error) => return mcp_error_response(error),
            };
            let allowed_tool_names = access_resolution
                .allowed_tools
                .iter()
                .map(|tool| tool.upstream_name.as_str())
                .collect::<std::collections::HashSet<_>>();
            let upstream = match gateway
                .prepare_upstream_for_auth(&auth, server.clone())
                .await
            {
                Ok(upstream) => upstream,
                Err(error) => return mcp_error_response(error),
            };
            let outcome = proxy_tools_list(
                &state.mcp_http_client,
                &method,
                &headers,
                body,
                &upstream,
                &allowed_tool_names,
                id.as_ref(),
            )
            .await;
            let (status, error_code) = response_outcome(&outcome);
            let mut log_input = tool_invocation_log_input(
                &upstream,
                &id,
                None,
                "tools/list",
                "tools/list",
                status,
                McpToolPolicyResult::Allowed,
                error_code,
                None,
                None,
                started_at,
            );
            log_input.metadata = tools_list_metadata(
                access_resolution.referenced_server_count,
                access_resolution.allowed_tools.len() as i64,
                access_resolution.filtered_tool_count,
            );
            let _ = invocation_logger.log_invocation(&auth, log_input).await;
            outcome
        }
        Ok(McpRpcRequest::ToolsCall {
            id,
            tool_name,
            arguments,
        }) => {
            return handle_tools_call(
                &state, &auth, &method, &headers, body, server, started_at, id, tool_name,
                arguments,
            )
            .await;
        }
        Ok(McpRpcRequest::Other) => {
            let upstream = match gateway.prepare_upstream_for_auth(&auth, server).await {
                Ok(upstream) => upstream,
                Err(error) => return mcp_error_response(error),
            };
            proxy_upstream(&state.mcp_http_client, &method, &headers, body, &upstream).await
        }
        Err(error) => {
            let log_upstream = gateway_service::McpGatewayUpstream {
                server: server.clone(),
                headers: None,
            };
            let _ = invocation_logger
                .log_invocation(
                    &auth,
                    McpInvocationLogInput {
                        request_log_id: None,
                        parent_invocation_id: None,
                        request_id: Uuid::new_v4().to_string(),
                        server_id: Some(log_upstream.server.mcp_server_id),
                        server_display_key: log_upstream.server.server_key.clone(),
                        server_display_name: log_upstream.server.display_name.clone(),
                        tool_id: None,
                        tool_display_key: "unknown".to_string(),
                        tool_display_name: "unknown".to_string(),
                        status: McpToolInvocationStatus::InvalidRequest,
                        policy_result: McpToolPolicyResult::NotEvaluated,
                        latency_ms: Some(started_at.elapsed().as_millis() as i64),
                        error_code: Some("invalid_json_rpc".to_string()),
                        arguments_json: None,
                        result_json: None,
                        metadata: Map::new(),
                        occurred_at: OffsetDateTime::now_utc(),
                    },
                )
                .await;
            return mcp_jsonrpc_error_response(
                StatusCode::BAD_REQUEST,
                None,
                -32600,
                &error.to_string(),
            );
        }
    };

    match response_result {
        Ok(response) => {
            tracing::Span::current().record("status_code", i64::from(response.status().as_u16()));
            tracing::debug!(
                elapsed_ms = started_at.elapsed().as_millis(),
                "proxied MCP streamable HTTP request"
            );
            response
        }
        Err(error) => mcp_error_response(error),
    }
}

pub use aggregate::mcp_aggregate_streamable_http;
pub use code_mode::code_mode_streamable_http;

fn body_read_exceeded_limit(error: &axum::Error) -> bool {
    error
        .source()
        .is_some_and(|source| source.to_string().contains("length limit exceeded"))
        || error.to_string().contains("length limit exceeded")
}

#[allow(clippy::too_many_arguments)]
async fn handle_tools_call(
    state: &AppState,
    auth: &gateway_core::AuthenticatedApiKey,
    method: &axum::http::Method,
    headers: &HeaderMap,
    body: Bytes,
    server: gateway_core::ExternalMcpServerRecord,
    started_at: Instant,
    id: Option<serde_json::Value>,
    tool_name: String,
    arguments: Option<serde_json::Value>,
) -> Response<Body> {
    let access = McpAccess::new(state.store.clone());
    let invocation_logger = McpInvocationLogging::new(state.store.clone());
    let log_upstream = gateway_service::McpGatewayUpstream {
        server: server.clone(),
        headers: None,
    };
    let allowed_tool = match access
        .allowed_tool_for_call(auth, server.mcp_server_id, &tool_name)
        .await
    {
        Ok(tool) => tool,
        Err(error) => return mcp_error_response(error),
    };
    let Some(tool) = allowed_tool else {
        let _ = invocation_logger
            .log_invocation(
                auth,
                tool_invocation_log_input(
                    &log_upstream,
                    &id,
                    None,
                    &tool_name,
                    &tool_name,
                    McpToolInvocationStatus::PolicyDenied,
                    McpToolPolicyResult::Denied,
                    Some("mcp_tool_not_granted".to_string()),
                    arguments.clone(),
                    None,
                    started_at,
                ),
            )
            .await;
        return mcp_jsonrpc_error_response(
            StatusCode::FORBIDDEN,
            id.as_ref(),
            -32001,
            "MCP tool is not granted for this API key",
        );
    };

    let gateway = McpGatewayService::new(state.store.clone());
    let upstream = match gateway.prepare_upstream_for_auth(auth, server).await {
        Ok(upstream) => upstream,
        Err(error) => {
            let _ = invocation_logger
                .log_invocation(
                    auth,
                    tool_invocation_log_input(
                        &log_upstream,
                        &id,
                        Some(tool.mcp_tool_id),
                        &tool.upstream_name,
                        &tool.display_name,
                        McpToolInvocationStatus::Unauthorized,
                        McpToolPolicyResult::Allowed,
                        Some(error.error_code().to_string()),
                        arguments.clone(),
                        None,
                        started_at,
                    ),
                )
                .await;
            return mcp_error_response(error);
        }
    };
    let outcome = proxy_upstream(&state.mcp_http_client, method, headers, body, &upstream).await;
    let (status, error_code) = response_outcome(&outcome);
    let _ = invocation_logger
        .log_invocation(
            auth,
            tool_invocation_log_input(
                &upstream,
                &id,
                Some(tool.mcp_tool_id),
                &tool.upstream_name,
                &tool.display_name,
                status,
                McpToolPolicyResult::Allowed,
                error_code,
                arguments,
                None,
                started_at,
            ),
        )
        .await;
    match outcome {
        Ok(response) => response,
        Err(error) => mcp_error_response(error),
    }
}

fn tools_list_metadata(
    referenced_server_count: i64,
    exposed_tool_count: i64,
    filtered_tool_count: i64,
) -> Map<String, serde_json::Value> {
    Map::from_iter([
        ("mcp_method".to_string(), json!("tools/list")),
        (
            "referenced_mcp_server_count".to_string(),
            json!(referenced_server_count),
        ),
        ("exposed_tool_count".to_string(), json!(exposed_tool_count)),
        (
            "filtered_tool_count".to_string(),
            json!(filtered_tool_count),
        ),
    ])
}

#[allow(clippy::too_many_arguments)]
fn tool_invocation_log_input(
    upstream: &gateway_service::McpGatewayUpstream,
    id: &Option<serde_json::Value>,
    tool_id: Option<uuid::Uuid>,
    tool_display_key: &str,
    tool_display_name: &str,
    status: McpToolInvocationStatus,
    policy_result: McpToolPolicyResult,
    error_code: Option<String>,
    arguments_json: Option<serde_json::Value>,
    result_json: Option<serde_json::Value>,
    started_at: Instant,
) -> McpInvocationLogInput {
    McpInvocationLogInput {
        request_log_id: None,
        parent_invocation_id: None,
        request_id: mcp_request_id(id),
        server_id: Some(upstream.server.mcp_server_id),
        server_display_key: upstream.server.server_key.clone(),
        server_display_name: upstream.server.display_name.clone(),
        tool_id,
        tool_display_key: tool_display_key.to_string(),
        tool_display_name: tool_display_name.to_string(),
        status,
        policy_result,
        latency_ms: Some(started_at.elapsed().as_millis() as i64),
        error_code,
        arguments_json,
        result_json,
        metadata: Map::new(),
        occurred_at: OffsetDateTime::now_utc(),
    }
}

fn response_outcome(
    outcome: &Result<Response<Body>, GatewayError>,
) -> (McpToolInvocationStatus, Option<String>) {
    match outcome {
        Ok(response) if response.status().is_success() => (McpToolInvocationStatus::Success, None),
        Ok(response) => (
            McpToolInvocationStatus::UpstreamError,
            Some(format!("http_{}", response.status().as_u16())),
        ),
        Err(GatewayError::Provider(ProviderError::Timeout)) => (
            McpToolInvocationStatus::Timeout,
            Some("timeout".to_string()),
        ),
        Err(error) => (
            McpToolInvocationStatus::GatewayError,
            Some(error.to_string()),
        ),
    }
}

fn extract_mcp_gateway_api_key(headers: &HeaderMap) -> Result<String, AuthError> {
    let authorization_token = headers
        .get(AUTHORIZATION)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| AuthError::InvalidAuthorizationHeader)
                .and_then(extract_bearer_token)
                .map(str::to_string)
        })
        .transpose()?;

    let explicit_key = headers
        .get(X_OCEANS_API_KEY)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| AuthError::InvalidAuthorizationHeader)
                .map(str::trim)
                .and_then(|value| {
                    if value.is_empty() {
                        Err(AuthError::MissingBearerToken)
                    } else {
                        Ok(value.to_string())
                    }
                })
        })
        .transpose()?;

    match (authorization_token, explicit_key) {
        (Some(authorization_token), Some(explicit_key)) if authorization_token == explicit_key => {
            Ok(authorization_token)
        }
        (Some(_), Some(_)) => Err(AuthError::ConflictingApiKeyHeaders),
        (Some(authorization_token), None) => Ok(authorization_token),
        (None, Some(explicit_key)) => Ok(explicit_key),
        (None, None) => Err(AuthError::MissingAuthorizationHeader),
    }
}

fn mcp_error_response(error: GatewayError) -> Response<Body> {
    let status =
        StatusCode::from_u16(error.http_status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    tracing::Span::current().record("status_code", i64::from(status.as_u16()));
    let mut response = (
        status,
        Json(OpenAiErrorEnvelope::from_gateway_error(&error)),
    )
        .into_response();
    if status == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn auth_extractor_accepts_authorization_only() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer gwk_id.secret"),
        );
        assert_eq!(
            extract_mcp_gateway_api_key(&headers).expect("token"),
            "gwk_id.secret"
        );
    }

    #[test]
    fn auth_extractor_accepts_explicit_header_only() {
        let mut headers = HeaderMap::new();
        headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
        assert_eq!(
            extract_mcp_gateway_api_key(&headers).expect("token"),
            "gwk_id.secret"
        );
    }

    #[test]
    fn auth_extractor_accepts_identical_dual_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer gwk_id.secret"),
        );
        headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
        assert_eq!(
            extract_mcp_gateway_api_key(&headers).expect("token"),
            "gwk_id.secret"
        );
    }

    #[test]
    fn auth_extractor_rejects_missing_credentials() {
        let headers = HeaderMap::new();
        let error = extract_mcp_gateway_api_key(&headers).expect_err("missing");
        assert!(matches!(error, AuthError::MissingAuthorizationHeader));
    }

    #[test]
    fn auth_extractor_rejects_malformed_authorization_even_with_explicit_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Basic gwk_id.secret"),
        );
        headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
        let error = extract_mcp_gateway_api_key(&headers).expect_err("malformed");
        assert!(matches!(error, AuthError::InvalidAuthorizationHeader));
    }

    #[test]
    fn auth_extractor_rejects_conflicting_dual_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer gwk_id.secret"),
        );
        headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.other"));
        let error = extract_mcp_gateway_api_key(&headers).expect_err("conflict");
        assert!(matches!(error, AuthError::ConflictingApiKeyHeaders));
    }
}
