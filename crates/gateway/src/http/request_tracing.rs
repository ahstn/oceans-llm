use std::{future::Future, time::Duration};

use gateway_core::{AuthenticatedApiKey, ModelRoute, ProviderClient, ProviderError};
use gateway_service::{ResolvedGatewayRequest, StreamChunkObservation};
use http::{Request, Response};
use opentelemetry::global;
use opentelemetry_http::HeaderExtractor;
use tower_http::{
    classify::ServerErrorsFailureClass,
    trace::{OnFailure, OnResponse},
};
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
        gateway.error.type = tracing::field::Empty,
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

impl OnFailure<ServerErrorsFailureClass> for RecordHttpFailure {
    fn on_failure(
        &mut self,
        failure_classification: ServerErrorsFailureClass,
        _latency: Duration,
        span: &Span,
    ) {
        span.record(
            "error.type",
            server_failure_error_type(&failure_classification),
        );
        span.record("otel.status_code", "ERROR");
    }
}

fn server_failure_error_type(failure: &ServerErrorsFailureClass) -> String {
    match failure {
        ServerErrorsFailureClass::StatusCode(status) => status.as_u16().to_string(),
        ServerErrorsFailureClass::Error(_) => "request_failure".to_string(),
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

pub(super) struct StreamTrace {
    span: Option<Span>,
    started_at: std::time::Instant,
    first_chunk_seen: bool,
    first_output_seen: bool,
    usage_seen: bool,
    terminal_event_seen: bool,
    chunk_count: u64,
    byte_count: u64,
    finished: bool,
}

impl StreamTrace {
    pub fn new(
        operation: &'static str,
        request_id: &str,
        route: &ModelRoute,
        provider: &dyn ProviderClient,
        started_at: std::time::Instant,
    ) -> Self {
        Self {
            span: Some(tracing::info_span!(
                "gateway.provider.stream",
                request_id = %request_id,
                gen_ai.operation.name = operation,
                gen_ai.request.model = %route.upstream_model,
                gen_ai.provider.name = %provider.provider_type(),
                gateway.provider.key = %route.provider_key,
                gateway.stream.time_to_first_chunk_ms = tracing::field::Empty,
                gateway.stream.time_to_first_output_ms = tracing::field::Empty,
                gateway.stream.duration_ms = tracing::field::Empty,
                gateway.stream.chunk_count = tracing::field::Empty,
                gateway.stream.byte_count = tracing::field::Empty,
                gateway.stream.terminal_event_seen = tracing::field::Empty,
                gateway.stream.termination_reason = tracing::field::Empty,
                error.type = tracing::field::Empty,
                otel.status_code = tracing::field::Empty,
            )),
            started_at,
            first_chunk_seen: false,
            first_output_seen: false,
            usage_seen: false,
            terminal_event_seen: false,
            chunk_count: 0,
            byte_count: 0,
            finished: false,
        }
    }

    pub fn observe_chunk(&mut self, byte_count: usize, observation: StreamChunkObservation) {
        let Some(span) = self.span.as_ref() else {
            return;
        };
        self.chunk_count = self.chunk_count.saturating_add(1);
        self.byte_count = self.byte_count.saturating_add(byte_count as u64);

        if !self.first_chunk_seen {
            self.first_chunk_seen = true;
            let elapsed_ms = self.started_at.elapsed().as_secs_f64() * 1_000.0;
            span.record("gateway.stream.time_to_first_chunk_ms", elapsed_ms);
            span.in_scope(|| {
                tracing::info!(elapsed_ms, "provider stream received first chunk");
            });
        }
        if observation.has_output && !self.first_output_seen {
            self.first_output_seen = true;
            let elapsed_ms = self.started_at.elapsed().as_secs_f64() * 1_000.0;
            span.record("gateway.stream.time_to_first_output_ms", elapsed_ms);
            span.in_scope(|| {
                tracing::info!(elapsed_ms, "provider stream received first semantic output");
            });
        }
        if observation.has_usage && !self.usage_seen {
            self.usage_seen = true;
            span.in_scope(|| {
                tracing::info!("provider stream received usage");
            });
        }
        if observation.has_terminal_event {
            self.terminal_event_seen = true;
        }
    }

    pub fn finish(&mut self, termination_reason: &'static str, error_type: Option<&'static str>) {
        if self.finished {
            return;
        }
        self.finished = true;
        let Some(span) = self.span.take() else {
            return;
        };
        span.record(
            "gateway.stream.duration_ms",
            self.started_at.elapsed().as_secs_f64() * 1_000.0,
        );
        span.record("gateway.stream.chunk_count", self.chunk_count);
        span.record("gateway.stream.byte_count", self.byte_count);
        span.record(
            "gateway.stream.terminal_event_seen",
            self.terminal_event_seen,
        );
        span.record("gateway.stream.termination_reason", termination_reason);
        if let Some(error_type) = error_type {
            span.record("error.type", error_type);
            span.record("otel.status_code", "ERROR");
        }
        span.in_scope(|| {
            tracing::info!(termination_reason, "provider stream terminated");
        });
    }
}

impl Drop for StreamTrace {
    fn drop(&mut self) {
        if !self.finished {
            self.finish("client_cancelled", Some("client_cancelled"));
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;

    #[test]
    fn server_failure_error_type_does_not_export_error_details() {
        assert_eq!(
            server_failure_error_type(&ServerErrorsFailureClass::StatusCode(
                StatusCode::SERVICE_UNAVAILABLE,
            )),
            "503"
        );
        assert_eq!(
            server_failure_error_type(&ServerErrorsFailureClass::Error(
                "secret upstream detail".to_string(),
            )),
            "request_failure"
        );
    }
}
