use super::*;

pub(crate) struct PassiveRequestRecord<'a> {
    pub auth: &'a AuthenticatedApiKey,
    pub request_id: &'a str,
    pub request_log_id: Option<Uuid>,
    pub harness_key: &'a str,
    pub harness_label: &'a str,
    pub metadata: &'a PassiveRequestMetadata,
    pub response_body: Option<&'a Value>,
    pub occurred_at: OffsetDateTime,
    pub completed_at: OffsetDateTime,
    pub terminal_success: Option<bool>,
    pub payload_truncated: bool,
    pub requested_model_key: &'a str,
    pub operation: &'a str,
    pub request_tags: Value,
    pub boundary_group_key: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedPassiveRequestRecord {
    auth: AuthenticatedApiKey,
    request_id: String,
    request_log_id: Option<Uuid>,
    harness_key: String,
    harness_label: String,
    metadata: PassiveRequestMetadata,
    response_payload_available: bool,
    requested_model_key: String,
    operation: String,
    request_tags: Value,
    boundary_group_key: String,
    observations: Vec<InferredObservation>,
    occurred_at: OffsetDateTime,
    completed_at: OffsetDateTime,
    terminal_success: Option<bool>,
    payload_truncated: bool,
}

impl PassiveRequestRecord<'_> {
    pub(crate) fn prepare(&self) -> PreparedPassiveRequestRecord {
        let mut observations = observations_for_response(&ResponseObservationInput {
            request_id: self.request_id,
            metadata: self.metadata,
            response_body: self.response_body,
            occurred_at: self.occurred_at,
            completed_at: self.completed_at,
        });
        scope_file_identifiers(
            &mut observations,
            usage_ownership_scope_key(self.auth).ok().as_deref(),
        );
        PreparedPassiveRequestRecord {
            auth: self.auth.clone(),
            request_id: self.request_id.to_string(),
            request_log_id: self.request_log_id,
            harness_key: self.harness_key.to_string(),
            harness_label: self.harness_label.to_string(),
            requested_model_key: self.requested_model_key.to_string(),
            operation: self.operation.to_string(),
            request_tags: self.request_tags.clone(),
            metadata: self.metadata.clone(),
            response_payload_available: self.response_body.is_some(),
            observations,
            boundary_group_key: self.boundary_group_key.to_string(),
            occurred_at: self.occurred_at,
            completed_at: self.completed_at,
            terminal_success: self.terminal_success,
            payload_truncated: self.payload_truncated,
        }
    }
}

struct PassiveRequestOwnership {
    ownership_scope_key: String,
    analytics_team_id: Option<Uuid>,
    session_source_id: Option<Uuid>,
    normalized_session_id: Option<String>,
}

struct PreparedSessionInput {
    request_link: AgentSessionRequestLinkRecord,
    observation_set: AgentObservationSetRecord,
    truncated_observation_set: AgentObservationSetRecord,
}

pub(crate) async fn record_prepared_passive_request<S>(
    store: &S,
    mut input: PreparedPassiveRequestRecord,
    desired_versions: &AgentAnalysisDesiredVersions,
) -> Result<Uuid, GatewayError>
where
    S: AgentSessionAnalysisRepository + BudgetRepository + IdentityRepository + Sync,
{
    let mut ownership = resolve_passive_request_ownership(store, &input).await?;
    let session = acquire_session_window(store, &input, &ownership, desired_versions).await?;
    let session_input = prepare_session_input(store, &mut input, &mut ownership, &session).await?;
    append_passive_request(
        store,
        &input,
        &ownership,
        session,
        session_input,
        desired_versions,
    )
    .await
}

async fn resolve_passive_request_ownership<S>(
    store: &S,
    input: &PreparedPassiveRequestRecord,
) -> Result<PassiveRequestOwnership, GatewayError>
where
    S: AgentSessionAnalysisRepository + IdentityRepository + Sync,
{
    let ownership_scope_key = usage_ownership_scope_key(&input.auth)?;
    let analytics_team_id = if let Some(user_id) = input.auth.owner_user_id {
        store
            .get_team_membership_for_user(user_id)
            .await?
            .map(|membership| membership.team_id)
    } else {
        input.auth.owner_team_id
    };
    let normalized_session_id = input.metadata.normalized_session_id(&ownership_scope_key);
    // The source stays keyed by harness: the store accepts one harness per source, and harnesses
    // that share a session namespace still store the same `normalized_session_id`.
    let session_source = if let Some(normalized_session_id) = &normalized_session_id {
        let now = input.completed_at;
        let agent_session_source_id = stable_uuid(
            SESSION_SOURCE_ID_NAMESPACE,
            &json!({
                "scope": ownership_scope_key,
                "adapter": input.harness_key,
                "session": normalized_session_id,
            })
            .to_string(),
        );
        let source_team_id = store
            .load_agent_session_source(agent_session_source_id)
            .await?
            .map_or(analytics_team_id, |source| source.team_id);
        Some(
            store
                .upsert_agent_session_source(&AgentSessionSourceRecord {
                    agent_session_source_id,
                    ownership_scope_key: ownership_scope_key.clone(),
                    api_key_id: input.auth.id,
                    user_id: input.auth.owner_user_id,
                    team_id: source_team_id,
                    service_account_id: input.auth.owner_service_account_id,
                    actor_user_id: None,
                    adapter_namespace: input.harness_key.to_string(),
                    normalized_session_id: normalized_session_id.clone(),
                    adapter_version: input.metadata.adapter_version.clone(),
                    source_provenance: input
                        .metadata
                        .session_source
                        .clone()
                        .unwrap_or_else(|| "unreported".to_string()),
                    harness_key: input.harness_key.to_string(),
                    harness_label: input.harness_label.to_string(),
                    first_seen_at: input.occurred_at,
                    last_seen_at: input.completed_at,
                    created_at: now,
                    updated_at: now,
                })
                .await?,
        )
    } else {
        None
    };
    Ok(PassiveRequestOwnership {
        ownership_scope_key,
        analytics_team_id,
        session_source_id: session_source
            .as_ref()
            .map(|record| record.agent_session_source_id),
        normalized_session_id,
    })
}

async fn acquire_session_window<S>(
    store: &S,
    input: &PreparedPassiveRequestRecord,
    ownership: &PassiveRequestOwnership,
    desired_versions: &AgentAnalysisDesiredVersions,
) -> Result<AgentSessionRecord, GatewayError>
where
    S: gateway_core::AgentSessionTraceRepository + Sync,
{
    for _ in 0..MAX_SESSION_WINDOW_CAS_ATTEMPTS {
        let session = if let Some(session) = store
            .get_open_agent_session(
                &ownership.ownership_scope_key,
                ownership.session_source_id,
                &input.harness_key,
                &input.boundary_group_key,
            )
            .await?
        {
            session
        } else {
            insert_or_load_session_window(store, input, ownership, "open agent session disappeared")
                .await?
        };
        if input.occurred_at - session.input_watermark_at < SESSION_IDLE_GAP {
            return Ok(session);
        }
        let expected_input_watermark_at = session.input_watermark_at;
        let mut finalized_session = session;
        finalized_session.lifecycle = SessionLifecycleState::Finalized;
        finalized_session.ended_at = Some(expected_input_watermark_at);
        finalized_session.input_watermark_at = input.completed_at;
        finalized_session.finalized_reason = Some("idle_gap".to_string());
        finalized_session.updated_at = input.completed_at;
        let analysis = super::worker::analysis_request(
            finalized_session.agent_session_id,
            "session_finalized",
            &expected_input_watermark_at
                .unix_timestamp_nanos()
                .to_string(),
            input.completed_at,
            desired_versions,
        );
        store
            .finalize_agent_session_if_unchanged(
                &finalized_session,
                expected_input_watermark_at,
                Some(&analysis),
            )
            .await?;
        // Both a lost finalisation race and a lost creation race can expose another idle window.
    }
    Err(GatewayError::Internal(
        "agent session window changed too often while enforcing the idle gap".to_string(),
    ))
}

async fn insert_or_load_session_window<S>(
    store: &S,
    input: &PreparedPassiveRequestRecord,
    ownership: &PassiveRequestOwnership,
    disappeared_message: &'static str,
) -> Result<AgentSessionRecord, GatewayError>
where
    S: gateway_core::AgentSessionTraceRepository + Sync,
{
    let confidence = if ownership.session_source_id.is_some() {
        Confidence::High
    } else {
        Confidence::Medium
    };
    let session = AgentSessionRecord {
        agent_session_id: stable_uuid(
            SESSION_ID_NAMESPACE,
            &json!({
                "scope": ownership.ownership_scope_key,
                "session": ownership.session_source_id,
                "harness": input.harness_key,
                "boundary": input.boundary_group_key,
                "first_request": input.request_id,
            })
            .to_string(),
        ),
        agent_session_source_id: ownership.session_source_id,
        ownership_scope_key: ownership.ownership_scope_key.clone(),
        api_key_id: input.auth.id,
        user_id: input.auth.owner_user_id,
        team_id: ownership.analytics_team_id,
        service_account_id: input.auth.owner_service_account_id,
        actor_user_id: None,
        harness_key: input.harness_key.clone(),
        requested_model_key: input.requested_model_key.clone(),
        operation: input.operation.clone(),
        caller_class: caller_class(&input.auth).to_string(),
        request_tags: input.request_tags.clone(),
        boundary_group_key: input.boundary_group_key.clone(),
        boundary_policy_version: SESSION_BOUNDARY_POLICY_VERSION.to_string(),
        lifecycle: SessionLifecycleState::Open,
        boundary_confidence: confidence,
        started_at: input.occurred_at,
        ended_at: None,
        input_watermark_at: input.completed_at,
        finalized_reason: None,
        created_at: input.completed_at,
        updated_at: input.completed_at,
    };
    if store.insert_agent_session_if_absent(&session).await? {
        Ok(session)
    } else {
        store
            .get_open_agent_session(
                &ownership.ownership_scope_key,
                ownership.session_source_id,
                &input.harness_key,
                &input.boundary_group_key,
            )
            .await?
            .ok_or_else(|| GatewayError::Internal(disappeared_message.to_string()))
    }
}

async fn prepare_session_input<S>(
    store: &S,
    input: &mut PreparedPassiveRequestRecord,
    ownership: &mut PassiveRequestOwnership,
    session: &AgentSessionRecord,
) -> Result<PreparedSessionInput, GatewayError>
where
    S: BudgetRepository + Sync,
{
    let usage_event_id = store
        .get_usage_ledger_by_request_and_scope(&input.request_id, &ownership.ownership_scope_key)
        .await?
        .map(|usage| usage.usage_event_id);
    let terminal_success = if input.terminal_success == Some(false) && usage_event_id.is_some() {
        None
    } else {
        input.terminal_success
    };
    let mut limitations = Vec::new();
    if ownership.session_source_id.is_none() {
        limitations.push(LimitationCode::SessionUnobserved);
    }
    if input.metadata.session_limitation.is_some() {
        limitations.push(LimitationCode::RequestIncomplete);
    }
    if usage_event_id.is_none() {
        limitations.push(LimitationCode::UsageUnavailable);
    }
    let (execution_id, parent_execution_id) = input
        .metadata
        .hashed_lineage(&ownership.ownership_scope_key);
    let request_link = AgentSessionRequestLinkRecord {
        agent_session_id: session.agent_session_id,
        request_id: input.request_id.clone(),
        request_log_id: input.request_log_id,
        usage_event_id,
        ordinal: 0,
        execution_id,
        parent_execution_id,
        normalized_session_id: ownership.normalized_session_id.take(),
        correlation_confidence: session.boundary_confidence,
        limitation_codes: limitations,
        occurred_at: input.occurred_at,
        completed_at: Some(input.completed_at),
        terminal_success,
    };
    let session_correlation = if input.metadata.external_session_id.is_some() {
        "observed"
    } else {
        input
            .metadata
            .session_limitation
            .map_or("unobserved", SessionCorrelationLimitation::as_str)
    };
    let coverage = json!({
        "request_metadata": input.metadata.body_inspected,
        "session_correlation": session_correlation,
        "response_payload": input.response_payload_available,
        "response_payload_truncated": input.payload_truncated,
        "nested_facts_truncated": false,
    });
    let observation_set_id = observation_set_id(
        session.agent_session_id,
        &input.request_id,
        input.completed_at,
    );
    let mut observations = std::mem::take(&mut input.observations);
    assign_observation_ids(session.agent_session_id, &mut observations);
    let observation_set = AgentObservationSetRecord {
        observation_set_id,
        agent_session_id: session.agent_session_id,
        parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        source_watermark_at: input.completed_at,
        coverage: coverage.clone(),
        created_at: input.completed_at,
        observations,
    };
    let mut truncated_coverage = coverage;
    truncated_coverage["nested_facts_truncated"] = Value::Bool(true);
    let mut truncated_observations = observation_set.observations.clone();
    truncate_nested_facts(&mut truncated_observations);
    assign_observation_ids(session.agent_session_id, &mut truncated_observations);
    let truncated_observation_set = AgentObservationSetRecord {
        coverage: truncated_coverage,
        observations: truncated_observations,
        observation_set_id,
        agent_session_id: session.agent_session_id,
        parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        source_watermark_at: input.completed_at,
        created_at: input.completed_at,
    };
    Ok(PreparedSessionInput {
        request_link,
        observation_set,
        truncated_observation_set,
    })
}

async fn append_passive_request<S>(
    store: &S,
    input: &PreparedPassiveRequestRecord,
    ownership: &PassiveRequestOwnership,
    mut session: AgentSessionRecord,
    prepared: PreparedSessionInput,
    desired_versions: &AgentAnalysisDesiredVersions,
) -> Result<Uuid, GatewayError>
where
    S: gateway_core::AgentSessionTraceRepository + Sync,
{
    let PreparedSessionInput {
        mut request_link,
        mut observation_set,
        mut truncated_observation_set,
    } = prepared;
    let mut append_attempts = 0_usize;
    let append_result = loop {
        let analysis = super::worker::analysis_request(
            session.agent_session_id,
            "new_input",
            &input.request_id,
            input.completed_at,
            desired_versions,
        );
        let session_input = gateway_core::AgentSessionInput {
            request: Some(&request_link),
            observations: Some(gateway_core::AgentSessionObservationInput {
                set: &observation_set,
                truncated_set: &truncated_observation_set,
                maximum_nested_facts: MAX_AGENT_SESSION_NESTED_FACTS,
            }),
            analysis: Some(&analysis),
        };
        match store.append_agent_session_input(session_input).await {
            Ok(result) => break result,
            Err(StoreError::AgentSessionWindowClosed(closed_session_id))
                if closed_session_id == session.agent_session_id.to_string()
                    && append_attempts < MAX_SESSION_WINDOW_CAS_ATTEMPTS =>
            {
                append_attempts = append_attempts.saturating_add(1);
                session = acquire_session_window(store, input, ownership, desired_versions).await?;
                request_link.agent_session_id = session.agent_session_id;
                request_link.correlation_confidence = session.boundary_confidence;
                rebind_observation_set(
                    &mut observation_set,
                    session.agent_session_id,
                    &input.request_id,
                );
                rebind_observation_set(
                    &mut truncated_observation_set,
                    session.agent_session_id,
                    &input.request_id,
                );
            }
            Err(error) => return Err(error.into()),
        }
    };

    let stored_coverage = if append_result.nested_facts_truncated {
        truncated_observation_set.coverage
    } else {
        observation_set.coverage
    };
    if let Some(request_log_id) = input.request_log_id {
        store
            .link_request_log_to_agent_session(&AgentRequestLogLinkRecord {
                request_log_id,
                agent_session_source_id: ownership.session_source_id,
                agent_session_id: session.agent_session_id,
                analysis_source: "passive".to_string(),
                coverage: stored_coverage,
            })
            .await?;
    }
    Ok(session.agent_session_id)
}

fn rebind_observation_set(set: &mut AgentObservationSetRecord, session_id: Uuid, request_id: &str) {
    set.agent_session_id = session_id;
    set.observation_set_id = observation_set_id(session_id, request_id, set.source_watermark_at);
    assign_observation_ids(session_id, &mut set.observations);
}

fn caller_class(auth: &AuthenticatedApiKey) -> &'static str {
    if auth.owner_service_account_id.is_some() {
        "service_account"
    } else if auth.owner_user_id.is_some() {
        "user"
    } else {
        "api_key"
    }
}

pub(crate) fn session_boundary_group_key(tags: &RequestTags) -> String {
    agent_session_analysis::identity::hashing::boundary_group_key(
        tags.service.as_deref(),
        tags.component.as_deref(),
        tags.env.as_deref(),
        tags.bespoke
            .iter()
            .map(|tag| (tag.key.as_str(), tag.value.as_str())),
    )
}

#[cfg(test)]
mod tests;
