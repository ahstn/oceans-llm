use std::{collections::BTreeMap, time::Duration};

use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use url::Url;

pub const DEFAULT_PROTOCOL_VERSION: &str = "2025-03-26";
pub const MCP_PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcRequest<T = Value> {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<T>,
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
pub struct ToolsListResponse {
    #[serde(default)]
    pub tools: Vec<McpTool>,
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
        self.send(
            JsonRpcRequest::new(
                JsonRpcId::Number(1),
                "initialize",
                Some(InitializeRequest::default()),
            ),
            None,
        )
        .await
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
        let response: ToolsListResponse = self
            .send(
                JsonRpcRequest::new(JsonRpcId::Number(3), "tools/list", Some(json!({}))),
                headers,
            )
            .await?;
        normalize_tools(response.tools)
    }

    pub fn build_request<T: Serialize>(
        &self,
        request: &JsonRpcRequest<T>,
        headers: Option<&BTreeMap<String, String>>,
    ) -> Result<reqwest::Request, McpClientError> {
        let mut request_headers = HeaderMap::new();
        request_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        request_headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
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
        let response = self
            .client
            .execute(self.build_request(&request, headers)?)
            .await
            .map_err(classify_reqwest_error)?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| McpClientError::Transport(error.to_string()))?;
        if !status.is_success() {
            return Err(McpClientError::Http {
                status: status.as_u16(),
                body: bounded_body(&body),
            });
        }
        decode_json_rpc_result(status, &body)
    }
}

pub fn decode_json_rpc_result<T: DeserializeOwned + Default>(
    _status: StatusCode,
    body: &str,
) -> Result<T, McpClientError> {
    let response: JsonRpcResponse<T> =
        serde_json::from_str(body).map_err(|error| McpClientError::InvalidResponse {
            message: error.to_string(),
        })?;
    if response.jsonrpc != "2.0" {
        return Err(McpClientError::InvalidResponse {
            message: "JSON-RPC version must be 2.0".to_string(),
        });
    }
    if let Some(error) = response.error {
        return Err(McpClientError::JsonRpc(error));
    }
    response
        .result
        .ok_or_else(|| McpClientError::InvalidResponse {
            message: "JSON-RPC response is missing result".to_string(),
        })
}

pub fn normalize_tools(tools: Vec<McpTool>) -> Result<Vec<NormalizedMcpTool>, McpClientError> {
    let mut normalized = Vec::with_capacity(tools.len());
    for tool in tools {
        let name = tool.name.trim();
        if name.is_empty() {
            return Err(McpClientError::InvalidToolSchema {
                tool_name: tool.name,
                message: "tool name cannot be empty".to_string(),
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
        let result: ToolsListResponse =
            decode_json_rpc_result(StatusCode::OK, body).expect("decode result");

        assert!(result.tools.is_empty());
    }

    #[test]
    fn classifies_json_rpc_error() {
        let body = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"missing"}}"#;
        let error = decode_json_rpc_result::<Value>(StatusCode::OK, body).expect_err("error");

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
}
