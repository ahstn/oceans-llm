use std::{
    error::Error as _,
    io,
    time::{Duration, Instant},
};

use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Path, State},
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode,
        header::{ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, WWW_AUTHENTICATE},
    },
    response::IntoResponse,
};
use futures_util::TryStreamExt;
use gateway_core::{
    ApiKeyOwnerKind, AuthError, ExternalMcpServerRecord, GatewayError, OpenAiErrorEnvelope,
    ProviderError, auth::extract_bearer_token,
};
use gateway_service::{McpGatewayService, McpGatewayUpstream};
use url::Url;

use crate::http::state::AppState;

const X_OCEANS_API_KEY: &str = "x-oceans-api-key";
const MCP_PROTOCOL_VERSION: &str = "mcp-protocol-version";
const MCP_SESSION_ID: &str = "mcp-session-id";
const LAST_EVENT_ID: &str = "last-event-id";
const MAX_MCP_REQUEST_BODY_BYTES: usize = 4 * 1024 * 1024;

#[tracing::instrument(
    skip(state, request),
    fields(
        server_key = %server_key,
        mcp_server_id = tracing::field::Empty,
        upstream_auth_mode = tracing::field::Empty,
        owner_kind = tracing::field::Empty,
        status_code = tracing::field::Empty,
    )
)]
pub async fn mcp_streamable_http_proxy(
    State(state): State<AppState>,
    Path(server_key): Path<String>,
    request: Request<Body>,
) -> Response<Body> {
    let started_at = Instant::now();
    let method = request.method().clone();
    let has_query = request.uri().query().is_some();
    let headers = request.headers().clone();

    let bearer_token = match extract_mcp_gateway_api_key(&headers) {
        Ok(token) => token,
        Err(error) => return mcp_error_response(error.into()),
    };

    let auth = match state.service.authenticate_bearer_token(&bearer_token).await {
        Ok(auth) => auth,
        Err(error) => return mcp_error_response(error),
    };
    tracing::Span::current().record("owner_kind", auth.owner_kind.as_str());
    if !matches!(
        auth.owner_kind,
        ApiKeyOwnerKind::User | ApiKeyOwnerKind::ServiceAccount
    ) {
        return mcp_error_response(AuthError::InsufficientPrivileges.into());
    }
    if has_query {
        return mcp_error_response(GatewayError::InvalidRequest(
            "query strings are not accepted on MCP gateway routes".to_string(),
        ));
    }

    let gateway = McpGatewayService::new(state.store.clone());
    let upstream = match gateway.prepare_upstream(&server_key).await {
        Ok(upstream) => upstream,
        Err(error) => return mcp_error_response(error),
    };
    tracing::Span::current().record("mcp_server_id", upstream.server.mcp_server_id.to_string());
    tracing::Span::current().record("upstream_auth_mode", upstream.server.auth_mode.as_str());

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

    match proxy_upstream(&state.mcp_http_client, &method, &headers, body, &upstream).await {
        Ok(response) => {
            tracing::Span::current().record("status_code", i64::from(response.status().as_u16()));
            tracing::debug!(
                elapsed_ms = started_at.elapsed().as_millis(),
                "proxied MCP streamable HTTP request"
            );
            response
        }
        Err(error) => mcp_error_response(error),
    }
}

fn body_read_exceeded_limit(error: &axum::Error) -> bool {
    error
        .source()
        .is_some_and(|source| source.to_string().contains("length limit exceeded"))
        || error.to_string().contains("length limit exceeded")
}

fn extract_mcp_gateway_api_key(headers: &HeaderMap) -> Result<String, AuthError> {
    let authorization_token = headers
        .get(AUTHORIZATION)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| AuthError::InvalidAuthorizationHeader)
                .and_then(extract_bearer_token)
                .map(str::to_string)
        })
        .transpose()?;

    let explicit_key = headers
        .get(X_OCEANS_API_KEY)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| AuthError::InvalidAuthorizationHeader)
                .map(str::trim)
                .and_then(|value| {
                    if value.is_empty() {
                        Err(AuthError::MissingBearerToken)
                    } else {
                        Ok(value.to_string())
                    }
                })
        })
        .transpose()?;

    match (authorization_token, explicit_key) {
        (Some(authorization_token), Some(explicit_key)) if authorization_token == explicit_key => {
            Ok(authorization_token)
        }
        (Some(_), Some(_)) => Err(AuthError::ConflictingApiKeyHeaders),
        (Some(authorization_token), None) => Ok(authorization_token),
        (None, Some(explicit_key)) => Ok(explicit_key),
        (None, None) => Err(AuthError::MissingAuthorizationHeader),
    }
}

async fn proxy_upstream(
    client: &reqwest::Client,
    method: &Method,
    inbound_headers: &HeaderMap,
    body: axum::body::Bytes,
    upstream: &McpGatewayUpstream,
) -> Result<Response<Body>, GatewayError> {
    let upstream_url = upstream_url(&upstream.server)?;
    let method = reqwest::Method::from_bytes(method.as_str().as_bytes()).map_err(|error| {
        GatewayError::InvalidRequest(format!("unsupported HTTP method: {error}"))
    })?;
    let is_long_lived_receive = method == reqwest::Method::GET;
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
        copy_response_header(name, &headers, response_headers)?;
    }
    builder
        .body(Body::from_stream(stream))
        .map_err(|error| GatewayError::Internal(format!("failed building MCP response: {error}")))
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

fn mcp_error_response(error: GatewayError) -> Response<Body> {
    let status =
        StatusCode::from_u16(error.http_status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    tracing::Span::current().record("status_code", i64::from(status.as_u16()));
    let mut response = (
        status,
        Json(OpenAiErrorEnvelope::from_gateway_error(&error)),
    )
        .into_response();
    if status == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn auth_extractor_accepts_authorization_only() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer gwk_id.secret"),
        );
        assert_eq!(
            extract_mcp_gateway_api_key(&headers).expect("token"),
            "gwk_id.secret"
        );
    }

    #[test]
    fn auth_extractor_accepts_explicit_header_only() {
        let mut headers = HeaderMap::new();
        headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
        assert_eq!(
            extract_mcp_gateway_api_key(&headers).expect("token"),
            "gwk_id.secret"
        );
    }

    #[test]
    fn auth_extractor_accepts_identical_dual_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer gwk_id.secret"),
        );
        headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
        assert_eq!(
            extract_mcp_gateway_api_key(&headers).expect("token"),
            "gwk_id.secret"
        );
    }

    #[test]
    fn auth_extractor_rejects_missing_credentials() {
        let headers = HeaderMap::new();
        let error = extract_mcp_gateway_api_key(&headers).expect_err("missing");
        assert!(matches!(error, AuthError::MissingAuthorizationHeader));
    }

    #[test]
    fn auth_extractor_rejects_malformed_authorization_even_with_explicit_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Basic gwk_id.secret"),
        );
        headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
        let error = extract_mcp_gateway_api_key(&headers).expect_err("malformed");
        assert!(matches!(error, AuthError::InvalidAuthorizationHeader));
    }

    #[test]
    fn auth_extractor_rejects_conflicting_dual_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer gwk_id.secret"),
        );
        headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.other"));
        let error = extract_mcp_gateway_api_key(&headers).expect_err("conflict");
        assert!(matches!(error, AuthError::ConflictingApiKeyHeaders));
    }
}
