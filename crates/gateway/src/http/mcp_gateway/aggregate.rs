use std::{time::Duration as StdDuration, time::Instant};

use axum::{
    Json,
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, HeaderValue, Request, Response, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
};
use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, GatewayError, McpAggregateSessionRepository,
    McpToolInvocationStatus, McpToolPolicyResult, NewMcpAggregateSessionRecord, ProviderError,
};
use gateway_mcp::{
    DEFAULT_PROTOCOL_VERSION, JsonRpcId, MCP_PROTOCOL_VERSION_HEADER, MCP_SESSION_ID_HEADER,
    McpClientError, McpTool, StreamableHttpClient,
    server::{
        JSON_RPC_INVALID_PARAMS, JSON_RPC_METHOD_NOT_FOUND, JSON_RPC_POLICY_DENIED,
        McpServerMessage, call_tool_error_result, call_tool_result, initialize_result,
        json_rpc_error, json_rpc_success, parse_client_message, tools_list_result,
    },
};
use gateway_service::{
    CallMcpToolInput, DescribeMcpToolInput, MAX_SEARCH_LIMIT, McpCatalog, McpGatewayService,
    McpInvocationLogInput, McpInvocationLogging, SearchMcpToolsInput,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::http::state::AppState;

use super::{
    MAX_MCP_REQUEST_BODY_BYTES, body_read_exceeded_limit, extract_mcp_gateway_api_key,
    json_rpc::mcp_request_id, mcp_error_response,
};

const AGGREGATE_SERVER_NAME: &str = "oceans-mcp-gateway";
const SESSION_TTL_HOURS: i64 = 12;

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
        "DELETE" => handle_delete(&state, &auth, &headers).await,
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
        } => initialize_session(&state, &auth, id, protocol_version).await,
        McpServerMessage::InitializedNotification => {
            let Some((session_id, token_hash)) = session_identity(&headers) else {
                return session_http_error(
                    StatusCode::BAD_REQUEST,
                    None,
                    "MCP session id is required",
                );
            };
            match validate_session(&state, &auth, &token_hash, false, None).await {
                Ok(session) if session.session_id == session_id => {
                    match state
                        .store
                        .update_mcp_aggregate_session_initialized(
                            session_id,
                            &token_hash,
                            OffsetDateTime::now_utc(),
                        )
                        .await
                    {
                        Ok(Some(_)) => StatusCode::ACCEPTED.into_response(),
                        Ok(None) => session_http_error(
                            StatusCode::NOT_FOUND,
                            None,
                            "MCP session was not found",
                        ),
                        Err(error) => mcp_error_response(error.into()),
                    }
                }
                Ok(_) => {
                    session_http_error(StatusCode::NOT_FOUND, None, "MCP session was not found")
                }
                Err(response) => response,
            }
        }
        McpServerMessage::ToolsList { id } => {
            match validate_request_session(&state, &auth, &headers, &id).await {
                Ok(_) => list_builtin_tools(id),
                Err(response) => response,
            }
        }
        McpServerMessage::ToolsCall {
            id,
            name,
            arguments,
        } => match validate_request_session(&state, &auth, &headers, &id).await {
            Ok(_) => call_builtin_tool(&state, &auth, id, name, arguments).await,
            Err(response) => response,
        },
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

async fn initialize_session(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    id: JsonRpcId,
    _protocol_version: String,
) -> Response<Body> {
    let now = OffsetDateTime::now_utc();
    let session_id = Uuid::new_v4();
    let raw_token = signed_session_token(&state.identity_token_secret, session_id);
    let token_hash = token_hash(&raw_token);
    let session = NewMcpAggregateSessionRecord {
        session_id,
        token_hash,
        api_key_id: auth.id,
        owner_kind: auth.owner_kind,
        owner_user_id: auth.owner_user_id,
        owner_team_id: auth.owner_team_id,
        owner_service_account_id: auth.owner_service_account_id,
        protocol_version: DEFAULT_PROTOCOL_VERSION.to_string(),
        expires_at: now + Duration::hours(SESSION_TTL_HOURS),
        created_at: now,
    };
    match state.store.create_mcp_aggregate_session(&session).await {
        Ok(_) => json_rpc_response(
            StatusCode::OK,
            json_rpc_success(
                id,
                initialize_result(AGGREGATE_SERVER_NAME, env!("CARGO_PKG_VERSION")),
            )
            .unwrap_or_else(serialization_error),
            Some(raw_token),
        ),
        Err(error) => mcp_error_response(error.into()),
    }
}

async fn handle_delete(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    headers: &HeaderMap,
) -> Response<Body> {
    let Some((session_id, token_hash)) = session_identity(headers) else {
        return session_http_error(StatusCode::BAD_REQUEST, None, "MCP session id is required");
    };
    match validate_session(state, auth, &token_hash, false, None).await {
        Ok(session) if session.session_id == session_id => {
            match state
                .store
                .revoke_mcp_aggregate_session(session_id, &token_hash, OffsetDateTime::now_utc())
                .await
            {
                Ok(true) => StatusCode::ACCEPTED.into_response(),
                Ok(false) => {
                    session_http_error(StatusCode::NOT_FOUND, None, "MCP session was not found")
                }
                Err(error) => mcp_error_response(error.into()),
            }
        }
        Ok(_) => session_http_error(StatusCode::NOT_FOUND, None, "MCP session was not found"),
        Err(response) => response,
    }
}

async fn validate_request_session(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    headers: &HeaderMap,
    id: &JsonRpcId,
) -> Result<gateway_core::McpAggregateSessionRecord, Response<Body>> {
    let Some((session_id, token_hash)) = session_identity(headers) else {
        return Err(session_http_error(
            StatusCode::BAD_REQUEST,
            Some(id.clone()),
            "MCP session id is required",
        ));
    };
    let session = validate_session(state, auth, &token_hash, true, Some(id)).await?;
    if session.session_id != session_id {
        return Err(session_http_error(
            StatusCode::NOT_FOUND,
            Some(id.clone()),
            "MCP session was not found",
        ));
    }
    match state
        .store
        .touch_mcp_aggregate_session(session_id, &token_hash, OffsetDateTime::now_utc())
        .await
    {
        Ok(Some(session)) => Ok(session),
        Ok(None) => Err(session_http_error(
            StatusCode::NOT_FOUND,
            Some(id.clone()),
            "MCP session was not found",
        )),
        Err(error) => Err(mcp_error_response(error.into())),
    }
}

async fn validate_session(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    token_hash: &str,
    require_initialized: bool,
    request_id: Option<&JsonRpcId>,
) -> Result<gateway_core::McpAggregateSessionRecord, Response<Body>> {
    let session = match state
        .store
        .get_mcp_aggregate_session_by_token_hash(token_hash)
        .await
    {
        Ok(Some(session)) => session,
        Ok(None) => {
            return Err(session_http_error(
                StatusCode::NOT_FOUND,
                request_id.cloned(),
                "MCP session was not found",
            ));
        }
        Err(error) => return Err(mcp_error_response(error.into())),
    };
    if session.api_key_id != auth.id
        || session.owner_kind != auth.owner_kind
        || session.owner_user_id != auth.owner_user_id
        || session.owner_team_id != auth.owner_team_id
        || session.owner_service_account_id != auth.owner_service_account_id
    {
        return Err(session_http_error(
            StatusCode::NOT_FOUND,
            request_id.cloned(),
            "MCP session was not found",
        ));
    }
    let now = OffsetDateTime::now_utc();
    if session.revoked_at.is_some() || session.expires_at <= now {
        return Err(session_http_error(
            StatusCode::NOT_FOUND,
            request_id.cloned(),
            "MCP session was not found",
        ));
    }
    if require_initialized && !session.initialized {
        return Err(session_http_error(
            StatusCode::BAD_REQUEST,
            request_id.cloned(),
            "MCP session has not completed initialization",
        ));
    }
    Ok(session)
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
        Err(error) => return aggregate_gateway_error(id, error, json!({"address": input.address})),
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
        Err(error) => {
            return aggregate_gateway_error(
                id,
                error,
                json!({"address": input.address, "server_key": record.server.server_key}),
            );
        }
    };

    let client =
        match StreamableHttpClient::new(&upstream.server.server_url, upstream_timeout(&upstream)) {
            Ok(client) => client,
            Err(error) => {
                let gateway_error = map_mcp_client_error(error);
                return aggregate_gateway_error(
                    id,
                    gateway_error,
                    json!({"address": input.address, "server_key": record.server.server_key}),
                );
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

fn aggregate_gateway_error(
    id: JsonRpcId,
    error: GatewayError,
    structured: Value,
) -> Response<Body> {
    aggregate_tool_error(id, error.to_string(), error.error_code(), structured)
}

fn upstream_timeout(upstream: &gateway_service::McpGatewayUpstream) -> StdDuration {
    StdDuration::from_millis(upstream.server.timeout_ms.max(1) as u64)
}

fn map_mcp_client_error(error: McpClientError) -> GatewayError {
    match error {
        McpClientError::Timeout => ProviderError::Timeout.into(),
        McpClientError::Http { status, body } => {
            ProviderError::UpstreamHttp { status, body }.into()
        }
        McpClientError::ResponseTooLarge { limit_bytes } => {
            GatewayError::PayloadTooLarge { limit_bytes }
        }
        other => ProviderError::Transport(other.to_string()).into(),
    }
}

fn invocation_status_for_error(error: &GatewayError) -> McpToolInvocationStatus {
    match error {
        GatewayError::Provider(ProviderError::Timeout) => McpToolInvocationStatus::Timeout,
        GatewayError::Provider(ProviderError::UpstreamHttp { .. })
        | GatewayError::Provider(ProviderError::Transport(_)) => {
            McpToolInvocationStatus::UpstreamError
        }
        _ => McpToolInvocationStatus::GatewayError,
    }
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

fn session_identity(headers: &HeaderMap) -> Option<(Uuid, String)> {
    let raw = headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())?;
    let (raw_id, _signature) = raw.split_once('.')?;
    let session_id = Uuid::parse_str(raw_id).ok()?;
    Some((session_id, token_hash(raw)))
}

fn signed_session_token(secret: &str, session_id: Uuid) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"mcp-session:");
    hasher.update(secret.as_bytes());
    hasher.update(b":");
    hasher.update(session_id.as_bytes());
    let signature = hex_string(&hasher.finalize());
    format!("{session_id}.{}", &signature[..32])
}

fn token_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex_string(&hasher.finalize())
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn session_http_error(status: StatusCode, id: Option<JsonRpcId>, message: &str) -> Response<Body> {
    json_rpc_response(
        status,
        json_rpc_error(id, JSON_RPC_INVALID_PARAMS, message),
        None,
    )
}

fn json_rpc_response(
    status: StatusCode,
    payload: Value,
    session_id: Option<String>,
) -> Response<Body> {
    let mut response = (status, Json(payload)).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response.headers_mut().insert(
        MCP_PROTOCOL_VERSION_HEADER,
        HeaderValue::from_static(DEFAULT_PROTOCOL_VERSION),
    );
    if let Some(session_id) = session_id
        && let Ok(value) = HeaderValue::from_str(&session_id)
    {
        response.headers_mut().insert(MCP_SESSION_ID_HEADER, value);
    }
    response
}

fn serialization_error(error: serde_json::Error) -> Value {
    json_rpc_error(None, JSON_RPC_INVALID_PARAMS, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_session_token_is_visible_ascii_and_parseable() {
        let session_id = Uuid::new_v4();
        let token = signed_session_token("secret", session_id);
        assert!(token.bytes().all(|byte| (0x21..=0x7e).contains(&byte)));
        let mut headers = HeaderMap::new();
        headers.insert(
            MCP_SESSION_ID_HEADER,
            HeaderValue::from_str(&token).unwrap(),
        );
        let (parsed_id, parsed_hash) = session_identity(&headers).expect("session identity");
        assert_eq!(parsed_id, session_id);
        assert_eq!(parsed_hash, token_hash(&token));
    }

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
