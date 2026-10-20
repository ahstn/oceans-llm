use axum::{
    Json,
    body::Body,
    http::{HeaderValue, Method, Response, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
};
use gateway_core::GatewayError;
use gateway_mcp::server::json_rpc_error_with_id;
use serde_json::{Value, json};
use uuid::Uuid;

/// Longest client-supplied method name or target kept for logs and error text.
const MAX_LOGGED_RPC_TEXT_CHARS: usize = 256;

/// Policy class of one MCP streamable HTTP request on a per-server route.
///
/// Every request must land in exactly one class. Methods the gateway does not
/// recognise are `Unknown` and denied, so a new upstream capability is never
/// exposed before the gateway knows how to govern it.
pub(super) enum McpRpcRequest {
    ToolsList {
        id: Option<Value>,
    },
    ToolsCall {
        id: Option<Value>,
        tool_name: String,
        arguments: Option<Value>,
    },
    /// `initialize`. It needs no grant, but its result loses the `tasks`
    /// capability because the gateway does not proxy MCP tasks.
    Initialize,
    /// Session plumbing that carries no server data: `ping`,
    /// `logging/setLevel`, client notifications, client replies to server
    /// requests, and `DELETE` session termination.
    Lifecycle,
    /// `GET` with no body opens the server-to-client SSE stream.
    EventStream,
    /// Server data outside tools: resources, prompts, and completions.
    Governed {
        id: Option<Value>,
        method: GovernedMcpMethod,
        target: Option<String>,
        params: Option<Value>,
    },
    Unknown {
        id: Option<Value>,
        method: String,
    },
}

/// MCP methods that read server data outside `tools/*`. They need at least one
/// effective grant on the server, and their calls are logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GovernedMcpMethod {
    ResourcesList,
    ResourcesTemplatesList,
    ResourcesRead,
    ResourcesSubscribe,
    ResourcesUnsubscribe,
    PromptsList,
    PromptsGet,
    CompletionComplete,
}

impl GovernedMcpMethod {
    fn from_method(method: &str) -> Option<Self> {
        Some(match method {
            "resources/list" => Self::ResourcesList,
            "resources/templates/list" => Self::ResourcesTemplatesList,
            "resources/read" => Self::ResourcesRead,
            "resources/subscribe" => Self::ResourcesSubscribe,
            "resources/unsubscribe" => Self::ResourcesUnsubscribe,
            "prompts/list" => Self::PromptsList,
            "prompts/get" => Self::PromptsGet,
            "completion/complete" => Self::CompletionComplete,
            _ => return None,
        })
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::ResourcesList => "resources/list",
            Self::ResourcesTemplatesList => "resources/templates/list",
            Self::ResourcesRead => "resources/read",
            Self::ResourcesSubscribe => "resources/subscribe",
            Self::ResourcesUnsubscribe => "resources/unsubscribe",
            Self::PromptsList => "prompts/list",
            Self::PromptsGet => "prompts/get",
            Self::CompletionComplete => "completion/complete",
        }
    }

    /// Methods whose results carry resource or prompt content, so they pass
    /// through MCP result guardrails like `tools/call` results do.
    pub(super) fn guards_result(self) -> bool {
        matches!(self, Self::ResourcesRead | Self::PromptsGet)
    }

    /// The params field naming what the call targets, if any.
    fn target_field(self) -> Option<&'static str> {
        match self {
            Self::ResourcesRead | Self::ResourcesSubscribe | Self::ResourcesUnsubscribe => {
                Some("uri")
            }
            Self::PromptsGet => Some("name"),
            _ => None,
        }
    }
}

fn is_lifecycle_method(method: &str, has_id: bool) -> bool {
    match method {
        "ping" | "logging/setLevel" => true,
        // The notifications a client sends to a server. They never get a response, so
        // they cannot return server data. Other names go through deny-by-default.
        "notifications/initialized"
        | "notifications/cancelled"
        | "notifications/progress"
        | "notifications/roots/list_changed" => !has_id,
        _ => false,
    }
}

/// A request rejected before it is classified. `id` is set once the body has
/// been read far enough to find one, so the error response can echo it.
#[derive(Debug)]
pub(super) struct McpRpcRejection {
    pub id: Option<Value>,
    pub error: GatewayError,
}

impl From<GatewayError> for McpRpcRejection {
    fn from(error: GatewayError) -> Self {
        Self { id: None, error }
    }
}

pub(super) fn parse_mcp_rpc_request(
    http_method: &Method,
    body: &[u8],
) -> Result<McpRpcRequest, McpRpcRejection> {
    if body.is_empty() {
        return match *http_method {
            Method::GET => Ok(McpRpcRequest::EventStream),
            Method::DELETE => Ok(McpRpcRequest::Lifecycle),
            _ => Err(
                GatewayError::InvalidRequest("MCP JSON-RPC body is required".to_string()).into(),
            ),
        };
    }
    // Only a POST carries a JSON-RPC message. The upstream treats a GET as its event
    // stream and a DELETE as session end whatever the body says, so a body there must
    // not decide which checks apply.
    if *http_method != Method::POST {
        return Err(GatewayError::InvalidRequest(format!(
            "MCP {http_method} requests must not have a body"
        ))
        .into());
    }
    let value: Value = serde_json::from_slice(body).map_err(|error| {
        GatewayError::InvalidRequest(format!("invalid MCP JSON-RPC body: {error}"))
    })?;
    let Some(object) = value.as_object() else {
        return Err(GatewayError::InvalidRequest(
            "MCP gateway policy supports single JSON-RPC request objects".to_string(),
        )
        .into());
    };
    let id = object.get("id").cloned();
    let reject = |message: &str| McpRpcRejection {
        id: id.clone(),
        error: GatewayError::InvalidRequest(message.to_string()),
    };
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        // A client reply to a server-initiated request (sampling, elicitation, roots)
        // carries an id and exactly one of `result` or `error`.
        let is_reply = id.is_some()
            && object.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
            && object.contains_key("result") != object.contains_key("error");
        if is_reply {
            return Ok(McpRpcRequest::Lifecycle);
        }
        return Err(reject("MCP JSON-RPC method is required"));
    };
    match method {
        "initialize" => Ok(McpRpcRequest::Initialize),
        "tools/list" => Ok(McpRpcRequest::ToolsList { id }),
        "tools/call" => {
            let params = object
                .get("params")
                .and_then(Value::as_object)
                .ok_or_else(|| reject("tools/call params must be an object"))?;
            let tool_name = params
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| reject("tools/call params.name is required"))?
                .to_string();
            // A task's result comes back through `tasks/result`, which the gateway
            // cannot tie to a tool grant, so the call would never finish.
            if params.contains_key("task") {
                return Err(reject(
                    "task-augmented tools/call is not supported through the gateway; call the tool without `task`",
                ));
            }
            Ok(McpRpcRequest::ToolsCall {
                id,
                tool_name,
                arguments: params.get("arguments").cloned(),
            })
        }
        method if is_lifecycle_method(method, id.is_some()) => Ok(McpRpcRequest::Lifecycle),
        method => match GovernedMcpMethod::from_method(method) {
            Some(governed) => {
                let params = object.get("params").cloned();
                let target = governed
                    .target_field()
                    .and_then(|field| params.as_ref()?.get(field)?.as_str())
                    .map(truncate_rpc_text);
                Ok(McpRpcRequest::Governed {
                    id,
                    method: governed,
                    target,
                    params,
                })
            }
            None => Ok(McpRpcRequest::Unknown {
                id,
                method: truncate_rpc_text(method),
            }),
        },
    }
}

pub(super) fn truncate_rpc_text(value: &str) -> String {
    value.chars().take(MAX_LOGGED_RPC_TEXT_CHARS).collect()
}

/// Every JSON-RPC error the per-server proxy sends is built here, on
/// `gateway_mcp`'s error shape.
pub(super) fn mcp_jsonrpc_error_response_with_data(
    status: StatusCode,
    id: Option<&Value>,
    code: i64,
    message: &str,
    data: Option<Value>,
) -> Response<Body> {
    json_response(
        status,
        json_rpc_error_with_id(
            Some(id.cloned().unwrap_or(Value::Null)),
            code,
            message,
            data,
        ),
    )
}

pub(super) fn mcp_jsonrpc_error_response(
    status: StatusCode,
    id: Option<&Value>,
    code: i64,
    message: &str,
) -> Response<Body> {
    mcp_jsonrpc_error_response_with_data(status, id, code, message, None)
}

/// Rejects a notification. The transport requires an HTTP error status, and
/// the JSON-RPC error it may carry has no `id` because none was sent.
pub(super) fn mcp_notification_error_response(
    status: StatusCode,
    code: i64,
    message: &str,
) -> Response<Body> {
    mcp_notification_error_response_with_data(status, code, message, None)
}

fn mcp_notification_error_response_with_data(
    status: StatusCode,
    code: i64,
    message: &str,
    data: Option<Value>,
) -> Response<Body> {
    json_response(status, json_rpc_error_with_id(None, code, message, data))
}

/// Refuses a parsed request: an error naming its `id`, or for a
/// notification, which sent none, an error without one.
pub(super) fn mcp_refusal_response(
    status: StatusCode,
    id: Option<&Value>,
    code: i64,
    message: &str,
    data: Option<Value>,
) -> Response<Body> {
    match id {
        Some(_) => mcp_jsonrpc_error_response_with_data(status, id, code, message, data),
        None => mcp_notification_error_response_with_data(status, code, message, data),
    }
}

fn json_response(status: StatusCode, body: Value) -> Response<Body> {
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

/// A JSON-RPC success response that echoes the request id as sent.
pub(super) fn mcp_jsonrpc_result_response(id: Option<&Value>, result: Value) -> Response<Body> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": id.cloned().unwrap_or(Value::Null),
        "result": result,
    });
    let mut response = (StatusCode::OK, Json(body)).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

pub(super) fn mcp_request_id(id: &Option<Value>) -> String {
    id.as_ref()
        .map(|value| match value {
            Value::String(value) => value.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}
