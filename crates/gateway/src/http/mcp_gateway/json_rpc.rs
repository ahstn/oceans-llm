use axum::{
    Json,
    body::Body,
    http::{HeaderValue, Response, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
};
use gateway_core::GatewayError;
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) enum McpRpcRequest {
    ToolsList {
        id: Option<Value>,
    },
    ToolsCall {
        id: Option<Value>,
        tool_name: String,
        arguments: Option<Value>,
    },
    Other,
}

pub(super) fn parse_mcp_rpc_request(body: &[u8]) -> Result<McpRpcRequest, GatewayError> {
    if body.is_empty() {
        return Ok(McpRpcRequest::Other);
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
    match object.get("method").and_then(Value::as_str) {
        Some("tools/list") => Ok(McpRpcRequest::ToolsList { id }),
        Some("tools/call") => {
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
        _ => Ok(McpRpcRequest::Other),
    }
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

pub(super) fn mcp_request_id(id: &Option<Value>) -> String {
    id.as_ref()
        .map(|value| match value {
            Value::String(value) => value.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

pub(super) fn response_json(body: &[u8]) -> Option<Value> {
    serde_json::from_slice(body).ok()
}
