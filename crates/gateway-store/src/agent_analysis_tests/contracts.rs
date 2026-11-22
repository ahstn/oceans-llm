use gateway_core::{
    AgentSessionRecord, AgentSessionRequestLinkRecord, AgentSessionTraceRepository,
    SessionLifecycleState, StoreError,
};
use time::Duration;
use uuid::Uuid;

pub(super) async fn finalization_is_irreversible(
    store: &impl AgentSessionTraceRepository,
    template: &AgentSessionRecord,
) {
    let open = AgentSessionRecord {
        agent_session_id: Uuid::new_v4(),
        boundary_group_key: Uuid::new_v4().to_string(),
        lifecycle: SessionLifecycleState::Open,
        ended_at: None,
        finalized_reason: None,
        ..template.clone()
    };
    assert!(store.insert_agent_session_if_absent(&open).await.unwrap());
    let request = AgentSessionRequestLinkRecord {
        agent_session_id: open.agent_session_id,
        request_id: Uuid::new_v4().to_string(),
        request_log_id: None,
        usage_event_id: None,
        ordinal: 0,
        execution_id: None,
        parent_execution_id: None,
        normalized_session_id: None,
        correlation_confidence: open.boundary_confidence,
        limitation_codes: Vec::new(),
        occurred_at: open.started_at,
        completed_at: Some(open.input_watermark_at),
        terminal_success: Some(true),
    };
    assert!(
        store
            .append_agent_session_input(gateway_core::AgentSessionInput::request(&request))
            .await
            .unwrap()
            .request_inserted
    );
    let finalized = AgentSessionRecord {
        lifecycle: SessionLifecycleState::Finalized,
        ended_at: Some(open.input_watermark_at),
        finalized_reason: Some("idle_gap".to_string()),
        ..open.clone()
    };
    assert!(
        store
            .finalize_agent_session_if_unchanged(&finalized, open.input_watermark_at, None)
            .await
            .unwrap()
    );
    let before = store
        .load_agent_session_trace(open.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    // A delayed creator/finalizer must not restore its stale open-window record.
    assert!(!store.insert_agent_session_if_absent(&open).await.unwrap());
    assert!(
        !store
            .finalize_agent_session_if_unchanged(&open, open.input_watermark_at, None)
            .await
            .unwrap()
    );
    let late = AgentSessionRequestLinkRecord {
        request_id: Uuid::new_v4().to_string(),
        completed_at: Some(open.input_watermark_at + Duration::seconds(1)),
        ..request
    };
    assert!(matches!(
        store
            .append_agent_session_input(gateway_core::AgentSessionInput::request(&late))
            .await,
        Err(StoreError::AgentSessionWindowClosed(_))
    ));
    let after = store
        .load_agent_session_trace(open.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.session, before.session);
    assert_eq!(after.session.lifecycle, SessionLifecycleState::Finalized);
    assert_eq!(after.requests, before.requests);
}

pub(super) trait CounterStore: AgentSessionTraceRepository + Sync {
    async fn counters(&self, session_id: Uuid) -> (i64, i64, i64, i64);
    async fn migration_history(&self, session_id: Uuid) -> (i64, i64, i64, i64);
    async fn fixture_sql(&self, sql: &str);
}

impl CounterStore for crate::LibsqlStore {
    async fn counters(&self, session_id: Uuid) -> (i64, i64, i64, i64) {
        let mut rows = self.connection().query(
            "SELECT request_count, next_ordinal, nested_fact_count, input_generation FROM agent_sessions WHERE agent_session_id = ?1",
            [session_id.to_string()],
        ).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();
        (
            row.get(0).unwrap(),
            row.get(1).unwrap(),
            row.get(2).unwrap(),
            row.get(3).unwrap(),
        )
    }
    async fn migration_history(&self, session_id: Uuid) -> (i64, i64, i64, i64) {
        let mut rows = self.connection().query(
            "SELECT COUNT(*), COALESCE(SUM(stale), 0), COUNT(superseded_by_analysis_id), (SELECT COUNT(*) FROM agent_analysis_recompute_queue WHERE agent_session_id = ?1) FROM agent_session_analyses WHERE agent_session_id = ?1",
            [session_id.to_string()],
        ).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();
        (
            row.get(0).unwrap(),
            row.get(1).unwrap(),
            row.get(2).unwrap(),
            row.get(3).unwrap(),
        )
    }
    async fn fixture_sql(&self, sql: &str) {
        self.connection().execute_batch(sql).await.unwrap();
    }
}

impl CounterStore for crate::PostgresStore {
    async fn counters(&self, session_id: Uuid) -> (i64, i64, i64, i64) {
        sqlx::query_as("SELECT request_count, next_ordinal, nested_fact_count, input_generation FROM agent_sessions WHERE agent_session_id = $1")
            .bind(session_id.to_string()).fetch_one(self.pool()).await.unwrap()
    }
    async fn migration_history(&self, session_id: Uuid) -> (i64, i64, i64, i64) {
        sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(stale), 0), COUNT(superseded_by_analysis_id), (SELECT COUNT(*) FROM agent_analysis_recompute_queue WHERE agent_session_id = $1) FROM agent_session_analyses WHERE agent_session_id = $1")
            .bind(session_id.to_string()).fetch_one(self.pool()).await.unwrap()
    }
    async fn fixture_sql(&self, sql: &str) {
        sqlx::raw_sql(sql).execute(self.pool()).await.unwrap();
    }
}

pub(super) fn open_fixture(template: &AgentSessionRecord) -> AgentSessionRecord {
    AgentSessionRecord {
        agent_session_id: Uuid::new_v4(),
        boundary_group_key: Uuid::new_v4().to_string(),
        lifecycle: SessionLifecycleState::Open,
        ended_at: None,
        finalized_reason: None,
        ..template.clone()
    }
}

pub(super) fn request_fixture(session: &AgentSessionRecord) -> AgentSessionRequestLinkRecord {
    AgentSessionRequestLinkRecord {
        agent_session_id: session.agent_session_id,
        request_id: Uuid::new_v4().to_string(),
        request_log_id: None,
        usage_event_id: None,
        ordinal: 0,
        execution_id: None,
        parent_execution_id: None,
        normalized_session_id: None,
        correlation_confidence: session.boundary_confidence,
        limitation_codes: Vec::new(),
        occurred_at: session.started_at,
        completed_at: Some(session.input_watermark_at),
        terminal_success: Some(true),
    }
}

pub(super) fn observation_fixture(
    request: &AgentSessionRequestLinkRecord,
    tools: usize,
) -> gateway_core::AgentObservationSetRecord {
    use gateway_core::{
        BoundedObservationFacts, BoundedToolDefinitionFact, EvidenceQuality, InferredObservation,
        InferredObservationKind,
    };
    gateway_core::AgentObservationSetRecord {
        observation_set_id: Uuid::new_v4(),
        agent_session_id: request.agent_session_id,
        parser_version: "contract".to_string(),
        source_watermark_at: request.completed_at.unwrap(),
        coverage: serde_json::json!({"nested_facts_truncated": false}),
        created_at: request.occurred_at,
        observations: vec![InferredObservation {
            observation_id: Uuid::new_v4(),
            kind: InferredObservationKind::SessionMetadataClassified,
            source_request_id: request.request_id.clone(),
            parser_version: "contract".to_string(),
            evidence: EvidenceQuality::Direct,
            occurred_at: request.occurred_at,
            facts: BoundedObservationFacts {
                supplied_tools: (0..tools)
                    .map(|index| BoundedToolDefinitionFact {
                        name: format!("tool-{index}"),
                        server_key: None,
                        token_estimate: 1,
                    })
                    .collect(),
                ..Default::default()
            },
            limitations: Vec::new(),
        }],
    }
}

pub(super) async fn install_legacy_schema(options: &crate::StoreConnectionOptions) {
    crate::run_migrations_with_options_for_test(
        options,
        crate::MigrationTestHook {
            fail_after_apply_version: Some(85),
            ..Default::default()
        },
    )
    .await
    .expect_err("V85 rolls back, leaving a real V84 database for upgrade");
}

pub(super) async fn counter_backfill(
    store: &(impl CounterStore + gateway_core::AgentAnalysisQueueRepository),
    template: &AgentSessionRecord,
    options: &crate::StoreConnectionOptions,
) {
    let session = open_fixture(template);
    store
        .insert_agent_session_if_absent(&session)
        .await
        .unwrap();
    let id = session.agent_session_id;
    let millis = i64::try_from(session.started_at.unix_timestamp_nanos() / 1_000_000).unwrap();
    for (request, ordinal) in [("legacy-a", 2), ("legacy-b", 7)] {
        store.fixture_sql(&format!(
            "INSERT INTO agent_session_requests (agent_session_id, request_id, ordinal, correlation_confidence, limitation_codes_json, occurred_at, completed_at, terminal_success) VALUES ('{id}', '{request}', {ordinal}, 'high', '[]', {millis}, {millis}, TRUE)"
        )).await;
    }
    let set_id = Uuid::new_v4();
    let observation_id = Uuid::new_v4();
    let seconds = session.started_at.unix_timestamp();
    store.fixture_sql(&format!(
        "INSERT INTO agent_inferred_observation_sets (observation_set_id, agent_session_id, parser_version, source_watermark_at, coverage_json, created_at) VALUES ('{set_id}', '{id}', 'contract', {millis}, '{{}}', {seconds})"
    )).await;
    store.fixture_sql(&format!(
        "INSERT INTO agent_inferred_observations (observation_id, observation_set_id, agent_session_id, kind, source_request_id, evidence, occurred_at, facts_json, limitation_codes_json) VALUES ('{observation_id}', '{set_id}', '{id}', 'session_metadata_classified', 'legacy-a', 'direct', {seconds}, '{{\"supplied_tools\":[{{\"name\":\"a\",\"token_estimate\":1}},{{\"name\":\"b\",\"token_estimate\":1}},{{\"name\":\"c\",\"token_estimate\":1}}]}}', '[]')"
    )).await;
    install_legacy_history(store, &session, set_id).await;
    crate::run_migrations_with_options(options).await.unwrap();
    assert_eq!(store.counters(id).await, (2, 8, 3, 3));
    assert_eq!(store.migration_history(id).await, (2, 2, 1, 1));
    let migrated = store
        .claim_agent_analysis(
            "upgrade-worker",
            session.input_watermark_at,
            session.input_watermark_at + Duration::minutes(1),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(migrated.agent_session_id, id);
    assert_eq!(migrated.desired_versions.configuration_version, "legacy-v2");
    assert_eq!(migrated.attempts, 1);
    assert_eq!(migrated.lease_epoch, 1);
    let request = request_fixture(&session);
    store
        .append_agent_session_input(gateway_core::AgentSessionInput::request(&request))
        .await
        .unwrap();
    assert_eq!(store.counters(id).await, (3, 9, 3, 4));
    let trace = store.load_agent_session_trace(id).await.unwrap().unwrap();
    assert_eq!(
        trace
            .requests
            .iter()
            .find(|link| link.request_id == request.request_id)
            .unwrap()
            .ordinal,
        8
    );
    store
        .fixture_sql(&format!(
            "DELETE FROM agent_sessions WHERE agent_session_id = '{id}'"
        ))
        .await;
}

pub(super) async fn atomic_input_invariants(
    store: &impl CounterStore,
    template: &AgentSessionRecord,
) {
    use gateway_core::{
        AgentSessionInput, AgentSessionObservationInput, MAX_AGENT_SESSION_NESTED_FACTS,
        MAX_AGENT_SESSION_REQUESTS,
    };
    let session = open_fixture(template);
    store
        .insert_agent_session_if_absent(&session)
        .await
        .unwrap();
    let first = request_fixture(&session);
    let observations = observation_fixture(&first, MAX_AGENT_SESSION_NESTED_FACTS);
    let input = AgentSessionInput {
        request: Some(&first),
        observations: Some(AgentSessionObservationInput {
            set: &observations,
            truncated_set: &observations,
            maximum_nested_facts: MAX_AGENT_SESSION_NESTED_FACTS,
        }),
        analysis: None,
    };
    let result = store.append_agent_session_input(input).await.unwrap();
    assert!(result.request_inserted && result.observations_inserted);
    assert_eq!(
        store.counters(session.agent_session_id).await,
        (1, 1, 2048, 1)
    );
    let replay = store.append_agent_session_input(input).await.unwrap();
    assert!(!replay.request_inserted && !replay.observations_inserted);
    assert_eq!(
        store.counters(session.agent_session_id).await,
        (1, 1, 2048, 1)
    );

    let second = request_fixture(&session);
    let full = observation_fixture(&second, 1);
    let mut truncated = full.clone();
    truncated.coverage["nested_facts_truncated"] = true.into();
    truncated.observations[0].facts.supplied_tools.clear();
    let input = AgentSessionInput {
        request: Some(&second),
        observations: Some(AgentSessionObservationInput {
            set: &full,
            truncated_set: &truncated,
            maximum_nested_facts: MAX_AGENT_SESSION_NESTED_FACTS,
        }),
        analysis: None,
    };
    assert!(
        store
            .append_agent_session_input(input)
            .await
            .unwrap()
            .nested_facts_truncated
    );
    let replay = store.append_agent_session_input(input).await.unwrap();
    assert!(
        replay.nested_facts_truncated && !replay.request_inserted && !replay.observations_inserted
    );
    assert_eq!(
        store.counters(session.agent_session_id).await,
        (2, 2, 2048, 2)
    );
    assert_atomic_rollback(store, &session, &observations).await;

    for _ in 2..MAX_AGENT_SESSION_REQUESTS {
        let request = request_fixture(&session);
        assert!(
            store
                .append_agent_session_input(AgentSessionInput::request(&request))
                .await
                .unwrap()
                .request_inserted
        );
    }
    let overflow = request_fixture(&session);
    assert!(matches!(
        store
            .append_agent_session_input(AgentSessionInput::request(&overflow))
            .await,
        Err(StoreError::AgentSessionWindowClosed(_))
    ));
    assert!(
        !store
            .append_agent_session_input(AgentSessionInput::request(&first))
            .await
            .unwrap()
            .request_inserted
    );
    assert_eq!(
        store.counters(session.agent_session_id).await,
        (1000, 1000, 2048, 1001)
    );
    let trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(trace.requests.len(), 1000);
    assert_eq!(trace.session.lifecycle, SessionLifecycleState::Finalized);
    let ordinals = trace
        .requests
        .iter()
        .map(|request| request.ordinal)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ordinals, (0..1000).collect());
}

async fn assert_atomic_rollback(
    store: &impl CounterStore,
    session: &AgentSessionRecord,
    existing: &gateway_core::AgentObservationSetRecord,
) {
    let request = request_fixture(session);
    let mut conflicting = observation_fixture(&request, 0);
    conflicting.observations[0].observation_id = existing.observations[0].observation_id;
    let input = gateway_core::AgentSessionInput {
        request: Some(&request),
        observations: gateway_core::AgentSessionInput::observations(&conflicting).observations,
        analysis: None,
    };
    assert!(store.append_agent_session_input(input).await.is_err());
    assert_eq!(
        store.counters(session.agent_session_id).await,
        (2, 2, 2048, 2)
    );
    let trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !trace
            .requests
            .iter()
            .any(|stored| stored.request_id == request.request_id)
    );
    assert_eq!(
        store
            .load_agent_observation_sets(session.agent_session_id)
            .await
            .unwrap()
            .len(),
        2
    );
}

pub(super) async fn concurrent_open_window(
    store: &impl CounterStore,
    template: &AgentSessionRecord,
) {
    let first = open_fixture(template);
    let second = AgentSessionRecord {
        agent_session_id: Uuid::new_v4(),
        ..first.clone()
    };
    let (left, right) = tokio::join!(
        store.insert_agent_session_if_absent(&first),
        store.insert_agent_session_if_absent(&second)
    );
    assert_ne!(left.unwrap(), right.unwrap());
    let stored = store
        .get_open_agent_session(
            &first.ownership_scope_key,
            first.agent_session_source_id,
            &first.harness_key,
            &first.boundary_group_key,
        )
        .await
        .unwrap()
        .unwrap();
    assert!([first.agent_session_id, second.agent_session_id].contains(&stored.agent_session_id));
}

pub(super) async fn postgres_input_allows_foreign_key_readers(
    store: &crate::PostgresStore,
    template: &AgentSessionRecord,
) {
    let session = open_fixture(template);
    store
        .insert_agent_session_if_absent(&session)
        .await
        .unwrap();
    let mut foreign_key_reader = store.pool().begin().await.unwrap();
    // PostgreSQL takes this same lock while inserting an analysis queue row.
    sqlx::query(
        "SELECT agent_session_id FROM agent_sessions WHERE agent_session_id = $1 FOR KEY SHARE",
    )
    .bind(session.agent_session_id.to_string())
    .fetch_one(&mut *foreign_key_reader)
    .await
    .unwrap();
    let request = request_fixture(&session);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        store.append_agent_session_input(gateway_core::AgentSessionInput::request(&request)),
    )
    .await
    .expect("session writer must not wait for a foreign-key reader")
    .unwrap();
    assert!(result.request_inserted);
    foreign_key_reader.commit().await.unwrap();
}

async fn install_legacy_history(
    store: &impl CounterStore,
    session: &AgentSessionRecord,
    set_id: Uuid,
) {
    let id = session.agent_session_id;
    let seconds = session.started_at.unix_timestamp();
    let watermark =
        i64::try_from(session.input_watermark_at.unix_timestamp_nanos() / 1_000_000).unwrap();
    let report = agent_session_analysis::analyze_session(
        &agent_session_analysis::SessionTrace {
            requests: vec![],
            activity_intervals: vec![],
            observations: vec![],
            lifecycle: session.lifecycle,
            boundary_confidence: session.boundary_confidence,
            evidence: agent_session_analysis::TraceEvidence::default(),
        },
        &agent_session_analysis::AnalysisPolicy::default(),
        None,
    )
    .unwrap();
    let json = serde_json::to_string(&report).unwrap().replace('\'', "''");
    let report_ids = [Uuid::new_v4(), Uuid::new_v4()];
    for (index, analysis_id) in report_ids.iter().enumerate() {
        store.fixture_sql(&format!(
            "INSERT INTO agent_session_analyses (analysis_id, agent_session_id, report_schema_version, boundary_policy_version, input_watermark_at, observation_set_id, observation_parser_version, analyzer_version, score_policy_version, pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size, cohort_snapshot_digest, direct_mcp_snapshot_digest, analyzed_at, report_json, stale, expires_at, ownership_scope_key, configuration_version) VALUES ('{analysis_id}', '{id}', 'legacy', 'legacy', {watermark}, '{set_id}', 'contract', 'legacy', 'legacy', 'legacy', 'legacy-{index}', 7, 0, 'none', 'none', {seconds}, '{json}', 0, {seconds}, '{}', '')",
            session.ownership_scope_key,
        )).await;
    }
    store.fixture_sql(&format!("UPDATE agent_session_analyses SET superseded_by_analysis_id = '{}' WHERE analysis_id = '{}'", report_ids[1], report_ids[0])).await;
    for (index, status) in [(1, "leased"), (2, "completed")] {
        let queue_id = Uuid::new_v4();
        let versions = serde_json::json!({
            "report_schema_version": "legacy", "boundary_policy_version": "legacy",
            "observation_parser_version": "contract", "analyzer_version": "legacy",
            "score_policy_version": "legacy", "pricing_policy_version": "legacy", "cohort_version": "legacy",
            "configuration_version": format!("legacy-v{index}"),
        }).to_string();
        let updated = seconds + index;
        store.fixture_sql(&format!(
            "INSERT INTO agent_analysis_recompute_queue (queue_item_id, agent_session_id, reason, desired_versions_json, status, lease_owner, lease_expires_at, attempts, max_attempts, available_at, created_at, updated_at) VALUES ('{queue_id}', '{id}', 'new_input', '{versions}', '{status}', 'legacy-worker', {updated}, 3, 3, {seconds}, {seconds}, {updated})"
        )).await;
    }
}
