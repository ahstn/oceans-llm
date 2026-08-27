use std::{fmt, future::Future, time::Duration};

use gateway_core::{AuthenticatedApiKey, ModelRoute, ProviderClient, ProviderError};
use gateway_service::ResolvedGatewayRequest;
use http::{Request, Response};
use opentelemetry::global;
use opentelemetry_http::HeaderExtractor;
use tower_http::trace::{OnFailure, OnResponse};
use tracing::{Instrument, Span};
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

pub(super) fn provider_operation_span(
    request_id: &str,
    operation: &'static str,
    auth: &AuthenticatedApiKey,
    resolved: &ResolvedGatewayRequest,
    route: &ModelRoute,
    provider: &dyn ProviderClient,
    stream: bool,
) -> Span {
    tracing::info_span!(
        "gateway.provider.operation",
        request_id = %request_id,
        gen_ai.operation.name = operation,
        gen_ai.request.model = %route.upstream_model,
        gen_ai.provider.name = %provider.provider_type(),
        gateway.requested_model = %resolved.selection.requested_model.model_key,
        gateway.resolved_model = %resolved.selection.execution_model.model_key,
        gateway.provider.key = %route.provider_key,
        gateway.request.stream = stream,
        gateway.ownership.kind = %auth.owner_kind.as_str(),
        error.type = tracing::field::Empty,
        otel.status_code = tracing::field::Empty,
    )
}

pub(super) async fn trace_provider_operation<F, T>(
    span: Span,
    operation: F,
) -> Result<T, ProviderError>
where
    F: Future<Output = Result<T, ProviderError>>,
{
    let result = operation.instrument(span.clone()).await;
    if let Err(error) = &result {
        span.record("error.type", provider_error_type(error));
        span.record("otel.status_code", "ERROR");
    }
    result
}

fn provider_error_type(error: &ProviderError) -> &'static str {
    match error {
        ProviderError::InvalidRequest(_) => "invalid_request",
        ProviderError::UpstreamHttp { .. } => "upstream_http",
        ProviderError::Timeout => "timeout",
        ProviderError::Transport(_) => "transport",
        ProviderError::PartialUsage { .. } => "partial_usage",
        ProviderError::NotImplemented(_) => "not_implemented",
    }
}
