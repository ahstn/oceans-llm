mod aggregate;
mod credentials;
mod guardrails;
mod json_rpc;
mod methods;
mod rate_limits;
mod upstream;

use std::{error::Error as _, time::Instant};

use axum::{
    Json,
    body::{Body, Bytes, to_bytes},
    extract::{Path, State},
    http::{
        HeaderMap, HeaderValue, Method, Request, Response, StatusCode,
        header::{AUTHORIZATION, WWW_AUTHENTICATE},
    },
    response::IntoResponse,
};
use credentials::{tool_call_auth_error_response, upstream_auth_error_response};
use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, GatewayError, McpOauthResource,
    McpToolInvocationStatus, McpToolPolicyResult, McpToolProjection, OpenAiErrorEnvelope,
    ProviderError, SCOPE_MCP_PROXY, auth::extract_bearer_token, is_oauth_access_token,
};
use gateway_guardrails::{EvaluationPayload, GuardrailEvaluation};
use gateway_service::{McpAccess, McpClientOauthService, McpGatewayService, McpInvocationLogInput};
use guardrails::{
    enforce_direct_mcp_result, evaluate_mcp_call, evaluate_mcp_result, guardrail_decision_metadata,
    guardrail_denied_response,
};
use json_rpc::{McpRpcRequest, mcp_jsonrpc_error_response, mcp_request_id, parse_mcp_rpc_request};
use methods::{
    GovernedRequest, deny_unknown_method, handle_event_stream, handle_governed_request,
    proxy_lifecycle,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use upstream::{proxy_tools_list, proxy_upstream};
use uuid::Uuid;

use crate::http::{
    mcp_client_oauth::{bearer_challenge, bound_to_oauth_access},
    request_policy::RequestPolicyContext,
    state::AppState,
};

const X_OCEANS_API_KEY: &str = "x-oceans-api-key";
const MCP_PROTOCOL_VERSION: &str = "mcp-protocol-version";
const MCP_SESSION_ID: &str = "mcp-session-id";
const LAST_EVENT_ID: &str = "last-event-id";
const MAX_MCP_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_MCP_REWRITE_BODY_BYTES: u64 = 4 * 1024 * 1024;
const GUARDRAIL_POLICY_DENIED_CODE: i64 = -32001;
/// The documented JSON-RPC code for a credential missing a scope or server.
const INSUFFICIENT_SCOPE_CODE: i64 = -32001;

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
    let policy = request
        .extensions()
        .get::<RequestPolicyContext>()
        .cloned()
        .unwrap_or_default();

    let resource = McpOauthResource::from_path(request.uri().path());

    let auth = match authenticate_mcp_request(&state, &headers, resource.as_ref(), SCOPE_MCP_PROXY)
        .await
        .and_then(|auth| {
            // A key limited to other servers never learns whether this one exists.
            auth.require_mcp_server(&server_key.to_ascii_lowercase())?;
            Ok(auth)
        }) {
        Ok(auth) => auth,
        Err(error) => return mcp_auth_error_response(&state, resource.as_ref(), error),
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

    let response = dispatch_mcp_request(
        &state,
        &policy,
        &auth,
        &server_key,
        &method,
        &headers,
        body,
        started_at,
    )
    .await;
    let response = bound_to_oauth_access(&state, &auth, response);
    tracing::Span::current().record("status_code", i64::from(response.status().as_u16()));
    tracing::debug!(
        elapsed_ms = started_at.elapsed().as_millis(),
        "proxied MCP streamable HTTP request"
    );
    response
}

/// Classifies one authenticated MCP request and applies the matching policy.
///
/// Every JSON-RPC method lands in exactly one branch. Anything the gateway does
/// not recognise is denied, so new upstream capabilities stay closed until the
/// gateway knows how to govern them.
#[allow(clippy::too_many_arguments)]
async fn dispatch_mcp_request(
    state: &AppState,
    policy: &RequestPolicyContext,
    auth: &AuthenticatedApiKey,
    server_key: &str,
    method: &Method,
    headers: &HeaderMap,
    body: Bytes,
    started_at: Instant,
) -> Response<Body> {
    let gateway = McpGatewayService::new(state.store.clone())
        .with_oauth_runtime(state.mcp_oauth_runtime.clone());
    let server = match gateway.load_active_server(server_key).await {
        Ok(server) => server,
        Err(error) => return mcp_error_response(error),
    };
    let access = McpAccess::new(state.store.clone());
    // A team-only server another team's caller cannot reach does not exist for
    // them, on every method, including the grant-free lifecycle ones that
    // never resolve grants: they get the same not-found as a missing server,
    // and its upstream is never contacted.
    if server.ownership.visibility == gateway_core::McpServerVisibility::Team {
        let reachable = match access.subjects_and_team(auth).await {
            Ok((_, team_id)) => server.ownership.is_reachable_from(team_id),
            Err(error) => return mcp_error_response(error),
        };
        if !reachable {
            return mcp_error_response(GatewayError::Store(gateway_core::StoreError::NotFound(
                format!("external MCP server `{}` not found", server.server_key),
            )));
        }
    }
    tracing::Span::current().record("mcp_server_id", server.mcp_server_id.to_string());
    tracing::Span::current().record("upstream_auth_mode", server.auth_mode.as_str());

    let rpc_request = parse_mcp_rpc_request(method, &body);

    match rpc_request {
        Ok(McpRpcRequest::ToolsList { id }) => {
            let access_resolution = match access
                .effective_tools_for_api_key(
                    auth,
                    Some(server.mcp_server_id),
                    McpToolProjection::Access,
                )
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
                .prepare_upstream_for_auth(auth, server.clone())
                .await
            {
                Ok(upstream) => upstream,
                Err(error) => {
                    return upstream_auth_error_response(state, auth, &server, error);
                }
            };
            let outcome = proxy_tools_list(
                &state.mcp_http_clients,
                method,
                headers,
                body,
                &upstream,
                &allowed_tool_names,
                id.as_ref(),
            )
            .await;
            let (status, error_code) = response_outcome(&outcome);
            let mut log_input = tool_invocation_log_input(
                &upstream.server,
                "tools/list",
                &id,
                None,
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
            let _ = state.service.log_mcp_tool_invocation(auth, log_input).await;
            match outcome {
                Ok(response) => response,
                Err(error) => mcp_error_response(error),
            }
        }
        Ok(McpRpcRequest::ToolsCall {
            id,
            tool_name,
            arguments,
        }) => {
            handle_tools_call(
                state, policy, auth, method, headers, body, server, started_at, id, tool_name,
                arguments,
            )
            .await
        }
        Ok(McpRpcRequest::Governed {
            id,
            method: governed,
            target,
            params,
        }) => {
            handle_governed_request(
                state,
                auth,
                method,
                headers,
                body,
                server,
                started_at,
                GovernedRequest {
                    id,
                    method: governed,
                    target,
                    params,
                },
            )
            .await
        }
        Ok(McpRpcRequest::Initialize) => {
            proxy_lifecycle(state, auth, method, headers, body, server, true).await
        }
        Ok(McpRpcRequest::Lifecycle) => {
            proxy_lifecycle(state, auth, method, headers, body, server, false).await
        }
        Ok(McpRpcRequest::EventStream) => {
            handle_event_stream(state, auth, method, headers, body, server).await
        }
        Ok(McpRpcRequest::Unknown { id, method }) => {
            deny_unknown_method(state, auth, &server, started_at, id, method).await
        }
        Err(rejection) => {
            let _ = state
                .service
                .log_mcp_tool_invocation(
                    auth,
                    McpInvocationLogInput {
                        mcp_tool_invocation_id: None,
                        request_log_id: None,
                        request_id: Uuid::new_v4().to_string(),
                        server_id: Some(server.mcp_server_id),
                        server_display_key: server.server_key,
                        server_display_name: server.display_name,
                        tool_id: None,
                        tool_display_key: "unknown".to_string(),
                        tool_display_name: "unknown".to_string(),
                        mcp_method: "invalid".to_string(),
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
            mcp_jsonrpc_error_response(
                StatusCode::BAD_REQUEST,
                rejection.id.as_ref(),
                -32600,
                &rejection.error.to_string(),
            )
        }
    }
}

pub use aggregate::mcp_aggregate_streamable_http;

fn body_read_exceeded_limit(error: &axum::Error) -> bool {
    error
        .source()
        .is_some_and(|source| source.to_string().contains("length limit exceeded"))
        || error.to_string().contains("length limit exceeded")
}

#[allow(clippy::too_many_arguments)]
async fn handle_tools_call(
    state: &AppState,
    policy: &RequestPolicyContext,
    auth: &AuthenticatedApiKey,
    method: &Method,
    headers: &HeaderMap,
    body: Bytes,
    server: gateway_core::ExternalMcpServerRecord,
    started_at: Instant,
    id: Option<serde_json::Value>,
    tool_name: String,
    arguments: Option<serde_json::Value>,
) -> Response<Body> {
    let access = McpAccess::new(state.store.clone());
    let mcp_tool_invocation_id = Uuid::new_v4();
    let allowed_tool = match access
        .allowed_tool_for_call(auth, server.mcp_server_id, &tool_name)
        .await
    {
        Ok(tool) => tool,
        Err(error) => return mcp_error_response(error),
    };
    let Some(tool) = allowed_tool else {
        let _ = state
            .service
            .log_mcp_tool_invocation(
                auth,
                tool_invocation_log_input(
                    &server,
                    "tools/call",
                    &id,
                    Some(mcp_tool_invocation_id),
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
    match rate_limits::check_tool_call(state, policy, auth, &server).await {
        Ok(None) => {}
        Ok(Some(rejected)) => {
            let mut log_input = tool_invocation_log_input(
                &server,
                "tools/call",
                &id,
                Some(mcp_tool_invocation_id),
                Some(tool.mcp_tool_id),
                &tool.upstream_name,
                &tool.display_name,
                McpToolInvocationStatus::PolicyDenied,
                McpToolPolicyResult::Denied,
                Some(rate_limits::RATE_LIMITED_ERROR_CODE.to_string()),
                arguments,
                None,
                started_at,
            );
            log_input.metadata = rate_limits::rejection_metadata(&rejected);
            let _ = state.service.log_mcp_tool_invocation(auth, log_input).await;
            return rate_limits::rejection_response(id.as_ref(), &rejected);
        }
        Err(error) => {
            // The limiter cannot check the call in `closed` mode: a JSON-RPC
            // error with the HTTP 503, as the aggregate route returns.
            let log_input = tool_invocation_log_input(
                &server,
                "tools/call",
                &id,
                Some(mcp_tool_invocation_id),
                Some(tool.mcp_tool_id),
                &tool.upstream_name,
                &tool.display_name,
                McpToolInvocationStatus::GatewayError,
                McpToolPolicyResult::NotEvaluated,
                Some(error.error_code().to_string()),
                arguments,
                None,
                started_at,
            );
            let _ = state.service.log_mcp_tool_invocation(auth, log_input).await;
            return rate_limits::unavailable_response(id.as_ref(), &error);
        }
    }
    let normalized_arguments = arguments.clone().unwrap_or_else(|| json!({}));
    let guardrail_request_id = mcp_request_id(&id);
    let guardrail_evaluation = evaluate_mcp_call(
        state,
        Some(&guardrail_request_id),
        Some(mcp_tool_invocation_id),
        &server.server_key,
        &tool.upstream_name,
        normalized_arguments,
    )
    .await;
    if guardrail_evaluation.denied() {
        let mut log_input = tool_invocation_log_input(
            &server,
            "tools/call",
            &id,
            Some(mcp_tool_invocation_id),
            Some(tool.mcp_tool_id),
            &tool.upstream_name,
            &tool.display_name,
            McpToolInvocationStatus::PolicyDenied,
            McpToolPolicyResult::Denied,
            Some("guardrail_policy_denied".to_string()),
            arguments,
            None,
            started_at,
        );
        log_input.metadata = guardrail_decision_metadata(&guardrail_evaluation);
        let _ = state.service.log_mcp_tool_invocation(auth, log_input).await;
        return guardrail_denied_response(id.as_ref(), &guardrail_evaluation);
    }

    let transformed = guardrail_evaluation
        .decisions
        .iter()
        .any(|decision| decision.transformed);
    let guarded_arguments = match &guardrail_evaluation.output {
        EvaluationPayload::McpCall { call } => call.arguments.clone(),
        _ => unreachable!("MCP call evaluation preserves payload kind"),
    };
    let body = if transformed {
        match rewrite_tools_call_arguments(&body, &guarded_arguments) {
            Ok(body) => body,
            Err(error) => return mcp_error_response(error),
        }
    } else {
        body
    };
    let arguments = transformed.then_some(guarded_arguments).or(arguments);

    let gateway = McpGatewayService::new(state.store.clone())
        .with_oauth_runtime(state.mcp_oauth_runtime.clone());
    let upstream = match gateway
        .prepare_upstream_for_auth(auth, server.clone())
        .await
    {
        Ok(upstream) => upstream,
        Err(error) => {
            let _ = state
                .service
                .log_mcp_tool_invocation(
                    auth,
                    tool_invocation_log_input(
                        &server,
                        "tools/call",
                        &id,
                        Some(mcp_tool_invocation_id),
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
            return tool_call_auth_error_response(state, auth, &server, id.as_ref(), error);
        }
    };
    let outcome =
        match proxy_upstream(&state.mcp_http_clients, method, headers, body, &upstream).await {
            Ok(response) => {
                enforce_direct_mcp_result(
                    state,
                    &upstream.server.server_key,
                    &tool.upstream_name,
                    id.as_ref(),
                    mcp_tool_invocation_id,
                    response,
                )
                .await
            }
            Err(error) => Err(error),
        };
    let (status, error_code, policy_result) = guarded_outcome_log_fields(&outcome);
    let mut log_input = tool_invocation_log_input(
        &upstream.server,
        "tools/call",
        &id,
        Some(mcp_tool_invocation_id),
        Some(tool.mcp_tool_id),
        &tool.upstream_name,
        &tool.display_name,
        status,
        policy_result,
        error_code,
        arguments,
        None,
        started_at,
    );
    log_input.metadata = guardrail_decision_metadata(&guardrail_evaluation);
    if let Ok((_, Some(result_evaluation), _)) = &outcome {
        log_input.metadata.insert(
            "guardrail_result".to_string(),
            Value::Object(guardrail_decision_metadata(result_evaluation)),
        );
    }
    let _ = state.service.log_mcp_tool_invocation(auth, log_input).await;
    match outcome {
        Ok((response, _, _)) => response,
        Err(error) => mcp_error_response(error),
    }
}

/// Invocation log status for a proxied call whose result may have been guarded.
fn guarded_outcome_log_fields(
    outcome: &Result<(Response<Body>, Option<GuardrailEvaluation>, bool), GatewayError>,
) -> (McpToolInvocationStatus, Option<String>, McpToolPolicyResult) {
    match outcome {
        Ok((_, _, true)) => (
            McpToolInvocationStatus::PolicyDenied,
            Some("guardrail_policy_denied".to_string()),
            McpToolPolicyResult::Denied,
        ),
        Ok((response, _, false)) if response.status().is_success() => (
            McpToolInvocationStatus::Success,
            None,
            McpToolPolicyResult::Allowed,
        ),
        Ok((response, _, false)) => (
            McpToolInvocationStatus::UpstreamError,
            Some(format!("http_{}", response.status().as_u16())),
            McpToolPolicyResult::Allowed,
        ),
        Err(GatewayError::Provider(ProviderError::Timeout)) => (
            McpToolInvocationStatus::Timeout,
            Some("timeout".to_string()),
            McpToolPolicyResult::Allowed,
        ),
        Err(error) => (
            McpToolInvocationStatus::GatewayError,
            Some(error.to_string()),
            McpToolPolicyResult::Allowed,
        ),
    }
}

fn rewrite_tools_call_arguments(body: &[u8], arguments: &Value) -> Result<Bytes, GatewayError> {
    let mut request: Value = serde_json::from_slice(body)
        .map_err(|error| GatewayError::InvalidRequest(format!("invalid MCP request: {error}")))?;
    let params = request
        .get_mut("params")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            GatewayError::InvalidRequest("tools/call params must be an object".into())
        })?;
    params.insert("arguments".to_string(), arguments.clone());
    serde_json::to_vec(&request)
        .map(Bytes::from)
        .map_err(|error| GatewayError::Internal(format!("failed to encode MCP request: {error}")))
}

fn tools_list_metadata(
    referenced_server_count: i64,
    exposed_tool_count: i64,
    filtered_tool_count: i64,
) -> Map<String, serde_json::Value> {
    Map::from_iter([
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
    server: &gateway_core::ExternalMcpServerRecord,
    mcp_method: &str,
    id: &Option<serde_json::Value>,
    mcp_tool_invocation_id: Option<Uuid>,
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
        mcp_tool_invocation_id,
        request_log_id: None,
        request_id: mcp_request_id(id),
        server_id: Some(server.mcp_server_id),
        server_display_key: server.server_key.clone(),
        server_display_name: server.display_name.clone(),
        tool_id,
        tool_display_key: tool_display_key.to_string(),
        tool_display_name: tool_display_name.to_string(),
        mcp_method: mcp_method.to_string(),
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

/// Authenticates an MCP request with a `gwk_` API key, or with an `oat_`
/// OAuth access token issued for `resource` when OAuth for MCP clients is on.
/// Either credential must carry `scope`, the route's scope.
async fn authenticate_mcp_request(
    state: &AppState,
    headers: &HeaderMap,
    resource: Option<&McpOauthResource>,
    scope: &str,
) -> Result<AuthenticatedApiKey, GatewayError> {
    let token = extract_mcp_bearer_token(headers)?;
    if !is_oauth_access_token(&token) {
        return state
            .service
            .authenticate_bearer_token_for_scope(&token, scope)
            .await;
    }
    // The MCP authorization spec allows access tokens only in `Authorization`.
    let sent_as_api_key = headers
        .get(X_OCEANS_API_KEY)
        .and_then(|value| value.to_str().ok())
        .is_some_and(is_oauth_access_token);
    let (Some(settings), Some(resource), false) =
        (state.mcp_client_oauth.clone(), resource, sent_as_api_key)
    else {
        return Err(AuthError::OauthTokenInvalid.into());
    };
    McpClientOauthService::new(state.store.clone(), settings)
        .authenticate_access_token(&token, resource, OffsetDateTime::now_utc())
        .await
}

/// The credential from `Authorization: Bearer` or `x-oceans-api-key`: a
/// `gwk_` API key, or an `oat_` OAuth access token.
fn extract_mcp_bearer_token(headers: &HeaderMap) -> Result<String, AuthError> {
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

/// An authentication failure on an MCP route. When OAuth for MCP clients is
/// enabled, the `WWW-Authenticate` header points at the route's metadata.
fn mcp_auth_error_response(
    state: &AppState,
    resource: Option<&McpOauthResource>,
    error: GatewayError,
) -> Response<Body> {
    let challenge = state
        .mcp_client_oauth
        .as_deref()
        .zip(resource)
        .and_then(|(settings, resource)| bearer_challenge(settings, resource, &error));
    let mut response = match &error {
        GatewayError::Auth(
            AuthError::InsufficientScope { .. }
            | AuthError::FullAccessRequired
            | AuthError::McpServerNotInScope,
        ) => insufficient_scope_response(&error),
        _ => mcp_error_response(error),
    };
    if let Some(challenge) = challenge {
        response.headers_mut().insert(WWW_AUTHENTICATE, challenge);
    }
    response
}

/// A `403` JSON-RPC error for a credential that lacks a scope or server. It is
/// sent before the body is read, so the `id` is `null`.
fn insufficient_scope_response(error: &GatewayError) -> Response<Body> {
    let mut data = json!({ "error": error.error_code() });
    if let GatewayError::Auth(AuthError::InsufficientScope { required }) = error {
        data["required_scope"] = json!(required);
    }
    tracing::Span::current().record("status_code", i64::from(StatusCode::FORBIDDEN.as_u16()));
    let body = json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": INSUFFICIENT_SCOPE_CODE,
            "message": error.to_string(),
            "data": data,
        }
    });
    (StatusCode::FORBIDDEN, Json(body)).into_response()
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
mod tests;
