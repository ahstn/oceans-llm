use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use gateway_core::{
    ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthMode, AuthenticatedApiKey, ChatCompletionsRequest,
    EmbeddingsRequest, GlobalRole, IdentityRepository, ModelAccessMode, RequestAttemptRecord,
    RequestAttemptStatus, RequestLogDetail, RequestLogPage, RequestLogPayloadRecord,
    RequestLogPurgeResult, RequestLogQuery, RequestLogRecord, RequestLogRepository, RequestTags,
    StoreError, TeamMembershipRecord, TeamRecord, UserRecord, UserStatus,
};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    RequestLogIconMetadata,
    redaction::{RequestLogPayloadCaptureMode, RequestLogPayloadPolicy, parse_payload_path},
};

#[derive(Clone, Default)]
struct InMemoryRepo {
    users: Arc<Mutex<Vec<UserRecord>>>,
    logs: Arc<Mutex<Vec<RequestLogRecord>>>,
    payloads: Arc<Mutex<Vec<RequestLogPayloadRecord>>>,
    attempts: Arc<Mutex<Vec<RequestAttemptRecord>>>,
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

    async fn insert_request_log_with_attempts(
        &self,
        log: &RequestLogRecord,
        payload: Option<&RequestLogPayloadRecord>,
        attempts: &[RequestAttemptRecord],
    ) -> Result<(), StoreError> {
        self.insert_request_log(log, payload).await?;
        self.attempts
            .lock()
            .expect("attempts lock")
            .extend(attempts.iter().cloned());
        Ok(())
    }

    async fn list_request_logs(
        &self,
        _query: &RequestLogQuery,
    ) -> Result<RequestLogPage, StoreError> {
        let logs = self.logs.lock().expect("logs lock").clone();
        Ok(RequestLogPage {
            total: logs.len() as u64,
            items: logs,
            page: 1,
            page_size: 50,
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
        let attempts = self
            .attempts
            .lock()
            .expect("attempts lock")
            .iter()
            .filter(|attempt| attempt.request_log_id == request_log_id)
            .cloned()
            .collect();
        Ok(RequestLogDetail {
            log,
            payload,
            attempts,
        })
    }

    async fn purge_request_logs_older_than(
        &self,
        cutoff: OffsetDateTime,
        dry_run: bool,
    ) -> Result<RequestLogPurgeResult, StoreError> {
        let matched_count = self
            .logs
            .lock()
            .expect("logs lock")
            .iter()
            .filter(|log| log.occurred_at < cutoff)
            .count() as u64;
        if dry_run {
            return Ok(RequestLogPurgeResult {
                cutoff,
                dry_run,
                matched_count,
                deleted_count: 0,
            });
        }

        self.logs
            .lock()
            .expect("logs lock")
            .retain(|log| log.occurred_at >= cutoff);
        Ok(RequestLogPurgeResult {
            cutoff,
            dry_run,
            matched_count,
            deleted_count: matched_count,
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
        tags: Vec::new(),
        created_at: OffsetDateTime::now_utc(),
        updated_at: OffsetDateTime::now_utc(),
    }
}

fn sample_auth(user_id: Uuid) -> AuthenticatedApiKey {
    AuthenticatedApiKey {
        id: Uuid::new_v4(),
        public_id: "dev123".to_string(),
        name: "dev".to_string(),
        model_grant_mode: ApiKeyModelGrantMode::Explicit,
        owner_kind: ApiKeyOwnerKind::User,
        owner_user_id: Some(user_id),
        owner_team_id: None,
        owner_service_account_id: None,
    }
}

fn sample_service_account_auth() -> AuthenticatedApiKey {
    AuthenticatedApiKey {
        id: Uuid::new_v4(),
        public_id: "dev123".to_string(),
        name: "dev".to_string(),
        model_grant_mode: ApiKeyModelGrantMode::Explicit,
        owner_kind: ApiKeyOwnerKind::ServiceAccount,
        owner_user_id: None,
        owner_team_id: Some(Uuid::new_v4()),
        owner_service_account_id: Some(Uuid::new_v4()),
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

fn sample_embeddings_request() -> EmbeddingsRequest {
    EmbeddingsRequest {
        model: "embeddings".to_string(),
        input: json!("hello"),
        extra: BTreeMap::new(),
    }
}

fn sample_attempt(
    request_log_id: Uuid,
    request_id: &str,
    status: RequestAttemptStatus,
) -> RequestAttemptRecord {
    RequestAttemptRecord {
        request_attempt_id: Uuid::new_v4(),
        request_log_id,
        request_id: request_id.to_string(),
        attempt_number: 1,
        route_id: Uuid::new_v4(),
        provider_key: "vertex-prod".to_string(),
        upstream_model: "google/gemini-embedding-001".to_string(),
        status,
        status_code: match status {
            RequestAttemptStatus::Success => Some(200),
            RequestAttemptStatus::ProviderError => Some(502),
            RequestAttemptStatus::StreamStartError | RequestAttemptStatus::StreamError => None,
        },
        error_code: match status {
            RequestAttemptStatus::Success => None,
            RequestAttemptStatus::ProviderError => Some("upstream_transport".to_string()),
            RequestAttemptStatus::StreamStartError | RequestAttemptStatus::StreamError => {
                Some("stream_error".to_string())
            }
        },
        error_detail: match status {
            RequestAttemptStatus::Success => None,
            RequestAttemptStatus::ProviderError => Some("connection reset".to_string()),
            RequestAttemptStatus::StreamStartError | RequestAttemptStatus::StreamError => None,
        },
        error_detail_truncated: false,
        retryable: matches!(status, RequestAttemptStatus::ProviderError),
        terminal: true,
        produced_final_response: matches!(status, RequestAttemptStatus::Success),
        stream: false,
        started_at: OffsetDateTime::now_utc(),
        completed_at: Some(OffsetDateTime::now_utc()),
        latency_ms: Some(42),
        metadata: Default::default(),
    }
}

fn sample_log(request_id: &str, occurred_at: OffsetDateTime) -> RequestLogRecord {
    RequestLogRecord {
        request_log_id: Uuid::new_v4(),
        request_id: request_id.to_string(),
        api_key_id: Uuid::new_v4(),
        user_id: None,
        team_id: None,
        service_account_id: None,
        model_key: "fast".to_string(),
        resolved_model_key: "fast".to_string(),
        provider_key: "openai-prod".to_string(),
        status_code: Some(200),
        latency_ms: Some(42),
        prompt_tokens: Some(1),
        completion_tokens: Some(2),
        total_tokens: Some(3),
        error_code: None,
        has_payload: false,
        request_payload_truncated: false,
        response_payload_truncated: false,
        request_tags: RequestTags::default(),
        tool_cardinality: Default::default(),
        user_agent_raw: None,
        agent_harness_key: "unknown".to_string(),
        agent_harness_label: "Unknown".to_string(),
        metadata: Default::default(),
        occurred_at,
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

mod attempts;
mod harness;
mod measurement;
mod persistence;
mod stream;
mod tool_cardinality;
