use std::time::Duration;

use futures_util::future::join_all;
use gateway_core::{GuardrailDecisionEventRecord, GuardrailDecisionRepository};
use gateway_guardrails::{
    DecisionAction, EffectiveScope, FailureDisposition, GuardPhase, GuardrailEvaluation,
    ManagedService,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::http::state::AppState;

pub async fn record_guardrail_evaluation(
    state: &AppState,
    request_id: Option<&str>,
    mcp_tool_invocation_id: Option<Uuid>,
    evaluation: &GuardrailEvaluation,
) {
    let mut records = Vec::with_capacity(evaluation.decisions.len());
    for decision in &evaluation.decisions {
        let phase = phase_name(decision.phase);
        let action = action_name(decision.action);
        let failure_disposition = decision.failure_disposition.map(failure_disposition_name);
        state.metrics.record_guardrail_decision(
            phase,
            action,
            &decision.evaluator,
            failure_disposition,
            decision.latency_micros,
        );
        tracing::info!(
            decision_id = %decision.decision_id,
            request_id,
            guardrail_phase = phase,
            guardrail_scope = %scope_name(&decision.scope),
            evaluator = %decision.evaluator,
            managed_service = decision.managed_service.map(managed_service_name),
            pack_id = decision.matched_rule.as_ref().map(|rule| rule.pack_id.as_str()),
            rule_id = decision.matched_rule.as_ref().map(|rule| rule.rule_id.as_str()),
            action,
            reason_code = %decision.reason_code,
            latency_micros = decision.latency_micros,
            failure_disposition,
            transformed = decision.transformed,
            content_hash = %decision.content_hash,
            "guardrail decision"
        );
        records.push(GuardrailDecisionEventRecord {
            decision_id: decision.decision_id.as_uuid(),
            request_id: request_id.map(str::to_string),
            mcp_tool_invocation_id,
            phase: phase.to_string(),
            effective_scope: scope_name(&decision.scope),
            evaluator: decision.evaluator.clone(),
            managed_service: decision
                .managed_service
                .map(managed_service_name)
                .map(str::to_string),
            pack_id: decision
                .matched_rule
                .as_ref()
                .map(|rule| rule.pack_id.clone()),
            rule_id: decision
                .matched_rule
                .as_ref()
                .map(|rule| rule.rule_id.clone()),
            action: action.to_string(),
            reason_code: decision.reason_code.to_string(),
            latency_micros: i64::try_from(decision.latency_micros).unwrap_or(i64::MAX),
            failure_disposition: failure_disposition.map(str::to_string),
            transformed: decision.transformed,
            content_hash: decision.content_hash.clone(),
            occurred_at: OffsetDateTime::now_utc(),
        });
    }

    let writes = records
        .iter()
        .map(|record| state.store.insert_guardrail_decision(record));
    match tokio::time::timeout(Duration::from_millis(250), join_all(writes)).await {
        Ok(results) => {
            for (record, result) in records.iter().zip(results) {
                if let Err(error) = result {
                    tracing::warn!(
                        decision_id = %record.decision_id,
                        error = %error,
                        "failed to persist guardrail decision"
                    );
                }
            }
        }
        Err(_) => tracing::warn!(
            decision_count = records.len(),
            persistence_deadline_ms = 250,
            "guardrail decision persistence deadline exceeded"
        ),
    }
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

fn scope_name(scope: &EffectiveScope) -> String {
    match scope {
        EffectiveScope::Global => "global".to_string(),
        EffectiveScope::ModelRoute(route) => format!("model_route:{route}"),
        EffectiveScope::McpServer(server) => format!("mcp_server:{server}"),
    }
}

fn managed_service_name(service: ManagedService) -> &'static str {
    match service {
        ManagedService::AmazonBedrock => "amazon_bedrock",
        ManagedService::GoogleModelArmor => "google_model_armor",
    }
}

fn failure_disposition_name(disposition: FailureDisposition) -> &'static str {
    match disposition {
        FailureDisposition::FailOpen => "fail_open",
        FailureDisposition::FailClosed => "fail_closed",
    }
}
