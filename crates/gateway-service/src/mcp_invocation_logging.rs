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
        ApiKeyOwnerKind::ServiceAccount
            if auth.owner_service_account_id.is_none() || auth.owner_team_id.is_none() =>
        {
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
