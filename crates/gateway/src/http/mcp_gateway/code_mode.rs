//! `/code-mode-mcp` route: a gateway-owned MCP server exposing exactly the
//! `explore` and `execute` tools. Mirrors the aggregate `/mcp` auth and
//! session semantics via the shared `session` module; dispatches tool calls
//! to the `CodeModeService` in gateway-service.

use axum::{
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, Request, Response, StatusCode},
    response::IntoResponse,
};
use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, GatewayError, McpSessionSurface,
};
use gateway_mcp::{
    JsonRpcId,
    server::{
        JSON_RPC_INVALID_PARAMS, JSON_RPC_METHOD_NOT_FOUND, JSON_RPC_POLICY_DENIED,
        McpServerMessage, code_mode_error_result, code_mode_execute_definition,
        code_mode_explore_definition, code_mode_result, json_rpc_error, json_rpc_success,
        parse_client_message, tools_list_result,
    },
};
use gateway_service::{CapabilityProfile, CodeModeRunOutcome, CodeModeService};
use serde_json::Value;
use uuid::Uuid;

use crate::http::state::AppState;

use super::{
    MAX_MCP_REQUEST_BODY_BYTES, body_read_exceeded_limit, extract_mcp_gateway_api_key,
    mcp_error_response,
    session::{
        handle_initialized_notification, handle_session_delete, initialize_session,
        json_rpc_response, serialization_error, validate_request_session,
    },
};

const CODE_MODE_SERVER_NAME: &str = "oceans-code-mode";

pub async fn code_mode_streamable_http(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Response<Body> {
    if !state.code_mode.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    if request.uri().query().is_some() {
        return mcp_error_response(GatewayError::InvalidRequest(
            "query strings are not accepted on MCP gateway routes".to_string(),
        ));
    }

    let method = request.method().clone();
    let headers = request.headers().clone();
    if method != axum::http::Method::POST && method != axum::http::Method::DELETE {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let bearer_token = match extract_mcp_gateway_api_key(&headers) {
        Ok(token) => token,
        Err(error) => return mcp_error_response(error.into()),
    };
    let auth = match state.service.authenticate_bearer_token(&bearer_token).await {
        Ok(auth) => auth,
        Err(error) => return mcp_error_response(error),
    };
    if !matches!(
        auth.owner_kind,
        ApiKeyOwnerKind::User | ApiKeyOwnerKind::ServiceAccount
    ) {
        return mcp_error_response(AuthError::InsufficientPrivileges.into());
    }

    match method.as_str() {
        "DELETE" => {
            handle_session_delete(&state, &auth, McpSessionSurface::CodeMode, &headers).await
        }
        "POST" => handle_post(state, auth, headers, request).await,
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn handle_post(
    state: AppState,
    auth: AuthenticatedApiKey,
    headers: HeaderMap,
    request: Request<Body>,
) -> Response<Body> {
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

    let message = match parse_client_message(&body) {
        Ok(message) => message,
        Err(error) => {
            return json_rpc_response(
                StatusCode::BAD_REQUEST,
                json_rpc_error(error.id, error.code, error.message),
                None,
            );
        }
    };

    match message {
        McpServerMessage::Initialize {
            id,
            protocol_version,
        } => {
            initialize_session(
                &state,
                &auth,
                McpSessionSurface::CodeMode,
                id,
                protocol_version,
                CODE_MODE_SERVER_NAME,
            )
            .await
        }
        McpServerMessage::InitializedNotification => {
            handle_initialized_notification(&state, &auth, McpSessionSurface::CodeMode, &headers)
                .await
        }
        McpServerMessage::ToolsList { id } => {
            match validate_request_session(&state, &auth, McpSessionSurface::CodeMode, &headers, &id)
                .await
            {
                Ok(_) => list_code_mode_tools(id),
                Err(response) => response,
            }
        }
        McpServerMessage::ToolsCall {
            id,
            name,
            arguments,
        } => {
            match validate_request_session(&state, &auth, McpSessionSurface::CodeMode, &headers, &id)
                .await
            {
                Ok(_) => call_code_mode_tool(&state, &auth, id, name, arguments).await,
                Err(response) => response,
            }
        }
        McpServerMessage::OtherRequest { id, method } => json_rpc_response(
            StatusCode::OK,
            json_rpc_error(
                Some(id),
                JSON_RPC_METHOD_NOT_FOUND,
                format!("MCP method `{method}` is not supported by the Code Mode gateway"),
            ),
            None,
        ),
        McpServerMessage::OtherNotification { .. } | McpServerMessage::ClientResponse => {
            StatusCode::ACCEPTED.into_response()
        }
    }
}

fn list_code_mode_tools(id: JsonRpcId) -> Response<Body> {
    let tools = vec![
        code_mode_explore_definition(),
        code_mode_execute_definition(),
    ];
    json_rpc_response(
        StatusCode::OK,
        json_rpc_success(id, tools_list_result(tools)).unwrap_or_else(serialization_error),
        None,
    )
}

async fn call_code_mode_tool(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    id: JsonRpcId,
    name: String,
    arguments: Value,
) -> Response<Body> {
    let Some(profile) = CapabilityProfile::from_tool_name(&name) else {
        return json_rpc_response(
            StatusCode::FORBIDDEN,
            json_rpc_error(
                Some(id),
                JSON_RPC_POLICY_DENIED,
                "Code Mode gateway exposes only explore and execute",
            ),
            None,
        );
    };
    // The configured MCP invocation payload policy governs parent and nested
    // payload capture (on/off), byte caps, and key-based redaction.
    let service = CodeModeService::new_with_payload_policy(
        state.store.clone(),
        state.code_mode.executor.clone(),
        state.code_mode.limits.clone(),
        state.service.mcp_invocation_payload_policy(),
    );
    let request_id = code_mode_request_id(&id);

    let code = match arguments.get("code").and_then(Value::as_str) {
        Some(code) if !code.trim().is_empty() => code.to_string(),
        _ => {
            let message = "`code` is required and must be a non-empty string";
            service
                .log_invalid_request(auth, profile, request_id, message)
                .await;
            return json_rpc_response(
                StatusCode::BAD_REQUEST,
                json_rpc_error(Some(id), JSON_RPC_INVALID_PARAMS, message),
                None,
            );
        }
    };

    let outcome = service.run(auth, profile, &code, request_id).await;
    code_mode_tool_response(id, outcome)
}

fn code_mode_tool_response(id: JsonRpcId, outcome: CodeModeRunOutcome) -> Response<Body> {
    let result = match outcome.error {
        None => code_mode_result(outcome.result, outcome.logs, outcome.truncated),
        Some(error) => code_mode_error_result(
            error,
            outcome
                .error_code
                .unwrap_or_else(|| "code_execution_error".to_string()),
            outcome.logs,
            outcome.truncated,
        ),
    };
    json_rpc_response(
        StatusCode::OK,
        json_rpc_success(id, result).unwrap_or_else(serialization_error),
        None,
    )
}

fn code_mode_request_id(id: &JsonRpcId) -> String {
    let suffix = Uuid::new_v4().simple();
    match id {
        JsonRpcId::Number(value) => format!("code-mode-{value}-{suffix}"),
        JsonRpcId::String(value) => format!("code-mode-{value}-{suffix}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_mode_tools_list_contains_only_explore_and_execute() {
        let names = [
            code_mode_explore_definition(),
            code_mode_execute_definition(),
        ]
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
        assert_eq!(names, vec!["explore", "execute"]);
    }
}
