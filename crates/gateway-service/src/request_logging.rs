use std::{collections::BTreeMap, sync::Arc};

use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, ChatCompletionsRequest, GatewayError,
    IdentityRepository, OpenAiErrorEnvelope, RequestLogDetail, RequestLogPage,
    RequestLogPayloadRecord, RequestLogQuery, RequestLogRecord, RequestLogRepository,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::redaction::{redact_header_value, redact_json_value};

const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_STREAM_EVENTS: usize = 128;

#[derive(Debug, Clone)]
pub struct ChatRequestLogContext {
    pub request_log_id: Uuid,
    pub request_id: String,
    pub requested_model_key: String,
    pub resolved_model_key: String,
    request_json: Value,
    request_payload_truncated: bool,
}

#[derive(Debug, Clone)]
pub struct StreamFailureSummary {
    pub status_code: i64,
    pub error_code: String,
}

#[derive(Debug, Clone, Default)]
pub struct StreamResponseCollector {
    events: Vec<Value>,
    usage: Option<Value>,
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
struct ChatCompletionLogSummary {
    provider_key: String,
    attempt_count: usize,
    stream: bool,
    status_code: i64,
    error_code: Option<String>,
    latency_ms: i64,
    usage: UsageSummary,
}

impl ChatCompletionLogSummary {
    fn success(
        provider_key: String,
        attempt_count: usize,
        stream: bool,
        latency_ms: i64,
        usage: UsageSummary,
    ) -> Self {
        Self {
            provider_key,
            attempt_count,
            stream,
            status_code: 200,
            error_code: None,
            latency_ms,
            usage,
        }
    }

    fn failure(
        provider_key: String,
        attempt_count: usize,
        stream: bool,
        latency_ms: i64,
        status_code: i64,
        error_code: String,
    ) -> Self {
        Self {
            provider_key,
            attempt_count,
            stream,
            status_code,
            error_code: Some(error_code),
            latency_ms,
            usage: UsageSummary::default(),
        }
    }
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
        let Ok(text) = std::str::from_utf8(chunk) else {
            self.truncated = true;
            return;
        };

        for line in text.lines() {
            let Some(payload) = line.strip_prefix("data: ") else {
                continue;
            };
            let payload = payload.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }

            let event = match serde_json::from_str::<Value>(payload) {
                Ok(value) => redact_json_value(&value),
                Err(_) => json!({ "raw": payload }),
            };

            if self.usage.is_none() {
                self.usage = event.get("usage").cloned();
            }

            if self.events.len() >= MAX_STREAM_EVENTS {
                self.truncated = true;
                continue;
            }

            self.events.push(event);
        }
    }

    #[must_use]
    pub fn usage(&self) -> Option<&Value> {
        self.usage.as_ref()
    }

    fn into_payload(self, failure: Option<&StreamFailureSummary>) -> (Value, bool) {
        truncate_payload(json!({
            "stream": true,
            "events": self.events,
            "usage": self.usage,
            "error": failure.map(|failure| {
                json!({
                    "status_code": failure.status_code,
                    "code": failure.error_code,
                })
            }),
        }))
        .map_truncated(self.truncated)
    }
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
}

impl<R> RequestLogging<R>
where
    R: IdentityRepository + RequestLogRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    #[must_use]
    pub fn begin_chat_request(
        &self,
        request_id: &str,
        requested_model_key: &str,
        resolved_model_key: &str,
        request: &ChatCompletionsRequest,
        request_headers: &BTreeMap<String, String>,
    ) -> ChatRequestLogContext {
        let sanitized_headers = request_headers
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(redact_header_value(key, value))))
            .collect::<Map<_, _>>();
        let request_body =
            redact_json_value(&serde_json::to_value(request).unwrap_or_else(|_| json!({})));
        let (request_json, request_payload_truncated) = truncate_payload(json!({
            "headers": sanitized_headers,
            "body": request_body,
        }));

        ChatRequestLogContext {
            request_log_id: Uuid::new_v4(),
            request_id: request_id.to_string(),
            requested_model_key: requested_model_key.to_string(),
            resolved_model_key: resolved_model_key.to_string(),
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
        StreamResponseCollector::default()
    }

    pub async fn log_non_stream_success(
        &self,
        api_key: &AuthenticatedApiKey,
        context: &ChatRequestLogContext,
        provider_key: &str,
        attempt_count: usize,
        latency_ms: i64,
        response_body: &Value,
    ) -> Result<LoggedRequest, GatewayError> {
        let sanitized_response = redact_json_value(response_body);
        let usage = usage_summary_from_value(sanitized_response.get("usage"));
        let (response_json, response_payload_truncated) =
            truncate_payload(json!({ "body": sanitized_response }));
        self.persist_chat_log(
            api_key,
            context,
            ChatCompletionLogSummary::success(
                provider_key.to_string(),
                attempt_count,
                false,
                latency_ms,
                usage,
            ),
            response_json,
            response_payload_truncated,
        )
        .await
    }

    pub async fn log_non_stream_failure(
        &self,
        api_key: &AuthenticatedApiKey,
        context: &ChatRequestLogContext,
        provider_key: &str,
        attempt_count: usize,
        latency_ms: i64,
        gateway_error: &GatewayError,
    ) -> Result<LoggedRequest, GatewayError> {
        let response_json = json!({
            "body": redact_json_value(
                &serde_json::to_value(OpenAiErrorEnvelope::from_gateway_error(gateway_error))
                    .unwrap_or_else(|_| json!({ "error": gateway_error.to_string() })),
            ),
        });
        let (response_json, response_payload_truncated) = truncate_payload(response_json);
        self.persist_chat_log(
            api_key,
            context,
            ChatCompletionLogSummary::failure(
                provider_key.to_string(),
                attempt_count,
                false,
                latency_ms,
                gateway_error.http_status_code().into(),
                gateway_error.error_code().to_string(),
            ),
            response_json,
            response_payload_truncated,
        )
        .await
    }

    pub async fn log_stream_result(
        &self,
        api_key: &AuthenticatedApiKey,
        context: &ChatRequestLogContext,
        provider_key: &str,
        attempt_count: usize,
        latency_ms: i64,
        collector: StreamResponseCollector,
        failure: Option<StreamFailureSummary>,
    ) -> Result<LoggedRequest, GatewayError> {
        let usage = usage_summary_from_value(collector.usage());
        let (response_json, response_payload_truncated) = collector.into_payload(failure.as_ref());
        let summary = match failure {
            Some(failure) => ChatCompletionLogSummary::failure(
                provider_key.to_string(),
                attempt_count,
                true,
                latency_ms,
                failure.status_code,
                failure.error_code,
            ),
            None => ChatCompletionLogSummary::success(
                provider_key.to_string(),
                attempt_count,
                true,
                latency_ms,
                usage,
            ),
        };
        self.persist_chat_log(
            api_key,
            context,
            summary,
            response_json,
            response_payload_truncated,
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
    ) -> Result<Option<RequestLogDetail>, GatewayError> {
        self.repo
            .get_request_log_detail(request_log_id)
            .await
            .map_err(Into::into)
    }

    async fn persist_chat_log(
        &self,
        api_key: &AuthenticatedApiKey,
        context: &ChatRequestLogContext,
        summary: ChatCompletionLogSummary,
        response_json: Value,
        response_payload_truncated: bool,
    ) -> Result<LoggedRequest, GatewayError> {
        if !self.should_log_request(api_key).await? {
            return Ok(LoggedRequest {
                request_log_id: context.request_log_id,
                wrote: false,
            });
        }

        let metadata = request_log_metadata(summary.attempt_count, summary.stream);
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
            has_payload: true,
            request_payload_truncated: context.request_payload_truncated,
            response_payload_truncated,
            metadata,
            occurred_at: OffsetDateTime::now_utc(),
        };
        let payload = RequestLogPayloadRecord {
            request_log_id: context.request_log_id,
            request_json: context.request_json.clone(),
            response_json,
        };

        self.repo.insert_request_log(&log, Some(&payload)).await?;

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

    let prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_i64);
    let completion_tokens = usage.get("completion_tokens").and_then(Value::as_i64);
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

fn request_log_metadata(attempt_count: usize, stream: bool) -> Map<String, Value> {
    let mut metadata = Map::new();
    metadata.insert("stream".to_string(), Value::Bool(stream));
    metadata.insert("fallback_used".to_string(), Value::Bool(attempt_count > 1));
    metadata.insert(
        "attempt_count".to_string(),
        Value::Number(i64::try_from(attempt_count).unwrap_or(i64::MAX).into()),
    );
    metadata
}

fn truncate_payload(value: Value) -> (Value, bool) {
    match serde_json::to_vec(&value) {
        Ok(bytes) if bytes.len() > MAX_PAYLOAD_BYTES => (
            json!({
                "truncated": true,
                "size_bytes": bytes.len(),
                "preview": String::from_utf8_lossy(&bytes[..MAX_PAYLOAD_BYTES.min(bytes.len())]).to_string(),
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
        StoreError, TeamMembershipRecord, TeamRecord, UserRecord,
    };
    use serde_json::{Value, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::{RequestLogging, StreamFailureSummary};

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
        ) -> Result<Option<RequestLogDetail>, StoreError> {
            let logs = self.logs.lock().expect("logs lock");
            let Some(log) = logs
                .iter()
                .find(|log| log.request_log_id == request_log_id)
                .cloned()
            else {
                return Ok(None);
            };
            let payload = self
                .payloads
                .lock()
                .expect("payloads lock")
                .iter()
                .find(|payload| payload.request_log_id == request_log_id)
                .cloned();
            Ok(Some(RequestLogDetail { log, payload }))
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
            status: "active".to_string(),
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
        );

        let wrote = logging
            .log_non_stream_success(
                &auth,
                &context,
                "openai-prod",
                1,
                120,
                &json!({"usage": {"prompt_tokens": 1, "completion_tokens": 2}}),
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
        );

        let wrote = logging
            .log_non_stream_success(
                &auth,
                &context,
                "openai-prod",
                2,
                120,
                &json!({"usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}}),
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
        assert_eq!(logs[0].metadata["fallback_used"], Value::Bool(true));
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
        );
        let mut collector = logging.new_stream_response_collector();
        collector.observe_chunk(br#"data: {"delta":"hello"}"#);

        let wrote = logging
            .log_stream_result(
                &auth,
                &context,
                "openai-prod",
                1,
                120,
                collector,
                Some(StreamFailureSummary {
                    status_code: 502,
                    error_code: "stream_error".to_string(),
                }),
            )
            .await
            .expect("stream failure log");

        assert!(wrote.wrote);
        let payload = repo.payloads.lock().expect("payloads lock");
        assert_eq!(payload[0].response_json["error"]["code"], "stream_error");
    }
}
