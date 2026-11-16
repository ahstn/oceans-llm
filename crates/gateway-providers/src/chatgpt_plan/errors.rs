use futures_util::StreamExt;
use gateway_core::{ProviderError, ProviderStream, SseEventParser};
use serde_json::Value;

pub(super) fn response_request_id(headers: &reqwest::header::HeaderMap) -> Option<String> {
    ["x-request-id", "request-id", "openai-request-id"]
        .into_iter()
        .find_map(|name| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string)
        })
}
pub(super) fn upstream_error(
    status: u16,
    body: String,
    request_id: Option<String>,
) -> ProviderError {
    // Do not replace direct-admission {"detail": ...} bodies with an API error envelope.
    ProviderError::UpstreamHttp {
        status,
        body,
        request_id,
    }
}

fn stream_error_status(value: &Value) -> u16 {
    let error = value
        .get("response")
        .and_then(|response| response.get("error"))
        .filter(|error| !error.is_null())
        .or_else(|| value.get("error"))
        .unwrap_or(value);
    match error.get("code").and_then(Value::as_str) {
        Some("subscription_sharing_invalid_user") => 401,
        Some(
            "subscription_sharing_user_not_eligible"
            | "subscription_sharing_route_not_supported"
            | "chatpass_v2_scope_not_authorized"
            | "chatpass_v2_invalid_authorization_context",
        ) => 403,
        Some("subscription_sharing_usage_limit_exceeded") => 429,
        Some(
            "subscription_sharing_usage_unavailable" | "subscription_sharing_user_unavailable",
        ) => 503,
        Some("subscription_sharing_unsupported_capability") => 400,
        _ => 502,
    }
}

/// Preserve completed and incomplete terminal events, including their usage, for
/// client handling and accounting. Failed events must still reach the gateway as
/// errors rather than successful byte chunks.
pub(super) fn require_terminal(
    mut normalized: ProviderStream,
    request_id: Option<String>,
) -> ProviderStream {
    Box::pin(async_stream::stream! {
        let mut parser = SseEventParser::default();
        let mut terminal = false;
        while let Some(chunk) = normalized.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };
            let events = match parser.push_bytes(&chunk) {
                Ok(events) => events,
                Err(error) => {
                    yield Err(ProviderError::Transport(error.to_string()));
                    return;
                }
            };
            for event in events {
                let data = event.data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let value: Value = match serde_json::from_str(data) {
                    Ok(value) => value,
                    Err(error) => {
                        yield Err(ProviderError::Transport(error.to_string()));
                        return;
                    }
                };
                let event_type = value.get("type").and_then(Value::as_str)
                    .or(event.event.as_deref());
                if matches!(event_type, Some("response.failed" | "error"))
                    || value.get("error").is_some_and(|error| !error.is_null())
                {
                    yield Err(upstream_error(stream_error_status(&value), event.data, request_id));
                    return;
                }
                if matches!(event_type, Some("response.completed" | "response.incomplete")) {
                    terminal = true;
                }
            }
            yield Ok(chunk);
        }
        if let Err(error) = parser.finish() {
            yield Err(ProviderError::Transport(error.to_string()));
            return;
        }
        if !terminal {
            yield Err(ProviderError::Transport(
                "chatgpt_plan stream ended without response.completed or response.incomplete".to_string(),
            ));
        }
    })
}
