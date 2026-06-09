use std::{collections::HashSet, io, time::Duration};

use axum::{
    body::{Body, Bytes},
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode,
        header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE, WWW_AUTHENTICATE},
    },
};
use futures_util::TryStreamExt;
use gateway_core::{ExternalMcpServerRecord, GatewayError, ProviderError};
use gateway_service::McpGatewayUpstream;
use serde_json::Value;
use url::Url;

use super::{
    LAST_EVENT_ID, MAX_MCP_REWRITE_BODY_BYTES, MCP_PROTOCOL_VERSION, MCP_SESSION_ID,
    mcp_error_response,
};

pub(super) struct BufferedMcpResponse {
    pub(super) status: StatusCode,
    headers: reqwest::header::HeaderMap,
    body: Bytes,
}

impl BufferedMcpResponse {
    pub(super) fn body(&self) -> &[u8] {
        &self.body
    }

    pub(super) fn into_response(self) -> Response<Body> {
        response_from_parts(self.status, &self.headers, Body::from(self.body))
            .unwrap_or_else(mcp_error_response)
    }
}

pub(super) async fn proxy_upstream(
    client: &reqwest::Client,
    method: &Method,
    inbound_headers: &HeaderMap,
    body: Bytes,
    upstream: &McpGatewayUpstream,
) -> Result<Response<Body>, GatewayError> {
    let upstream_url = upstream_url(&upstream.server)?;
    let method = reqwest::Method::from_bytes(method.as_str().as_bytes()).map_err(|error| {
        GatewayError::InvalidRequest(format!("unsupported HTTP method: {error}"))
    })?;
    let is_long_lived_receive =
        method == reqwest::Method::GET || accepts_event_stream(inbound_headers);
    let mut request = client.request(method, upstream_url).body(body);
    if !is_long_lived_receive {
        request = request.timeout(Duration::from_millis(
            upstream.server.timeout_ms.max(1) as u64
        ));
    }

    request = apply_forwarded_request_headers(request, inbound_headers)?;
    if let Some(headers) = &upstream.headers {
        request = apply_gateway_managed_upstream_headers(request, headers)?;
    }

    let response = request.send().await.map_err(map_reqwest_error)?;
    response_from_upstream(response)
}

pub(super) async fn proxy_buffered(
    client: &reqwest::Client,
    method: &Method,
    inbound_headers: &HeaderMap,
    body: Bytes,
    upstream: &McpGatewayUpstream,
) -> Result<BufferedMcpResponse, GatewayError> {
    let upstream_url = upstream_url(&upstream.server)?;
    let method = reqwest::Method::from_bytes(method.as_str().as_bytes()).map_err(|error| {
        GatewayError::InvalidRequest(format!("unsupported HTTP method: {error}"))
    })?;
    let mut request = client
        .request(method, upstream_url)
        .timeout(Duration::from_millis(
            upstream.server.timeout_ms.max(1) as u64
        ))
        .body(body);
    request = apply_forwarded_request_headers(request, inbound_headers)?;
    if let Some(headers) = &upstream.headers {
        request = apply_gateway_managed_upstream_headers(request, headers)?;
    }

    let response = request.send().await.map_err(map_reqwest_error)?;
    if response.content_length().unwrap_or(0) > MAX_MCP_REWRITE_BODY_BYTES {
        return Err(GatewayError::PayloadTooLarge {
            limit_bytes: MAX_MCP_REWRITE_BODY_BYTES as usize,
        });
    }
    let status = StatusCode::from_u16(response.status().as_u16()).map_err(|error| {
        GatewayError::Internal(format!("upstream returned invalid status code: {error}"))
    })?;
    let headers = response.headers().clone();
    let body = response.bytes().await.map_err(map_reqwest_error)?;
    if body.len() as u64 > MAX_MCP_REWRITE_BODY_BYTES {
        return Err(GatewayError::PayloadTooLarge {
            limit_bytes: MAX_MCP_REWRITE_BODY_BYTES as usize,
        });
    }
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
    response_from_parts(
        response.status,
        &response.headers,
        Body::from(filtered_body),
    )
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
    if value.get("error").is_some() {
        return serde_json::to_vec(&value).map_err(|error| {
            GatewayError::Internal(format!("failed encoding MCP error: {error}"))
        });
    }
    filter_tools_array(&mut value, allowed_tool_names, id)?;
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
    let mut out = String::with_capacity(text.len());
    for event in text.split("\n\n") {
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
        filter_tools_array(&mut value, allowed_tool_names, None)?;
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
    upstream_headers: &reqwest::header::HeaderMap,
    body: Body,
) -> Result<Response<Body>, GatewayError> {
    let mut builder = Response::builder().status(status);
    let response_headers = builder.headers_mut().ok_or_else(|| {
        GatewayError::Internal("failed constructing MCP upstream response".to_string())
    })?;
    for name in [
        CONTENT_TYPE.as_str(),
        CACHE_CONTROL.as_str(),
        MCP_PROTOCOL_VERSION,
        MCP_SESSION_ID,
        WWW_AUTHENTICATE.as_str(),
    ] {
        copy_response_header(name, upstream_headers, response_headers)?;
    }
    builder
        .body(body)
        .map_err(|error| GatewayError::Internal(format!("failed building MCP response: {error}")))
}

fn accepts_event_stream(headers: &HeaderMap) -> bool {
    headers.get_all(ACCEPT).iter().any(|value| {
        value
            .to_str()
            .is_ok_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
    })
}

fn upstream_url(server: &ExternalMcpServerRecord) -> Result<Url, GatewayError> {
    Url::parse(&server.server_url)
        .map_err(|error| GatewayError::InvalidRequest(format!("server_url is invalid: {error}")))
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
        let header_name =
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                GatewayError::InvalidRequest(format!("invalid header name: {error}"))
            })?;
        for value in inbound_headers.get_all(name).iter() {
            let header_value = value.to_str().map_err(|_| {
                GatewayError::InvalidRequest(format!("{name} header must be visible ASCII"))
            })?;
            request = request.header(header_name.clone(), header_value);
        }
    }
    Ok(request)
}

fn apply_gateway_managed_upstream_headers(
    mut request: reqwest::RequestBuilder,
    headers: &std::collections::BTreeMap<String, String>,
) -> Result<reqwest::RequestBuilder, GatewayError> {
    for (name, value) in headers {
        let header_name =
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                GatewayError::InvalidRequest(format!("configured MCP header is invalid: {error}"))
            })?;
        let header_value = reqwest::header::HeaderValue::from_str(value).map_err(|error| {
            GatewayError::InvalidRequest(format!("configured MCP header value is invalid: {error}"))
        })?;
        request = request.header(header_name, header_value);
    }
    Ok(request)
}

fn response_from_upstream(response: reqwest::Response) -> Result<Response<Body>, GatewayError> {
    let status = StatusCode::from_u16(response.status().as_u16()).map_err(|error| {
        GatewayError::Internal(format!("upstream returned invalid status code: {error}"))
    })?;
    let headers = response.headers().clone();
    let stream = response
        .bytes_stream()
        .map_err(|error| io::Error::other(error.to_string()));
    response_from_parts(status, &headers, Body::from_stream(stream))
}

fn copy_response_header(
    name: &str,
    upstream_headers: &reqwest::header::HeaderMap,
    response_headers: &mut HeaderMap,
) -> Result<(), GatewayError> {
    let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
        GatewayError::Internal(format!("invalid response header name: {error}"))
    })?;
    for value in upstream_headers.get_all(name).iter() {
        let value = HeaderValue::from_bytes(value.as_bytes()).map_err(|error| {
            GatewayError::Internal(format!("invalid upstream response header value: {error}"))
        })?;
        response_headers.append(header_name.clone(), value);
    }
    Ok(())
}

fn map_reqwest_error(error: reqwest::Error) -> GatewayError {
    if error.is_timeout() {
        return ProviderError::Timeout.into();
    }
    ProviderError::Transport(error.to_string()).into()
}
