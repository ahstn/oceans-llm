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

use std::{cell::RefCell, future::Future};

use axum::{
    body::Bytes,
    extract::Request,
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use gateway_core::{ModelRoute, Money4};
use gateway_service::RecordedChatUsage;
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) const X_OCEANS_MODEL: HeaderName = HeaderName::from_static("x-oceans-model");
pub(crate) const X_OCEANS_ROUTE_ID: HeaderName = HeaderName::from_static("x-oceans-route-id");
pub(crate) const X_OCEANS_PROVIDER: HeaderName = HeaderName::from_static("x-oceans-provider");
pub(crate) const X_OCEANS_ATTEMPT_COUNT: HeaderName =
    HeaderName::from_static("x-oceans-attempt-count");
pub(crate) const X_OCEANS_RESPONSE_COST_USD: HeaderName =
    HeaderName::from_static("x-oceans-response-cost-usd");

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
        insert(X_OCEANS_PROVIDER, route.provider_key);
        insert(X_OCEANS_MODEL, route.model_key);
        if success {
            insert(X_OCEANS_ATTEMPT_COUNT, self.attempt_count.to_string());
            if let Some(cost_usd) = self.cost_usd {
                insert(X_OCEANS_RESPONSE_COST_USD, cost_usd.format_4dp());
            }
        }
    }
}

/// Adds `usage.oceans.cost` to a chunk that holds exactly one SSE event with a JSON
/// usage payload. Returns `None` when the chunk has any other shape, so the caller
/// forwards it unchanged.
pub(crate) fn with_stream_cost(chunk: &[u8], cost_usd: Money4) -> Option<Bytes> {
    let text = std::str::from_utf8(chunk).ok()?;
    let mut event = None;
    let mut data_lines = Vec::new();
    for line in text.strip_suffix("\n\n")?.split('\n') {
        if let Some(name) = line.strip_prefix("event:") {
            if event.replace(name.trim()).is_some() {
                return None;
            }
        } else {
            let data = line.strip_prefix("data:")?;
            data_lines.push(data.strip_prefix(' ').unwrap_or(data));
        }
    }
    let mut payload = serde_json::from_str::<Value>(&data_lines.join("\n")).ok()?;
    let usage = if payload.get("usage").is_some_and(Value::is_object) {
        payload.get_mut("usage")
    } else if payload
        .pointer("/response/usage")
        .is_some_and(Value::is_object)
    {
        payload.pointer_mut("/response/usage")
    } else {
        payload.pointer_mut("/message/usage")
    }?;
    usage.as_object_mut()?.insert(
        "oceans".to_string(),
        json!({ "cost": cost_usd.format_4dp() }),
    );

    let mut rendered = String::new();
    if let Some(event) = event {
        rendered.push_str("event: ");
        rendered.push_str(event);
        rendered.push('\n');
    }
    rendered.push_str("data: ");
    rendered.push_str(&payload.to_string());
    rendered.push_str("\n\n");
    Some(Bytes::from(rendered))
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

    #[test]
    fn stream_cost_skips_chunks_it_cannot_rewrite() {
        let cost = Money4::from_scaled(12);
        for chunk in [
            "data: [DONE]\n\n",
            "data: {\"choices\":[]}\n\n",
            "data: {\"usage\":{}}\n\ndata: {\"usage\":{}}\n\n",
            ": keep-alive\ndata: {\"usage\":{}}\n\n",
            "data: {\"usage\":{}}\n",
        ] {
            assert_eq!(with_stream_cost(chunk.as_bytes(), cost), None, "{chunk:?}");
        }
    }
}
