use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use gateway_core::{
    GuardrailDecisionEventRecord, GuardrailDecisionQuery, GuardrailDecisionRepository,
    MAX_GUARDRAIL_DECISION_PAGE_SIZE,
};
use gateway_guardrails::{
    DecisionAction, DecisionId, EffectivePolicy, EvaluationInput, EvaluationPayload,
    FailureDisposition, GuardPhase, ManagedCheckKind, PackRegistry, PolicyResolver, PolicyTarget,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use utoipa::ToSchema;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{Envelope, envelope},
    error::AppError,
    guardrail_events::record_guardrail_evaluation,
    state::AppState,
};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardEvaluationRequest {
    pub tool_name: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub arguments: Option<Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GuardEvaluationResponse {
    pub decision_id: String,
    pub action: String,
    pub allowed: bool,
    pub matched_rule: Option<MatchedRuleResponse>,
    pub reason_code: Option<String>,
    pub failure_disposition: Option<String>,
    pub transformed: bool,
    pub output_command: Option<String>,
    pub output_arguments: Option<Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MatchedRuleResponse {
    pub pack_id: String,
    pub rule_id: String,
    pub matched_field: String,
    pub description: String,
    pub safer_action: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/guardrails/evaluate",
    request_body = GuardEvaluationRequest,
    responses(
        (status = 200, description = "Guardrail decision", body = GuardEvaluationResponse),
        (status = 401, description = "Missing or invalid API key"),
        (status = 400, description = "Invalid evaluation request")
    ),
    security(("gateway_api_key" = [])),
    tag = "Guardrails"
)]
pub async fn evaluate_guardrail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GuardEvaluationRequest>,
) -> Result<Json<GuardEvaluationResponse>, AppError> {
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let _auth = state.service.authenticate(authorization).await?;

    let payload = match (request.command, request.arguments) {
        (Some(command), None) => EvaluationPayload::ShellCommand { command },
        (None, Some(arguments)) => EvaluationPayload::ToolCall {
            name: request.tool_name,
            arguments,
        },
        (Some(_), Some(_)) => {
            return Err(AppError(gateway_core::GatewayError::InvalidRequest(
                "guard evaluation accepts either command or arguments, not both".to_string(),
            )));
        }
        (None, None) => {
            return Err(AppError(gateway_core::GatewayError::InvalidRequest(
                "guard evaluation requires command or arguments".to_string(),
            )));
        }
    };
    let policy = PolicyResolver::new(&state.guardrail_config).resolve(PolicyTarget::Global);
    let evaluation = state
        .guardrail_engine
        .evaluate(
            &policy,
            &state.guardrail_config,
            EvaluationInput::new(GuardPhase::HarnessPreTool, payload),
        )
        .await;
    record_guardrail_evaluation(&state, None, None, &evaluation).await;
    let relevant = evaluation
        .decisions
        .iter()
        .rev()
        .find(|decision| decision.action != DecisionAction::Allow)
        .or_else(|| evaluation.decisions.last());
    let matched_rule = relevant
        .and_then(|decision| decision.matched_rule.as_ref())
        .map(|matched| MatchedRuleResponse {
            pack_id: matched.pack_id.clone(),
            rule_id: matched.rule_id.clone(),
            matched_field: matched.matched_field.clone(),
            description: matched.description.clone(),
            safer_action: matched.safer_action.clone(),
        });

    let (output_command, output_arguments) = match &evaluation.output {
        EvaluationPayload::ShellCommand { command } => (Some(command.clone()), None),
        EvaluationPayload::ToolCall { arguments, .. } => (None, Some(arguments.clone())),
        _ => (None, None),
    };
    Ok(Json(GuardEvaluationResponse {
        decision_id: relevant
            .map(|decision| decision.decision_id.to_string())
            .unwrap_or_else(|| DecisionId::new().to_string()),
        action: action_name(evaluation.action).to_string(),
        allowed: !evaluation.denied(),
        matched_rule,
        reason_code: relevant.map(|decision| decision.reason_code.to_string()),
        failure_disposition: relevant
            .and_then(|decision| decision.failure_disposition)
            .map(failure_disposition_name)
            .map(str::to_string),
        transformed: evaluation
            .decisions
            .iter()
            .any(|decision| decision.transformed),
        output_command,
        output_arguments,
    }))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GuardrailPoliciesView {
    pub default: EffectiveGuardrailPolicyView,
    pub model_routes: BTreeMap<String, EffectiveGuardrailPolicyView>,
    pub mcp_servers: BTreeMap<String, EffectiveGuardrailPolicyView>,
    pub managed_checks: Vec<ManagedCheckView>,
    pub built_in_packs: Vec<BuiltInPackView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EffectiveGuardrailPolicyView {
    pub enabled: bool,
    pub mode: String,
    pub packs: Vec<String>,
    pub managed_checks: Vec<String>,
    pub stream_buffer_bytes: usize,
    pub scope: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ManagedCheckView {
    pub name: String,
    pub kind: String,
    pub phases: Vec<String>,
    pub timeout_ms: u64,
    pub failure_disposition: String,
    pub max_content_bytes: usize,
    pub resource: String,
    pub prompt_resource: Option<String>,
    pub response_resource: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BuiltInPackView {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct GuardrailDecisionListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub request_id: Option<String>,
    pub phase: Option<String>,
    pub action: Option<String>,
    pub evaluator: Option<String>,
    pub occurred_at_start: Option<String>,
    pub occurred_at_end: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GuardrailDecisionPageView {
    pub items: Vec<GuardrailDecisionView>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GuardrailDecisionView {
    pub decision_id: String,
    pub request_id: Option<String>,
    pub mcp_tool_invocation_id: Option<String>,
    pub phase: String,
    pub effective_scope: String,
    pub evaluator: String,
    pub managed_service: Option<String>,
    pub pack_id: Option<String>,
    pub rule_id: Option<String>,
    pub action: String,
    pub reason_code: String,
    pub latency_micros: i64,
    pub failure_disposition: Option<String>,
    pub transformed: bool,
    pub content_hash: String,
    pub occurred_at: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/guardrails/policies",
    responses((status = 200, body = Envelope<GuardrailPoliciesView>)),
    security(("session_cookie" = [])),
    tag = "Guardrails"
)]
pub async fn get_guardrail_policies(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<GuardrailPoliciesView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let resolver = PolicyResolver::new(&state.guardrail_config);
    let model_routes = state
        .guardrail_config
        .model_routes
        .keys()
        .map(|route| {
            (
                route.clone(),
                policy_view(resolver.resolve(PolicyTarget::ModelRoute(route))),
            )
        })
        .collect();
    let mcp_servers = state
        .guardrail_config
        .mcp_servers
        .keys()
        .map(|server| {
            (
                server.clone(),
                policy_view(resolver.resolve(PolicyTarget::McpServer(server))),
            )
        })
        .collect();
    let managed_checks = state
        .guardrail_config
        .managed_checks
        .iter()
        .map(|(name, check)| {
            let (kind, resource, prompt_resource, response_resource) = match check.kind {
                ManagedCheckKind::AmazonBedrock => (
                    "amazon_bedrock",
                    check
                        .bedrock
                        .as_ref()
                        .map(|config| config.guardrail_identifier.clone())
                        .unwrap_or_default(),
                    None,
                    None,
                ),
                ManagedCheckKind::GoogleModelArmor => {
                    let config = check.model_armor.as_ref();
                    (
                        "google_model_armor",
                        String::new(),
                        config.and_then(|config| config.prompt_template.clone()),
                        config.and_then(|config| config.response_template.clone()),
                    )
                }
            };
            ManagedCheckView {
                name: name.clone(),
                kind: kind.to_string(),
                phases: check
                    .phases
                    .iter()
                    .copied()
                    .map(phase_name)
                    .map(str::to_string)
                    .collect(),
                timeout_ms: check.timeout_ms,
                failure_disposition: failure_disposition_name(check.failure_disposition)
                    .to_string(),
                max_content_bytes: check.max_content_bytes,
                resource,
                prompt_resource,
                response_resource,
            }
        })
        .collect();
    let built_in_packs = PackRegistry::built_in()
        .into_iter()
        .map(|pack| BuiltInPackView {
            id: pack.id.to_string(),
            version: pack.version.to_string(),
        })
        .collect();
    Ok(Json(envelope(GuardrailPoliciesView {
        default: policy_view(resolver.resolve(PolicyTarget::Global)),
        model_routes,
        mcp_servers,
        managed_checks,
        built_in_packs,
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/guardrails/decisions",
    params(GuardrailDecisionListQuery),
    responses((status = 200, body = Envelope<GuardrailDecisionPageView>)),
    security(("session_cookie" = [])),
    tag = "Guardrails"
)]
pub async fn list_guardrail_decisions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GuardrailDecisionListQuery>,
) -> Result<Json<Envelope<GuardrailDecisionPageView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let page = state
        .store
        .list_guardrail_decisions(&GuardrailDecisionQuery {
            page: query.page.unwrap_or(1).max(1),
            page_size: query
                .page_size
                .unwrap_or(50)
                .clamp(1, MAX_GUARDRAIL_DECISION_PAGE_SIZE),
            request_id: non_empty(query.request_id),
            phase: non_empty(query.phase),
            action: non_empty(query.action),
            evaluator: non_empty(query.evaluator),
            occurred_at_start: parse_timestamp(query.occurred_at_start.as_deref())?,
            occurred_at_end: parse_timestamp(query.occurred_at_end.as_deref())?,
        })
        .await?;
    Ok(Json(envelope(GuardrailDecisionPageView {
        items: page.items.into_iter().map(decision_view).collect(),
        page: page.page,
        page_size: page.page_size,
        total: page.total,
    })))
}

fn policy_view(policy: EffectivePolicy) -> EffectiveGuardrailPolicyView {
    EffectiveGuardrailPolicyView {
        enabled: policy.enabled,
        mode: match policy.mode {
            gateway_guardrails::PolicyMode::Audit => "audit",
            gateway_guardrails::PolicyMode::Deny => "deny",
        }
        .to_string(),
        packs: policy
            .packs
            .into_iter()
            .map(|pack| pack.to_string())
            .collect(),
        managed_checks: policy.managed_checks,
        stream_buffer_bytes: policy.stream_buffer_bytes,
        scope: match policy.scope {
            gateway_guardrails::EffectiveScope::Global => "global".to_string(),
            gateway_guardrails::EffectiveScope::ModelRoute(route) => {
                format!("model_route:{route}")
            }
            gateway_guardrails::EffectiveScope::McpServer(server) => {
                format!("mcp_server:{server}")
            }
        },
    }
}

fn decision_view(record: GuardrailDecisionEventRecord) -> GuardrailDecisionView {
    GuardrailDecisionView {
        decision_id: record.decision_id.to_string(),
        request_id: record.request_id,
        mcp_tool_invocation_id: record.mcp_tool_invocation_id.map(|value| value.to_string()),
        phase: record.phase,
        effective_scope: record.effective_scope,
        evaluator: record.evaluator,
        managed_service: record.managed_service,
        pack_id: record.pack_id,
        rule_id: record.rule_id,
        action: record.action,
        reason_code: record.reason_code,
        latency_micros: record.latency_micros,
        failure_disposition: record.failure_disposition,
        transformed: record.transformed,
        content_hash: record.content_hash,
        occurred_at: record
            .occurred_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| record.occurred_at.to_string()),
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn parse_timestamp(value: Option<&str>) -> Result<Option<OffsetDateTime>, AppError> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            OffsetDateTime::parse(value, &Rfc3339).map_err(|error| {
                AppError(gateway_core::GatewayError::InvalidRequest(format!(
                    "invalid guardrail timestamp `{value}`: {error}"
                )))
            })
        })
        .transpose()
}

fn phase_name(phase: GuardPhase) -> &'static str {
    match phase {
        GuardPhase::Prompt => "prompt",
        GuardPhase::ModelResponse => "model_response",
        GuardPhase::GeneratedToolCall => "generated_tool_call",
        GuardPhase::McpCall => "mcp_call",
        GuardPhase::McpResult => "mcp_result",
        GuardPhase::HarnessPreTool => "harness_pre_tool",
    }
}

fn action_name(action: DecisionAction) -> &'static str {
    match action {
        DecisionAction::Allow => "allow",
        DecisionAction::Audit => "audit",
        DecisionAction::Deny => "deny",
        DecisionAction::Transformed => "transformed",
    }
}

fn failure_disposition_name(disposition: FailureDisposition) -> &'static str {
    match disposition {
        FailureDisposition::FailOpen => "fail_open",
        FailureDisposition::FailClosed => "fail_closed",
    }
}
