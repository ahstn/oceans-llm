use gateway_core::{GatewayError, ModelRoute, RequestAttemptRecord, RequestAttemptStatus};
use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::redaction::{
    RequestLogPayloadPolicy, redact_json_value_with_policy, truncate_large_payload_fields,
};

use super::RequestLogContext;

const MAX_ATTEMPT_ERROR_DETAIL_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone)]
pub struct RequestAttemptOutcome {
    pub status: RequestAttemptStatus,
    pub status_code: Option<i64>,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub retryable: bool,
    pub produced_final_response: bool,
}

#[must_use]
pub fn successful_attempt_outcome() -> RequestAttemptOutcome {
    RequestAttemptOutcome {
        status: RequestAttemptStatus::Success,
        status_code: Some(200),
        error_code: None,
        error_detail: None,
        retryable: false,
        produced_final_response: true,
    }
}

#[must_use]
pub fn failed_attempt_outcome(
    status: RequestAttemptStatus,
    gateway_error: &GatewayError,
    retryable: bool,
    detail: impl Into<String>,
) -> RequestAttemptOutcome {
    RequestAttemptOutcome {
        status,
        status_code: Some(gateway_error.http_status_code().into()),
        error_code: Some(gateway_error.error_code().to_string()),
        error_detail: Some(detail.into()),
        retryable,
        produced_final_response: false,
    }
}

#[must_use]
pub fn build_request_attempt(
    context: &RequestLogContext,
    route: &ModelRoute,
    attempt_number: i64,
    stream: bool,
    started_at: OffsetDateTime,
    completed_at: OffsetDateTime,
    outcome: RequestAttemptOutcome,
) -> RequestAttemptRecord {
    let (error_detail, error_detail_truncated) = outcome
        .error_detail
        .as_deref()
        .map(|detail| truncate_attempt_error_detail(detail, &context.payload_policy))
        .map(|(detail, truncated)| (Some(detail), truncated))
        .unwrap_or((None, false));
    RequestAttemptRecord {
        request_attempt_id: Uuid::new_v4(),
        request_log_id: context.request_log_id,
        request_id: context.request_id.clone(),
        attempt_number,
        route_id: route.id,
        provider_key: route.provider_key.clone(),
        upstream_model: route.upstream_model.clone(),
        status: outcome.status,
        status_code: outcome.status_code,
        error_code: outcome.error_code,
        error_detail,
        error_detail_truncated,
        retryable: outcome.retryable,
        terminal: true,
        produced_final_response: outcome.produced_final_response,
        stream,
        started_at,
        completed_at: Some(completed_at),
        latency_ms: Some(
            (completed_at - started_at)
                .whole_milliseconds()
                .try_into()
                .unwrap_or(i64::MAX),
        ),
        metadata: Map::new(),
    }
}

#[must_use]
pub fn offset_now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

pub(super) fn truncate_attempt_error_detail(
    detail: &str,
    payload_policy: &RequestLogPayloadPolicy,
) -> (String, bool) {
    let sanitized = sanitize_attempt_error_detail(detail, payload_policy);
    if sanitized.len() <= MAX_ATTEMPT_ERROR_DETAIL_BYTES {
        return (sanitized, false);
    }
    (
        String::from_utf8_lossy(&sanitized.as_bytes()[..MAX_ATTEMPT_ERROR_DETAIL_BYTES])
            .to_string(),
        true,
    )
}

fn sanitize_attempt_error_detail(detail: &str, payload_policy: &RequestLogPayloadPolicy) -> String {
    if !payload_policy.should_capture_payloads() {
        return format!("[redacted error detail; {} bytes]", detail.len());
    }

    match serde_json::from_str::<Value>(detail) {
        Ok(parsed @ (Value::Object(_) | Value::Array(_))) => {
            let redacted = redact_json_value_with_policy(&parsed, payload_policy);
            serde_json::to_string(&truncate_large_payload_fields(&redacted)).unwrap_or_else(|_| {
                format!("[redacted structured error detail; {} bytes]", detail.len())
            })
        }
        Ok(_) | Err(_) => format!("[redacted error detail; {} bytes]", detail.len()),
    }
}
