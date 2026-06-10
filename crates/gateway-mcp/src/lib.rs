use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use futures_util::StreamExt;
use reqwest::{
    Client,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use url::Url;

pub mod server;

pub const DEFAULT_PROTOCOL_VERSION: &str = "2025-03-26";
pub const MCP_PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";
pub const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";
const MAX_RESPONSE_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOOLS_LIST_PAGES: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcRequest<T = Value> {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcNotification<T = Value> {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<T>,
}

impl<T> JsonRpcNotification<T> {
    #[must_use]
    pub fn new(method: impl Into<String>, params: Option<T>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}

impl<T> JsonRpcRequest<T> {
    #[must_use]
    pub fn new(id: JsonRpcId, method: impl Into<String>, params: Option<T>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum JsonRpcId {
    Number(i64),
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse<T = Value> {
    pub jsonrpc: String,
    pub id: Option<JsonRpcId>,
    #[serde(default)]
    pub result: Option<T>,
    #[serde(default)]
    pub error: Option<JsonRpcErrorObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub protocol_version: String,
    pub capabilities: Value,
    pub client_info: McpImplementation,
}

impl Default for InitializeRequest {
    fn default() -> Self {
        Self {
            protocol_version: DEFAULT_PROTOCOL_VERSION.to_string(),
            capabilities: json!({}),
            client_info: McpImplementation {
                name: "oceans-llm".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpImplementation {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub protocol_version: String,
    #[serde(default)]
    pub capabilities: Value,
    #[serde(default)]
    pub server_info: Option<McpImplementation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolsListResponse {
    #[serde(default)]
    pub tools: Vec<McpTool>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolsListRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCallRequest {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCallResponse {
    #[serde(default)]
    pub content: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedMcpTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub schema_hash: String,
}

#[derive(Debug, Clone)]
pub struct StreamableHttpClient {
    client: Client,
    endpoint: Url,
    protocol_version: String,
}

impl StreamableHttpClient {
    pub fn new(endpoint: &str, timeout: Duration) -> Result<Self, McpClientError> {
        let endpoint = Url::parse(endpoint).map_err(|error| McpClientError::InvalidUrl {
            url: endpoint.to_string(),
            message: error.to_string(),
        })?;
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| McpClientError::Transport(error.to_string()))?;
        Ok(Self {
            client,
            endpoint,
            protocol_version: DEFAULT_PROTOCOL_VERSION.to_string(),
        })
    }

    #[must_use]
    pub fn with_protocol_version(mut self, protocol_version: impl Into<String>) -> Self {
        self.protocol_version = protocol_version.into();
        self
    }

    pub async fn initialize(&self) -> Result<InitializeResponse, McpClientError> {
        self.initialize_with_session(None)
            .await
            .map(|(response, _session_id)| response)
    }

    pub async fn ping(&self) -> Result<Value, McpClientError> {
        self.send(
            JsonRpcRequest::new(JsonRpcId::Number(2), "ping", Some(json!({}))),
            None,
        )
        .await
    }

    pub async fn list_tools(
        &self,
        headers: Option<&BTreeMap<String, String>>,
    ) -> Result<Vec<NormalizedMcpTool>, McpClientError> {
        let mut request_headers = headers.cloned().unwrap_or_default();
        let (_initialize, session_id) = self.initialize_with_session(headers).await?;
        if let Some(session_id) = session_id {
            request_headers.insert(MCP_SESSION_ID_HEADER.to_string(), session_id);
        }
        self.send_notification(
            JsonRpcNotification::<Value>::new("notifications/initialized", None),
            Some(&request_headers),
        )
        .await?;
        let mut cursor = None;
        let mut seen_cursors = BTreeSet::new();
        let mut tools = Vec::new();
        for page_index in 0..MAX_TOOLS_LIST_PAGES {
            let response: ToolsListResponse = self
                .send(
                    JsonRpcRequest::new(
                        JsonRpcId::Number((page_index + 3) as i64),
                        "tools/list",
                        Some(ToolsListRequest {
                            cursor: cursor.clone(),
                        }),
                    ),
                    Some(&request_headers),
                )
                .await?;
            tools.extend(response.tools);
            let Some(next_cursor) = response.next_cursor.filter(|value| !value.is_empty()) else {
                return normalize_tools(tools);
            };
            if !seen_cursors.insert(next_cursor.clone()) {
                return Err(McpClientError::InvalidResponse {
                    message: "MCP tools/list returned a repeated nextCursor".to_string(),
                });
            }
            cursor = Some(next_cursor);
        }
        Err(McpClientError::InvalidResponse {
            message: format!("MCP tools/list exceeded {MAX_TOOLS_LIST_PAGES} pages"),
        })
    }

    pub async fn call_tool(
        &self,
        headers: Option<&BTreeMap<String, String>>,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolsCallResponse, McpClientError> {
        let mut request_headers = headers.cloned().unwrap_or_default();
        let (_initialize, session_id) = self.initialize_with_session(headers).await?;
        if let Some(session_id) = session_id {
            request_headers.insert(MCP_SESSION_ID_HEADER.to_string(), session_id);
        }
        self.send_notification(
            JsonRpcNotification::<Value>::new("notifications/initialized", None),
            Some(&request_headers),
        )
        .await?;
        self.send(
            JsonRpcRequest::new(
                JsonRpcId::Number(3),
                "tools/call",
                Some(ToolsCallRequest {
                    name: tool_name.to_string(),
                    arguments,
                }),
            ),
            Some(&request_headers),
        )
        .await
    }

    pub fn build_request<T: Serialize>(
        &self,
        request: &T,
        headers: Option<&BTreeMap<String, String>>,
    ) -> Result<reqwest::Request, McpClientError> {
        let mut request_headers = HeaderMap::new();
        request_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        request_headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        request_headers.insert(
            HeaderName::from_static(MCP_PROTOCOL_VERSION_HEADER),
            HeaderValue::from_str(&self.protocol_version)
                .map_err(|error| McpClientError::InvalidHeader(error.to_string()))?,
        );
        if let Some(headers) = headers {
            for (name, value) in headers {
                let name = HeaderName::from_bytes(name.as_bytes())
                    .map_err(|error| McpClientError::InvalidHeader(error.to_string()))?;
                let value = HeaderValue::from_str(value)
                    .map_err(|error| McpClientError::InvalidHeader(error.to_string()))?;
                request_headers.insert(name, value);
            }
        }

        self.client
            .post(self.endpoint.clone())
            .headers(request_headers)
            .json(request)
            .build()
            .map_err(|error| McpClientError::Transport(error.to_string()))
    }

    async fn send<T: Serialize, R: DeserializeOwned + Default>(
        &self,
        request: JsonRpcRequest<T>,
        headers: Option<&BTreeMap<String, String>>,
    ) -> Result<R, McpClientError> {
        self.send_with_session(request, headers)
            .await
            .map(|(response, _session_id)| response)
    }

    async fn initialize_with_session(
        &self,
        headers: Option<&BTreeMap<String, String>>,
    ) -> Result<(InitializeResponse, Option<String>), McpClientError> {
        self.send_with_session(
            JsonRpcRequest::new(
                JsonRpcId::Number(1),
                "initialize",
                Some(InitializeRequest::default()),
            ),
            headers,
        )
        .await
    }

    async fn send_notification<T: Serialize>(
        &self,
        notification: JsonRpcNotification<T>,
        headers: Option<&BTreeMap<String, String>>,
    ) -> Result<(), McpClientError> {
        let response = self
            .client
            .execute(self.build_request(&notification, headers)?)
            .await
            .map_err(classify_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            let body = read_bounded_body(response).await?;
            return Err(McpClientError::Http {
                status: status.as_u16(),
                body: bounded_body(&body),
            });
        }
        Ok(())
    }

    async fn send_with_session<T: Serialize, R: DeserializeOwned + Default>(
        &self,
        request: JsonRpcRequest<T>,
        headers: Option<&BTreeMap<String, String>>,
    ) -> Result<(R, Option<String>), McpClientError> {
        let response = self
            .client
            .execute(self.build_request(&request, headers)?)
            .await
            .map_err(classify_reqwest_error)?;
        let status = response.status();
        let is_sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"));
        let session_id = response
            .headers()
            .get(MCP_SESSION_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let oversized_content_length = response
            .content_length()
            .filter(|length| *length > MAX_RESPONSE_BODY_BYTES as u64);
        if oversized_content_length.is_some() {
            return Err(McpClientError::ResponseTooLarge {
                limit_bytes: MAX_RESPONSE_BODY_BYTES,
            });
        }
        let body = read_bounded_body(response).await?;
        if !status.is_success() {
            return Err(McpClientError::Http {
                status: status.as_u16(),
                body: bounded_body(&body),
            });
        }
        let result = if is_sse {
            decode_sse_json_rpc_result_for_id(&body, Some(&request.id))?
        } else {
            decode_json_rpc_result(&body)?
        };
        Ok((result, session_id))
    }
}

pub fn decode_json_rpc_result<T: DeserializeOwned + Default>(
    body: &str,
) -> Result<T, McpClientError> {
    decode_json_rpc_result_for_id(body, None)?.ok_or_else(|| McpClientError::InvalidResponse {
        message: "JSON-RPC response is missing result".to_string(),
    })
}

fn decode_json_rpc_result_for_id<T: DeserializeOwned + Default>(
    body: &str,
    expected_id: Option<&JsonRpcId>,
) -> Result<Option<T>, McpClientError> {
    let response: JsonRpcResponse<T> =
        serde_json::from_str(body).map_err(|error| McpClientError::InvalidResponse {
            message: error.to_string(),
        })?;
    if response.jsonrpc != "2.0" {
        return Err(McpClientError::InvalidResponse {
            message: "JSON-RPC version must be 2.0".to_string(),
        });
    }
    if let Some(expected_id) = expected_id {
        match response.id.as_ref() {
            Some(actual_id) if actual_id == expected_id => {}
            Some(_) => return Ok(None),
            None if response.error.is_none() => return Ok(None),
            None => {}
        }
    }
    if let Some(error) = response.error {
        return Err(McpClientError::JsonRpc(error));
    }
    response
        .result
        .ok_or_else(|| McpClientError::InvalidResponse {
            message: "JSON-RPC response is missing result".to_string(),
        })
        .map(Some)
}

pub fn decode_sse_json_rpc_result<T: DeserializeOwned + Default>(
    body: &str,
) -> Result<T, McpClientError> {
    decode_sse_json_rpc_result_for_id(body, None)
}

fn decode_sse_json_rpc_result_for_id<T: DeserializeOwned + Default>(
    body: &str,
    expected_id: Option<&JsonRpcId>,
) -> Result<T, McpClientError> {
    let mut data_lines = Vec::new();
    let mut saw_data_event = false;
    for line in body.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            if !data_lines.is_empty() {
                saw_data_event = true;
                if let Some(result) =
                    decode_json_rpc_result_for_id(&data_lines.join("\n"), expected_id)?
                {
                    return Ok(result);
                }
                data_lines.clear();
            }
            continue;
        }
        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_string());
        }
    }
    if !data_lines.is_empty() {
        saw_data_event = true;
        if let Some(result) = decode_json_rpc_result_for_id(&data_lines.join("\n"), expected_id)? {
            return Ok(result);
        }
    }
    let message = if saw_data_event {
        "SSE response did not contain a JSON-RPC result event for the request"
    } else {
        "SSE response did not contain a JSON-RPC data event"
    };
    Err(McpClientError::InvalidResponse {
        message: message.to_string(),
    })
}

async fn read_bounded_body(response: reqwest::Response) -> Result<String, McpClientError> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| McpClientError::Transport(error.to_string()))?;
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BODY_BYTES {
            return Err(McpClientError::ResponseTooLarge {
                limit_bytes: MAX_RESPONSE_BODY_BYTES,
            });
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|error| McpClientError::InvalidResponse {
        message: error.to_string(),
    })
}

pub fn normalize_tools(tools: Vec<McpTool>) -> Result<Vec<NormalizedMcpTool>, McpClientError> {
    let mut normalized = Vec::with_capacity(tools.len());
    let mut names = BTreeSet::new();
    for tool in tools {
        let name = tool.name.trim();
        if name.is_empty() {
            return Err(McpClientError::InvalidToolSchema {
                tool_name: tool.name,
                message: "tool name cannot be empty".to_string(),
            });
        }
        if !names.insert(name.to_string()) {
            return Err(McpClientError::InvalidToolSchema {
                tool_name: name.to_string(),
                message: "duplicate tool name".to_string(),
            });
        }
        let input_schema = if tool.input_schema.is_null() {
            json!({"type": "object", "properties": {}})
        } else {
            tool.input_schema
        };
        if !input_schema.is_object() {
            return Err(McpClientError::InvalidToolSchema {
                tool_name: name.to_string(),
                message: "inputSchema must be a JSON object".to_string(),
            });
        }
        let input_schema = canonicalize_json(&input_schema);
        let schema_hash = schema_hash(&input_schema)?;
        normalized.push(NormalizedMcpTool {
            name: name.to_string(),
            description: tool.description.map(|value| value.trim().to_string()),
            input_schema,
            schema_hash,
        });
    }
    Ok(normalized)
}

pub fn schema_hash(value: &Value) -> Result<String, McpClientError> {
    let canonical = serde_json::to_vec(&canonicalize_json(value)).map_err(|error| {
        McpClientError::InvalidResponse {
            message: error.to_string(),
        }
    })?;
    let digest = Sha256::digest(canonical);
    Ok(format!("sha256:{digest:x}"))
}

#[must_use]
pub fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_json).collect()),
        Value::Object(object) => {
            let mut sorted = BTreeMap::new();
            for (key, value) in object {
                sorted.insert(key.clone(), canonicalize_json(value));
            }
            let mut map = Map::new();
            for (key, value) in sorted {
                map.insert(key, value);
            }
            Value::Object(map)
        }
        _ => value.clone(),
    }
}

fn classify_reqwest_error(error: reqwest::Error) -> McpClientError {
    if error.is_timeout() {
        McpClientError::Timeout
    } else {
        McpClientError::Transport(error.to_string())
    }
}

fn bounded_body(body: &str) -> String {
    const MAX_BODY: usize = 4096;
    if body.len() <= MAX_BODY {
        return body.to_string();
    }
    body.chars().take(MAX_BODY).collect()
}

#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    #[error("invalid MCP server URL `{url}`: {message}")]
    InvalidUrl { url: String, message: String },
    #[error("invalid MCP request header: {0}")]
    InvalidHeader(String),
    #[error("MCP upstream timed out")]
    Timeout,
    #[error("MCP transport failure: {0}")]
    Transport(String),
    #[error("MCP upstream returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("MCP upstream response exceeded {limit_bytes} bytes")]
    ResponseTooLarge { limit_bytes: usize },
    #[error("MCP JSON-RPC error: {0:?}")]
    JsonRpc(JsonRpcErrorObject),
    #[error("invalid MCP response: {message}")]
    InvalidResponse { message: String },
    #[error("invalid MCP tool schema for `{tool_name}`: {message}")]
    InvalidToolSchema { tool_name: String, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_rpc_result() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
        let result: ToolsListResponse = decode_json_rpc_result(body).expect("decode result");

        assert!(result.tools.is_empty());
    }

    #[test]
    fn parses_tools_list_cursor_and_serializes_request_cursor() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[],"nextCursor":"page-2"}}"#;
        let result: ToolsListResponse = decode_json_rpc_result(body).expect("decode result");
        assert_eq!(result.next_cursor.as_deref(), Some("page-2"));

        let request = JsonRpcRequest::new(
            JsonRpcId::Number(2),
            "tools/list",
            Some(ToolsListRequest {
                cursor: Some("page-2".to_string()),
            }),
        );
        let encoded = serde_json::to_value(request).expect("serialize request");
        assert_eq!(encoded["params"], json!({"cursor": "page-2"}));
    }

    #[test]
    fn serializes_tools_call_request() {
        let request = JsonRpcRequest::new(
            JsonRpcId::Number(3),
            "tools/call",
            Some(ToolsCallRequest {
                name: "search".to_string(),
                arguments: json!({"q": "mcp"}),
            }),
        );
        let encoded = serde_json::to_value(request).expect("serialize request");
        assert_eq!(
            encoded["params"],
            json!({"name": "search", "arguments": {"q": "mcp"}})
        );
    }

    #[test]
    fn classifies_json_rpc_error() {
        let body = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"missing"}}"#;
        let error = decode_json_rpc_result::<Value>(body).expect_err("error");

        assert!(matches!(error, McpClientError::JsonRpc(_)));
    }

    #[test]
    fn request_includes_protocol_version_header() {
        let client =
            StreamableHttpClient::new("https://mcp.example.com/mcp", Duration::from_secs(5))
                .expect("client");
        let request = client
            .build_request(
                &JsonRpcRequest::new(JsonRpcId::Number(1), "ping", Some(json!({}))),
                None,
            )
            .expect("request");

        assert_eq!(
            request.headers().get(MCP_PROTOCOL_VERSION_HEADER).unwrap(),
            DEFAULT_PROTOCOL_VERSION
        );
        assert_eq!(
            request.headers().get(ACCEPT).unwrap(),
            "application/json, text/event-stream"
        );
    }

    #[test]
    fn decodes_sse_json_rpc_result() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n\n";
        let result: ToolsListResponse = decode_sse_json_rpc_result(body).expect("decode result");

        assert!(result.tools.is_empty());
    }

    #[test]
    fn decodes_matching_sse_response_after_non_response_events() {
        let body = concat!(
            "event: message\n",
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"progress\":1}}\n",
            "\n",
            "event: message\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":99,\"method\":\"sampling/createMessage\",\"params\":{}}\n",
            "\n",
            "event: message\n",
            "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[{\"name\":\"search\",\"inputSchema\":{\"type\":\"object\"}}]}}\n",
            "\n",
        );
        let result: ToolsListResponse =
            decode_sse_json_rpc_result_for_id(body, Some(&JsonRpcId::Number(1)))
                .expect("decode matching result");

        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name, "search");
    }

    #[test]
    fn normalizes_tool_schema_and_hash_is_stable() {
        let first =
            json!({"type":"object","properties":{"b":{"type":"string"},"a":{"type":"number"}}});
        let second =
            json!({"properties":{"a":{"type":"number"},"b":{"type":"string"}},"type":"object"});

        assert_eq!(
            schema_hash(&first).expect("first hash"),
            schema_hash(&second).expect("second hash")
        );
    }

    #[test]
    fn rejects_non_object_input_schema() {
        let error = normalize_tools(vec![McpTool {
            name: "bad".to_string(),
            description: None,
            input_schema: json!("not object"),
        }])
        .expect_err("invalid schema");

        assert!(matches!(error, McpClientError::InvalidToolSchema { .. }));
    }

    #[test]
    fn rejects_duplicate_tool_names() {
        let error = normalize_tools(vec![
            McpTool {
                name: "search".to_string(),
                description: None,
                input_schema: json!({"type": "object"}),
            },
            McpTool {
                name: " search ".to_string(),
                description: None,
                input_schema: json!({"type": "object"}),
            },
        ])
        .expect_err("duplicate name");

        assert!(matches!(error, McpClientError::InvalidToolSchema { .. }));
    }
}
