//! Native transport helpers. Accounting and guardrails inspect these bodies without
//! rebuilding them from the Chat Completions projection used by other public APIs.
use bytes::Bytes;
use futures_util::StreamExt;
use gateway_core::{NativeMessagesResponse, ProviderError, ProviderRequestContext, ProviderStream};
use serde_json::Value;
use std::collections::BTreeMap;

fn response_headers(response: &crate::http::TracedResponse) -> BTreeMap<String, String> {
    response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            let name = name.as_str();
            (name.starts_with("anthropic-")
                || name.starts_with("x-openrouter-")
                || name.starts_with("x-ratelimit-")
                || name.starts_with("ratelimit-")
                || matches!(name, "retry-after" | "x-should-retry" | "request-id"))
            .then(|| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.to_string(), value.to_string()))
            })
            .flatten()
        })
        .collect()
}

pub(crate) async fn json_response(
    response: crate::http::TracedResponse,
) -> Result<NativeMessagesResponse<Value>, ProviderError> {
    let headers = response_headers(&response);
    let body = response
        .bytes()
        .await
        .map_err(crate::http::map_reqwest_error)?;
    let body = serde_json::from_slice(&body)
        .map_err(|error| ProviderError::Transport(error.to_string()))?;
    Ok(NativeMessagesResponse { body, headers })
}

pub(crate) fn stream_response(
    response: crate::http::TracedResponse,
) -> NativeMessagesResponse<ProviderStream> {
    let headers = response_headers(&response);
    NativeMessagesResponse {
        body: response_stream(response.bytes_stream()),
        headers,
    }
}

/// Forward the open Anthropic capability/header namespace, never gateway credentials,
/// cookies or connection headers. Beta lists are bounded and merged by `beta` instead.
pub(crate) fn request_headers(
    request: reqwest::RequestBuilder,
    context: &ProviderRequestContext,
) -> reqwest::RequestBuilder {
    let mut forwarded = reqwest::header::HeaderMap::new();
    for (name, value) in &context.request_headers {
        let name = name.to_ascii_lowercase();
        if (name.starts_with("anthropic-") || name.starts_with("x-claude-code-"))
            && name != "anthropic-beta"
            && let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::try_from(name),
                reqwest::header::HeaderValue::from_str(value),
            )
        {
            forwarded.insert(name, value);
        }
    }
    request.headers(forwarded)
}

pub(crate) async fn execute_request(
    client: &reqwest::Client,
    request: reqwest::Request,
    provider_type: &str,
    provider_key: &str,
) -> Result<crate::http::TracedResponse, ProviderError> {
    let response = crate::http::execute_request(client, request, provider_type, provider_key)
        .await
        .map_err(crate::http::map_reqwest_error)?;
    if !response.status().is_success() {
        return Err(ProviderError::NativeMessagesHttp {
            status: response.status().as_u16(),
            headers: response_headers(&response),
            body: response
                .text()
                .await
                .map_err(crate::http::map_reqwest_error)?,
        });
    }
    Ok(response)
}

pub(crate) fn response_stream<S>(upstream: S) -> ProviderStream
where
    S: futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    Box::pin(upstream.map(|chunk| chunk.map_err(crate::http::map_reqwest_error)))
}
