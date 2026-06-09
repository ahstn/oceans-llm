use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    DEFAULT_PROTOCOL_VERSION, InitializeRequest, InitializeResponse, JsonRpcErrorObject, JsonRpcId,
    JsonRpcResponse, McpImplementation, McpTool, ToolsListResponse,
};

pub const JSON_RPC_PARSE_ERROR: i64 = -32700;
pub const JSON_RPC_INVALID_REQUEST: i64 = -32600;
pub const JSON_RPC_METHOD_NOT_FOUND: i64 = -32601;
pub const JSON_RPC_INVALID_PARAMS: i64 = -32602;
pub const JSON_RPC_POLICY_DENIED: i64 = -32001;

#[derive(Debug, Clone, PartialEq)]
pub enum McpServerMessage {
    Initialize {
        id: JsonRpcId,
        protocol_version: String,
    },
    InitializedNotification,
    ToolsList {
        id: JsonRpcId,
    },
    ToolsCall {
        id: JsonRpcId,
        name: String,
        arguments: Value,
    },
    OtherRequest {
        id: JsonRpcId,
        method: String,
    },
    OtherNotification {
        method: String,
    },
    ClientResponse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerParseError {
    pub code: i64,
    pub message: String,
    pub id: Option<JsonRpcId>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawJsonRpcMessage {
    jsonrpc: String,
    #[serde(default)]
    id: Option<JsonRpcId>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolsCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
}

pub fn parse_client_message(body: &[u8]) -> Result<McpServerMessage, McpServerParseError> {
    let value: Value = serde_json::from_slice(body).map_err(|error| McpServerParseError {
        code: JSON_RPC_PARSE_ERROR,
        message: error.to_string(),
        id: None,
    })?;
    if value.is_array() {
        return Err(McpServerParseError {
            code: JSON_RPC_INVALID_REQUEST,
            message: "JSON-RPC batches are not accepted on MCP Streamable HTTP".to_string(),
            id: None,
        });
    }
    if value
        .as_object()
        .is_some_and(|object| !object.contains_key("method") && object.contains_key("id"))
    {
        return Ok(McpServerMessage::ClientResponse);
    }
    let raw: RawJsonRpcMessage =
        serde_json::from_value(value).map_err(|error| McpServerParseError {
            code: JSON_RPC_INVALID_REQUEST,
            message: error.to_string(),
            id: None,
        })?;
    if raw.jsonrpc != "2.0" {
        return Err(McpServerParseError {
            code: JSON_RPC_INVALID_REQUEST,
            message: "JSON-RPC version must be 2.0".to_string(),
            id: raw.id,
        });
    }
    match (raw.method.as_str(), raw.id) {
        ("initialize", Some(id)) => {
            let request: InitializeRequest =
                serde_json::from_value(raw.params).map_err(|error| McpServerParseError {
                    code: JSON_RPC_INVALID_PARAMS,
                    message: error.to_string(),
                    id: Some(id.clone()),
                })?;
            Ok(McpServerMessage::Initialize {
                id,
                protocol_version: request.protocol_version,
            })
        }
        ("notifications/initialized", None) => Ok(McpServerMessage::InitializedNotification),
        ("tools/list", Some(id)) => Ok(McpServerMessage::ToolsList { id }),
        ("tools/call", Some(id)) => {
            let params: ToolsCallParams =
                serde_json::from_value(raw.params).map_err(|error| McpServerParseError {
                    code: JSON_RPC_INVALID_PARAMS,
                    message: error.to_string(),
                    id: Some(id.clone()),
                })?;
            Ok(McpServerMessage::ToolsCall {
                id,
                name: params.name,
                arguments: params.arguments,
            })
        }
        (_, Some(id)) => Ok(McpServerMessage::OtherRequest {
            id,
            method: raw.method,
        }),
        (_, None) => Ok(McpServerMessage::OtherNotification { method: raw.method }),
    }
}

pub fn initialize_result(server_name: &str, server_version: &str) -> InitializeResponse {
    InitializeResponse {
        protocol_version: DEFAULT_PROTOCOL_VERSION.to_string(),
        capabilities: json!({"tools": {"listChanged": false}}),
        server_info: Some(McpImplementation {
            name: server_name.to_string(),
            version: server_version.to_string(),
        }),
    }
}

pub fn tools_list_result(tools: Vec<McpTool>) -> ToolsListResponse {
    ToolsListResponse {
        tools,
        next_cursor: None,
    }
}

pub fn call_tool_result(text: impl Into<String>, structured_content: Value) -> CallToolResult {
    CallToolResult {
        content: vec![ToolContent::Text { text: text.into() }],
        structured_content: Some(structured_content),
        is_error: Some(false),
    }
}

pub fn json_rpc_success<T: Serialize>(
    id: JsonRpcId,
    result: T,
) -> Result<Value, serde_json::Error> {
    serde_json::to_value(JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: Some(id),
        result: Some(result),
        error: None,
    })
}

pub fn json_rpc_error(id: Option<JsonRpcId>, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": JsonRpcErrorObject {
            code,
            message: message.into(),
            data: None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_initialize() {
        let message = parse_client_message(
            br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#,
        )
        .expect("message");
        assert_eq!(
            message,
            McpServerMessage::Initialize {
                id: JsonRpcId::Number(1),
                protocol_version: "2025-11-25".to_string()
            }
        );
    }

    #[test]
    fn rejects_batches() {
        let error = parse_client_message(br#"[]"#).expect_err("batch rejected");
        assert_eq!(error.code, JSON_RPC_INVALID_REQUEST);
    }

    #[test]
    fn accepts_client_responses_for_accepted_http_response() {
        let message = parse_client_message(br#"{"jsonrpc":"2.0","id":1,"result":{}}"#)
            .expect("client response");
        assert_eq!(message, McpServerMessage::ClientResponse);
    }
}
