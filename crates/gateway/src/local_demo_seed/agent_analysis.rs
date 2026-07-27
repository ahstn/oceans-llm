use anyhow::Context;
use gateway_core::{
    AgentObservationSetRecord, AgentRequestLogLinkRecord, AgentSessionAnalysisRepository,
    AgentSessionRecord, AgentTaskRequestLinkRecord, AgentTaskWindowRecord, ApiKeyRecord,
    BoundedObservationFacts, Confidence, EvidenceQuality, InferredObservation,
    InferredObservationKind, LimitationCode, RequestTags, TaskLifecycleState,
};
use gateway_service::{desired_versions, enqueue_agent_analysis};
use gateway_store::AnyStore;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    DemoPayloadProfile, LocalDemoRequestFixture, agent_session_fixtures, demo_fixture_occurred_at,
    demo_request_log_uuid, demo_usage_event_uuid, local_demo_uuid, usage,
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
    seeded_at: OffsetDateTime,
    response_payload_truncated: bool,
) -> anyhow::Result<()> {
    let session_request = agent_session_fixtures::request_metadata(fixture);
    let session = session_request.map(|request| request.session);
    let task_key = session.map_or(fixture.request_id, |session| session.key);
    let agent_task_id = local_demo_uuid("agent_task", task_key);
    let existing = store
        .load_agent_task_trace(agent_task_id)
        .await
        .context("failed checking for an existing demo agent task")?;
    if existing.as_ref().is_some_and(|trace| {
        trace
            .requests
            .iter()
            .any(|request| request.request_id == fixture.request_id)
    }) {
        return Ok(());
    }

    let versions = desired_versions();
    let (started_at, ended_at) = task_bounds(fixture, seeded_at);
    let agent_session_id = session.map(|session| local_demo_uuid("agent_session", session.key));
    if let Some(session) = session {
        seed_demo_session(
            store,
            session,
            api_key,
            team_id,
            ownership_scope_key,
            started_at,
            ended_at,
        )
        .await?;
    }

    if existing.is_none() {
        let requested_model_key = session.map_or(fixture.resolved_model_key, |session| {
            session_fixtures(session)
                .next()
                .expect("demo session fixture exists")
                .resolved_model_key
        });
        let task = AgentTaskWindowRecord {
            agent_task_id,
            agent_session_id,
            ownership_scope_key: ownership_scope_key.to_string(),
            api_key_id: api_key.id,
            user_id: api_key.owner_user_id,
            team_id,
            service_account_id: api_key.owner_service_account_id,
            actor_user_id: None,
            harness_key: if session.is_some() {
                "codex"
            } else {
                "opencode"
            }
            .to_string(),
            requested_model_key: requested_model_key.to_string(),
            operation: "chat_completions".to_string(),
            caller_class: if api_key.owner_service_account_id.is_some() {
                "service_account"
            } else {
                "user"
            }
            .to_string(),
            request_tags: serde_json::to_value(request_tags)
                .context("failed serializing demo agent task request tags")?,
            boundary_group_key: local_demo_uuid("agent_task_boundary", task_key).to_string(),
            boundary_policy_version: versions.boundary_policy_version.clone(),
            lifecycle: TaskLifecycleState::Finalized,
            boundary_confidence: if session.is_some() {
                Confidence::High
            } else {
                Confidence::Medium
            },
            started_at,
            ended_at: Some(ended_at),
            input_watermark_at: ended_at,
            finalized_reason: Some("local_demo_seed".to_string()),
            created_at: ended_at,
            updated_at: ended_at,
        };
        store
            .insert_agent_task_if_absent(&task)
            .await
            .with_context(|| format!("failed inserting demo agent task `{task_key}`"))?;
    }

    let correlation_confidence = if session.is_some() {
        Confidence::High
    } else {
        Confidence::Medium
    };
    store
        .append_agent_task_request(&AgentTaskRequestLinkRecord {
            agent_task_id,
            request_id: fixture.request_id.to_string(),
            request_log_id: Some(demo_request_log_uuid(fixture.request_id)),
            usage_event_id: Some(demo_usage_event_uuid(fixture.request_id)),
            ordinal: 0,
            execution_id: None,
            parent_execution_id: None,
            normalized_session_id: session.map(|session| session.normalized_session_id.to_string()),
            correlation_confidence,
            limitation_codes: request_limitations(
                fixture,
                response_payload_truncated,
                session.is_none(),
            ),
            occurred_at: occurred_at - time::Duration::milliseconds(fixture.latency_ms.max(1)),
            completed_at: Some(occurred_at),
            terminal_success: Some(fixture.error_code.is_none() && fixture.status_code < 400),
        })
        .await
        .with_context(|| format!("failed linking demo agent task `{task_key}`"))?;

    let coverage = request_coverage(fixture, response_payload_truncated, session.is_some());
    store
        .link_request_log_to_agent_task(&AgentRequestLogLinkRecord {
            request_log_id: demo_request_log_uuid(fixture.request_id),
            agent_session_id,
            agent_task_id,
            analysis_source: "local_demo_seed".to_string(),
            coverage: coverage.clone(),
        })
        .await
        .with_context(|| format!("failed linking demo request log `{}`", fixture.request_id))?;

    let should_finalize_observations = session_request
        .map(|request| request.step + 1 == request.session.request_count)
        .unwrap_or(true);
    if !should_finalize_observations {
        return Ok(());
    }

    let observations = if let Some(session) = session {
        session_fixtures(session)
            .flat_map(|request| {
                demo_observations(
                    request,
                    demo_fixture_occurred_at(seeded_at, request),
                    &versions.observation_parser_version,
                )
            })
            .collect()
    } else {
        demo_observations(fixture, occurred_at, &versions.observation_parser_version)
    };
    let observation_set_id = local_demo_uuid("agent_observation_set", task_key);
    store
        .append_agent_observation_set(&AgentObservationSetRecord {
            observation_set_id,
            agent_task_id,
            parser_version: versions.observation_parser_version,
            source_watermark_at: ended_at,
            coverage,
            created_at: ended_at,
            observations,
        })
        .await
        .with_context(|| {
            format!("failed inserting observations for demo agent task `{task_key}`")
        })?;
    enqueue_agent_analysis(store, agent_task_id, "local_demo_seed", task_key, ended_at)
        .await
        .with_context(|| format!("failed queueing demo agent task `{task_key}`"))?;

    Ok(())
}

async fn seed_demo_session(
    store: &AnyStore,
    session: &'static agent_session_fixtures::DemoAgentSessionFixture,
    api_key: &ApiKeyRecord,
    team_id: Option<Uuid>,
    ownership_scope_key: &str,
    first_seen_at: OffsetDateTime,
    last_seen_at: OffsetDateTime,
) -> anyhow::Result<()> {
    let agent_session_id = local_demo_uuid("agent_session", session.key);
    store
        .upsert_agent_session(&AgentSessionRecord {
            agent_session_id,
            ownership_scope_key: ownership_scope_key.to_string(),
            api_key_id: api_key.id,
            user_id: api_key.owner_user_id,
            team_id,
            service_account_id: api_key.owner_service_account_id,
            actor_user_id: None,
            normalized_session_id: session.normalized_session_id.to_string(),
            adapter_namespace: "codex".to_string(),
            adapter_version: "codex-v1".to_string(),
            source_provenance: "body:client_metadata.session_id".to_string(),
            harness_key: "codex".to_string(),
            harness_label: "Codex".to_string(),
            first_seen_at,
            last_seen_at,
            created_at: first_seen_at,
            updated_at: last_seen_at,
        })
        .await
        .with_context(|| format!("failed inserting demo agent session `{}`", session.key))?;
    Ok(())
}

fn task_bounds(
    fixture: &LocalDemoRequestFixture,
    seeded_at: OffsetDateTime,
) -> (OffsetDateTime, OffsetDateTime) {
    if let Some(request) = agent_session_fixtures::request_metadata(fixture) {
        let first = session_fixtures(request.session)
            .next()
            .expect("demo session fixture exists");
        let first_completed_at = demo_fixture_occurred_at(seeded_at, first);
        session_fixtures(request.session).skip(1).fold(
            (
                first_completed_at - time::Duration::milliseconds(first.latency_ms.max(1)),
                first_completed_at,
            ),
            |(started_at, ended_at), request| {
                let completed_at = demo_fixture_occurred_at(seeded_at, request);
                (
                    started_at.min(
                        completed_at - time::Duration::milliseconds(request.latency_ms.max(1)),
                    ),
                    ended_at.max(completed_at),
                )
            },
        )
    } else {
        let completed_at = demo_fixture_occurred_at(seeded_at, fixture);
        (
            completed_at - time::Duration::milliseconds(fixture.latency_ms.max(1)),
            completed_at,
        )
    }
}

fn session_fixtures(
    session: &'static agent_session_fixtures::DemoAgentSessionFixture,
) -> impl Iterator<Item = &'static LocalDemoRequestFixture> {
    agent_session_fixtures::session_requests(session)
}

fn request_limitations(
    fixture: &LocalDemoRequestFixture,
    response_payload_truncated: bool,
    session_unobserved: bool,
) -> Vec<LimitationCode> {
    let mut limitations = Vec::new();
    if session_unobserved {
        limitations.push(LimitationCode::SessionUnobserved);
    }
    if fixture.payload_profile == DemoPayloadProfile::SummaryOnly {
        limitations.push(LimitationCode::PayloadUnavailable);
    }
    if response_payload_truncated {
        limitations.push(LimitationCode::PayloadTruncated);
    }
    limitations
}

fn request_coverage(
    fixture: &LocalDemoRequestFixture,
    response_payload_truncated: bool,
    session_observed: bool,
) -> serde_json::Value {
    json!({
        "request_metadata": true,
        "session_correlation": if session_observed { "observed" } else { "unobserved" },
        "response_payload": fixture.payload_profile != DemoPayloadProfile::SummaryOnly,
        "response_payload_truncated": response_payload_truncated,
    })
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
    let session_request = agent_session_fixtures::request_metadata(fixture);
    let tool_name = session_request.map(|request| request.tool_name.to_string());
    let kind = session_request
        .map(|request| request.observation_kind)
        .unwrap_or(InferredObservationKind::ToolCallClassified);
    vec![InferredObservation {
        observation_id: local_demo_uuid("agent_observation", fixture.request_id),
        kind,
        source_request_id: fixture.request_id.to_string(),
        parser_version: parser_version.to_string(),
        evidence: EvidenceQuality::InferredHigh,
        occurred_at,
        facts: BoundedObservationFacts {
            message_count: Some(2),
            prompt_bytes: Some(fixture.prompt.len() as u64),
            supplied_tool_count: u32::try_from(supplied_tool_count).ok(),
            tool_schema_bytes: tool_name.as_ref().map(|_| 420),
            tool_schema_token_estimate: tool_name.as_ref().map(|_| 105),
            tool_name,
            ..BoundedObservationFacts::default()
        },
        limitations: vec![LimitationCode::SemanticVerificationUnavailable],
    }]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn demo_sessions_have_complete_ordered_tool_using_requests() {
        for session in agent_session_fixtures::DEMO_AGENT_SESSIONS {
            let fixtures = session_fixtures(session).collect::<Vec<_>>();
            assert_eq!(fixtures.len(), session.request_count);

            let mut tools = BTreeSet::new();
            let mut file_tool_calls = 0;
            let first = fixtures.first().expect("demo session has requests");
            for (expected_step, fixture) in fixtures.iter().enumerate() {
                let request = agent_session_fixtures::request_metadata(fixture)
                    .expect("session request should have metadata");
                assert_eq!(request.session.key, session.key);
                assert_eq!(request.step, expected_step);
                assert!(tools.insert(request.tool_name));
                assert_eq!(fixture.api_key_public_id, first.api_key_public_id);
                assert_eq!(fixture.service, first.service);
                assert_eq!(fixture.component, first.component);
                assert_eq!(fixture.bespoke_value, first.bespoke_value);
                assert!(fixture.error_code.is_none());
                assert!(fixture.prompt_tokens.is_some());
                assert!(fixture.completion_tokens.is_some());
                assert_ne!(fixture.payload_profile, DemoPayloadProfile::SummaryOnly);
                assert_eq!(
                    usage::demo_tool_cardinality(fixture).invoked_tool_count,
                    Some(1)
                );
                if matches!(
                    request.observation_kind,
                    InferredObservationKind::FileReadSuspected
                        | InferredObservationKind::FileCreateSuspected
                        | InferredObservationKind::FileEditSuspected
                        | InferredObservationKind::FileOverwriteSuspected
                ) {
                    file_tool_calls += 1;
                }
            }
            assert_eq!(file_tool_calls, session.expected_file_tool_calls);
        }
    }

    #[test]
    fn route_migration_session_exercises_long_read_write_workflow() {
        let session = agent_session_fixtures::DEMO_AGENT_SESSIONS
            .iter()
            .find(|session| session.key == "repository-route-migration")
            .expect("route migration session exists");
        assert!(session.request_count > 8);
        assert!(session.expected_file_tool_calls > 3);
    }
}
