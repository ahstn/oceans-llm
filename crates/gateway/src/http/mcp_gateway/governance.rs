//! The governance steps every MCP call runs between authorization and the
//! upstream request, shared by the per-server proxy (`tools/call` and the
//! resources, prompts and completion methods) and aggregate `call_tool`.
//!
//! Each route authorizes its own way (a tool grant by name or address, or a
//! server grant), sends the upstream request its own way, and writes its own
//! response format. What happens in between is decided here, in this order:
//!
//! 1. the server's MCP call rate limits,
//! 2. request guardrails on tool arguments, for `tools/call`,
//! 3. the upstream credential for the caller.
//!
//! A new step belongs in [`GovernedCall::admit`], so no route can skip it.

use axum::{body::Body, http::Response};
use gateway_core::{
    AuthenticatedApiKey, ExternalMcpServerRecord, GatewayError, McpToolInvocationStatus,
    McpToolPolicyResult,
};
use gateway_guardrails::{EvaluationPayload, GuardrailEvaluation};
use gateway_service::{McpGatewayService, McpGatewayUpstream, RateLimitStatus};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::{
    credentials::{tool_call_auth_error_response, upstream_auth_error_response},
    guardrails::{evaluate_mcp_call, guardrail_decision_metadata, guardrail_denied_response},
    rate_limits,
};
use crate::http::{request_policy::RequestPolicyContext, state::AppState};

/// One governed call, from authorization to the upstream request.
pub(super) struct GovernedCall<'a> {
    state: &'a AppState,
    policy: &'a RequestPolicyContext,
    auth: &'a AuthenticatedApiKey,
    server: &'a ExternalMcpServerRecord,
    pub(super) invocation_id: Uuid,
    pub(super) request_id: String,
}

/// Why a call stopped before reaching the upstream.
pub(super) enum Refusal {
    RateLimited(RateLimitStatus),
    /// The limiter could not check the call and `failure_mode` is `closed`.
    RateLimitUnavailable(GatewayError),
    GuardrailDenied(GuardrailEvaluation),
    /// The caller has no usable upstream credential.
    Credential(GatewayError),
}

/// A refusal, with the guarded arguments when a step after argument
/// guardrails refused the call, so the route can log them.
pub(super) struct Refused {
    pub(super) refusal: Refusal,
    pub(super) arguments: Option<GuardedArguments>,
}

/// Tool arguments after request guardrails. `value` is moved out of the
/// evaluation's output, which no longer holds them.
pub(super) struct GuardedArguments {
    pub(super) value: Value,
    pub(super) evaluation: GuardrailEvaluation,
}

impl GuardedArguments {
    /// Whether a guardrail rewrote the arguments.
    pub(super) fn transformed(&self) -> bool {
        self.evaluation
            .decisions
            .iter()
            .any(|decision| decision.transformed)
    }
}

/// A call every step admitted.
pub(super) struct Admitted {
    /// Set for `tools/call`.
    pub(super) arguments: Option<GuardedArguments>,
    pub(super) upstream: McpGatewayUpstream,
}

/// What a refusal records in the invocation log.
pub(super) struct RefusalLog {
    pub(super) status: McpToolInvocationStatus,
    pub(super) policy_result: McpToolPolicyResult,
    pub(super) error_code: String,
    pub(super) metadata: Map<String, Value>,
}

impl<'a> GovernedCall<'a> {
    /// `invocation_id` names the call in guardrail events and the invocation
    /// log, so the route passes the id it logs the call under.
    pub(super) fn new(
        state: &'a AppState,
        policy: &'a RequestPolicyContext,
        auth: &'a AuthenticatedApiKey,
        server: &'a ExternalMcpServerRecord,
        invocation_id: Uuid,
        request_id: String,
    ) -> Self {
        Self {
            state,
            policy,
            auth,
            server,
            invocation_id,
            request_id,
        }
    }

    /// Runs every step for an authorized call. `tool_call` names the tool
    /// and its arguments for `tools/call`; other methods pass `None`. A
    /// refusal from a step after argument guardrails also returns the guarded
    /// arguments, so they can be logged.
    pub(super) async fn admit(
        &self,
        tool_call: Option<(&str, Value)>,
    ) -> Result<Admitted, Box<Refused>> {
        let refused = |refusal, arguments| Box::new(Refused { refusal, arguments });
        match rate_limits::check_mcp_call(self.state, self.policy, self.auth, self.server).await {
            Ok(None) => {}
            Ok(Some(rejected)) => return Err(refused(Refusal::RateLimited(rejected), None)),
            Err(error) => return Err(refused(Refusal::RateLimitUnavailable(error), None)),
        }
        let arguments = match tool_call {
            Some((tool_name, arguments)) => {
                let mut evaluation = evaluate_mcp_call(
                    self.state,
                    Some(&self.request_id),
                    Some(self.invocation_id),
                    &self.server.server_key,
                    tool_name,
                    arguments,
                )
                .await;
                if evaluation.denied() {
                    return Err(refused(Refusal::GuardrailDenied(evaluation), None));
                }
                let value = match &mut evaluation.output {
                    EvaluationPayload::McpCall { call } => std::mem::take(&mut call.arguments),
                    _ => unreachable!("MCP call evaluation preserves payload kind"),
                };
                Some(GuardedArguments { value, evaluation })
            }
            None => None,
        };
        match McpGatewayService::new(
            self.state.store.clone(),
            self.state.mcp_http_clients.url_policy().clone(),
        )
        .with_oauth_runtime(self.state.mcp_oauth_runtime.clone())
        .prepare_upstream_for_auth(self.auth, self.server.clone())
        .await
        {
            Ok(upstream) => Ok(Admitted {
                arguments,
                upstream,
            }),
            Err(error) => Err(refused(Refusal::Credential(error), arguments)),
        }
    }
}

impl Refusal {
    /// The invocation-log fields for this refusal.
    pub(super) fn log(&self) -> RefusalLog {
        let (status, policy_result, error_code, metadata) = match self {
            Self::RateLimited(rejected) => (
                McpToolInvocationStatus::PolicyDenied,
                McpToolPolicyResult::Denied,
                rate_limits::RATE_LIMITED_ERROR_CODE.to_string(),
                rate_limits::rejection_metadata(rejected),
            ),
            Self::RateLimitUnavailable(error) => (
                McpToolInvocationStatus::GatewayError,
                McpToolPolicyResult::NotEvaluated,
                error.error_code().to_string(),
                Map::new(),
            ),
            Self::GuardrailDenied(evaluation) => (
                McpToolInvocationStatus::PolicyDenied,
                McpToolPolicyResult::Denied,
                "guardrail_policy_denied".to_string(),
                guardrail_decision_metadata(evaluation),
            ),
            Self::Credential(error) => (
                match error {
                    GatewayError::McpCredentialRequired { .. }
                    | GatewayError::McpCredentialExpired { .. } => {
                        McpToolInvocationStatus::Unauthorized
                    }
                    // Preparing the credential failed for another reason,
                    // such as an unavailable store.
                    _ => McpToolInvocationStatus::GatewayError,
                },
                McpToolPolicyResult::Allowed,
                error.error_code().to_string(),
                Map::new(),
            ),
        };
        RefusalLog {
            status,
            policy_result,
            error_code,
            metadata,
        }
    }

    /// The per-server proxy's response: a JSON-RPC error with an HTTP status,
    /// without an `id` for a notification. A credential refusal of a
    /// `tools/call` or of any notification is JSON-RPC too (see
    /// `tool_call_auth_error_response`); for other requests it is the proxy's
    /// HTTP error with the connection guidance.
    pub(super) fn direct_response(
        self,
        state: &AppState,
        auth: &AuthenticatedApiKey,
        server: &ExternalMcpServerRecord,
        id: Option<&Value>,
        tool_call: bool,
    ) -> Response<Body> {
        match self {
            Self::RateLimited(rejected) => rate_limits::rejection_response(id, &rejected),
            Self::RateLimitUnavailable(error) => rate_limits::unavailable_response(id, &error),
            Self::GuardrailDenied(evaluation) => guardrail_denied_response(id, &evaluation),
            Self::Credential(error) if tool_call || id.is_none() => {
                tool_call_auth_error_response(state, auth, server, id, error)
            }
            Self::Credential(error) => upstream_auth_error_response(state, auth, server, error),
        }
    }
}
