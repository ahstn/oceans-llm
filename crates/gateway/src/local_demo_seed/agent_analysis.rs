use anyhow::Context;
use gateway_core::{
    AgentObservationSetRecord, AgentRequestLogLinkRecord, AgentSessionAnalysisRepository,
    AgentTaskRequestLinkRecord, AgentTaskWindowRecord, ApiKeyRecord, BoundedObservationFacts,
    Confidence, EvidenceQuality, InferredObservation, InferredObservationKind, LimitationCode,
    RequestTags, TaskLifecycleState,
};
use gateway_service::{desired_versions, enqueue_agent_analysis};
use gateway_store::AnyStore;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    DemoPayloadProfile, LocalDemoRequestFixture, demo_request_log_uuid, demo_usage_event_uuid,
    local_demo_uuid, usage,
};

#[allow(clippy::too_many_arguments)]
pub(super) async fn seed_demo_agent_task(
    store: &AnyStore,
    fixture: &LocalDemoRequestFixture,
    api_key: &ApiKeyRecord,
    team_id: Option<Uuid>,
    ownership_scope_key: &str,
    request_tags: &RequestTags,
    occurred_at: OffsetDateTime,
    response_payload_truncated: bool,
) -> anyhow::Result<()> {
    let agent_task_id = local_demo_uuid("agent_task", fixture.request_id);
    if store
        .load_agent_task_trace(agent_task_id)
        .await
        .context("failed checking for an existing demo agent task")?
        .is_some()
    {
        return Ok(());
    }

    let versions = desired_versions();
    let started_at = occurred_at - time::Duration::milliseconds(fixture.latency_ms.max(1));
    let request_tags = serde_json::to_value(request_tags)
        .context("failed serializing demo agent task request tags")?;
    let task = AgentTaskWindowRecord {
        agent_task_id,
        agent_session_id: None,
        ownership_scope_key: ownership_scope_key.to_string(),
        api_key_id: api_key.id,
        user_id: api_key.owner_user_id,
        team_id,
        service_account_id: api_key.owner_service_account_id,
        actor_user_id: None,
        harness_key: "opencode".to_string(),
        requested_model_key: fixture.resolved_model_key.to_string(),
        operation: "chat_completions".to_string(),
        caller_class: if api_key.owner_service_account_id.is_some() {
            "service_account"
        } else {
            "user"
        }
        .to_string(),
        request_tags,
        boundary_group_key: local_demo_uuid("agent_task_boundary", fixture.request_id).to_string(),
        boundary_policy_version: versions.boundary_policy_version.clone(),
        lifecycle: TaskLifecycleState::Finalized,
        boundary_confidence: Confidence::Medium,
        started_at,
        ended_at: Some(occurred_at),
        input_watermark_at: occurred_at,
        finalized_reason: Some("local_demo_seed".to_string()),
        created_at: occurred_at,
        updated_at: occurred_at,
    };
    store
        .insert_agent_task_if_absent(&task)
        .await
        .with_context(|| format!("failed inserting demo agent task `{}`", fixture.request_id))?;

    let mut limitation_codes = vec![LimitationCode::SessionUnobserved];
    if fixture.payload_profile == DemoPayloadProfile::SummaryOnly {
        limitation_codes.push(LimitationCode::PayloadUnavailable);
    }
    if response_payload_truncated {
        limitation_codes.push(LimitationCode::PayloadTruncated);
    }
    store
        .append_agent_task_request(&AgentTaskRequestLinkRecord {
            agent_task_id,
            request_id: fixture.request_id.to_string(),
            request_log_id: Some(demo_request_log_uuid(fixture.request_id)),
            usage_event_id: Some(demo_usage_event_uuid(fixture.request_id)),
            ordinal: 0,
            execution_id: None,
            parent_execution_id: None,
            normalized_session_id: None,
            correlation_confidence: Confidence::Medium,
            limitation_codes,
            occurred_at: started_at,
            completed_at: Some(occurred_at),
            terminal_success: Some(fixture.error_code.is_none() && fixture.status_code < 400),
        })
        .await
        .with_context(|| format!("failed linking demo agent task `{}`", fixture.request_id))?;

    let response_payload_available = fixture.payload_profile != DemoPayloadProfile::SummaryOnly;
    let coverage = json!({
        "request_metadata": true,
        "session_correlation": "unobserved",
        "response_payload": response_payload_available,
        "response_payload_truncated": response_payload_truncated,
    });
    let observations =
        demo_observations(fixture, occurred_at, &versions.observation_parser_version);
    store
        .append_agent_observation_set(&AgentObservationSetRecord {
            observation_set_id: local_demo_uuid("agent_observation_set", fixture.request_id),
            agent_task_id,
            parser_version: versions.observation_parser_version,
            source_watermark_at: occurred_at,
            coverage: coverage.clone(),
            created_at: occurred_at,
            observations,
        })
        .await
        .with_context(|| {
            format!(
                "failed inserting observations for demo agent task `{}`",
                fixture.request_id
            )
        })?;
    store
        .link_request_log_to_agent_task(&AgentRequestLogLinkRecord {
            request_log_id: demo_request_log_uuid(fixture.request_id),
            agent_session_id: None,
            agent_task_id,
            analysis_source: "local_demo_seed".to_string(),
            coverage,
        })
        .await
        .with_context(|| format!("failed linking demo request log `{}`", fixture.request_id))?;
    enqueue_agent_analysis(
        store,
        agent_task_id,
        "local_demo_seed",
        fixture.request_id,
        occurred_at,
    )
    .await
    .with_context(|| format!("failed queueing demo agent task `{}`", fixture.request_id))?;

    Ok(())
}

fn demo_observations(
    fixture: &LocalDemoRequestFixture,
    occurred_at: OffsetDateTime,
    parser_version: &str,
) -> Vec<InferredObservation> {
    let cardinality = usage::demo_tool_cardinality(fixture);
    let Some(supplied_tool_count) = cardinality.exposed_tool_count else {
        return Vec::new();
    };

    vec![InferredObservation {
        observation_id: local_demo_uuid("agent_observation", fixture.request_id),
        kind: InferredObservationKind::ToolCallClassified,
        source_request_id: fixture.request_id.to_string(),
        parser_version: parser_version.to_string(),
        evidence: EvidenceQuality::InferredHigh,
        occurred_at,
        facts: BoundedObservationFacts {
            message_count: Some(2),
            prompt_bytes: Some(fixture.prompt.len() as u64),
            supplied_tool_count: u32::try_from(supplied_tool_count).ok(),
            ..BoundedObservationFacts::default()
        },
        limitations: vec![LimitationCode::SemanticVerificationUnavailable],
    }]
}
