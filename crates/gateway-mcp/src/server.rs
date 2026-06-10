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

pub fn call_tool_error_result(
    text: impl Into<String>,
    error_code: impl Into<String>,
    structured_content: Value,
) -> CallToolResult {
    let mut structured_content = structured_content;
    if let Some(object) = structured_content.as_object_mut() {
        object.insert("error_code".to_string(), json!(error_code.into()));
    }
    CallToolResult {
        content: vec![ToolContent::Text { text: text.into() }],
        structured_content: Some(structured_content),
        is_error: Some(true),
    }
}

pub const CODE_MODE_TRUNCATION_MARKER: &str = "--- TRUNCATED ---";

const CODE_MODE_API_TYPINGS: &str = r#"The `code` argument is a JavaScript async arrow function body executed in a sandbox.
The sandbox exposes a single `oceans` API (every call is re-authorized by the gateway):

  declare const oceans: {
    // Search MCP tools you are granted. Empty query lists everything granted.
    searchTools(args: { query?: string; limit?: number; offset?: number; server_key?: string }):
      Promise<{ items: { address: string; score: number; server: object; tool: object }[]; total: number; next_offset?: number }>;
    // Describe one granted tool by canonical mcp://{server_key}/tools/{tool_name} address.
    describeTool(args: { address: string }):
      Promise<{ address: string; server: object; tool: { input_schema: object; schema_hash: string } }>;
    // Call one granted tool by canonical address (execute profile only).
    callTool(args: { address: string; arguments?: object; schema_hash?: string }): Promise<object>;
  };

Host calls complete synchronously from the sandbox's point of view (no event loop).
Grant/capability denials and invalid arguments throw catchable exceptions. Structured
tool errors (credential_required, credential_expired, tool_schema_changed) and upstream
failures RESOLVE successfully with an aggregate-style result object:
`{ content, isError: true, structuredContent: { error_code, ... } }` — check
`result.isError` before using a callTool result. console.log/warn/error lines
are captured into the response logs. Oversized output is truncated and marked with
`--- TRUNCATED ---`."#;

const EXPLORE_EXAMPLE: &str = r#"Example:
  const { items } = await oceans.searchTools({ query: "github issues" });
  const details = await Promise.all(items.slice(0, 3).map((item) => oceans.describeTool({ address: item.address })));
  return details.map((d) => ({ address: d.address, required: d.tool.input_schema.required }));"#;

const EXECUTE_EXAMPLE: &str = r#"Example:
  const { items } = await oceans.searchTools({ query: "create issue", server_key: "github" });
  const tool = await oceans.describeTool({ address: items[0].address });
  const result = await oceans.callTool({
    address: tool.address,
    arguments: { title: "Bug report", body: "Details..." },
    schema_hash: tool.tool.schema_hash,
  });
  if (result.isError) {
    throw new Error(result.structuredContent.error_code + ": " + result.content[0].text);
  }
  return result;"#;

#[must_use]
pub fn code_mode_explore_definition() -> McpTool {
    McpTool {
        name: "explore".to_string(),
        description: Some(format!(
            "Run JavaScript that explores your granted MCP tools using oceans.searchTools and \
             oceans.describeTool, then returns a small projection of the results. \
             oceans.callTool is NOT available in explore.\n\n{CODE_MODE_API_TYPINGS}\n\n{EXPLORE_EXAMPLE}"
        )),
        input_schema: code_mode_input_schema(),
    }
}

#[must_use]
pub fn code_mode_execute_definition() -> McpTool {
    McpTool {
        name: "execute".to_string(),
        description: Some(format!(
            "Run JavaScript that searches, describes, and calls your granted MCP tools via the \
             oceans API. Every oceans.callTool invocation is re-authorized and logged by the \
             gateway.\n\n{CODE_MODE_API_TYPINGS}\n\n{EXECUTE_EXAMPLE}"
        )),
        input_schema: code_mode_input_schema(),
    }
}

fn code_mode_input_schema() -> Value {
    json!({
        "type": "object",
        "required": ["code"],
        "properties": {
            "code": {
                "type": "string",
                "description": "JavaScript async arrow function body to execute in the sandbox."
            }
        },
        "additionalProperties": false
    })
}

/// Successful Code Mode result: text content plus the structured outcome.
#[must_use]
pub fn code_mode_result(
    result: Option<Value>,
    logs: Vec<String>,
    truncated: bool,
) -> CallToolResult {
    let mut text = match &result {
        Some(value) => serde_json::to_string_pretty(value)
            .unwrap_or_else(|_| "<unserializable result>".to_string()),
        None => "null".to_string(),
    };
    if truncated {
        text.push('\n');
        text.push_str(CODE_MODE_TRUNCATION_MARKER);
    }
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(json!({
            "result": result,
            "logs": logs,
            "truncated": truncated,
        })),
        is_error: Some(false),
    }
}

/// Failed Code Mode result: `isError: true` with an `Error: ...` text line.
#[must_use]
pub fn code_mode_error_result(
    message: impl Into<String>,
    error_code: impl Into<String>,
    logs: Vec<String>,
    truncated: bool,
) -> CallToolResult {
    let message = message.into();
    let mut text = format!("Error: {message}");
    if truncated {
        text.push('\n');
        text.push_str(CODE_MODE_TRUNCATION_MARKER);
    }
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(json!({
            "error": message,
            "error_code": error_code.into(),
            "logs": logs,
            "truncated": truncated,
        })),
        is_error: Some(true),
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
    fn code_mode_definitions_require_single_code_param() {
        for tool in [
            code_mode_explore_definition(),
            code_mode_execute_definition(),
        ] {
            assert_eq!(tool.input_schema["required"], json!(["code"]));
            assert_eq!(
                tool.input_schema["properties"]
                    .as_object()
                    .expect("properties")
                    .len(),
                1
            );
            assert_eq!(tool.input_schema["additionalProperties"], json!(false));
            let description = tool.description.expect("description");
            assert!(description.contains("oceans.searchTools"));
            assert!(description.contains("Example:"));
        }
        assert_eq!(code_mode_explore_definition().name, "explore");
        assert_eq!(code_mode_execute_definition().name, "execute");
    }

    #[test]
    fn code_mode_results_follow_marker_and_error_conventions() {
        let ok = code_mode_result(Some(json!({"a": 1})), vec!["log".to_string()], true);
        assert_eq!(ok.is_error, Some(false));
        let ToolContent::Text { text } = &ok.content[0];
        assert!(text.ends_with(CODE_MODE_TRUNCATION_MARKER));

        let err = code_mode_error_result("boom", "code_execution_error", Vec::new(), false);
        assert_eq!(err.is_error, Some(true));
        let ToolContent::Text { text } = &err.content[0];
        assert!(text.starts_with("Error: boom"));
        let structured = err.structured_content.expect("structured");
        assert_eq!(structured["error_code"], json!("code_execution_error"));
    }

    #[test]
    fn accepts_client_responses_for_accepted_http_response() {
        let message = parse_client_message(br#"{"jsonrpc":"2.0","id":1,"result":{}}"#)
            .expect("client response");
        assert_eq!(message, McpServerMessage::ClientResponse);
    }
}
