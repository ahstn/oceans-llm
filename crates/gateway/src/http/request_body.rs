use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
};

use axum::{
    Json,
    body::{Body, Bytes, HttpBody},
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use gateway_core::{GatewayError, OpenAiErrorBody, OpenAiErrorEnvelope};
use http::StatusCode;
use http_body::{Frame, SizeHint};
use serde::Serialize;

pub(super) const DEFAULT_MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Serialize)]
struct BodyLimitResponse {
    error: BodyLimitDetails,
}

#[derive(Serialize)]
struct BodyLimitDetails {
    #[serde(flatten)]
    error: OpenAiErrorBody,
    limit_bytes: usize,
    received_bytes: u64,
    request_id: String,
}

// Count frames without buffering a second copy or changing streaming/trailer semantics.
struct ObservedBody {
    inner: Body,
    received: Arc<AtomicU64>,
}

impl HttpBody for ObservedBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let frame = Pin::new(&mut self.inner).poll_frame(cx);
        if let Poll::Ready(Some(Ok(frame))) = &frame
            && let Some(data) = frame.data_ref()
        {
            self.received
                .fetch_add(data.len() as u64, Ordering::Relaxed);
        }
        frame
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

pub(super) async fn observe_request_body(request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing")
        .to_owned();
    let declared_bytes = request
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let received = Arc::new(AtomicU64::new(0));
    let (parts, body) = request.into_parts();
    let body = Body::new(ObservedBody {
        inner: body,
        received: received.clone(),
    });
    let response = next.run(Request::from_parts(parts, body)).await;
    let received_bytes = received.load(Ordering::Relaxed);
    let span = tracing::Span::current();
    span.record("gateway.request.body.received_bytes", received_bytes);
    span.record("gateway.request.body.limit_bytes", DEFAULT_MAX_BYTES as u64);
    if let Some(bytes) = declared_bytes {
        span.record("gateway.request.body.declared_bytes", bytes);
    }

    // A provider 413 must retain its own envelope and limit. This branch only
    // handles a locally consumed body that crossed our extractor limit.
    if response.status() != StatusCode::PAYLOAD_TOO_LARGE
        || received_bytes <= DEFAULT_MAX_BYTES as u64
    {
        return response;
    }
    let error = GatewayError::PayloadTooLarge {
        limit_bytes: DEFAULT_MAX_BYTES,
    };
    span.record("gateway.error.type", error.error_type());
    span.record("error.type", error.error_code());
    span.record("otel.status_code", "ERROR");
    tracing::warn!(%request_id, received_bytes, declared_bytes,
        limit_bytes = DEFAULT_MAX_BYTES, "request body exceeded gateway limit");
    let mut error_body = OpenAiErrorEnvelope::from_gateway_error(&error).error;
    error_body.message.push_str(
        ". Reduce conversation history, tool output, or attachment size before retrying.",
    );
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(BodyLimitResponse {
            error: BodyLimitDetails {
                error: error_body,
                limit_bytes: DEFAULT_MAX_BYTES,
                received_bytes,
                request_id,
            },
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests;
