use std::{fmt, time::Duration};

use http::{Request, Response};
use opentelemetry::global;
use opentelemetry_http::HeaderExtractor;
use tower_http::trace::{OnFailure, OnResponse};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub(super) fn make_http_request_span<B>(request: &Request<B>) -> Span {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing");
    let method = request.method().as_str();

    let span = tracing::info_span!(
        "http.server.request",
        otel.name = %method,
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
        http.request.method = %method,
        http.response.status_code = tracing::field::Empty,
        url.path = %request.uri().path(),
        error.type = tracing::field::Empty,
        method = %method,
        uri = %request.uri().path(),
        request_id = %request_id,
        http.route = tracing::field::Empty,
        requested_model = tracing::field::Empty,
        resolved_model = tracing::field::Empty,
        provider = tracing::field::Empty,
        stream = tracing::field::Empty,
        ownership_kind = tracing::field::Empty,
        gen_ai.request.model = tracing::field::Empty,
        gen_ai.response.model = tracing::field::Empty,
        gen_ai.provider.name = tracing::field::Empty,
        gateway.provider.key = tracing::field::Empty,
    );
    let parent_context = global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(request.headers()))
    });
    let _ = span.set_parent(parent_context);

    span
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RecordHttpResponse;

impl<B> OnResponse<B> for RecordHttpResponse {
    fn on_response(self, response: &Response<B>, _latency: Duration, span: &Span) {
        let status = response.status();
        span.record("http.response.status_code", status.as_u16());
        if status.is_server_error() {
            span.record("error.type", status.as_u16().to_string());
            span.record("otel.status_code", "ERROR");
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RecordHttpFailure;

impl<FailureClass> OnFailure<FailureClass> for RecordHttpFailure
where
    FailureClass: fmt::Display,
{
    fn on_failure(
        &mut self,
        failure_classification: FailureClass,
        _latency: Duration,
        span: &Span,
    ) {
        span.record("error.type", failure_classification.to_string());
        span.record("otel.status_code", "ERROR");
    }
}
