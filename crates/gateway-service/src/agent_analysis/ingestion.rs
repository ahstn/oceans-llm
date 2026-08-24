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
        let mut observations = observations_for_response(self);
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
pub(super) fn scope_file_identifiers(
    observations: &mut [InferredObservation],
    ownership_scope: Option<&str>,
) {
    for observation in observations {
        if let Some(ownership_scope) = ownership_scope {
            for interaction in &mut observation.facts.file_interactions {
                interaction.opaque_file_id = hash_identifier(
                    &json!({
                        "ownership_scope": ownership_scope,
                        "file_identifier": interaction.opaque_file_id,
                    })
                    .to_string(),
                );
            }
        } else {
            observation.facts.file_interactions.clear();
        }
    }
}

pub(crate) async fn record_prepared_passive_request<S>(
    store: &S,
    input: PreparedPassiveRequestRecord,
    desired_versions: &AgentAnalysisDesiredVersions,
) -> Result<Uuid, GatewayError>
where
    S: AgentSessionAnalysisRepository + BudgetRepository + IdentityRepository + Sync,
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
    let normalized_session_id = input
        .metadata
        .external_session_id
        .as_deref()
        .map(|candidate| {
            hash_lineage_candidate(&ownership_scope_key, &input.harness_key, candidate)
        });
    let session_source = if input.metadata.external_session_id.is_some() {
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
                    normalized_session_id: normalized_session_id
                        .clone()
                        .expect("session ID exists when a session candidate exists"),
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
    let session_source_id = session_source
        .as_ref()
        .map(|record| record.agent_session_source_id);
    let mut open_session = store
        .get_open_agent_session(
            &ownership_scope_key,
            session_source_id,
            &input.harness_key,
            &input.boundary_group_key,
        )
        .await?;
    if let Some(session) = open_session.as_ref()
        && input.occurred_at - session.input_watermark_at >= SESSION_IDLE_GAP
    {
        let expected_input_watermark_at = session.input_watermark_at;
        let mut finalized_session = session.clone();
        finalized_session.lifecycle = SessionLifecycleState::Finalized;
        finalized_session.ended_at = Some(expected_input_watermark_at);
        finalized_session.input_watermark_at = input.completed_at;
        finalized_session.finalized_reason = Some("idle_gap".to_string());
        finalized_session.updated_at = input.completed_at;
        if store
            .finalize_agent_session_if_unchanged(&finalized_session, expected_input_watermark_at)
            .await?
        {
            store
                .mark_agent_session_analyses_stale(finalized_session.agent_session_id, None)
                .await?;
            enqueue_analysis_with_versions(
                store,
                finalized_session.agent_session_id,
                "session_finalized",
                &expected_input_watermark_at
                    .unix_timestamp_nanos()
                    .to_string(),
                input.completed_at,
                desired_versions,
            )
            .await?;
            open_session = None;
        } else {
            open_session = store
                .get_open_agent_session(
                    &ownership_scope_key,
                    session_source_id,
                    &input.harness_key,
                    &input.boundary_group_key,
                )
                .await?;
        }
    }
    let mut window_cas_attempts = 0_usize;
    while let Some(existing_session) = open_session.as_ref() {
        if store
            .count_agent_session_requests(existing_session.agent_session_id)
            .await?
            < gateway_core::MAX_AGENT_SESSION_REQUESTS
        {
            break;
        }
        if window_cas_attempts >= MAX_SESSION_WINDOW_CAS_ATTEMPTS {
            return Err(GatewayError::Internal(
                "agent session window changed too often while enforcing the request limit"
                    .to_string(),
            ));
        }
        window_cas_attempts = window_cas_attempts.saturating_add(1);
        let expected_watermark = existing_session.input_watermark_at;
        let mut finalized = existing_session.clone();
        finalized.lifecycle = SessionLifecycleState::Finalized;
        finalized.ended_at = Some(expected_watermark);
        finalized.finalized_reason = Some("request_limit".to_string());
        finalized.updated_at = input.completed_at;
        if store
            .finalize_agent_session_if_unchanged(&finalized, expected_watermark)
            .await?
        {
            store
                .mark_agent_session_analyses_stale(finalized.agent_session_id, None)
                .await?;
            enqueue_analysis_with_versions(
                store,
                finalized.agent_session_id,
                "session_finalized",
                &expected_watermark.unix_timestamp_nanos().to_string(),
                input.completed_at,
                desired_versions,
            )
            .await?;
            open_session = None;
        } else {
            open_session = store
                .get_open_agent_session(
                    &ownership_scope_key,
                    session_source_id,
                    &input.harness_key,
                    &input.boundary_group_key,
                )
                .await?;
        }
    }
    let build_new_session = || {
        let confidence = if session_source_id.is_some() {
            Confidence::High
        } else {
            Confidence::Medium
        };
        AgentSessionRecord {
            agent_session_id: stable_uuid(
                SESSION_ID_NAMESPACE,
                &json!({
                    "scope": ownership_scope_key,
                    "session": session_source_id,
                    "harness": input.harness_key,
                    "boundary": input.boundary_group_key,
                    "first_request": input.request_id,
                })
                .to_string(),
            ),
            agent_session_source_id: session_source_id,
            ownership_scope_key: ownership_scope_key.clone(),
            api_key_id: input.auth.id,
            user_id: input.auth.owner_user_id,
            team_id: analytics_team_id,
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
        }
    };
    let mut session = if let Some(session) = open_session {
        session
    } else {
        let session = build_new_session();
        if store.insert_agent_session_if_absent(&session).await? {
            session
        } else {
            store
                .get_open_agent_session(
                    &ownership_scope_key,
                    session_source_id,
                    &input.harness_key,
                    &input.boundary_group_key,
                )
                .await?
                .ok_or_else(|| {
                    GatewayError::Internal("open agent session disappeared".to_string())
                })?
        }
    };
    let usage_event_id = store
        .get_usage_ledger_by_request_and_scope(&input.request_id, &ownership_scope_key)
        .await?
        .map(|usage| usage.usage_event_id);
    let terminal_success = if input.terminal_success == Some(false) && usage_event_id.is_some() {
        None
    } else {
        input.terminal_success
    };
    let mut limitations = Vec::new();
    if session_source_id.is_none() {
        limitations.push(LimitationCode::SessionUnobserved);
    }
    if input.metadata.session_limitation.is_some() {
        limitations.push(LimitationCode::RequestIncomplete);
    }
    if usage_event_id.is_none() {
        limitations.push(LimitationCode::UsageUnavailable);
    }
    let execution_id = input.metadata.execution_id.as_deref().map(|candidate| {
        hash_lineage_candidate(&ownership_scope_key, &input.harness_key, candidate)
    });
    let parent_execution_id = input
        .metadata
        .parent_execution_id
        .as_deref()
        .map(|candidate| {
            hash_lineage_candidate(&ownership_scope_key, &input.harness_key, candidate)
        });
    let mut request_link = AgentSessionRequestLinkRecord {
        agent_session_id: session.agent_session_id,
        request_id: input.request_id.clone(),
        request_log_id: input.request_log_id,
        usage_event_id,
        ordinal: 0,
        execution_id,
        parent_execution_id,
        normalized_session_id,
        correlation_confidence: session.boundary_confidence,
        limitation_codes: limitations,
        occurred_at: input.occurred_at,
        completed_at: Some(input.completed_at),
        terminal_success,
    };
    let mut append_attempts = 0_usize;
    let request_inserted = loop {
        match store.append_agent_session_request(&request_link).await {
            Ok(inserted) => break inserted,
            Err(StoreError::AgentSessionWindowClosed(closed_session_id))
                if closed_session_id == session.agent_session_id.to_string()
                    && append_attempts < MAX_SESSION_WINDOW_CAS_ATTEMPTS =>
            {
                append_attempts = append_attempts.saturating_add(1);
                store
                    .mark_agent_session_analyses_stale(session.agent_session_id, None)
                    .await?;
                enqueue_analysis_with_versions(
                    store,
                    session.agent_session_id,
                    "session_finalized",
                    &session
                        .input_watermark_at
                        .unix_timestamp_nanos()
                        .to_string(),
                    input.completed_at,
                    desired_versions,
                )
                .await?;
                session = if let Some(open_session) = store
                    .get_open_agent_session(
                        &ownership_scope_key,
                        session_source_id,
                        &input.harness_key,
                        &input.boundary_group_key,
                    )
                    .await?
                {
                    open_session
                } else {
                    let candidate = build_new_session();
                    if store.insert_agent_session_if_absent(&candidate).await? {
                        candidate
                    } else {
                        store
                            .get_open_agent_session(
                                &ownership_scope_key,
                                session_source_id,
                                &input.harness_key,
                                &input.boundary_group_key,
                            )
                            .await?
                            .ok_or_else(|| {
                                GatewayError::Internal(
                                    "replacement agent session disappeared".to_string(),
                                )
                            })?
                    }
                };
                request_link.agent_session_id = session.agent_session_id;
                request_link.correlation_confidence = session.boundary_confidence;
            }
            Err(error) => return Err(error.into()),
        }
    };
    if request_inserted && input.completed_at > session.input_watermark_at {
        session.input_watermark_at = input.completed_at;
        session.updated_at = input.completed_at;
        store.update_agent_session_window(&session).await?;
    }

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
    let observation_set_id = stable_uuid(
        OBSERVATION_SET_ID_NAMESPACE,
        &json!({
            "session": session.agent_session_id,
            "request": input.request_id,
            "parser": OBSERVATION_PARSER_VERSION,
            "watermark": input.completed_at.unix_timestamp_nanos(),
        })
        .to_string(),
    );
    let mut observations = input.observations;
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
        ..observation_set.clone()
    };
    let append_result = store
        .append_bounded_agent_observation_set(
            &observation_set,
            &truncated_observation_set,
            MAX_AGENT_SESSION_NESTED_FACTS,
        )
        .await?;
    let stored_coverage = if append_result.nested_facts_truncated {
        truncated_observation_set.coverage
    } else {
        observation_set.coverage
    };
    if let Some(request_log_id) = input.request_log_id {
        store
            .link_request_log_to_agent_session(&AgentRequestLogLinkRecord {
                request_log_id,
                agent_session_source_id: session_source_id,
                agent_session_id: session.agent_session_id,
                analysis_source: "passive".to_string(),
                coverage: stored_coverage,
            })
            .await?;
    }
    store
        .mark_agent_session_analyses_stale(session.agent_session_id, None)
        .await?;
    enqueue_analysis_with_versions(
        store,
        session.agent_session_id,
        "new_input",
        &input.request_id,
        input.completed_at,
        desired_versions,
    )
    .await?;
    Ok(session.agent_session_id)
}

fn truncate_nested_facts(observations: &mut [InferredObservation]) {
    for observation in observations {
        observation.facts.supplied_tools.clear();
        observation.facts.supplied_skills.clear();
        observation.facts.file_interactions.clear();
        if !observation
            .limitations
            .contains(&LimitationCode::PayloadTruncated)
        {
            observation
                .limitations
                .push(LimitationCode::PayloadTruncated);
        }
    }
}

fn assign_observation_ids(agent_session_id: Uuid, observations: &mut [InferredObservation]) {
    for (index, observation) in observations.iter_mut().enumerate() {
        observation.observation_id = stable_uuid(
            OBSERVATION_ID_NAMESPACE,
            &json!({
                "session": agent_session_id,
                "request": observation.source_request_id,
                "parser": OBSERVATION_PARSER_VERSION,
                "index": index,
                "kind": observation.kind,
                "facts": observation.facts,
            })
            .to_string(),
        );
    }
}

fn observations_for_response(input: &PassiveRequestRecord<'_>) -> Vec<InferredObservation> {
    let mut observations = Vec::new();
    if input.metadata.message_count.is_some()
        || input.metadata.prompt_bytes.is_some()
        || input.metadata.supplied_tool_count.is_some()
        || !input.metadata.supplied_skills.is_empty()
        || !input.metadata.file_interactions.is_empty()
        || input.metadata.reasoning_config_hash.is_some()
        || input.metadata.cache_requested.is_some()
    {
        observations.push(InferredObservation {
            observation_id: Uuid::nil(),
            kind: InferredObservationKind::SessionMetadataClassified,
            source_request_id: input.request_id.to_string(),
            parser_version: OBSERVATION_PARSER_VERSION.to_string(),
            evidence: EvidenceQuality::Direct,
            occurred_at: input.occurred_at,
            facts: BoundedObservationFacts {
                message_count: input.metadata.message_count,
                prompt_bytes: input.metadata.prompt_bytes,
                supplied_tool_count: input.metadata.supplied_tool_count,
                tool_schema_bytes: input.metadata.tool_schema_bytes,
                supplied_tools: input.metadata.supplied_tools.clone(),
                supplied_skills: input.metadata.supplied_skills.clone(),
                file_interactions: input.metadata.file_interactions.clone(),
                reasoning_config_hash: input.metadata.reasoning_config_hash.clone(),
                cache_requested: input.metadata.cache_requested,
                ..Default::default()
            },
            limitations: tool_inventory_limitations(input.metadata),
        });
    }
    if let Some(response) = input.response_body {
        let (finish_reason, incomplete_reason) = response_finish_reasons(response);
        if finish_reason.is_some() || incomplete_reason.is_some() {
            observations.push(InferredObservation {
                observation_id: Uuid::nil(),
                kind: InferredObservationKind::ResponseFinishClassified,
                source_request_id: input.request_id.to_string(),
                parser_version: OBSERVATION_PARSER_VERSION.to_string(),
                evidence: EvidenceQuality::Direct,
                occurred_at: input.completed_at,
                facts: BoundedObservationFacts {
                    finish_reason,
                    incomplete_reason,
                    ..Default::default()
                },
                limitations: Vec::new(),
            });
        }
        let mut calls = Vec::new();
        let scan_truncated = collect_tool_calls(response, &mut calls);
        let mut seen_call_ids = BTreeSet::new();
        calls.retain(|call| {
            call.id
                .map(|id| seen_call_ids.insert(id.to_string()))
                .unwrap_or(true)
        });
        let first_observation = observations.len();
        observations.extend(
            calls
                .into_iter()
                .map(|call| classify_tool_call(input, call)),
        );
        if scan_truncated {
            if let Some(observation) = observations.get_mut(first_observation) {
                observation
                    .limitations
                    .push(LimitationCode::PayloadTruncated);
            } else {
                observations.push(InferredObservation {
                    observation_id: Uuid::nil(),
                    kind: InferredObservationKind::ToolCallClassified,
                    source_request_id: input.request_id.to_string(),
                    parser_version: OBSERVATION_PARSER_VERSION.to_string(),
                    evidence: EvidenceQuality::Unavailable,
                    occurred_at: input.completed_at,
                    facts: BoundedObservationFacts::default(),
                    limitations: vec![LimitationCode::PayloadTruncated],
                });
            }
        }
    }
    observations
}

pub(super) fn tool_inventory_limitations(metadata: &PassiveRequestMetadata) -> Vec<LimitationCode> {
    let retained_count = u32::try_from(metadata.supplied_tools.len()).unwrap_or(u32::MAX);
    if metadata
        .supplied_tool_count
        .is_some_and(|supplied_count| supplied_count > retained_count)
    {
        vec![LimitationCode::ToolInventoryPotentialOnly]
    } else {
        Vec::new()
    }
}

pub(super) fn response_finish_reasons(response: &Value) -> (Option<String>, Option<String>) {
    let finish_reason = [
        "/choices/0/finish_reason",
        "/stop_reason",
        "/candidates/0/finishReason",
        "/response/choices/0/finish_reason",
        "/response/stop_reason",
    ]
    .into_iter()
    .find_map(|path| response.pointer(path).and_then(Value::as_str))
    .map(|value| value.chars().take(64).collect());
    let incomplete_reason = [
        "/incomplete_details/reason",
        "/response/incomplete_details/reason",
        "/incompleteDetails/reason",
    ]
    .into_iter()
    .find_map(|path| response.pointer(path).and_then(Value::as_str))
    .map(|value| value.chars().take(64).collect());
    (finish_reason, incomplete_reason)
}

pub(super) struct ToolCall<'a> {
    pub(super) id: Option<&'a str>,
    pub(super) name: &'a str,
    pub(super) arguments: Option<&'a str>,
}

pub(super) fn collect_tool_calls<'a>(value: &'a Value, calls: &mut Vec<ToolCall<'a>>) -> bool {
    let mut remaining_nodes = MAX_TOOL_CALL_SCAN_NODES;
    collect_tool_calls_bounded(value, calls, 0, &mut remaining_nodes)
}

fn collect_tool_calls_bounded<'a>(
    value: &'a Value,
    calls: &mut Vec<ToolCall<'a>>,
    depth: usize,
    remaining_nodes: &mut usize,
) -> bool {
    if calls.len() >= MAX_INFERRED_TOOL_CALLS
        || depth > MAX_TOOL_CALL_SCAN_DEPTH
        || *remaining_nodes == 0
    {
        return true;
    }
    *remaining_nodes -= 1;
    match value {
        Value::Array(values) => values
            .iter()
            .any(|value| collect_tool_calls_bounded(value, calls, depth + 1, remaining_nodes)),
        Value::Object(object) => {
            if let Some(function) = object.get("function").and_then(Value::as_object)
                && let Some(name) = function.get("name").and_then(Value::as_str)
            {
                calls.push(ToolCall {
                    id: object
                        .get("id")
                        .or_else(|| object.get("call_id"))
                        .and_then(Value::as_str),
                    name,
                    arguments: function.get("arguments").and_then(Value::as_str),
                });
            } else if matches!(
                object.get("type").and_then(Value::as_str),
                Some("function_call")
            ) && let Some(name) = object.get("name").and_then(Value::as_str)
            {
                calls.push(ToolCall {
                    id: object
                        .get("id")
                        .or_else(|| object.get("call_id"))
                        .and_then(Value::as_str),
                    name,
                    arguments: object.get("arguments").and_then(Value::as_str),
                });
            }
            object
                .values()
                .any(|value| collect_tool_calls_bounded(value, calls, depth + 1, remaining_nodes))
        }
        _ => false,
    }
}

pub(super) fn classify_tool_call(
    input: &PassiveRequestRecord<'_>,
    call: ToolCall<'_>,
) -> InferredObservation {
    let normalized = call.name.to_ascii_lowercase();
    let kind = if normalized.contains("search") || normalized.contains("grep") {
        InferredObservationKind::FileSearchSuspected
    } else if normalized.contains("read") {
        InferredObservationKind::FileReadSuspected
    } else if normalized.contains("overwrite") {
        InferredObservationKind::FileOverwriteSuspected
    } else if normalized.contains("edit") || normalized.contains("patch") {
        InferredObservationKind::FileEditSuspected
    } else if normalized.contains("create") || normalized.contains("write") {
        InferredObservationKind::FileCreateSuspected
    } else if normalized.contains("test")
        || normalized.contains("check")
        || normalized.contains("lint")
        || normalized.contains("build")
    {
        InferredObservationKind::VerificationResultClassified
    } else {
        InferredObservationKind::ToolCallClassified
    };
    // Tool arguments may contain sensitive paths. Classify the operation but
    // never persist a deterministic path-derived identifier.
    let _arguments = call.arguments;
    InferredObservation {
        observation_id: Uuid::nil(),
        kind,
        source_request_id: input.request_id.to_string(),
        parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        evidence: EvidenceQuality::InferredHigh,
        occurred_at: input.completed_at,
        facts: BoundedObservationFacts {
            tool_name: Some(call.name.chars().take(128).collect()),
            ..Default::default()
        },
        limitations: vec![LimitationCode::SemanticVerificationUnavailable],
    }
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
    let mut bespoke = tags
        .bespoke
        .iter()
        .map(|tag| (tag.key.as_str(), tag.value.as_str()))
        .collect::<Vec<_>>();
    bespoke.sort_unstable();
    hash_identifier(
        &json!({
            "service": tags.service,
            "component": tags.component,
            "environment": tags.env,
            "bespoke": bespoke,
        })
        .to_string(),
    )
}
