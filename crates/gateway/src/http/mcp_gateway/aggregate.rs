use std::{time::Duration as StdDuration, time::Instant};

use axum::{
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, Request, Response, StatusCode},
    response::IntoResponse,
};
use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, GatewayError, McpSessionSurface,
    McpToolInvocationStatus, McpToolPolicyResult,
};
use gateway_mcp::{
    JsonRpcId, McpTool, StreamableHttpClient,
    server::{
        JSON_RPC_INVALID_PARAMS, JSON_RPC_METHOD_NOT_FOUND, JSON_RPC_POLICY_DENIED,
        McpServerMessage, call_tool_error_result, call_tool_result, json_rpc_error,
        json_rpc_success, parse_client_message, tools_list_result,
    },
};
use gateway_service::{
    CallMcpToolInput, DescribeMcpToolInput, MAX_SEARCH_LIMIT, McpCatalog, McpGatewayService,
    McpInvocationLogInput, McpInvocationLogging, SearchMcpToolsInput, invocation_status_for_error,
    map_mcp_client_error,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use crate::http::state::AppState;

use super::{
    MAX_MCP_REQUEST_BODY_BYTES, body_read_exceeded_limit, extract_mcp_gateway_api_key,
    json_rpc::mcp_request_id,
    mcp_error_response,
    session::{
        handle_initialized_notification, handle_session_delete, initialize_session,
        json_rpc_response, serialization_error, validate_request_session,
    },
};

const AGGREGATE_SERVER_NAME: &str = "oceans-mcp-gateway";

pub async fn mcp_aggregate_streamable_http(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Response<Body> {
    if request.uri().query().is_some() {
        return mcp_error_response(GatewayError::InvalidRequest(
            "query strings are not accepted on MCP gateway routes".to_string(),
        ));
    }

    let method = request.method().clone();
    let headers = request.headers().clone();
    if method == axum::http::Method::GET {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
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
            handle_session_delete(&state, &auth, McpSessionSurface::Aggregate, &headers).await
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
                McpSessionSurface::Aggregate,
                id,
                protocol_version,
                AGGREGATE_SERVER_NAME,
            )
            .await
        }
        McpServerMessage::InitializedNotification => {
            handle_initialized_notification(&state, &auth, McpSessionSurface::Aggregate, &headers)
                .await
        }
        McpServerMessage::ToolsList { id } => {
            match validate_request_session(
                &state,
                &auth,
                McpSessionSurface::Aggregate,
                &headers,
                &id,
            )
            .await
            {
                Ok(_) => list_builtin_tools(id),
                Err(response) => response,
            }
        }
        McpServerMessage::ToolsCall {
            id,
            name,
            arguments,
        } => {
            match validate_request_session(
                &state,
                &auth,
                McpSessionSurface::Aggregate,
                &headers,
                &id,
            )
            .await
            {
                Ok(_) => call_builtin_tool(&state, &auth, id, name, arguments).await,
                Err(response) => response,
            }
        }
        McpServerMessage::OtherRequest { id, method } => json_rpc_response(
            StatusCode::OK,
            json_rpc_error(
                Some(id),
                JSON_RPC_METHOD_NOT_FOUND,
                format!("MCP method `{method}` is not supported by the aggregate gateway"),
            ),
            None,
        ),
        McpServerMessage::OtherNotification { .. } | McpServerMessage::ClientResponse => {
            StatusCode::ACCEPTED.into_response()
        }
    }
}

fn list_builtin_tools(id: JsonRpcId) -> Response<Body> {
    let tools = vec![
        search_tools_definition(),
        describe_tool_definition(),
        call_tool_definition(),
    ];
    json_rpc_response(
        StatusCode::OK,
        json_rpc_success(id, tools_list_result(tools)).unwrap_or_else(serialization_error),
        None,
    )
}

async fn call_builtin_tool(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    id: JsonRpcId,
    name: String,
    arguments: Value,
) -> Response<Body> {
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    let catalog = McpCatalog::new(state.store.clone());
    let result = match name.as_str() {
        "search_tools" => {
            let input: SearchMcpToolsInput = match serde_json::from_value(arguments) {
                Ok(input) => input,
                Err(error) => {
                    return json_rpc_response(
                        StatusCode::BAD_REQUEST,
                        json_rpc_error(Some(id), JSON_RPC_INVALID_PARAMS, error.to_string()),
                        None,
                    );
                }
            };
            catalog
                .search_tools(auth, input)
                .await
                .map(|output| ("Search completed", serde_json::to_value(output)))
        }
        "describe_tool" => {
            let input: DescribeMcpToolInput = match serde_json::from_value(arguments) {
                Ok(input) => input,
                Err(error) => {
                    return json_rpc_response(
                        StatusCode::BAD_REQUEST,
                        json_rpc_error(Some(id), JSON_RPC_INVALID_PARAMS, error.to_string()),
                        None,
                    );
                }
            };
            catalog
                .describe_tool(auth, input)
                .await
                .map(|output| ("Tool described", serde_json::to_value(output)))
        }
        "call_tool" => {
            let input: CallMcpToolInput = match serde_json::from_value(arguments) {
                Ok(input) => input,
                Err(error) => {
                    return json_rpc_response(
                        StatusCode::BAD_REQUEST,
                        json_rpc_error(Some(id), JSON_RPC_INVALID_PARAMS, error.to_string()),
                        None,
                    );
                }
            };
            return call_catalog_tool(state, auth, id, input).await;
        }
        _ => {
            return json_rpc_response(
                StatusCode::FORBIDDEN,
                json_rpc_error(
                    Some(id),
                    JSON_RPC_POLICY_DENIED,
                    "MCP aggregate gateway exposes only search_tools, describe_tool, and call_tool",
                ),
                None,
            );
        }
    };

    match result {
        Ok((text, Ok(structured))) => json_rpc_response(
            StatusCode::OK,
            json_rpc_success(id, call_tool_result(text, structured))
                .unwrap_or_else(serialization_error),
            None,
        ),
        Ok((_text, Err(error))) => json_rpc_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            json_rpc_error(Some(id), JSON_RPC_INVALID_PARAMS, error.to_string()),
            None,
        ),
        Err(GatewayError::InvalidRequest(message)) => json_rpc_response(
            StatusCode::BAD_REQUEST,
            json_rpc_error(Some(id), JSON_RPC_INVALID_PARAMS, message),
            None,
        ),
        Err(error) => mcp_error_response(error),
    }
}

async fn call_catalog_tool(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    id: JsonRpcId,
    input: CallMcpToolInput,
) -> Response<Body> {
    let started_at = Instant::now();
    let catalog = McpCatalog::new(state.store.clone());
    let record = match catalog
        .authorized_tool_by_address(auth, &input.address)
        .await
    {
        Ok(record) => record,
        Err(GatewayError::InvalidRequest(message)) => {
            return json_rpc_response(
                StatusCode::BAD_REQUEST,
                json_rpc_error(Some(id), JSON_RPC_INVALID_PARAMS, message),
                None,
            );
        }
        Err(error) => return mcp_error_response(error),
    };
    if let Some(schema_hash) = input.schema_hash.as_deref()
        && schema_hash != record.tool.schema_hash
    {
        return aggregate_tool_error(
            id,
            "Tool schema changed",
            "tool_schema_changed",
            json!({
                "address": input.address,
                "expected_schema_hash": schema_hash,
                "actual_schema_hash": record.tool.schema_hash,
                "schema_version": record.tool.schema_version
            }),
        );
    }

    let gateway = McpGatewayService::new(state.store.clone());
    let upstream = match gateway
        .prepare_upstream_for_auth(auth, record.server.clone())
        .await
    {
        Ok(upstream) => upstream,
        Err(error @ GatewayError::McpCredentialRequired { .. })
        | Err(error @ GatewayError::McpCredentialExpired { .. }) => {
            log_aggregate_invocation(AggregateInvocationLog {
                state,
                auth,
                id: &id,
                record: &record,
                status: McpToolInvocationStatus::Unauthorized,
                error_code: Some(error.error_code().to_string()),
                arguments_json: Some(input.arguments.clone()),
                result_json: None,
                started_at,
            })
            .await;
            return aggregate_tool_error(
                id,
                error.to_string(),
                error.error_code(),
                json!({"address": input.address, "server_key": record.server.server_key}),
            );
        }
        Err(error) => return mcp_error_response(error),
    };

    let client =
        match StreamableHttpClient::new(&upstream.server.server_url, upstream_timeout(&upstream)) {
            Ok(client) => client,
            Err(error) => {
                let gateway_error = map_mcp_client_error(error);
                return mcp_error_response(gateway_error);
            }
        };
    let arguments = if input.arguments.is_null() {
        json!({})
    } else {
        input.arguments.clone()
    };
    let outcome = client
        .call_tool(
            upstream.headers.as_ref(),
            &record.tool.upstream_name,
            arguments.clone(),
        )
        .await;
    match outcome {
        Ok(result) => {
            let result_json = serde_json::to_value(&result).ok();
            log_aggregate_invocation(AggregateInvocationLog {
                state,
                auth,
                id: &id,
                record: &record,
                status: if result.is_error.unwrap_or(false) {
                    McpToolInvocationStatus::UpstreamError
                } else {
                    McpToolInvocationStatus::Success
                },
                error_code: None,
                arguments_json: Some(arguments),
                result_json,
                started_at,
            })
            .await;
            json_rpc_response(
                StatusCode::OK,
                json_rpc_success(id, result).unwrap_or_else(serialization_error),
                None,
            )
        }
        Err(error) => {
            let gateway_error = map_mcp_client_error(error);
            log_aggregate_invocation(AggregateInvocationLog {
                state,
                auth,
                id: &id,
                record: &record,
                status: invocation_status_for_error(&gateway_error),
                error_code: Some(gateway_error.error_code().to_string()),
                arguments_json: Some(arguments),
                result_json: None,
                started_at,
            })
            .await;
            aggregate_tool_error(
                id,
                gateway_error.to_string(),
                gateway_error.error_code(),
                json!({"address": input.address, "server_key": record.server.server_key}),
            )
        }
    }
}

struct AggregateInvocationLog<'a> {
    state: &'a AppState,
    auth: &'a AuthenticatedApiKey,
    id: &'a JsonRpcId,
    record: &'a gateway_core::McpCatalogToolRecord,
    status: McpToolInvocationStatus,
    error_code: Option<String>,
    arguments_json: Option<Value>,
    result_json: Option<Value>,
    started_at: Instant,
}

async fn log_aggregate_invocation(input: AggregateInvocationLog<'_>) {
    let logger = McpInvocationLogging::new(input.state.store.clone());
    let id_value = json_rpc_id_value(input.id);
    let _ = logger
        .log_invocation(
            input.auth,
            McpInvocationLogInput {
                request_log_id: None,
                parent_invocation_id: None,
                request_id: mcp_request_id(&Some(id_value)),
                server_id: Some(input.record.server.mcp_server_id),
                server_display_key: input.record.server.server_key.clone(),
                server_display_name: input.record.server.display_name.clone(),
                tool_id: Some(input.record.tool.mcp_tool_id),
                tool_display_key: input.record.tool.upstream_name.clone(),
                tool_display_name: input.record.tool.display_name.clone(),
                status: input.status,
                policy_result: McpToolPolicyResult::Allowed,
                latency_ms: Some(input.started_at.elapsed().as_millis() as i64),
                error_code: input.error_code,
                arguments_json: input.arguments_json,
                result_json: input.result_json,
                metadata: Map::from_iter([
                    ("mcp_route".to_string(), json!("aggregate")),
                    ("aggregate_tool".to_string(), json!("call_tool")),
                ]),
                occurred_at: OffsetDateTime::now_utc(),
            },
        )
        .await;
}

fn aggregate_tool_error(
    id: JsonRpcId,
    message: impl Into<String>,
    error_code: impl Into<String>,
    structured: Value,
) -> Response<Body> {
    json_rpc_response(
        StatusCode::OK,
        json_rpc_success(id, call_tool_error_result(message, error_code, structured))
            .unwrap_or_else(serialization_error),
        None,
    )
}

fn upstream_timeout(upstream: &gateway_service::McpGatewayUpstream) -> StdDuration {
    StdDuration::from_millis(upstream.server.timeout_ms.max(1) as u64)
}

fn json_rpc_id_value(id: &JsonRpcId) -> Value {
    match id {
        JsonRpcId::Number(value) => json!(value),
        JsonRpcId::String(value) => json!(value),
    }
}

fn search_tools_definition() -> McpTool {
    McpTool {
        name: "search_tools".to_string(),
        description: Some(
            "Search authorized MCP tools across all configured gateway sources.".to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Lexical search query. Empty lists authorized tools."},
                "limit": {"type": "integer", "minimum": 1, "maximum": MAX_SEARCH_LIMIT, "default": 10},
                "offset": {"type": "integer", "minimum": 0, "default": 0},
                "server_key": {"type": "string", "description": "Optional MCP server namespace filter."}
            },
            "additionalProperties": false
        }),
    }
}

fn describe_tool_definition() -> McpTool {
    McpTool {
        name: "describe_tool".to_string(),
        description: Some(
            "Describe one authorized MCP tool by canonical mcp://{server_key}/tools/{tool_name} address."
                .to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "required": ["address"],
            "properties": {
                "address": {"type": "string", "description": "Canonical tool address returned by search_tools."}
            },
            "additionalProperties": false
        }),
    }
}

fn call_tool_definition() -> McpTool {
    McpTool {
        name: "call_tool".to_string(),
        description: Some("Call one authorized MCP tool by canonical address.".to_string()),
        input_schema: json!({
            "type": "object",
            "required": ["address"],
            "properties": {
                "address": {"type": "string", "description": "Canonical tool address returned by search_tools."},
                "arguments": {"type": "object", "default": {}},
                "schema_hash": {"type": "string", "description": "Optional expected schema hash."}
            },
            "additionalProperties": false
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_tools_list_contains_only_discovery_tools() {
        let names = [
            search_tools_definition(),
            describe_tool_definition(),
            call_tool_definition(),
        ]
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
        assert_eq!(names, vec!["search_tools", "describe_tool", "call_tool"]);
    }
}
