use std::sync::Arc;

use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, GatewayError, McpToolInvocationDetail,
    McpToolInvocationPage, McpToolInvocationPayloadRecord, McpToolInvocationQuery,
    McpToolInvocationRecord, McpToolInvocationRepository, McpToolInvocationStatus,
    McpToolPolicyResult,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::redaction::{
    RequestLogPayloadCaptureMode, RequestLogPayloadPolicy, redact_json_value_with_policy,
    truncate_large_payload_fields,
};

const DEFAULT_MCP_PAYLOAD_MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub struct McpInvocationPayloadPolicy {
    inner: RequestLogPayloadPolicy,
}

impl Default for McpInvocationPayloadPolicy {
    fn default() -> Self {
        Self {
            inner: RequestLogPayloadPolicy::new(
                RequestLogPayloadCaptureMode::RedactedPayloads,
                DEFAULT_MCP_PAYLOAD_MAX_BYTES,
                DEFAULT_MCP_PAYLOAD_MAX_BYTES,
                1,
                Vec::new(),
            ),
        }
    }
}

impl McpInvocationPayloadPolicy {
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            inner: RequestLogPayloadPolicy::new(
                RequestLogPayloadCaptureMode::Disabled,
                DEFAULT_MCP_PAYLOAD_MAX_BYTES,
                DEFAULT_MCP_PAYLOAD_MAX_BYTES,
                1,
                Vec::new(),
            ),
        }
    }

    #[must_use]
    pub fn from_request_log_policy(policy: RequestLogPayloadPolicy) -> Self {
        Self { inner: policy }
    }
}

#[derive(Debug, Clone)]
pub struct McpInvocationLogInput {
    pub request_log_id: Option<Uuid>,
    pub request_id: String,
    pub server_id: Option<Uuid>,
    pub server_display_key: String,
    pub server_display_name: String,
    pub tool_id: Option<Uuid>,
    pub tool_display_key: String,
    pub tool_display_name: String,
    pub status: McpToolInvocationStatus,
    pub policy_result: McpToolPolicyResult,
    pub latency_ms: Option<i64>,
    pub error_code: Option<String>,
    pub arguments_json: Option<Value>,
    pub result_json: Option<Value>,
    pub metadata: Map<String, Value>,
    pub occurred_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoggedMcpToolInvocation {
    pub mcp_tool_invocation_id: Uuid,
    pub wrote_payload: bool,
}

#[derive(Clone)]
pub struct McpInvocationLogging<R> {
    repo: Arc<R>,
    payload_policy: McpInvocationPayloadPolicy,
}

impl<R> McpInvocationLogging<R>
where
    R: McpToolInvocationRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self::new_with_payload_policy(repo, McpInvocationPayloadPolicy::default())
    }

    #[must_use]
    pub fn new_with_payload_policy(
        repo: Arc<R>,
        payload_policy: McpInvocationPayloadPolicy,
    ) -> Self {
        Self {
            repo,
            payload_policy,
        }
    }

    pub async fn log_invocation(
        &self,
        auth: &AuthenticatedApiKey,
        input: McpInvocationLogInput,
    ) -> Result<LoggedMcpToolInvocation, GatewayError> {
        let mcp_tool_invocation_id = Uuid::new_v4();
        let (payload, arguments_truncated, result_truncated) =
            self.payload_for(mcp_tool_invocation_id, &input);
        let invocation = McpToolInvocationRecord {
            mcp_tool_invocation_id,
            request_log_id: input.request_log_id,
            request_id: input.request_id,
            api_key_id: Some(auth.id),
            user_id: auth.owner_user_id,
            team_id: auth.owner_team_id,
            owner_kind: auth.owner_kind,
            server_id: input.server_id,
            server_display_key: input.server_display_key,
            server_display_name: input.server_display_name,
            tool_id: input.tool_id,
            tool_display_key: input.tool_display_key,
            tool_display_name: input.tool_display_name,
            status: input.status,
            policy_result: input.policy_result,
            latency_ms: input.latency_ms,
            error_code: input.error_code,
            has_payload: payload.is_some(),
            arguments_payload_truncated: arguments_truncated,
            result_payload_truncated: result_truncated,
            arguments_payload_redacted: payload.is_some(),
            result_payload_redacted: payload.is_some(),
            metadata: input.metadata,
            occurred_at: input.occurred_at,
        };

        validate_owner(auth)?;
        self.repo
            .insert_mcp_tool_invocation(&invocation, payload.as_ref())
            .await?;
        Ok(LoggedMcpToolInvocation {
            mcp_tool_invocation_id,
            wrote_payload: payload.is_some(),
        })
    }

    pub async fn list_invocations(
        &self,
        query: &McpToolInvocationQuery,
    ) -> Result<McpToolInvocationPage, GatewayError> {
        Ok(self.repo.list_mcp_tool_invocations(query).await?)
    }

    pub async fn get_invocation_detail(
        &self,
        mcp_tool_invocation_id: Uuid,
    ) -> Result<McpToolInvocationDetail, GatewayError> {
        Ok(self
            .repo
            .get_mcp_tool_invocation_detail(mcp_tool_invocation_id)
            .await?)
    }

    fn payload_for(
        &self,
        mcp_tool_invocation_id: Uuid,
        input: &McpInvocationLogInput,
    ) -> (Option<McpToolInvocationPayloadRecord>, bool, bool) {
        if !self.payload_policy.inner.should_capture_payloads() {
            return (None, false, false);
        }

        let (arguments_json, arguments_truncated) = sanitize_payload(
            input.arguments_json.as_ref().unwrap_or(&Value::Null),
            self.payload_policy.inner.request_max_bytes,
            &self.payload_policy.inner,
        );
        let (result_json, result_truncated) = sanitize_payload(
            input.result_json.as_ref().unwrap_or(&Value::Null),
            self.payload_policy.inner.response_max_bytes,
            &self.payload_policy.inner,
        );

        (
            Some(McpToolInvocationPayloadRecord {
                mcp_tool_invocation_id,
                arguments_json,
                result_json,
            }),
            arguments_truncated,
            result_truncated,
        )
    }
}

fn validate_owner(auth: &AuthenticatedApiKey) -> Result<(), GatewayError> {
    match auth.owner_kind {
        ApiKeyOwnerKind::User if auth.owner_user_id.is_none() => {
            Err(AuthError::ApiKeyOwnerInvalid.into())
        }
        ApiKeyOwnerKind::Team if auth.owner_team_id.is_none() => {
            Err(AuthError::ApiKeyOwnerInvalid.into())
        }
        _ => Ok(()),
    }
}

fn sanitize_payload(
    value: &Value,
    max_bytes: usize,
    policy: &RequestLogPayloadPolicy,
) -> (Value, bool) {
    let redacted = redact_json_value_with_policy(value, policy);
    let redacted = truncate_large_payload_fields(&redacted);
    match serde_json::to_vec(&redacted) {
        Ok(encoded) if encoded.len() <= max_bytes => (redacted, false),
        Ok(encoded) => {
            let preview = String::from_utf8_lossy(truncate_at_utf8_boundary(&encoded, max_bytes));
            (
                json!({
                    "truncated": true,
                    "preview": preview,
                }),
                true,
            )
        }
        Err(_) => (
            json!({
                "truncated": true,
                "error": "payload_serialization_failed",
            }),
            true,
        ),
    }
}

fn truncate_at_utf8_boundary(encoded: &[u8], max_bytes: usize) -> &[u8] {
    let truncate_at = max_bytes.min(encoded.len());
    let mut safe_truncate = truncate_at;
    while safe_truncate > 0 && std::str::from_utf8(&encoded[..safe_truncate]).is_err() {
        safe_truncate -= 1;
    }
    &encoded[..safe_truncate]
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyOwnerKind, AuthenticatedApiKey, McpToolInvocationDetail, McpToolInvocationPage,
        McpToolInvocationPayloadRecord, McpToolInvocationQuery, McpToolInvocationRecord,
        McpToolInvocationRepository, StoreError,
    };
    use serde_json::{Map, json};

    use super::*;

    #[derive(Clone, Default)]
    struct InMemoryRepo {
        invocations: Arc<Mutex<Vec<McpToolInvocationRecord>>>,
        payloads: Arc<Mutex<Vec<McpToolInvocationPayloadRecord>>>,
    }

    #[async_trait]
    impl McpToolInvocationRepository for InMemoryRepo {
        async fn insert_mcp_tool_invocation(
            &self,
            invocation: &McpToolInvocationRecord,
            payload: Option<&McpToolInvocationPayloadRecord>,
        ) -> Result<(), StoreError> {
            self.invocations
                .lock()
                .expect("invocations lock")
                .push(invocation.clone());
            if let Some(payload) = payload {
                self.payloads
                    .lock()
                    .expect("payloads lock")
                    .push(payload.clone());
            }
            Ok(())
        }

        async fn list_mcp_tool_invocations(
            &self,
            _query: &McpToolInvocationQuery,
        ) -> Result<McpToolInvocationPage, StoreError> {
            let items = self.invocations.lock().expect("invocations lock").clone();
            Ok(McpToolInvocationPage {
                total: items.len() as u64,
                items,
                page: 1,
                page_size: 100,
            })
        }

        async fn get_mcp_tool_invocation_detail(
            &self,
            mcp_tool_invocation_id: Uuid,
        ) -> Result<McpToolInvocationDetail, StoreError> {
            let invocation = self
                .invocations
                .lock()
                .expect("invocations lock")
                .iter()
                .find(|item| item.mcp_tool_invocation_id == mcp_tool_invocation_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound("missing invocation".to_string()))?;
            let payload = self
                .payloads
                .lock()
                .expect("payloads lock")
                .iter()
                .find(|payload| payload.mcp_tool_invocation_id == mcp_tool_invocation_id)
                .cloned();
            Ok(McpToolInvocationDetail {
                invocation,
                payload,
            })
        }
    }

    #[tokio::test]
    async fn disabled_payload_policy_still_writes_durable_summary() {
        let repo = Arc::new(InMemoryRepo::default());
        let logging = McpInvocationLogging::new_with_payload_policy(
            repo.clone(),
            McpInvocationPayloadPolicy::disabled(),
        );

        let result = logging
            .log_invocation(&sample_auth(), sample_input())
            .await
            .expect("log invocation");

        assert!(!result.wrote_payload);
        let invocations = repo.invocations.lock().expect("invocations lock");
        assert_eq!(invocations.len(), 1);
        assert!(!invocations[0].has_payload);
        assert!(repo.payloads.lock().expect("payloads lock").is_empty());
    }

    #[tokio::test]
    async fn redacts_and_truncates_mcp_payloads_with_separate_policy() {
        let repo = Arc::new(InMemoryRepo::default());
        let logging = McpInvocationLogging::new_with_payload_policy(
            repo.clone(),
            McpInvocationPayloadPolicy::from_request_log_policy(RequestLogPayloadPolicy::new(
                RequestLogPayloadCaptureMode::RedactedPayloads,
                32,
                4096,
                1,
                Vec::new(),
            )),
        );
        let mut input = sample_input();
        input.arguments_json = Some(json!({"api_key": "sk-secret", "large": "x".repeat(512)}));

        logging
            .log_invocation(&sample_auth(), input)
            .await
            .expect("log invocation");

        let invocations = repo.invocations.lock().expect("invocations lock");
        let payloads = repo.payloads.lock().expect("payloads lock");
        assert!(invocations[0].has_payload);
        assert!(invocations[0].arguments_payload_truncated);
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].arguments_json["truncated"], true);
    }

    #[test]
    fn truncated_preview_stops_at_utf8_boundary() {
        let policy = RequestLogPayloadPolicy::new(
            RequestLogPayloadCaptureMode::RedactedPayloads,
            4096,
            4096,
            1,
            Vec::new(),
        );
        let (payload, truncated) = sanitize_payload(&json!("éééé"), 4, &policy);

        assert!(truncated);
        let preview = payload["preview"].as_str().expect("preview string");
        assert!(!preview.contains('\u{fffd}'));
    }

    fn sample_auth() -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_user_id: None,
            owner_team_id: Some(Uuid::new_v4()),
            owner_service_account_id: None,
        }
    }

    fn sample_input() -> McpInvocationLogInput {
        McpInvocationLogInput {
            request_log_id: None,
            request_id: "req_123".to_string(),
            server_id: None,
            server_display_key: "github".to_string(),
            server_display_name: "GitHub".to_string(),
            tool_id: None,
            tool_display_key: "issues.create".to_string(),
            tool_display_name: "Create issue".to_string(),
            status: McpToolInvocationStatus::Success,
            policy_result: McpToolPolicyResult::Allowed,
            latency_ms: Some(42),
            error_code: None,
            arguments_json: Some(json!({"title": "test"})),
            result_json: Some(json!({"ok": true})),
            metadata: Map::new(),
            occurred_at: OffsetDateTime::now_utc(),
        }
    }
}
