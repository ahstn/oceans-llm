use axum::{
    Json,
    body::Body,
    http::{HeaderValue, Method, Response, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
};
use gateway_core::GatewayError;
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
    /// Session plumbing that carries no server data: `initialize`, `ping`,
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
        "initialize" | "ping" | "logging/setLevel" => true,
        // Notifications never get a response, so they cannot return server data.
        method => !has_id && method.starts_with("notifications/"),
    }
}

pub(super) fn parse_mcp_rpc_request(
    http_method: &Method,
    body: &[u8],
) -> Result<McpRpcRequest, GatewayError> {
    if body.is_empty() {
        return match *http_method {
            Method::GET => Ok(McpRpcRequest::EventStream),
            Method::DELETE => Ok(McpRpcRequest::Lifecycle),
            _ => Err(GatewayError::InvalidRequest(
                "MCP JSON-RPC body is required".to_string(),
            )),
        };
    }
    let value: Value = serde_json::from_slice(body).map_err(|error| {
        GatewayError::InvalidRequest(format!("invalid MCP JSON-RPC body: {error}"))
    })?;
    let Some(object) = value.as_object() else {
        return Err(GatewayError::InvalidRequest(
            "MCP gateway policy supports single JSON-RPC request objects".to_string(),
        ));
    };
    let id = object.get("id").cloned();
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        // A client reply to a server-initiated request (sampling, elicitation, roots).
        if id.is_some() && (object.contains_key("result") || object.contains_key("error")) {
            return Ok(McpRpcRequest::Lifecycle);
        }
        return Err(GatewayError::InvalidRequest(
            "MCP JSON-RPC method is required".to_string(),
        ));
    };
    match method {
        "tools/list" => Ok(McpRpcRequest::ToolsList { id }),
        "tools/call" => {
            let params = object
                .get("params")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    GatewayError::InvalidRequest("tools/call params must be an object".to_string())
                })?;
            let tool_name = params
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| {
                    GatewayError::InvalidRequest("tools/call params.name is required".to_string())
                })?
                .to_string();
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

pub(super) fn mcp_jsonrpc_error_response(
    status: StatusCode,
    id: Option<&Value>,
    code: i64,
    message: &str,
) -> Response<Body> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": id.cloned().unwrap_or(Value::Null),
        "error": {
            "code": code,
            "message": message,
        }
    });
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
