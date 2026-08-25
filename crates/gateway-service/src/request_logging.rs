use std::{collections::BTreeMap, sync::Arc};

use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, ChatCompletionsRequest, EmbeddingsRequest,
    GatewayError, IdentityRepository, OpenAiErrorEnvelope, RequestAttemptRecord, RequestLogDetail,
    RequestLogPage, RequestLogPayloadRecord, RequestLogPurgeResult, RequestLogQuery,
    RequestLogRecord, RequestLogRepository, RequestLogRetentionWindow, RequestTags,
    RequestToolCardinality, ResponsesRequest,
};

use crate::{REQUEST_LOG_MODEL_ICON_KEY, REQUEST_LOG_PROVIDER_ICON_KEY, RequestLogIconMetadata};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::agent_analysis::{
    PassiveRequestMetadata, extract_request_metadata, serialized_request_prompt_bytes,
};
use crate::payload_bounding::{bound_request_payload, bound_request_payload_after_known_fields};

use crate::redaction::{
    RequestLogPayloadCaptureMode, RequestLogPayloadPolicy, redact_json_value_with_policy,
    sanitize_diagnostic_headers, truncate_large_payload_fields,
    truncate_large_payload_fields_with_count,
};

mod attempts;
mod harness;
mod stream;
mod tool_cardinality;

pub use attempts::{
    RequestAttemptOutcome, build_request_attempt, failed_attempt_outcome, offset_now,
    successful_attempt_outcome,
};
pub use harness::{AgentHarness, classify_agent_harness};
pub use stream::{StreamFailureSummary, StreamLogResultInput, StreamResponseCollector};
pub use tool_cardinality::invoked_tool_count_from_response_body;

use harness::{normalized_user_agent, request_user_agent};
use tool_cardinality::shallow_tool_count_from_request_body;

#[cfg(test)]
use attempts::truncate_attempt_error_detail;

#[derive(Debug, Clone)]
pub struct RequestLogContext {
    pub request_log_id: Uuid,
    pub request_id: String,
    pub requested_model_key: String,
    pub resolved_model_key: String,
    pub operation: &'static str,
    pub request_tags: RequestTags,
    pub user_agent_raw: Option<String>,
    pub agent_harness_key: String,
    pub agent_harness_label: String,
    payload_policy: RequestLogPayloadPolicy,
    pub tool_cardinality: RequestToolCardinality,
    request_json: Option<Value>,
    pub(crate) request_payload_truncated: bool,
    pub(crate) started_at: OffsetDateTime,
    pub(crate) analysis_metadata: PassiveRequestMetadata,
    pub(crate) analysis_payload_permitted: bool,
}

#[derive(Debug, Clone)]
pub struct LoggedRequest {
    pub request_log_id: Uuid,
    pub wrote: bool,
    pub response_payload_truncated: bool,
    pub analysis_response: Option<Value>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageSummary {
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

#[derive(Debug, Clone)]
struct RequestLogSummary {
    provider_key: String,
    icon_metadata: RequestLogIconMetadata,
    stream: bool,
    status_code: i64,
    error_code: Option<String>,
    latency_ms: i64,
    usage: UsageSummary,
    invoked_tool_count: i64,
}

impl RequestLogSummary {
    fn success(
        provider_key: String,
        icon_metadata: RequestLogIconMetadata,
        stream: bool,
        latency_ms: i64,
        usage: UsageSummary,
        invoked_tool_count: i64,
    ) -> Self {
        Self {
            provider_key,
            icon_metadata,
            stream,
            status_code: 200,
            error_code: None,
            latency_ms,
            usage,
            invoked_tool_count,
        }
    }

    fn failure(
        provider_key: String,
        icon_metadata: RequestLogIconMetadata,
        stream: bool,
        latency_ms: i64,
        status_code: i64,
        error_code: String,
    ) -> Self {
        Self {
            provider_key,
            icon_metadata,
            stream,
            status_code,
            error_code: Some(error_code),
            latency_ms,
            usage: UsageSummary::default(),
            invoked_tool_count: 0,
        }
    }
}

struct OperationRequestLogInput<'a, T> {
    operation: &'static str,
    request_id: &'a str,
    requested_model_key: &'a str,
    resolved_model_key: &'a str,
    request: &'a T,
    request_headers: &'a BTreeMap<String, String>,
    request_tags: RequestTags,
}

struct PreparedRequestPayload {
    analysis_metadata: PassiveRequestMetadata,
    request_json: Option<Value>,
    request_payload_truncated: bool,
    analysis_payload_permitted: bool,
}

impl UsageSummary {
    #[must_use]
    pub fn has_usage(self) -> bool {
        self.prompt_tokens.is_some()
            || self.completion_tokens.is_some()
            || self.total_tokens.is_some()
    }
}

#[derive(Clone)]
pub struct RequestLogging<R> {
    repo: Arc<R>,
    payload_policy: RequestLogPayloadPolicy,
}

impl<R> RequestLogging<R>
where
    R: IdentityRepository + RequestLogRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self::new_with_payload_policy(repo, RequestLogPayloadPolicy::default())
    }

    #[must_use]
    pub fn new_with_payload_policy(repo: Arc<R>, payload_policy: RequestLogPayloadPolicy) -> Self {
        Self {
            repo,
            payload_policy,
        }
    }

    #[must_use]
    pub fn begin_chat_request(
        &self,
        request_id: &str,
        requested_model_key: &str,
        resolved_model_key: &str,
        request: &ChatCompletionsRequest,
        request_headers: &BTreeMap<String, String>,
        request_tags: RequestTags,
    ) -> RequestLogContext {
        self.begin_operation_request(OperationRequestLogInput {
            operation: "chat_completions",
            request_id,
            requested_model_key,
            resolved_model_key,
            request,
            request_headers,
            request_tags,
        })
    }

    #[must_use]
    pub fn begin_responses_request(
        &self,
        request_id: &str,
        requested_model_key: &str,
        resolved_model_key: &str,
        request: &ResponsesRequest,
        request_headers: &BTreeMap<String, String>,
        request_tags: RequestTags,
    ) -> RequestLogContext {
        self.begin_operation_request(OperationRequestLogInput {
            operation: "responses",
            request_id,
            requested_model_key,
            resolved_model_key,
            request,
            request_headers,
            request_tags,
        })
    }

    #[must_use]
    pub fn begin_embeddings_request(
        &self,
        request_id: &str,
        requested_model_key: &str,
        resolved_model_key: &str,
        request: &EmbeddingsRequest,
        request_headers: &BTreeMap<String, String>,
        request_tags: RequestTags,
    ) -> RequestLogContext {
        self.begin_operation_request(OperationRequestLogInput {
            operation: "embeddings",
            request_id,
            requested_model_key,
            resolved_model_key,
            request,
            request_headers,
            request_tags,
        })
    }

    fn begin_operation_request<T>(
        &self,
        input: OperationRequestLogInput<'_, T>,
    ) -> RequestLogContext
    where
        T: serde::Serialize,
    {
        let user_agent_raw = normalized_user_agent(request_user_agent(input.request_headers));
        let harness = classify_agent_harness(user_agent_raw.as_deref());
        let request_body = serde_json::to_value(input.request).unwrap_or_else(|_| json!({}));
        let exposed_tool_count = shallow_tool_count_from_request_body(&request_body);
        let prepared =
            self.prepare_request_payload(request_body, input.request_headers, harness.key);

        RequestLogContext {
            request_log_id: Uuid::new_v4(),
            request_id: input.request_id.to_string(),
            requested_model_key: input.requested_model_key.to_string(),
            resolved_model_key: input.resolved_model_key.to_string(),
            operation: input.operation,
            request_tags: input.request_tags,
            user_agent_raw,
            agent_harness_key: harness.key.to_string(),
            agent_harness_label: harness.label.to_string(),
            payload_policy: self.payload_policy.clone(),
            tool_cardinality: RequestToolCardinality {
                referenced_mcp_server_count: None,
                exposed_tool_count,
                invoked_tool_count: Some(0),
                filtered_tool_count: None,
            },
            request_json: prepared.request_json,
            request_payload_truncated: prepared.request_payload_truncated,
            started_at: OffsetDateTime::now_utc(),
            analysis_metadata: prepared.analysis_metadata,
            analysis_payload_permitted: prepared.analysis_payload_permitted,
        }
    }

    fn prepare_request_payload(
        &self,
        request_body: Value,
        request_headers: &BTreeMap<String, String>,
        harness_key: &str,
    ) -> PreparedRequestPayload {
        let analysis_payload_permitted = self.payload_policy.should_capture_payloads();
        if !analysis_payload_permitted {
            return PreparedRequestPayload {
                analysis_metadata: extract_request_metadata(
                    &Value::Null,
                    request_headers,
                    false,
                    harness_key,
                ),
                request_json: None,
                request_payload_truncated: false,
                analysis_payload_permitted,
            };
        }

        let original_prompt_bytes = serialized_request_prompt_bytes(&request_body);
        let redacted = redact_json_value_with_policy(
            &json!({
                "headers": sanitize_diagnostic_headers(request_headers),
                "body": request_body,
            }),
            &self.payload_policy,
        );
        let analysis_headers = redacted
            .get("headers")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
            .collect::<BTreeMap<_, _>>();
        let analysis_body = redacted.get("body").unwrap_or(&Value::Null);
        let mut analysis_metadata =
            extract_request_metadata(analysis_body, &analysis_headers, true, harness_key);
        analysis_metadata.prompt_bytes = original_prompt_bytes;

        let (storage_request, large_field_count) =
            truncate_large_payload_fields_with_count(&redacted);
        let (request_json, request_payload_truncated) = if large_field_count == 0 {
            bound_request_payload(storage_request, self.payload_policy.request_max_bytes)
        } else {
            let original_storage_size = crate::payload_bounding::serialized_size(&redacted).ok();
            bound_request_payload_after_known_fields(
                storage_request,
                self.payload_policy.request_max_bytes,
                original_storage_size,
                large_field_count,
            )
        };
        PreparedRequestPayload {
            analysis_metadata,
            request_json: Some(request_json),
            request_payload_truncated,
            analysis_payload_permitted,
        }
    }

    pub async fn should_log_request(
        &self,
        api_key: &AuthenticatedApiKey,
    ) -> Result<bool, GatewayError> {
        match api_key.owner_kind {
            ApiKeyOwnerKind::ServiceAccount => Ok(true),
            ApiKeyOwnerKind::User => {
                let user_id = api_key.owner_user_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
                let user = self
                    .repo
                    .get_user_by_id(user_id)
                    .await?
                    .ok_or(AuthError::ApiKeyOwnerInvalid)?;
                Ok(user.request_logging_enabled)
            }
        }
    }

    #[must_use]
    pub fn new_stream_response_collector(&self) -> StreamResponseCollector {
        StreamResponseCollector::with_payload_policy(self.payload_policy.clone())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn log_non_stream_success(
        &self,
        api_key: &AuthenticatedApiKey,
        context: &RequestLogContext,
        provider_key: &str,
        icon_metadata: RequestLogIconMetadata,
        latency_ms: i64,
        invoked_tool_count: i64,
        response_body: &Value,
        attempts: Vec<RequestAttemptRecord>,
    ) -> Result<LoggedRequest, GatewayError> {
        let usage = usage_summary_from_value(response_body.get("usage"));
        let (response_json, response_payload_truncated) =
            if self.payload_policy.should_capture_payloads() {
                let sanitized_response = redact_json_value_with_policy(
                    &json!({ "body": response_body }),
                    &self.payload_policy,
                );
                let sanitized_response = truncate_large_payload_fields(&sanitized_response);
                let (response_json, truncated) =
                    truncate_payload(sanitized_response, self.payload_policy.response_max_bytes);
                (Some(response_json), truncated)
            } else {
                (None, false)
            };
        self.persist_chat_log(
            api_key,
            context,
            RequestLogSummary::success(
                provider_key.to_string(),
                icon_metadata,
                false,
                latency_ms,
                usage,
                invoked_tool_count,
            ),
            response_json,
            response_payload_truncated,
            attempts,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn log_non_stream_failure(
        &self,
        api_key: &AuthenticatedApiKey,
        context: &RequestLogContext,
        provider_key: &str,
        icon_metadata: RequestLogIconMetadata,
        latency_ms: i64,
        gateway_error: &GatewayError,
        attempts: Vec<RequestAttemptRecord>,
    ) -> Result<LoggedRequest, GatewayError> {
        let (response_json, response_payload_truncated) = if self
            .payload_policy
            .should_capture_payloads()
        {
            let response_json = redact_json_value_with_policy(
                &json!({
                    "body": serde_json::to_value(OpenAiErrorEnvelope::from_gateway_error(gateway_error))
                        .unwrap_or_else(|_| json!({ "error": gateway_error.to_string() })),
                }),
                &self.payload_policy,
            );
            let (response_json, truncated) =
                truncate_payload(response_json, self.payload_policy.response_max_bytes);
            (Some(response_json), truncated)
        } else {
            (None, false)
        };
        self.persist_chat_log(
            api_key,
            context,
            RequestLogSummary::failure(
                provider_key.to_string(),
                icon_metadata,
                false,
                latency_ms,
                gateway_error.http_status_code().into(),
                gateway_error.error_code().to_string(),
            ),
            response_json,
            response_payload_truncated,
            attempts,
        )
        .await
    }

    pub async fn log_stream_result(
        &self,
        api_key: &AuthenticatedApiKey,
        context: &RequestLogContext,
        stream_result: StreamLogResultInput,
    ) -> Result<LoggedRequest, GatewayError> {
        let StreamLogResultInput {
            provider_key,
            icon_metadata,
            latency_ms,
            mut collector,
            failure,
            attempts,
        } = stream_result;
        collector.finish();
        let failure = failure.or_else(|| collector.failure().cloned());
        let usage = usage_summary_from_value(collector.usage());
        let invoked_tool_count = collector.invoked_tool_count();
        let (response_json, response_payload_truncated) =
            if self.payload_policy.should_capture_payloads() {
                let (response_json, response_payload_truncated) =
                    collector.into_payload(failure.as_ref());
                (Some(response_json), response_payload_truncated)
            } else {
                (None, false)
            };
        let summary = match failure {
            Some(failure) => RequestLogSummary::failure(
                provider_key,
                icon_metadata.clone(),
                true,
                latency_ms,
                failure.status_code,
                failure.error_code,
            ),
            None => RequestLogSummary::success(
                provider_key,
                icon_metadata,
                true,
                latency_ms,
                usage,
                invoked_tool_count,
            ),
        };
        self.persist_chat_log(
            api_key,
            context,
            summary,
            response_json,
            response_payload_truncated,
            attempts,
        )
        .await
    }

    pub async fn list_request_logs(
        &self,
        query: &RequestLogQuery,
    ) -> Result<RequestLogPage, GatewayError> {
        self.repo.list_request_logs(query).await.map_err(Into::into)
    }

    pub async fn get_request_log_detail(
        &self,
        request_log_id: Uuid,
    ) -> Result<RequestLogDetail, GatewayError> {
        self.repo
            .get_request_log_detail(request_log_id)
            .await
            .map_err(Into::into)
    }

    pub async fn purge_request_logs(
        &self,
        retention_window: RequestLogRetentionWindow,
        dry_run: bool,
    ) -> Result<RequestLogPurgeResult, GatewayError> {
        let cutoff = retention_window.cutoff_at(OffsetDateTime::now_utc());
        self.repo
            .purge_request_logs_older_than(cutoff, dry_run)
            .await
            .map_err(Into::into)
    }

    async fn persist_chat_log(
        &self,
        api_key: &AuthenticatedApiKey,
        context: &RequestLogContext,
        summary: RequestLogSummary,
        response_json: Option<Value>,
        response_payload_truncated: bool,
        attempts: Vec<RequestAttemptRecord>,
    ) -> Result<LoggedRequest, GatewayError> {
        if self.payload_policy.capture_mode == RequestLogPayloadCaptureMode::Disabled
            || !self.should_log_request(api_key).await?
        {
            return Ok(LoggedRequest {
                request_log_id: context.request_log_id,
                wrote: false,
                response_payload_truncated: false,
                analysis_response: None,
            });
        }

        let metadata = request_log_metadata(
            context.operation,
            summary.stream,
            &summary.icon_metadata,
            &self.payload_policy,
        );
        let has_payload = self.payload_policy.should_capture_payloads()
            && context.request_json.is_some()
            && response_json.is_some();
        let log = RequestLogRecord {
            request_log_id: context.request_log_id,
            request_id: context.request_id.clone(),
            api_key_id: api_key.id,
            user_id: api_key.owner_user_id,
            team_id: api_key.owner_team_id,
            service_account_id: api_key.owner_service_account_id,
            model_key: context.requested_model_key.clone(),
            resolved_model_key: context.resolved_model_key.clone(),
            provider_key: summary.provider_key,
            status_code: Some(summary.status_code),
            latency_ms: Some(summary.latency_ms),
            prompt_tokens: summary.usage.prompt_tokens,
            completion_tokens: summary.usage.completion_tokens,
            total_tokens: summary.usage.total_tokens,
            error_code: summary.error_code,
            has_payload,
            request_payload_truncated: has_payload && context.request_payload_truncated,
            response_payload_truncated: has_payload && response_payload_truncated,
            request_tags: context.request_tags.clone(),
            tool_cardinality: RequestToolCardinality {
                invoked_tool_count: Some(summary.invoked_tool_count),
                ..context.tool_cardinality
            },
            user_agent_raw: context.user_agent_raw.clone(),
            agent_harness_key: context.agent_harness_key.clone(),
            agent_harness_label: context.agent_harness_label.clone(),
            metadata,
            occurred_at: OffsetDateTime::now_utc(),
        };
        let analysis_response = if has_payload && !response_payload_truncated {
            response_json
                .as_ref()
                .map(|value| value.get("body").cloned().unwrap_or_else(|| value.clone()))
        } else {
            None
        };
        let payload = match (has_payload, context.request_json.clone(), response_json) {
            (true, Some(request_json), Some(response_json)) => Some(RequestLogPayloadRecord {
                request_log_id: context.request_log_id,
                request_json,
                response_json,
            }),
            _ => None,
        };

        self.repo
            .insert_request_log_with_attempts(&log, payload.as_ref(), &attempts)
            .await?;

        Ok(LoggedRequest {
            request_log_id: context.request_log_id,
            wrote: true,
            response_payload_truncated: has_payload && response_payload_truncated,
            analysis_response,
        })
    }
}

pub fn usage_summary_from_value(value: Option<&Value>) -> UsageSummary {
    let Some(usage) = value.and_then(Value::as_object) else {
        return UsageSummary::default();
    };

    let prompt_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_i64);
    let completion_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_i64);
    let total_tokens = match usage.get("total_tokens").and_then(Value::as_i64) {
        some @ Some(_) => some,
        None => match (prompt_tokens, completion_tokens) {
            (Some(prompt), Some(completion)) => prompt.checked_add(completion),
            _ => None,
        },
    };

    UsageSummary {
        prompt_tokens,
        completion_tokens,
        total_tokens,
    }
}

fn request_log_metadata(
    operation: &'static str,
    stream: bool,
    icon_metadata: &RequestLogIconMetadata,
    payload_policy: &RequestLogPayloadPolicy,
) -> Map<String, Value> {
    let mut metadata = Map::new();
    metadata.insert(
        "operation".to_string(),
        Value::String(operation.to_string()),
    );
    metadata.insert("stream".to_string(), Value::Bool(stream));
    metadata.insert(
        "payload_policy".to_string(),
        payload_policy.metadata_value(),
    );
    metadata.insert(
        REQUEST_LOG_PROVIDER_ICON_KEY.to_string(),
        Value::String(icon_metadata.provider_icon_key.as_str().to_string()),
    );
    if let Some(model_icon_key) = icon_metadata.model_icon_key {
        metadata.insert(
            REQUEST_LOG_MODEL_ICON_KEY.to_string(),
            Value::String(model_icon_key.as_str().to_string()),
        );
    }
    metadata
}

fn truncate_payload(value: Value, max_bytes: usize) -> (Value, bool) {
    match serde_json::to_vec(&value) {
        Ok(bytes) if bytes.len() > max_bytes => (
            json!({
                "truncated": true,
                "size_bytes": bytes.len(),
                "preview": String::from_utf8_lossy(&bytes[..max_bytes.min(bytes.len())]).to_string(),
            }),
            true,
        ),
        Ok(_) => (value, false),
        Err(_) => (
            json!({
                "truncated": true,
                "error": "payload_serialization_failed",
            }),
            true,
        ),
    }
}

#[cfg(test)]
mod tests;
