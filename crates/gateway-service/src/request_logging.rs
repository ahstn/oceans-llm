use std::{collections::BTreeMap, sync::Arc};

use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, ChatCompletionsRequest, EmbeddingsRequest,
    GatewayError, IdentityRepository, ModelRoute, OpenAiErrorEnvelope, RequestAttemptRecord,
    RequestAttemptStatus, RequestLogDetail, RequestLogPage, RequestLogPayloadRecord,
    RequestLogQuery, RequestLogRecord, RequestLogRepository, RequestTags, RequestToolCardinality,
    ResponsesRequest, SseEventParser,
};

use crate::{REQUEST_LOG_MODEL_ICON_KEY, REQUEST_LOG_PROVIDER_ICON_KEY, RequestLogIconMetadata};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::redaction::{
    RequestLogPayloadCaptureMode, RequestLogPayloadPolicy, redact_header_value,
    redact_json_value_with_policy, truncate_large_payload_fields,
};

#[derive(Debug, Clone)]
pub struct RequestLogContext {
    pub request_log_id: Uuid,
    pub request_id: String,
    pub requested_model_key: String,
    pub resolved_model_key: String,
    pub operation: &'static str,
    pub request_tags: RequestTags,
    payload_policy: RequestLogPayloadPolicy,
    pub tool_cardinality: RequestToolCardinality,
    request_json: Option<Value>,
    request_payload_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamFailureSummary {
    pub status_code: i64,
    pub error_code: String,
}

#[derive(Debug, Clone)]
pub struct StreamLogResultInput {
    pub provider_key: String,
    pub icon_metadata: RequestLogIconMetadata,
    pub latency_ms: i64,
    pub collector: StreamResponseCollector,
    pub failure: Option<StreamFailureSummary>,
    pub attempts: Vec<RequestAttemptRecord>,
}

#[derive(Debug, Clone, Default)]
pub struct StreamResponseCollector {
    parser: SseEventParser,
    payload_policy: RequestLogPayloadPolicy,
    events: Vec<Value>,
    usage: Option<Value>,
    failure: Option<StreamFailureSummary>,
    seen_tool_call_ids: HashSet<String>,
    anonymous_tool_call_count: i64,
    finished: bool,
    truncated: bool,
}

#[derive(Debug, Clone)]
pub struct LoggedRequest {
    pub request_log_id: Uuid,
    pub wrote: bool,
}

#[derive(Debug, Clone, Copy, Default)]
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

impl UsageSummary {
    #[must_use]
    pub fn has_usage(self) -> bool {
        self.prompt_tokens.is_some()
            || self.completion_tokens.is_some()
            || self.total_tokens.is_some()
    }
}

impl StreamResponseCollector {
    pub fn observe_chunk(&mut self, chunk: &[u8]) {
        if self.finished {
            return;
        }

        let events = match self.parser.push_bytes(chunk) {
            Ok(events) => events,
            Err(_) => {
                self.truncated = true;
                self.failure.get_or_insert_with(|| StreamFailureSummary {
                    status_code: 502,
                    error_code: "stream_parse_error".to_string(),
                });
                return;
            }
        };

        for event in events {
            let payload = event.data.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }

            let parsed = serde_json::from_str::<Value>(payload).ok();
            if let Some(usage) = parsed
                .as_ref()
                .and_then(usage_value_from_stream_event)
                .filter(|usage| !usage.is_null())
            {
                self.usage = Some(usage.clone());
            }
            if let Some(failure) = parsed.as_ref().and_then(stream_failure_from_value) {
                self.failure = Some(failure);
            }
            if let Some(parsed) = parsed.as_ref() {
                self.observe_tool_calls(parsed);
            }

            if self.events.len() >= self.payload_policy.stream_max_events {
                self.truncated = true;
                continue;
            }

            self.events
                .push(parsed.unwrap_or_else(|| json!({ "raw": payload })));
        }
    }

    pub fn finish(&mut self) {
        if self.finished {
            return;
        }

        self.finished = true;
        if self.parser.finish().is_err() {
            self.truncated = true;
            self.failure.get_or_insert_with(|| StreamFailureSummary {
                status_code: 502,
                error_code: "stream_parse_error".to_string(),
            });
        }
    }

    #[must_use]
    pub fn usage(&self) -> Option<&Value> {
        self.usage.as_ref()
    }

    #[must_use]
    pub fn failure(&self) -> Option<&StreamFailureSummary> {
        self.failure.as_ref()
    }

    #[must_use]
    pub fn invoked_tool_count(&self) -> i64 {
        i64::try_from(self.seen_tool_call_ids.len())
            .unwrap_or(i64::MAX)
            .saturating_add(self.anonymous_tool_call_count)
    }

    fn observe_tool_calls(&mut self, value: &Value) {
        for identity in tool_call_identities_from_value(value) {
            match identity {
                ToolCallIdentity::Known(id) => {
                    self.seen_tool_call_ids.insert(id);
                }
                ToolCallIdentity::Anonymous => {
                    self.anonymous_tool_call_count =
                        self.anonymous_tool_call_count.saturating_add(1);
                }
            }
        }
    }

    fn into_payload(self, failure: Option<&StreamFailureSummary>) -> (Value, bool) {
        let payload = redact_json_value_with_policy(
            &json!({
                "stream": true,
                "events": self.events,
                "usage": self.usage,
                "error": failure.map(|failure| {
                    json!({
                        "status_code": failure.status_code,
                        "code": failure.error_code,
                    })
                }),
            }),
            &self.payload_policy,
        );
        truncate_payload(
            truncate_large_payload_fields(&payload),
            self.payload_policy.response_max_bytes,
        )
        .map_truncated(self.truncated)
    }
}

fn stream_failure_from_value(value: &Value) -> Option<StreamFailureSummary> {
    let error = value.get("error")?.as_object()?;
    Some(StreamFailureSummary {
        status_code: 502,
        error_code: error
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("stream_error")
            .to_string(),
    })
}

fn usage_value_from_stream_event(value: &Value) -> Option<&Value> {
    value.get("usage").or_else(|| {
        value
            .get("response")
            .and_then(|response| response.get("usage"))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolCallIdentity {
    Known(String),
    Anonymous,
}

fn shallow_tool_count_from_request_body(value: &Value) -> Option<i64> {
    let tools = value.get("tools").or_else(|| {
        value
            .get("request")
            .and_then(|request| request.get("tools"))
    });

    match tools {
        None | Some(Value::Null) => Some(0),
        Some(Value::Array(items)) => Some(i64::try_from(items.len()).unwrap_or(i64::MAX)),
        Some(_) => Some(0),
    }
}

#[must_use]
pub fn invoked_tool_count_from_response_body(value: &Value) -> i64 {
    let identities = tool_call_identities_from_value(value);
    let mut known_ids = HashSet::new();
    let mut anonymous = 0_i64;
    for identity in identities {
        match identity {
            ToolCallIdentity::Known(id) => {
                known_ids.insert(id);
            }
            ToolCallIdentity::Anonymous => {
                anonymous = anonymous.saturating_add(1);
            }
        }
    }
    i64::try_from(known_ids.len())
        .unwrap_or(i64::MAX)
        .saturating_add(anonymous)
}

fn tool_call_identities_from_value(value: &Value) -> Vec<ToolCallIdentity> {
    let mut identities = Vec::new();
    collect_chat_tool_call_identities(value, &mut identities);
    collect_responses_tool_call_identities(value, &mut identities);
    identities
}

fn collect_chat_tool_call_identities(value: &Value, identities: &mut Vec<ToolCallIdentity>) {
    let Some(choices) = value.get("choices").and_then(Value::as_array) else {
        return;
    };
    for choice in choices {
        if let Some(message) = choice.get("message")
            && let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array)
        {
            collect_tool_call_array_identities(tool_calls, identities, true);
        }
        for delta in [
            choice.get("delta"),
            choice.get("chunk").and_then(|chunk| chunk.get("delta")),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                collect_tool_call_array_identities(tool_calls, identities, false);
            }
        }
    }
}

fn collect_responses_tool_call_identities(value: &Value, identities: &mut Vec<ToolCallIdentity>) {
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        for item in output {
            collect_tool_call_item_identity(item, identities, true);
        }
    }

    for key in ["item", "output_item"] {
        if let Some(item) = value.get(key) {
            collect_tool_call_item_identity(item, identities, true);
        }
    }

    if let Some(delta) = value.get("delta") {
        collect_tool_call_item_identity(delta, identities, false);
    }
}

fn collect_tool_call_array_identities(
    items: &[Value],
    identities: &mut Vec<ToolCallIdentity>,
    allow_anonymous: bool,
) {
    for item in items {
        push_tool_call_identity(item, identities, allow_anonymous);
    }
}

fn collect_tool_call_item_identity(
    item: &Value,
    identities: &mut Vec<ToolCallIdentity>,
    allow_anonymous: bool,
) {
    let object = match item.as_object() {
        Some(object) => object,
        None => return,
    };

    if let Some(tool_calls) = object.get("tool_calls").and_then(Value::as_array) {
        collect_tool_call_array_identities(tool_calls, identities, allow_anonymous);
        return;
    }

    let item_type = object
        .get("type")
        .or_else(|| object.get("item_type"))
        .and_then(Value::as_str);
    let is_tool_call = item_type.is_some_and(|item_type| {
        item_type == "function_call" || item_type == "tool_call" || item_type.contains("tool_call")
    }) || object.contains_key("function");

    if !is_tool_call {
        return;
    }

    push_tool_call_identity(item, identities, allow_anonymous);
}

fn push_tool_call_identity(
    item: &Value,
    identities: &mut Vec<ToolCallIdentity>,
    allow_anonymous: bool,
) {
    let Some(object) = item.as_object() else {
        return;
    };

    let id = object
        .get("id")
        .or_else(|| object.get("call_id"))
        .or_else(|| object.get("tool_call_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    identities.push(match id {
        Some(id) => ToolCallIdentity::Known(id.to_string()),
        None if allow_anonymous => ToolCallIdentity::Anonymous,
        None => return,
    });
}

trait PayloadResultExt {
    fn map_truncated(self, additional_truncated: bool) -> (Value, bool);
}

impl PayloadResultExt for (Value, bool) {
    fn map_truncated(self, additional_truncated: bool) -> (Value, bool) {
        (self.0, self.1 || additional_truncated)
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
        let request_body = serde_json::to_value(input.request).unwrap_or_else(|_| json!({}));
        let exposed_tool_count = shallow_tool_count_from_request_body(&request_body);
        let (request_json, request_payload_truncated) = if self
            .payload_policy
            .should_capture_payloads()
        {
            let sanitized_headers = input
                .request_headers
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(redact_header_value(key, value))))
                .collect::<Map<_, _>>();
            let redacted = redact_json_value_with_policy(
                &json!({
                    "headers": sanitized_headers,
                    "body": request_body,
                }),
                &self.payload_policy,
            );
            let redacted = truncate_large_payload_fields(&redacted);
            let (request_json, truncated) =
                truncate_payload(redacted, self.payload_policy.request_max_bytes);
            (Some(request_json), truncated)
        } else {
            (None, false)
        };

        RequestLogContext {
            request_log_id: Uuid::new_v4(),
            request_id: input.request_id.to_string(),
            requested_model_key: input.requested_model_key.to_string(),
            resolved_model_key: input.resolved_model_key.to_string(),
            operation: input.operation,
            request_tags: input.request_tags,
            payload_policy: self.payload_policy.clone(),
            tool_cardinality: RequestToolCardinality {
                referenced_mcp_server_count: None,
                exposed_tool_count,
                invoked_tool_count: Some(0),
                filtered_tool_count: None,
            },
            request_json,
            request_payload_truncated,
        }
    }

    pub async fn should_log_request(
        &self,
        api_key: &AuthenticatedApiKey,
    ) -> Result<bool, GatewayError> {
        match api_key.owner_kind {
            ApiKeyOwnerKind::Team => Ok(true),
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
        StreamResponseCollector {
            payload_policy: self.payload_policy.clone(),
            ..StreamResponseCollector::default()
        }
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
            metadata,
            occurred_at: OffsetDateTime::now_utc(),
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
        })
    }
}

#[must_use]
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

fn truncate_attempt_error_detail(
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
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyOwnerKind, AuthMode, AuthenticatedApiKey, ChatCompletionsRequest, GlobalRole,
        IdentityRepository, ModelAccessMode, RequestLogDetail, RequestLogPage,
        RequestLogPayloadRecord, RequestLogQuery, RequestLogRecord, RequestLogRepository,
        RequestTag, RequestTags, StoreError, TeamMembershipRecord, TeamRecord, UserRecord,
        UserStatus,
    };
    use serde_json::{Value, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::{
        RequestLogIconMetadata,
        redaction::{RequestLogPayloadCaptureMode, RequestLogPayloadPolicy, parse_payload_path},
    };

    use super::{
        RequestLogging, StreamFailureSummary, StreamLogResultInput, StreamResponseCollector,
        invoked_tool_count_from_response_body, shallow_tool_count_from_request_body,
        truncate_attempt_error_detail,
    };

    #[derive(Clone, Default)]
    struct InMemoryRepo {
        users: Arc<Mutex<Vec<UserRecord>>>,
        logs: Arc<Mutex<Vec<RequestLogRecord>>>,
        payloads: Arc<Mutex<Vec<RequestLogPayloadRecord>>>,
    }

    #[async_trait]
    impl IdentityRepository for InMemoryRepo {
        async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
            Ok(self
                .users
                .lock()
                .expect("users lock")
                .iter()
                .find(|user| user.user_id == user_id)
                .cloned())
        }

        async fn get_team_by_id(&self, _team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
            Ok(None)
        }

        async fn get_team_membership_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Option<TeamMembershipRecord>, StoreError> {
            Ok(None)
        }

        async fn list_allowed_model_keys_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }

        async fn list_allowed_model_keys_for_team(
            &self,
            _team_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl RequestLogRepository for InMemoryRepo {
        async fn insert_request_log(
            &self,
            log: &RequestLogRecord,
            payload: Option<&RequestLogPayloadRecord>,
        ) -> Result<(), StoreError> {
            self.logs.lock().expect("logs lock").push(log.clone());
            if let Some(payload) = payload {
                self.payloads
                    .lock()
                    .expect("payloads lock")
                    .push(payload.clone());
            }
            Ok(())
        }

        async fn list_request_logs(
            &self,
            _query: &RequestLogQuery,
        ) -> Result<RequestLogPage, StoreError> {
            Ok(RequestLogPage {
                items: self.logs.lock().expect("logs lock").clone(),
                page: 1,
                page_size: 50,
                total: self.logs.lock().expect("logs lock").len() as u64,
            })
        }

        async fn get_request_log_detail(
            &self,
            request_log_id: Uuid,
        ) -> Result<RequestLogDetail, StoreError> {
            let logs = self.logs.lock().expect("logs lock");
            let Some(log) = logs
                .iter()
                .find(|log| log.request_log_id == request_log_id)
                .cloned()
            else {
                return Err(StoreError::NotFound(format!(
                    "request log `{request_log_id}` not found"
                )));
            };
            let payload = self
                .payloads
                .lock()
                .expect("payloads lock")
                .iter()
                .find(|payload| payload.request_log_id == request_log_id)
                .cloned();
            Ok(RequestLogDetail {
                log,
                payload,
                attempts: Vec::new(),
            })
        }
    }

    fn user_record(user_id: Uuid, request_logging_enabled: bool) -> UserRecord {
        UserRecord {
            user_id,
            name: "test".to_string(),
            email: "user@example.com".to_string(),
            email_normalized: "user@example.com".to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            status: UserStatus::Active,
            must_change_password: false,
            request_logging_enabled,
            model_access_mode: ModelAccessMode::All,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        }
    }

    fn sample_auth(user_id: Uuid) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user_id),
            owner_team_id: None,
        }
    }

    fn sample_team_auth() -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_user_id: None,
            owner_team_id: Some(Uuid::new_v4()),
        }
    }

    fn sample_icon_metadata() -> RequestLogIconMetadata {
        RequestLogIconMetadata {
            provider_icon_key: crate::ProviderIconKey::OpenAI,
            model_icon_key: Some(crate::ModelIconKey::OpenAI),
        }
    }

    fn sample_request(stream: bool) -> ChatCompletionsRequest {
        ChatCompletionsRequest {
            model: "fast".to_string(),
            messages: Vec::new(),
            stream,
            extra: BTreeMap::new(),
        }
    }

    fn policy(
        capture_mode: RequestLogPayloadCaptureMode,
        request_max_bytes: usize,
        response_max_bytes: usize,
        stream_max_events: usize,
    ) -> RequestLogPayloadPolicy {
        RequestLogPayloadPolicy::new(
            capture_mode,
            request_max_bytes,
            response_max_bytes,
            stream_max_events,
            Vec::new(),
        )
    }

    fn policy_with_redaction_paths(paths: &[&str]) -> RequestLogPayloadPolicy {
        RequestLogPayloadPolicy::new(
            RequestLogPayloadCaptureMode::RedactedPayloads,
            4096,
            4096,
            4,
            paths
                .iter()
                .map(|path| parse_payload_path(path).expect("test path should parse"))
                .collect(),
        )
    }

    #[test]
    fn attempt_error_detail_redacts_structured_payloads_with_active_policy() {
        let policy = policy_with_redaction_paths(&["message"]);
        let (detail, truncated) = truncate_attempt_error_detail(
            r#"{"message":"secret prompt","api_key":"sk-test"}"#,
            &policy,
        );

        assert!(!truncated);
        assert!(!detail.contains("secret prompt"));
        assert!(!detail.contains("sk-test"));
        assert!(detail.contains("[REDACTED]"));
    }

    #[test]
    fn attempt_error_detail_suppresses_raw_text_when_payload_capture_is_disabled() {
        let policy = policy(RequestLogPayloadCaptureMode::SummaryOnly, 4096, 4096, 4);
        let (detail, truncated) =
            truncate_attempt_error_detail("provider leaked token sk-test", &policy);

        assert!(!truncated);
        assert!(!detail.contains("sk-test"));
        assert!(detail.starts_with("[redacted error detail; "));
        assert!(detail.ends_with(" bytes]"));
    }

    #[tokio::test]
    async fn suppresses_logging_for_user_toggle_disabled() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryRepo {
            users: Arc::new(Mutex::new(vec![user_record(user_id, false)])),
            logs: Arc::new(Mutex::new(Vec::new())),
            payloads: Arc::new(Mutex::new(Vec::new())),
        });
        let logging = RequestLogging::new(repo.clone());
        let auth = sample_auth(user_id);
        let context = logging.begin_chat_request(
            "req_1",
            "fast",
            "fast",
            &ChatCompletionsRequest {
                model: "fast".to_string(),
                messages: Vec::new(),
                stream: false,
                extra: BTreeMap::new(),
            },
            &BTreeMap::new(),
            RequestTags::default(),
        );

        let wrote = logging
            .log_non_stream_success(
                &auth,
                &context,
                "openai-prod",
                RequestLogIconMetadata {
                    provider_icon_key: crate::ProviderIconKey::OpenAI,
                    model_icon_key: Some(crate::ModelIconKey::OpenAI),
                },
                120,
                0,
                &json!({"usage": {"prompt_tokens": 1, "completion_tokens": 2}}),
                Vec::new(),
            )
            .await
            .expect("request logging should evaluate");

        assert!(!wrote.wrote);
        assert_eq!(repo.logs.lock().expect("logs lock").len(), 0);
    }

    #[tokio::test]
    async fn logs_team_owned_requests_with_payload_and_redaction() {
        let team_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryRepo::default());
        let logging = RequestLogging::new(repo.clone());
        let auth = AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_user_id: None,
            owner_team_id: Some(team_id),
        };
        let mut headers = BTreeMap::new();
        headers.insert("authorization".to_string(), "secret".to_string());
        let context = logging.begin_chat_request(
            "req_1",
            "fast",
            "fast",
            &ChatCompletionsRequest {
                model: "fast".to_string(),
                messages: Vec::new(),
                stream: false,
                extra: BTreeMap::from([("token".to_string(), Value::String("secret".to_string()))]),
            },
            &headers,
            RequestTags {
                service: Some("checkout".to_string()),
                component: Some("pricing_api".to_string()),
                env: Some("prod".to_string()),
                bespoke: vec![RequestTag {
                    key: "feature".to_string(),
                    value: "guest_checkout".to_string(),
                }],
            },
        );

        let wrote = logging
            .log_non_stream_success(
                &auth,
                &context,
                "openai-prod",
                RequestLogIconMetadata {
                    provider_icon_key: crate::ProviderIconKey::OpenAI,
                    model_icon_key: Some(crate::ModelIconKey::OpenAI),
                },
                120,
                0,
                &json!({"usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}}),
                Vec::new(),
            )
            .await
            .expect("request logging should evaluate");

        let logs = repo.logs.lock().expect("logs lock");
        let payloads = repo.payloads.lock().expect("payloads lock");
        assert!(wrote.wrote);
        assert_eq!(logs.len(), 1);
        assert!(logs[0].user_id.is_none());
        assert_eq!(logs[0].team_id, Some(team_id));
        assert!(logs[0].has_payload);
        assert_eq!(
            payloads[0].request_json["headers"]["authorization"],
            "[REDACTED]"
        );
        assert_eq!(payloads[0].request_json["body"]["token"], "[REDACTED]");
        assert_eq!(logs[0].request_tags.service.as_deref(), Some("checkout"));
        assert_eq!(logs[0].request_tags.bespoke[0].key, "feature");
        assert_eq!(
            logs[0].metadata["operation"],
            Value::String("chat_completions".to_string())
        );
        assert_eq!(logs[0].metadata["stream"], Value::Bool(false));
        assert!(logs[0].metadata.get("fallback_used").is_none());
        assert!(logs[0].metadata.get("attempt_count").is_none());
    }

    #[tokio::test]
    async fn disabled_payload_policy_writes_no_request_log_rows() {
        let repo = Arc::new(InMemoryRepo::default());
        let logging = RequestLogging::new_with_payload_policy(
            repo.clone(),
            policy(RequestLogPayloadCaptureMode::Disabled, 1024, 1024, 4),
        );
        let auth = sample_team_auth();
        let context = logging.begin_chat_request(
            "req_1",
            "fast",
            "fast",
            &sample_request(false),
            &BTreeMap::new(),
            RequestTags::default(),
        );

        let wrote = logging
            .log_non_stream_success(
                &auth,
                &context,
                "openai-prod",
                sample_icon_metadata(),
                120,
                0,
                &json!({"usage": {"prompt_tokens": 1, "completion_tokens": 2}}),
                Vec::new(),
            )
            .await
            .expect("request logging should evaluate");

        assert!(!wrote.wrote);
        assert!(repo.logs.lock().expect("logs lock").is_empty());
        assert!(repo.payloads.lock().expect("payloads lock").is_empty());
    }

    #[tokio::test]
    async fn summary_only_payload_policy_writes_summary_without_payload() {
        let repo = Arc::new(InMemoryRepo::default());
        let logging = RequestLogging::new_with_payload_policy(
            repo.clone(),
            policy(RequestLogPayloadCaptureMode::SummaryOnly, 1024, 1024, 4),
        );
        let auth = sample_team_auth();
        let context = logging.begin_chat_request(
            "req_1",
            "fast",
            "fast",
            &sample_request(false),
            &BTreeMap::new(),
            RequestTags::default(),
        );

        let wrote = logging
            .log_non_stream_success(
                &auth,
                &context,
                "openai-prod",
                sample_icon_metadata(),
                120,
                0,
                &json!({"usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}}),
                Vec::new(),
            )
            .await
            .expect("summary log");

        let logs = repo.logs.lock().expect("logs lock");
        assert!(wrote.wrote);
        assert_eq!(logs.len(), 1);
        assert!(!logs[0].has_payload);
        assert!(!logs[0].request_payload_truncated);
        assert!(!logs[0].response_payload_truncated);
        assert_eq!(
            logs[0].metadata["payload_policy"]["capture_mode"],
            "summary_only"
        );
        assert!(repo.payloads.lock().expect("payloads lock").is_empty());
    }

    #[tokio::test]
    async fn separate_payload_limits_mark_only_affected_side_truncated() {
        let repo = Arc::new(InMemoryRepo::default());
        let logging = RequestLogging::new_with_payload_policy(
            repo.clone(),
            policy(RequestLogPayloadCaptureMode::RedactedPayloads, 4096, 80, 4),
        );
        let auth = sample_team_auth();
        let context = logging.begin_chat_request(
            "req_1",
            "fast",
            "fast",
            &sample_request(false),
            &BTreeMap::new(),
            RequestTags::default(),
        );

        logging
            .log_non_stream_success(
                &auth,
                &context,
                "openai-prod",
                sample_icon_metadata(),
                120,
                0,
                &json!({
                    "choices": [{"message": {"content": "x".repeat(512)}}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
                }),
                Vec::new(),
            )
            .await
            .expect("truncated log");

        let logs = repo.logs.lock().expect("logs lock");
        let payloads = repo.payloads.lock().expect("payloads lock");
        assert!(logs[0].has_payload);
        assert!(!logs[0].request_payload_truncated);
        assert!(logs[0].response_payload_truncated);
        assert_eq!(payloads[0].response_json["truncated"], true);
    }

    #[tokio::test]
    async fn operator_redaction_paths_apply_to_wrapped_response_payloads() {
        let repo = Arc::new(InMemoryRepo::default());
        let logging = RequestLogging::new_with_payload_policy(
            repo.clone(),
            policy_with_redaction_paths(&["body.choices.*.message.content"]),
        );
        let auth = sample_team_auth();
        let context = logging.begin_chat_request(
            "req_1",
            "fast",
            "fast",
            &sample_request(false),
            &BTreeMap::new(),
            RequestTags::default(),
        );

        logging
            .log_non_stream_success(
                &auth,
                &context,
                "openai-prod",
                sample_icon_metadata(),
                120,
                0,
                &json!({
                    "choices": [{"message": {"content": "operator-secret"}}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
                }),
                Vec::new(),
            )
            .await
            .expect("redacted response log");

        let payloads = repo.payloads.lock().expect("payloads lock");
        assert_eq!(
            payloads[0].response_json["body"]["choices"][0]["message"]["content"],
            "[REDACTED]"
        );
    }

    #[tokio::test]
    async fn records_stream_failures_with_payload() {
        let repo = Arc::new(InMemoryRepo::default());
        let logging = RequestLogging::new(repo.clone());
        let auth = AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_user_id: None,
            owner_team_id: Some(Uuid::new_v4()),
        };
        let context = logging.begin_chat_request(
            "req_1",
            "fast",
            "fast",
            &ChatCompletionsRequest {
                model: "fast".to_string(),
                messages: Vec::new(),
                stream: true,
                extra: BTreeMap::new(),
            },
            &BTreeMap::new(),
            RequestTags::default(),
        );
        let mut collector = logging.new_stream_response_collector();
        collector.observe_chunk(
            br#"data: {"delta":"hello"}

"#,
        );

        let wrote = logging
            .log_stream_result(
                &auth,
                &context,
                StreamLogResultInput {
                    provider_key: "openai-prod".to_string(),
                    icon_metadata: RequestLogIconMetadata {
                        provider_icon_key: crate::ProviderIconKey::OpenAI,
                        model_icon_key: Some(crate::ModelIconKey::OpenAI),
                    },
                    latency_ms: 120,
                    collector,
                    failure: Some(StreamFailureSummary {
                        status_code: 502,
                        error_code: "stream_error".to_string(),
                    }),
                    attempts: Vec::new(),
                },
            )
            .await
            .expect("stream failure log");

        assert!(wrote.wrote);
        let logs = repo.logs.lock().expect("logs lock");
        let payload = repo.payloads.lock().expect("payloads lock");
        assert_eq!(
            logs[0].metadata["operation"],
            Value::String("chat_completions".to_string())
        );
        assert_eq!(logs[0].metadata["stream"], Value::Bool(true));
        assert!(logs[0].metadata.get("fallback_used").is_none());
        assert!(logs[0].metadata.get("attempt_count").is_none());
        assert_eq!(payload[0].response_json["error"]["code"], "stream_error");
    }

    #[tokio::test]
    async fn stream_event_storage_cap_does_not_stop_usage_parsing() {
        let repo = Arc::new(InMemoryRepo::default());
        let logging = RequestLogging::new_with_payload_policy(
            repo.clone(),
            policy(
                RequestLogPayloadCaptureMode::RedactedPayloads,
                4096,
                4096,
                1,
            ),
        );
        let auth = sample_team_auth();
        let context = logging.begin_chat_request(
            "req_1",
            "fast",
            "fast",
            &sample_request(true),
            &BTreeMap::new(),
            RequestTags::default(),
        );
        let mut collector = logging.new_stream_response_collector();
        collector.observe_chunk(
            br#"data: {"choices":[{"delta":{"content":"hello"}}]}

data: {"usage":{"prompt_tokens":4,"completion_tokens":5,"total_tokens":9}}

"#,
        );

        logging
            .log_stream_result(
                &auth,
                &context,
                StreamLogResultInput {
                    provider_key: "openai-prod".to_string(),
                    icon_metadata: sample_icon_metadata(),
                    latency_ms: 120,
                    collector,
                    failure: None,
                    attempts: Vec::new(),
                },
            )
            .await
            .expect("stream log");

        let logs = repo.logs.lock().expect("logs lock");
        let payload = repo.payloads.lock().expect("payloads lock");
        assert_eq!(logs[0].total_tokens, Some(9));
        assert!(logs[0].response_payload_truncated);
        assert_eq!(
            payload[0].response_json["events"].as_array().unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn operator_redaction_paths_apply_to_wrapped_stream_payloads() {
        let repo = Arc::new(InMemoryRepo::default());
        let logging = RequestLogging::new_with_payload_policy(
            repo.clone(),
            policy_with_redaction_paths(&["events.*.choices.*.delta.content"]),
        );
        let auth = sample_team_auth();
        let context = logging.begin_chat_request(
            "req_1",
            "fast",
            "fast",
            &sample_request(true),
            &BTreeMap::new(),
            RequestTags::default(),
        );
        let mut collector = logging.new_stream_response_collector();
        collector.observe_chunk(
            br#"data: {"choices":[{"delta":{"content":"operator-secret"}}]}

"#,
        );

        logging
            .log_stream_result(
                &auth,
                &context,
                StreamLogResultInput {
                    provider_key: "openai-prod".to_string(),
                    icon_metadata: sample_icon_metadata(),
                    latency_ms: 120,
                    collector,
                    failure: None,
                    attempts: Vec::new(),
                },
            )
            .await
            .expect("stream log");

        let payloads = repo.payloads.lock().expect("payloads lock");
        assert_eq!(
            payloads[0].response_json["events"][0]["choices"][0]["delta"]["content"],
            "[REDACTED]"
        );
    }

    #[test]
    fn collector_reassembles_split_frames_and_keeps_latest_usage() {
        let mut collector = StreamResponseCollector::default();

        collector.observe_chunk("data: {\"usage\":{\"prompt_tokens\":1".as_bytes());
        collector.observe_chunk(
            ",\"completion_tokens\":2,\"total_tokens\":3}}\n\ndata:{\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":5,\"total_tokens\":9}}\n\n"
                .as_bytes(),
        );
        collector.finish();

        assert_eq!(
            collector.usage(),
            Some(&json!({
                "prompt_tokens": 4,
                "completion_tokens": 5,
                "total_tokens": 9
            }))
        );
    }

    #[test]
    fn collector_reassembles_split_utf8_and_error_frames() {
        let mut collector = StreamResponseCollector::default();

        collector.observe_chunk(b"data: {\"delta\":\"");
        collector.observe_chunk(&[0xF0, 0x9F]);
        collector.observe_chunk(&[
            0x99, 0x82, b'"', b'}', b'\n', b'\n', b'd', b'a', b't', b'a', b':', b'{', b'"', b'e',
            b'r', b'r', b'o', b'r', b'"', b':', b'{', b'"', b'c', b'o', b'd', b'e', b'"', b':',
            b'"', b'u', b'p', b's', b't', b'r', b'e', b'a', b'm', b'_', b'b', b'a', b'd', b'"',
            b'}', b'}',
        ]);
        collector.observe_chunk(b"\n\n");
        collector.finish();

        assert_eq!(
            collector.failure(),
            Some(&StreamFailureSummary {
                status_code: 502,
                error_code: "upstream_bad".to_string(),
            })
        );
    }

    #[test]
    fn collector_accepts_data_prefix_without_space() {
        let mut collector = StreamResponseCollector::default();

        collector.observe_chunk(b"data:{\"value\":1}\n\n");
        collector.finish();

        let (payload, truncated) = collector.into_payload(None);
        assert!(!truncated);
        assert_eq!(payload["events"][0]["value"], 1);
    }

    #[test]
    fn shallow_tool_count_reads_chat_and_responses_shapes() {
        assert_eq!(shallow_tool_count_from_request_body(&json!({})), Some(0));
        assert_eq!(
            shallow_tool_count_from_request_body(&json!({
                "tools": [{"type": "function"}]
            })),
            Some(1)
        );
        assert_eq!(
            shallow_tool_count_from_request_body(&json!({
                "request": {
                    "tools": [
                        {"type": "function"},
                        {"type": "web_search_preview"}
                    ]
                }
            })),
            Some(2)
        );
        assert_eq!(
            shallow_tool_count_from_request_body(&json!({ "tools": "malformed" })),
            Some(0)
        );
        assert_eq!(
            shallow_tool_count_from_request_body(&json!({
                "request": { "tools": {"not": "array"} }
            })),
            Some(0)
        );
    }

    #[test]
    fn invoked_tool_count_reads_non_stream_chat_and_responses_artifacts() {
        assert_eq!(
            invoked_tool_count_from_response_body(&json!({
                "choices": [{
                    "message": {
                        "tool_calls": [
                            {"id": "call_1", "type": "function"},
                            {"id": "call_2", "type": "function"}
                        ]
                    }
                }]
            })),
            2
        );
        assert_eq!(
            invoked_tool_count_from_response_body(&json!({
                "output": [
                    {"id": "call_1", "type": "function_call"},
                    {"call_id": "call_2", "type": "function_call"}
                ]
            })),
            2
        );
        assert_eq!(
            invoked_tool_count_from_response_body(&json!({
                "choices": [{
                    "message": {
                        "tool_calls": [
                            {"id": "call_1", "type": "function"},
                            {"id": "call_1", "type": "function"}
                        ]
                    }
                }]
            })),
            1
        );
        assert_eq!(
            invoked_tool_count_from_response_body(&json!({
                "output": [{
                    "type": "message",
                    "tool_calls": [
                        {"id": "call_1", "type": "function"},
                        {"id": "call_2", "type": "function"}
                    ]
                }]
            })),
            2
        );
    }

    #[test]
    fn stream_collector_counts_invoked_tools_from_sse_events() {
        let mut collector = StreamResponseCollector::default();

        collector.observe_chunk(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call_1","type":"function"}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"id":"call_1","type":"function"}]}}]}

data: {"output":[{"id":"call_2","type":"function_call"}]}

"#,
        );
        collector.finish();

        assert_eq!(collector.invoked_tool_count(), 2);
    }

    #[test]
    fn stream_collector_ignores_chat_tool_call_delta_fragments_without_ids() {
        let mut collector = StreamResponseCollector::default();

        collector.observe_chunk(
            br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function"}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"city\""}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":":\"London\"}"}}]}}]}

"#,
        );
        collector.finish();

        assert_eq!(collector.invoked_tool_count(), 1);
    }
}
