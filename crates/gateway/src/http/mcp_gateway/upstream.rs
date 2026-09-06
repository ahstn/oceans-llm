use std::{collections::HashSet, io, time::Duration};

use axum::{
    body::{Body, Bytes},
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode,
        header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE},
    },
};
use futures_util::TryStreamExt;
use gateway_core::{GatewayError, ProviderError};
use gateway_service::McpGatewayUpstream;
use serde_json::Value;
use url::Url;

use super::{LAST_EVENT_ID, MAX_MCP_REWRITE_BODY_BYTES, MCP_PROTOCOL_VERSION, MCP_SESSION_ID};

pub(super) struct BufferedMcpResponse {
    pub(super) status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

pub(super) async fn proxy_upstream(
    client: &reqwest::Client,
    method: &Method,
    inbound_headers: &HeaderMap,
    body: Bytes,
    upstream: &McpGatewayUpstream,
) -> Result<Response<Body>, GatewayError> {
    let is_long_lived_receive = method == Method::GET || accepts_event_stream(inbound_headers);
    let mut request = upstream_request(client, method, inbound_headers, body, upstream)?;
    if !is_long_lived_receive {
        request = request.timeout(Duration::from_millis(
            upstream.server.timeout_ms.max(1) as u64
        ));
    }

    let response = request.send().await.map_err(map_reqwest_error)?;
    let status = response.status();
    let headers = response.headers().clone();
    let stream = response
        .bytes_stream()
        .map_err(|error| io::Error::other(error.without_url().to_string()));
    Ok(response_from_parts(
        status,
        &headers,
        Body::from_stream(stream),
    ))
}

pub(super) async fn proxy_buffered(
    client: &reqwest::Client,
    method: &Method,
    inbound_headers: &HeaderMap,
    body: Bytes,
    upstream: &McpGatewayUpstream,
) -> Result<BufferedMcpResponse, GatewayError> {
    let request = upstream_request(client, method, inbound_headers, body, upstream)?.timeout(
        Duration::from_millis(upstream.server.timeout_ms.max(1) as u64),
    );

    let response = request.send().await.map_err(map_reqwest_error)?;
    if response.content_length().unwrap_or(0) > MAX_MCP_REWRITE_BODY_BYTES {
        return Err(GatewayError::PayloadTooLarge {
            limit_bytes: MAX_MCP_REWRITE_BODY_BYTES as usize,
        });
    }
    let status = response.status();
    let headers = response.headers().clone();
    let body = read_limited_response_body(response).await?;
    Ok(BufferedMcpResponse {
        status,
        headers,
        body,
    })
}

pub(super) async fn proxy_tools_list(
    client: &reqwest::Client,
    method: &Method,
    inbound_headers: &HeaderMap,
    body: Bytes,
    upstream: &McpGatewayUpstream,
    allowed_tool_names: &HashSet<&str>,
    id: Option<&Value>,
) -> Result<Response<Body>, GatewayError> {
    let response = proxy_buffered(client, method, inbound_headers, body, upstream).await?;
    let content_type = response
        .headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let filtered_body = if content_type.contains("text/event-stream") {
        filter_tools_list_sse(&response.body, allowed_tool_names)?
    } else {
        filter_tools_list_json(&response.body, allowed_tool_names, id)?
    };
    Ok(response_from_parts(
        response.status,
        &response.headers,
        Body::from(filtered_body),
    ))
}

fn upstream_request(
    client: &reqwest::Client,
    method: &Method,
    inbound_headers: &HeaderMap,
    body: Bytes,
    upstream: &McpGatewayUpstream,
) -> Result<reqwest::RequestBuilder, GatewayError> {
    let url = Url::parse(&upstream.server.server_url)
        .map_err(|error| GatewayError::InvalidRequest(format!("server_url is invalid: {error}")))?;
    let request = client.request(method.clone(), url).body(body);
    let request = apply_forwarded_request_headers(request, inbound_headers)?;
    match &upstream.headers {
        Some(headers) => apply_gateway_managed_upstream_headers(request, headers),
        None => Ok(request),
    }
}

fn filter_tools_list_json(
    body: &[u8],
    allowed_tool_names: &HashSet<&str>,
    id: Option<&Value>,
) -> Result<Vec<u8>, GatewayError> {
    let mut value: Value = serde_json::from_slice(body).map_err(|error| {
        GatewayError::InvalidRequest(format!(
            "MCP tools/list upstream returned invalid JSON: {error}"
        ))
    })?;
    if value.get("error").is_none() || value.get("result").is_some() {
        filter_tools_array(&mut value, allowed_tool_names, id)?;
    }
    serde_json::to_vec(&value)
        .map_err(|error| GatewayError::Internal(format!("failed encoding MCP tools/list: {error}")))
}

fn filter_tools_list_sse(
    body: &[u8],
    allowed_tool_names: &HashSet<&str>,
) -> Result<Vec<u8>, GatewayError> {
    let text = std::str::from_utf8(body).map_err(|error| {
        GatewayError::InvalidRequest(format!("MCP tools/list SSE was not UTF-8: {error}"))
    })?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(text.len());
    for event in normalized.split("\n\n") {
        if event.trim().is_empty() {
            continue;
        }
        let mut data_lines = Vec::new();
        let mut passthrough_lines = Vec::new();
        for line in event.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                data_lines.push(data.trim_start());
            } else {
                passthrough_lines.push(line);
            }
        }
        let data = data_lines.join("\n");
        if data.is_empty() || data == "[DONE]" {
            for line in passthrough_lines {
                out.push_str(line);
                out.push('\n');
            }
            if !data.is_empty() {
                out.push_str("data: ");
                out.push_str(&data);
                out.push('\n');
            }
            out.push('\n');
            continue;
        }
        let mut value: Value = serde_json::from_str(&data).map_err(|error| {
            GatewayError::InvalidRequest(format!(
                "MCP tools/list SSE data was invalid JSON: {error}"
            ))
        })?;
        // Notifications and RPC errors can precede the tools result in an SSE response.
        // Any result must still pass the tool allowlist, even in a malformed envelope.
        if value.get("result").is_some() || !is_rpc_error_or_notification(&value) {
            filter_tools_array(&mut value, allowed_tool_names, None)?;
        }
        for line in passthrough_lines {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("data: ");
        out.push_str(&serde_json::to_string(&value).map_err(|error| {
            GatewayError::Internal(format!("failed encoding MCP SSE data: {error}"))
        })?);
        out.push_str("\n\n");
    }
    Ok(out.into_bytes())
}

fn is_rpc_error_or_notification(value: &Value) -> bool {
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return false;
    }
    if let Some(error) = value.get("error") {
        return value.get("method").is_none()
            && value
                .get("id")
                .is_some_and(|id| id.is_null() || id.is_string() || id.is_number())
            && error.get("code").is_some_and(Value::is_i64)
            && error.get("message").is_some_and(Value::is_string);
    }
    value.get("id").is_none()
        && value.get("method").is_some_and(Value::is_string)
        && value
            .get("params")
            .is_none_or(|params| params.is_object() || params.is_array())
}

async fn read_limited_response_body(response: reqwest::Response) -> Result<Bytes, GatewayError> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.try_next().await.map_err(map_reqwest_error)? {
        if body.len().saturating_add(chunk.len()) > MAX_MCP_REWRITE_BODY_BYTES as usize {
            return Err(GatewayError::PayloadTooLarge {
                limit_bytes: MAX_MCP_REWRITE_BODY_BYTES as usize,
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}

fn filter_tools_array(
    value: &mut Value,
    allowed_tool_names: &HashSet<&str>,
    id: Option<&Value>,
) -> Result<(), GatewayError> {
    let tools = value
        .get_mut("result")
        .and_then(Value::as_object_mut)
        .and_then(|result| result.get_mut("tools"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            GatewayError::InvalidRequest(
                "MCP tools/list response did not contain result.tools".to_string(),
            )
        })?;
    tools.retain(|tool| {
        tool.get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| allowed_tool_names.contains(name))
    });
    if let (Some(id), Some(object)) = (id, value.as_object_mut()) {
        object.insert("id".to_string(), id.clone());
    }
    Ok(())
}

fn response_from_parts(
    status: StatusCode,
    upstream_headers: &HeaderMap,
    body: Body,
) -> Response<Body> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    let response_headers = response.headers_mut();
    for name in [CONTENT_TYPE.as_str(), MCP_PROTOCOL_VERSION, MCP_SESSION_ID] {
        for value in upstream_headers.get_all(name) {
            response_headers.append(HeaderName::from_static(name), value.clone());
        }
    }
    response_headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(super) fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers.get_all(ACCEPT).iter().any(|value| {
        value
            .to_str()
            .is_ok_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
    })
}

fn apply_forwarded_request_headers(
    mut request: reqwest::RequestBuilder,
    inbound_headers: &HeaderMap,
) -> Result<reqwest::RequestBuilder, GatewayError> {
    for name in [
        ACCEPT.as_str(),
        CONTENT_TYPE.as_str(),
        MCP_PROTOCOL_VERSION,
        MCP_SESSION_ID,
        LAST_EVENT_ID,
    ] {
        let header_name = HeaderName::from_static(name);
        for value in inbound_headers.get_all(name).iter() {
            value.to_str().map_err(|_| {
                GatewayError::InvalidRequest(format!("{name} header must be visible ASCII"))
            })?;
            request = request.header(header_name.clone(), value.clone());
        }
    }
    Ok(request)
}

fn apply_gateway_managed_upstream_headers(
    mut request: reqwest::RequestBuilder,
    headers: &std::collections::BTreeMap<String, String>,
) -> Result<reqwest::RequestBuilder, GatewayError> {
    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            GatewayError::InvalidRequest(format!("configured MCP header is invalid: {error}"))
        })?;
        let header_value = HeaderValue::from_str(value).map_err(|error| {
            GatewayError::InvalidRequest(format!("configured MCP header value is invalid: {error}"))
        })?;
        request = request.header(header_name, header_value);
    }
    Ok(request)
}

fn map_reqwest_error(error: reqwest::Error) -> GatewayError {
    if error.is_timeout() {
        return ProviderError::Timeout.into();
    }
    ProviderError::Transport(error.without_url().to_string()).into()
}

#[cfg(test)]
mod tests;
