//! Routing and cost response headers for inference endpoints.
//!
//! [`attach_response_headers`] opens a per-request scope around the handler. Route
//! selection and non-stream usage accounting record into that scope, and the middleware
//! turns the record into headers once the handler returns. The scope is a task-local, so
//! handlers keep their signatures and code outside a scope (direct-call tests, stream
//! bodies polled after the response is sent) records nothing.
//!
//! Streaming responses get the routing headers only, because their headers are sent
//! before usage is known. The stream wrapper writes the cost into the final usage event
//! instead, using [`with_stream_cost`].

use std::{cell::RefCell, future::Future, ops::Range};

use axum::{
    body::Bytes,
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use gateway_core::{ModelRoute, Money4};
use gateway_service::RecordedChatUsage;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::Deserialize;
use serde_json::{Map, Value, json, value::RawValue};
use uuid::Uuid;

pub(crate) const X_OCEANS_MODEL: HeaderName = HeaderName::from_static("x-oceans-model");
pub(crate) const X_OCEANS_ROUTE_ID: HeaderName = HeaderName::from_static("x-oceans-route-id");
pub(crate) const X_OCEANS_PROVIDER: HeaderName = HeaderName::from_static("x-oceans-provider");
pub(crate) const X_OCEANS_ATTEMPT_COUNT: HeaderName =
    HeaderName::from_static("x-oceans-attempt-count");
pub(crate) const X_OCEANS_RESPONSE_COST_USD: HeaderName =
    HeaderName::from_static("x-oceans-response-cost-usd");

/// Bytes percent-encoded in identifier headers. Non-ASCII bytes are always encoded, so
/// printable ASCII keys without `%` are sent as-is.
const HEADER_VALUE_ESCAPES: &AsciiSet = &CONTROLS.add(b'%');

tokio::task_local! {
    static RESPONSE_METADATA: RefCell<ResponseMetadata>;
}

#[derive(Debug, Default)]
struct ResponseMetadata {
    route: Option<RouteMetadata>,
    attempt_count: u32,
    cost_usd: Option<Money4>,
}

#[derive(Debug)]
struct RouteMetadata {
    model_key: String,
    route_id: Uuid,
    provider_key: String,
}

pub(crate) async fn attach_response_headers(request: Request, next: Next) -> Response {
    with_response_headers(next.run(request)).await
}

/// Runs `handler` in a fresh metadata scope and adds the recorded headers to its response.
pub(crate) async fn with_response_headers<F>(handler: F) -> Response
where
    F: Future<Output = Response>,
{
    let (mut response, metadata) = RESPONSE_METADATA
        .scope(RefCell::default(), async {
            let response = handler.await;
            (response, RESPONSE_METADATA.with(RefCell::take))
        })
        .await;
    metadata.apply(&mut response);
    response
}

/// Records the route chosen for this request. Each selected route is one provider attempt.
pub(crate) fn record_route(model_key: &str, route: &ModelRoute) {
    let _ = RESPONSE_METADATA.try_with(|metadata| {
        let mut metadata = metadata.borrow_mut();
        metadata.route = Some(RouteMetadata {
            model_key: model_key.to_string(),
            route_id: route.id,
            provider_key: route.provider_key.clone(),
        });
        metadata.attempt_count += 1;
    });
}

/// Records the cost written to the usage ledger for this request.
pub(crate) fn record_cost(usage: &RecordedChatUsage) {
    let _ = RESPONSE_METADATA.try_with(|metadata| {
        metadata.borrow_mut().cost_usd = billable_cost(usage);
    });
}

/// The cost the request log shows for this usage, or `None` when the row is unpriced.
pub(crate) fn billable_cost(usage: &RecordedChatUsage) -> Option<Money4> {
    usage
        .pricing_status
        .counts_toward_spend()
        .then_some(usage.computed_cost_usd)
}

impl ResponseMetadata {
    fn apply(self, response: &mut Response) {
        let Some(route) = self.route else {
            return;
        };
        let success = response.status().is_success();
        let headers = response.headers_mut();
        let mut insert = |name: HeaderName, value: String| {
            if let Ok(value) = HeaderValue::from_str(&value) {
                headers.insert(name, value);
            }
        };
        insert(X_OCEANS_ROUTE_ID, route.route_id.to_string());
        insert(X_OCEANS_PROVIDER, encode_identifier(&route.provider_key));
        insert(X_OCEANS_MODEL, encode_identifier(&route.model_key));
        if success {
            insert(X_OCEANS_ATTEMPT_COUNT, self.attempt_count.to_string());
            if let Some(cost_usd) = self.cost_usd {
                insert(X_OCEANS_RESPONSE_COST_USD, cost_usd.format_4dp());
            }
        }
    }
}

/// Configured keys may hold any Unicode, but header values must be visible ASCII.
fn encode_identifier(key: &str) -> String {
    utf8_percent_encode(key, HEADER_VALUE_ESCAPES).to_string()
}

/// Splits `chunk` just before its last complete SSE event with a usage object, so the
/// events ahead of it can be forwarded while the usage event is held. Returns `None` when
/// the chunk has no such event.
pub(crate) fn split_before_usage_event(chunk: &Bytes) -> Option<(Bytes, Bytes)> {
    let text = std::str::from_utf8(chunk).ok()?;
    let usage_event = last_usage_event(text)?;
    let start = usage_event.range.start;
    Some((chunk.slice(..start), chunk.slice(start..)))
}

/// Adds `usage.oceans.cost` to the last complete SSE event in `chunk` that has a JSON
/// usage object. Every other byte is kept. Returns `None` when there is no such event, so
/// the caller forwards the chunk unchanged.
pub(crate) fn with_stream_cost(chunk: &[u8], cost_usd: Money4) -> Option<Bytes> {
    let text = std::str::from_utf8(chunk).ok()?;
    let usage_event = last_usage_event(text)?;
    let rewritten = usage_event.render_with_cost(cost_usd)?;
    let range = usage_event.range;
    Some(Bytes::from(
        [&text[..range.start], &rewritten, &text[range.end..]].concat(),
    ))
}

/// One SSE event that carries a usage object.
struct UsageEvent<'a> {
    /// The event's bytes in the chunk, including the blank line that ends it.
    range: Range<usize>,
    /// The event's lines, without the blank line that ends it.
    lines: Vec<&'a str>,
    /// The index in `lines` of the first `data:` line.
    first_data_line: usize,
    /// The `data:` lines joined with `\n`, as an SSE client sees them.
    data: String,
    /// Where the usage object sits in `data`.
    usage: Range<usize>,
    newline: &'static str,
}

/// Finds the last complete event in `text` with a usage object. A trailing event without
/// its blank line is ignored.
fn last_usage_event(text: &str) -> Option<UsageEvent<'_>> {
    // Keep the upstream line ending; SSE allows LF or CRLF.
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let terminator = newline.repeat(2);
    let mut ranges = Vec::new();
    let mut start = 0;
    while let Some(offset) = text[start..].find(&terminator) {
        let end = start + offset + terminator.len();
        ranges.push(start..end);
        start = end;
    }
    ranges.into_iter().rev().find_map(|range| {
        let lines: Vec<&str> = text[range.start..range.end - terminator.len()]
            .split(newline)
            .collect();
        let first_data_line = lines.iter().position(|line| data_value(line).is_some())?;
        let data = lines
            .iter()
            .filter_map(|line| data_value(line))
            .collect::<Vec<_>>()
            .join("\n");
        let usage = usage_object(&data)?.get();
        let usage_start = (usage.as_ptr() as usize).checked_sub(data.as_ptr() as usize)?;
        let usage = usage_start..usage_start + usage.len();
        Some(UsageEvent {
            range,
            lines,
            first_data_line,
            data,
            usage,
            newline,
        })
    })
}

/// The value of a `data` field line, or `None` for any other line.
fn data_value(line: &str) -> Option<&str> {
    if line == "data" {
        return Some("");
    }
    let value = line.strip_prefix("data:")?;
    Some(value.strip_prefix(' ').unwrap_or(value))
}

impl UsageEvent<'_> {
    fn render_with_cost(&self, cost_usd: Money4) -> Option<String> {
        let Range { start, end } = self.usage.clone();
        let data = &self.data;
        let mut fields = serde_json::from_str::<Map<String, Value>>(&data[start..end]).ok()?;
        let cost = json!({ "cost": cost_usd.format_4dp() });
        // Append to the upstream text so the rest of the event passes through byte for
        // byte. An existing `oceans` field (from a chained gateway) is replaced instead.
        let usage = if fields.contains_key("oceans") {
            fields.insert("oceans".to_string(), cost);
            Value::Object(fields).to_string()
        } else {
            let separator = if fields.is_empty() { "" } else { "," };
            format!("{}{separator}\"oceans\":{cost}}}", &data[start..end - 1])
        };
        let payload = [&data[..start], &usage, &data[end..]].concat();

        // Other fields and comments are kept as sent. The rewritten data goes where the
        // first `data:` line was.
        let mut rendered = String::new();
        for (index, line) in self.lines.iter().enumerate() {
            if index == self.first_data_line {
                for data_line in payload.split('\n') {
                    rendered.push_str("data: ");
                    rendered.push_str(data_line);
                    rendered.push_str(self.newline);
                }
            } else if data_value(line).is_none() {
                rendered.push_str(line);
                rendered.push_str(self.newline);
            }
        }
        rendered.push_str(self.newline);
        Some(rendered)
    }
}

/// The fields that can hold a stream's usage object. Other fields are skipped without
/// being parsed into a tree, so large completed responses are not copied.
#[derive(Deserialize)]
struct UsageCarrier<'a> {
    #[serde(borrow, default)]
    usage: Option<&'a RawValue>,
    #[serde(borrow, default)]
    response: Option<&'a RawValue>,
    #[serde(borrow, default)]
    message: Option<&'a RawValue>,
}

/// Finds `usage`, `response.usage` or `message.usage`, in that order, as a slice of `json`.
fn usage_object(json: &str) -> Option<&RawValue> {
    let is_object = |raw: &&RawValue| raw.get().starts_with('{');
    let carrier = serde_json::from_str::<UsageCarrier>(json).ok()?;
    if let Some(usage) = carrier.usage.filter(is_object) {
        return Some(usage);
    }
    [carrier.response, carrier.message]
        .into_iter()
        .flatten()
        .filter(is_object)
        .find_map(|nested| {
            serde_json::from_str::<UsageCarrier>(nested.get())
                .ok()?
                .usage
                .filter(is_object)
        })
}

#[cfg(test)]
mod endpoint_tests;

#[cfg(test)]
mod tests {
    use axum::{Json, http::StatusCode, response::IntoResponse};
    use gateway_core::UsagePricingStatus;

    use super::*;

    fn route() -> ModelRoute {
        serde_json::from_value(json!({
            "id": "00000000-0000-0000-0000-000000000042",
            "model_id": "00000000-0000-0000-0000-000000000001",
            "provider_key": "vertex",
            "upstream_model": "gemini-2.5-flash",
            "priority": 0,
            "weight": 1.0,
            "enabled": true,
            "capabilities": {}
        }))
        .expect("route")
    }

    fn usage(pricing_status: UsagePricingStatus, cost: i64) -> RecordedChatUsage {
        RecordedChatUsage {
            pricing_status,
            unpriced_reason: None,
            prompt_tokens: None,
            uncached_input_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            cost_usd: None,
            computed_cost_usd: Money4::from_scaled(cost),
        }
    }

    fn header(response: &Response, name: HeaderName) -> Option<&str> {
        response
            .headers()
            .get(name)
            .map(|value| value.to_str().expect("ascii header"))
    }

    #[tokio::test]
    async fn success_carries_routing_and_cost_headers() {
        let response = with_response_headers(async {
            record_route("fast", &route());
            record_cost(&usage(UsagePricingStatus::Priced, 12));
            Json(json!({})).into_response()
        })
        .await;

        assert_eq!(header(&response, X_OCEANS_MODEL), Some("fast"));
        assert_eq!(
            header(&response, X_OCEANS_ROUTE_ID),
            Some("00000000-0000-0000-0000-000000000042")
        );
        assert_eq!(header(&response, X_OCEANS_PROVIDER), Some("vertex"));
        assert_eq!(header(&response, X_OCEANS_ATTEMPT_COUNT), Some("1"));
        assert_eq!(
            header(&response, X_OCEANS_RESPONSE_COST_USD),
            Some("0.0012")
        );
    }

    #[tokio::test]
    async fn unpriced_success_omits_cost() {
        let response = with_response_headers(async {
            record_route("fast", &route());
            record_cost(&usage(UsagePricingStatus::Unpriced, 12));
            Json(json!({})).into_response()
        })
        .await;

        assert_eq!(header(&response, X_OCEANS_ATTEMPT_COUNT), Some("1"));
        assert_eq!(header(&response, X_OCEANS_RESPONSE_COST_USD), None);
    }

    #[tokio::test]
    async fn error_carries_route_but_not_attempt_or_cost() {
        let response = with_response_headers(async {
            record_route("fast", &route());
            record_cost(&usage(UsagePricingStatus::Priced, 12));
            StatusCode::TOO_MANY_REQUESTS.into_response()
        })
        .await;

        assert_eq!(header(&response, X_OCEANS_PROVIDER), Some("vertex"));
        assert!(header(&response, X_OCEANS_ROUTE_ID).is_some());
        assert_eq!(header(&response, X_OCEANS_ATTEMPT_COUNT), None);
        assert_eq!(header(&response, X_OCEANS_RESPONSE_COST_USD), None);
    }

    #[tokio::test]
    async fn responses_without_a_route_get_no_headers() {
        let response =
            with_response_headers(async { StatusCode::BAD_REQUEST.into_response() }).await;

        assert!(response.headers().is_empty());
    }

    #[test]
    fn recording_outside_a_scope_is_a_no_op() {
        record_route("fast", &route());
        record_cost(&usage(UsagePricingStatus::Priced, 12));
    }

    #[test]
    fn stream_cost_is_added_to_each_usage_location() {
        let cost = Money4::from_scaled(12);
        for (chunk, pointer) in [
            (
                "data: {\"choices\":[],\"usage\":{\"total_tokens\":3}}\n\n",
                "/usage/oceans/cost",
            ),
            (
                "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"total_tokens\":3}}}\n\n",
                "/response/usage/oceans/cost",
            ),
            (
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":3}}\n\n",
                "/usage/oceans/cost",
            ),
        ] {
            let rewritten = with_stream_cost(chunk.as_bytes(), cost).expect("rewritten");
            let rewritten = std::str::from_utf8(&rewritten).expect("utf8");
            if let Some(event_line) = chunk
                .lines()
                .next()
                .filter(|line| line.starts_with("event:"))
            {
                assert!(rewritten.starts_with(&format!("{event_line}\n")));
            }
            let data = rewritten
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .expect("data line");
            let payload: Value = serde_json::from_str(data).expect("json");
            assert_eq!(payload.pointer(pointer), Some(&json!("0.0012")));
            assert!(rewritten.ends_with("\n\n"));
        }
    }

    #[tokio::test]
    async fn identifiers_outside_visible_ascii_are_percent_encoded() {
        let mut route = route();
        route.provider_key = "vertex-é%".to_string();
        let response = with_response_headers(async move {
            record_route("modèle rapide", &route);
            Json(json!({})).into_response()
        })
        .await;

        assert_eq!(
            header(&response, X_OCEANS_PROVIDER),
            Some("vertex-%C3%A9%25")
        );
        assert_eq!(
            header(&response, X_OCEANS_MODEL),
            Some("mod%C3%A8le rapide")
        );
    }

    #[test]
    fn stream_cost_leaves_the_rest_of_the_event_untouched() {
        let cost = Money4::from_scaled(12);
        for (chunk, expected) in [
            (
                "data: {\"z\":1,\"usage\":{\"total_tokens\":3},\"a\":[{\"b\": 2}]}\n\n",
                "data: {\"z\":1,\"usage\":{\"total_tokens\":3,\"oceans\":{\"cost\":\"0.0012\"}},\"a\":[{\"b\": 2}]}\n\n",
            ),
            (
                "data: {\"usage\":{}}\n\n",
                "data: {\"usage\":{\"oceans\":{\"cost\":\"0.0012\"}}}\n\n",
            ),
            (
                "event: message_delta\r\ndata: {\"usage\":{\"output_tokens\":3}}\r\n\r\n",
                "event: message_delta\r\ndata: {\"usage\":{\"output_tokens\":3,\"oceans\":{\"cost\":\"0.0012\"}}}\r\n\r\n",
            ),
            (
                "data: {\"usage\":{\"oceans\":{\"cost\":\"9.0000\"}}}\n\n",
                "data: {\"usage\":{\"oceans\":{\"cost\":\"0.0012\"}}}\n\n",
            ),
            // Comments and other fields are kept where they were.
            (
                ": keep-alive\nid: 7\nevent: message_delta\nretry: 1000\ndata: {\"usage\":{}}\n\n",
                ": keep-alive\nid: 7\nevent: message_delta\nretry: 1000\ndata: {\"usage\":{\"oceans\":{\"cost\":\"0.0012\"}}}\n\n",
            ),
            (
                "data: {\"usage\":\ndata: {}}\nid: 7\n\n",
                "data: {\"usage\":\ndata: {\"oceans\":{\"cost\":\"0.0012\"}}}\nid: 7\n\n",
            ),
        ] {
            let rewritten = with_stream_cost(chunk.as_bytes(), cost).expect("rewritten");
            assert_eq!(std::str::from_utf8(&rewritten).unwrap(), expected);
        }
    }

    #[test]
    fn stream_cost_skips_chunks_it_cannot_rewrite() {
        let cost = Money4::from_scaled(12);
        for chunk in [
            "data: [DONE]\n\n",
            "data: {\"choices\":[]}\n\n",
            ": keep-alive\n\n",
            "data: {\"usage\":{}}\n",
        ] {
            assert_eq!(with_stream_cost(chunk.as_bytes(), cost), None, "{chunk:?}");
        }
    }

    #[test]
    fn stream_cost_goes_on_the_last_usage_event_of_a_coalesced_chunk() {
        let cost = Money4::from_scaled(12);
        for (chunk, expected) in [
            (
                "data: {\"usage\":{\"total_tokens\":3}}\n\ndata: [DONE]\n\n",
                "data: {\"usage\":{\"total_tokens\":3,\"oceans\":{\"cost\":\"0.0012\"}}}\n\ndata: [DONE]\n\n",
            ),
            (
                "data: {\"usage\":{}}\r\n\r\ndata: {\"usage\":{}}\r\n\r\ndata: [DONE]\r\n\r\n",
                "data: {\"usage\":{}}\r\n\r\ndata: {\"usage\":{\"oceans\":{\"cost\":\"0.0012\"}}}\r\n\r\ndata: [DONE]\r\n\r\n",
            ),
        ] {
            let rewritten = with_stream_cost(chunk.as_bytes(), cost).expect("rewritten");
            assert_eq!(std::str::from_utf8(&rewritten).unwrap(), expected);
        }
    }

    #[test]
    fn split_before_usage_event_releases_the_output_ahead_of_it() {
        let chunk = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"choices\":[],\"usage\":{}}\n\n",
        );
        let (before, from_usage) = split_before_usage_event(&chunk).expect("split");
        assert_eq!(
            &before[..],
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n"
        );
        assert_eq!(&from_usage[..], b"data: {\"choices\":[],\"usage\":{}}\n\n");
        assert_eq!(
            split_before_usage_event(&Bytes::from_static(b"data: {\"choices\":[]}\n\n")),
            None
        );
    }
}
