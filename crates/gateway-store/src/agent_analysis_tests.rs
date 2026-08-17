use gateway_core::{
    AgentAnalysisDesiredVersions, AgentAnalysisQueueRecord, AgentAnalysisQueueRepository,
    AgentAnalysisQueueStatus, AgentObservationSetRecord, AgentRequestLogLinkRecord,
    AgentSessionAnalysisRecord, AgentSessionListQuery, AgentSessionRecord,
    AgentSessionReportRepository, AgentSessionRequestLinkRecord, AgentSessionSourceRecord,
    AgentSessionTraceRepository, AuthMode, BoundedObservationFacts, BoundedToolDefinitionFact,
    Confidence, EvidenceQuality, GlobalRole, InferredObservation, InferredObservationKind,
    LimitationCode, ScoreMaturity, SessionLifecycleState, StoreError, UserStatus,
};
use serial_test::serial;
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
use crate::{
    LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations, run_migrations_with_options,
};

#[tokio::test]
#[serial]
async fn libsql_agent_analysis_repository_round_trips_and_cascades() {
    let temporary = tempdir().expect("tempdir");
    let database_path = temporary.path().join("gateway.db");
    run_migrations(&database_path).await.expect("migrations");
    let store = LibsqlStore::new_local(database_path.to_str().expect("database path"))
        .await
        .expect("store");
    let user = store
        .create_identity_user(
            "Analyst",
            "analyst@example.com",
            "analyst@example.com",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("user");
    let api_key_id = Uuid::new_v4();
    let now = OffsetDateTime::now_utc();
    store
        .connection()
        .execute(
            "INSERT INTO api_keys (id, public_id, secret_hash, name, status, owner_kind, owner_user_id, created_at) VALUES (?1, ?2, 'hash', 'analysis', 'active', 'user', ?3, ?4)",
            libsql::params![
                api_key_id.to_string(),
                format!("gw_test_{}", Uuid::new_v4()),
                user.user_id.to_string(),
                now.unix_timestamp()
            ],
        )
        .await
        .expect("api key");

    let request_log_id = Uuid::new_v4();
    store
        .connection()
        .execute(
            "INSERT INTO request_logs (request_log_id, request_id, api_key_id, user_id, model_key, provider_key, status_code, latency_ms, metadata_json, occurred_at, resolved_model_key) VALUES (?1, 'request-1', ?2, ?3, 'test-model', 'test-provider', 200, 1000, '{}', ?4, 'test-model')",
            libsql::params![
                request_log_id.to_string(),
                api_key_id.to_string(),
                user.user_id.to_string(),
                now.unix_timestamp()
            ],
        )
        .await
        .expect("request log");

    let scope = format!("user:{}", user.user_id);
    let session_source = AgentSessionSourceRecord {
        agent_session_source_id: Uuid::new_v4(),
        ownership_scope_key: scope.clone(),
        api_key_id,
        user_id: Some(user.user_id),
        team_id: None,
        service_account_id: None,
        actor_user_id: None,
        normalized_session_id: "session-1".to_string(),
        adapter_namespace: "codex".to_string(),
        adapter_version: "v1".to_string(),
        source_provenance: "session_id_header".to_string(),
        harness_key: "codex".to_string(),
        harness_label: "Codex".to_string(),
        first_seen_at: now,
        last_seen_at: now,
        created_at: now,
        updated_at: now,
    };
    let stored_session = store
        .upsert_agent_session_source(&session_source)
        .await
        .expect("session");
    assert_eq!(
        stored_session.agent_session_source_id,
        session_source.agent_session_source_id
    );
    assert_eq!(
        store
            .load_agent_session_source(session_source.agent_session_source_id)
            .await
            .expect("load session"),
        Some(stored_session)
    );

    let later_session = store
        .upsert_agent_session_source(&AgentSessionSourceRecord {
            first_seen_at: now + Duration::seconds(10),
            last_seen_at: now + Duration::seconds(10),
            created_at: now + Duration::seconds(10),
            updated_at: now + Duration::seconds(10),
            ..session_source.clone()
        })
        .await
        .expect("later session observation");
    assert_eq!(
        later_session.created_at.unix_timestamp(),
        now.unix_timestamp()
    );
    let out_of_order_session = store
        .upsert_agent_session_source(&AgentSessionSourceRecord {
            first_seen_at: now - Duration::seconds(5),
            last_seen_at: now - Duration::seconds(5),
            updated_at: now - Duration::seconds(5),
            ..session_source.clone()
        })
        .await
        .expect("out-of-order session observation");
    assert_eq!(
        out_of_order_session.first_seen_at.unix_timestamp(),
        (now - Duration::seconds(5)).unix_timestamp()
    );
    assert_eq!(
        out_of_order_session.last_seen_at,
        later_session.last_seen_at
    );
    assert_eq!(out_of_order_session.updated_at, later_session.updated_at);
    let upgraded_session = store
        .upsert_agent_session_source(&AgentSessionSourceRecord {
            adapter_version: "v2".to_string(),
            source_provenance: "different_source".to_string(),
            updated_at: now + Duration::seconds(20),
            ..session_source.clone()
        })
        .await
        .expect("adapter upgrade");
    assert_eq!(upgraded_session.adapter_version, "v2");
    assert_eq!(upgraded_session.source_provenance, "different_source");

    let session = AgentSessionRecord {
        agent_session_id: Uuid::new_v4(),
        agent_session_source_id: Some(session_source.agent_session_source_id),
        ownership_scope_key: scope.clone(),
        api_key_id,
        user_id: Some(user.user_id),
        team_id: None,
        service_account_id: None,
        actor_user_id: None,
        requested_model_key: "gpt-5.6".to_string(),
        operation: "responses".to_string(),
        caller_class: "user".to_string(),
        request_tags: serde_json::json!({
            "environment": "test",
            "bespoke": [{"key": "customer_tier", "value": "enterprise"}]
        }),
        boundary_group_key: "sha256:boundary".to_string(),
        harness_key: "codex".to_string(),
        boundary_policy_version: agent_session_analysis::SESSION_BOUNDARY_POLICY_VERSION
            .to_string(),
        lifecycle: SessionLifecycleState::Open,
        boundary_confidence: Confidence::High,
        started_at: now,
        ended_at: None,
        input_watermark_at: now + Duration::seconds(1),
        finalized_reason: None,
        created_at: now,
        updated_at: now,
    };
    assert!(
        store
            .insert_agent_session_if_absent(&session)
            .await
            .expect("session")
    );
    assert!(
        !store
            .insert_agent_session_if_absent(&session)
            .await
            .expect("exact session replay")
    );
    assert!(matches!(
        store
            .insert_agent_session_if_absent(&AgentSessionRecord {
                requested_model_key: "different-model".to_string(),
                ..session.clone()
            })
            .await,
        Err(StoreError::Conflict(_))
    ));

    let request_link = AgentSessionRequestLinkRecord {
        agent_session_id: session.agent_session_id,
        request_id: "request-1".to_string(),
        request_log_id: Some(request_log_id),
        usage_event_id: None,
        ordinal: 0,
        execution_id: Some("turn-1".to_string()),
        parent_execution_id: None,
        normalized_session_id: Some("sha256:session-1".to_string()),
        correlation_confidence: Confidence::High,
        limitation_codes: vec![],
        occurred_at: now,
        completed_at: Some(now + Duration::seconds(1)),
        terminal_success: Some(true),
    };
    assert!(
        store
            .append_agent_session_request(&request_link)
            .await
            .expect("request link")
    );
    assert!(
        !store
            .append_agent_session_request(&request_link)
            .await
            .expect("idempotent request link")
    );
    let conflicting_request_link = AgentSessionRequestLinkRecord {
        terminal_success: Some(false),
        ..request_link.clone()
    };
    assert!(matches!(
        store
            .append_agent_session_request(&conflicting_request_link)
            .await,
        Err(StoreError::Conflict(_))
    ));
    store
        .link_request_log_to_agent_session(&AgentRequestLogLinkRecord {
            request_log_id,
            agent_session_source_id: Some(session_source.agent_session_source_id),
            agent_session_id: session.agent_session_id,
            analysis_source: "passive".to_string(),
            coverage: serde_json::json!({"request_metadata": true}),
        })
        .await
        .expect("request log analytics link");
    let mut linked_rows = store
        .connection()
        .query(
            "SELECT agent_session_source_id, agent_session_id, agent_analysis_source, agent_analysis_coverage_json FROM request_logs WHERE request_log_id = ?1",
            [request_log_id.to_string()],
        )
        .await
        .expect("linked request log query");
    let linked = linked_rows
        .next()
        .await
        .expect("linked request log row")
        .expect("linked request log");
    assert_eq!(
        linked.get::<String>(0).expect("linked session"),
        session_source.agent_session_source_id.to_string()
    );
    assert_eq!(
        linked.get::<String>(1).expect("linked session"),
        session.agent_session_id.to_string()
    );
    assert_eq!(linked.get::<String>(2).expect("analysis source"), "passive");
    assert_eq!(
        linked.get::<String>(3).expect("analysis coverage"),
        r#"{"request_metadata":true}"#
    );
    let observation_set = AgentObservationSetRecord {
        observation_set_id: Uuid::new_v4(),
        agent_session_id: session.agent_session_id,
        parser_version: "passive-observations-v1".to_string(),
        source_watermark_at: now,
        coverage: serde_json::json!({"payload": true}),
        created_at: now,
        observations: vec![InferredObservation {
            observation_id: Uuid::new_v4(),
            kind: InferredObservationKind::FileEditSuspected,
            source_request_id: "request-1".to_string(),
            parser_version: "passive-observations-v1".to_string(),
            evidence: EvidenceQuality::InferredHigh,
            occurred_at: now,
            facts: BoundedObservationFacts {
                opaque_file_id: Some("sha256:file".to_string()),
                ..Default::default()
            },
            limitations: vec![LimitationCode::SemanticVerificationUnavailable],
        }],
    };
    assert!(
        store
            .append_agent_observation_set(&observation_set)
            .await
            .expect("observations")
    );
    assert!(
        !store
            .append_agent_observation_set(&observation_set)
            .await
            .expect("exact observation set replay")
    );
    assert!(matches!(
        store
            .append_agent_observation_set(&AgentObservationSetRecord {
                coverage: serde_json::json!({"payload": "conflicting"}),
                ..observation_set.clone()
            })
            .await,
        Err(StoreError::Conflict(_))
    ));

    let bounded_set = AgentObservationSetRecord {
        observation_set_id: Uuid::new_v4(),
        coverage: serde_json::json!({"nested_facts_truncated": false}),
        observations: vec![InferredObservation {
            observation_id: Uuid::new_v4(),
            facts: BoundedObservationFacts {
                supplied_tools: vec![BoundedToolDefinitionFact {
                    name: "read".to_string(),
                    server_key: None,
                    token_estimate: 12,
                }],
                ..Default::default()
            },
            ..observation_set.observations[0].clone()
        }],
        ..observation_set.clone()
    };
    let truncated_set = AgentObservationSetRecord {
        coverage: serde_json::json!({"nested_facts_truncated": true}),
        observations: vec![InferredObservation {
            observation_id: Uuid::new_v4(),
            facts: BoundedObservationFacts::default(),
            limitations: vec![LimitationCode::PayloadTruncated],
            ..bounded_set.observations[0].clone()
        }],
        ..bounded_set.clone()
    };
    let bounded_result = store
        .append_bounded_agent_observation_set(&bounded_set, &truncated_set, 0)
        .await
        .expect("bounded observation set");
    assert!(bounded_result.inserted);
    assert!(bounded_result.nested_facts_truncated);
    let bounded_replay = store
        .append_bounded_agent_observation_set(&bounded_set, &truncated_set, 0)
        .await
        .expect("bounded observation replay");
    assert!(!bounded_replay.inserted);
    assert!(bounded_replay.nested_facts_truncated);

    let second_observation_set = AgentObservationSetRecord {
        observation_set_id: Uuid::new_v4(),
        parser_version: "passive-observations-v1".to_string(),
        source_watermark_at: session.input_watermark_at,
        observations: vec![InferredObservation {
            observation_id: Uuid::new_v4(),
            kind: InferredObservationKind::FileReadSuspected,
            source_request_id: "request-2".to_string(),
            parser_version: "passive-observations-v1".to_string(),
            evidence: EvidenceQuality::InferredHigh,
            occurred_at: now + Duration::milliseconds(1),
            facts: BoundedObservationFacts::default(),
            limitations: vec![],
        }],
        ..observation_set.clone()
    };
    assert!(
        store
            .append_agent_observation_set(&second_observation_set)
            .await
            .expect("second observations")
    );
    let third_observation_set = AgentObservationSetRecord {
        observation_set_id: Uuid::new_v4(),
        parser_version: "passive-observations-v2".to_string(),
        created_at: now + Duration::seconds(1),
        observations: vec![InferredObservation {
            observation_id: Uuid::new_v4(),
            kind: InferredObservationKind::FileReadSuspected,
            source_request_id: "request-1".to_string(),
            parser_version: "passive-observations-v2".to_string(),
            evidence: EvidenceQuality::InferredHigh,
            occurred_at: now + Duration::milliseconds(2),
            facts: BoundedObservationFacts::default(),
            limitations: vec![],
        }],
        ..observation_set.clone()
    };
    assert!(
        store
            .append_agent_observation_set(&third_observation_set)
            .await
            .expect("third observations")
    );

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
    .expect("analysis report");
    let analysis = AgentSessionAnalysisRecord {
        analysis_id: Uuid::new_v4(),
        agent_session_id: session.agent_session_id,
        configuration_version: report.configuration_version.clone(),
        boundary_policy_version: session.boundary_policy_version.clone(),
        input_watermark_at: session.input_watermark_at,
        observation_set_id: third_observation_set.observation_set_id,
        observation_parser_version: third_observation_set.parser_version.clone(),
        pricing_policy_version: "cache-aware-v1".to_string(),
        cohort_version: "no-cohort-v1".to_string(),
        cohort_fallback_level: 7,
        cohort_sample_size: 0,
        cohort_snapshot_digest: "sha256:no-cohort".to_string(),
        direct_mcp_snapshot_digest: "sha256:no-direct-mcp".to_string(),
        analyzed_at: now,
        report,
        stale: false,
        superseded_by_analysis_id: None,
        expires_at: now + Duration::days(90),
        ownership_scope_key: scope.clone(),
        user_id: Some(user.user_id),
        service_account_id: None,
    };
    assert!(
        store
            .append_agent_session_analysis(&analysis)
            .await
            .expect("analysis")
    );
    assert!(
        !store
            .append_agent_session_analysis(&AgentSessionAnalysisRecord {
                analysis_id: Uuid::new_v4(),
                ..analysis.clone()
            })
            .await
            .expect("duplicate analysis")
    );
    assert!(
        !store
            .append_agent_session_analysis(&AgentSessionAnalysisRecord {
                analyzed_at: analysis.analyzed_at + Duration::seconds(1),
                expires_at: analysis.expires_at + Duration::seconds(1),
                ..analysis.clone()
            })
            .await
            .expect("retry analysis")
    );
    let mut reconfigured_report = analysis.report.clone();
    reconfigured_report.configuration_version = "config-v2".to_string();
    assert!(
        store
            .append_agent_session_analysis(&AgentSessionAnalysisRecord {
                analysis_id: Uuid::new_v4(),
                configuration_version: reconfigured_report.configuration_version.clone(),
                report: reconfigured_report,
                stale: true,
                ..analysis.clone()
            })
            .await
            .expect("reconfigured analysis")
    );
    let mut advanced_session = session.clone();
    advanced_session.input_watermark_at += Duration::seconds(1);
    advanced_session.updated_at += Duration::seconds(1);
    store
        .update_agent_session_window(&advanced_session)
        .await
        .expect("advance session watermark");
    assert!(matches!(
        store
            .append_agent_session_analysis(&AgentSessionAnalysisRecord {
                analysis_id: Uuid::new_v4(),
                cohort_version: "late-worker-v1".to_string(),
                ..analysis.clone()
            })
            .await,
        Err(StoreError::Conflict(_))
    ));
    assert!(
        store
            .append_agent_session_analysis(&AgentSessionAnalysisRecord {
                analysis_id: Uuid::new_v4(),
                input_watermark_at: advanced_session.input_watermark_at,
                analyzed_at: now + Duration::seconds(1),
                ..analysis.clone()
            })
            .await
            .expect("finalized watermark analysis")
    );
    let foreign_session = AgentSessionRecord {
        agent_session_id: Uuid::new_v4(),
        boundary_group_key: "sha256:foreign-boundary".to_string(),
        ..advanced_session.clone()
    };
    assert!(
        store
            .insert_agent_session_if_absent(&foreign_session)
            .await
            .expect("foreign session")
    );
    assert!(
        matches!(
            store
                .append_agent_session_analysis(&AgentSessionAnalysisRecord {
                    analysis_id: Uuid::new_v4(),
                    agent_session_id: foreign_session.agent_session_id,
                    input_watermark_at: foreign_session.input_watermark_at,
                    cohort_version: "foreign-observation-v1".to_string(),
                    ..analysis.clone()
                })
                .await,
            Err(StoreError::Conflict(_))
        ),
        "analysis must not reference another session's observation set"
    );
    store
        .connection()
        .execute(
            "DELETE FROM agent_sessions WHERE agent_session_id = ?1",
            [foreign_session.agent_session_id.to_string()],
        )
        .await
        .expect("delete foreign session fixture");

    let observation_sets = store
        .load_agent_observation_sets(session.agent_session_id)
        .await
        .expect("all observation sets");
    assert_eq!(observation_sets.len(), 4);
    let trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .expect("trace")
        .expect("session trace");
    assert_eq!(trace.requests.len(), 1);
    assert_eq!(trace.requests[0].terminal_success, Some(true));
    let latest_observation_set = trace.latest_observation_set.expect("observation set");
    let latest_parser_version = latest_observation_set.parser_version.clone();
    let observations = latest_observation_set.observations;
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].parser_version, latest_parser_version);
    let page = store
        .list_agent_sessions(&AgentSessionListQuery {
            ownership_scope_key: Some(scope),
            ..Default::default()
        })
        .await
        .expect("session page");
    assert_eq!(page.total, 1);
    assert_eq!(
        page.items[0].session.agent_session_id,
        session.agent_session_id
    );
    let session_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            agent_session_source_id: Some(session_source.agent_session_source_id),
            ..Default::default()
        })
        .await
        .expect("session page");
    assert_eq!(session_page.total, 1);
    let missing_session_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            agent_session_source_id: Some(Uuid::new_v4()),
            ..Default::default()
        })
        .await
        .expect("missing session page");
    assert_eq!(missing_session_page.total, 0);
    let harness_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            harness_key: Some("codex".to_string()),
            ..Default::default()
        })
        .await
        .expect("harness page");
    assert_eq!(harness_page.total, 1);
    let empty_harness_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            harness_key: Some("opencode".to_string()),
            ..Default::default()
        })
        .await
        .expect("empty harness page");
    assert_eq!(empty_harness_page.total, 0);
    let confidence_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            score_confidence: Some(Confidence::Low),
            ..Default::default()
        })
        .await
        .expect("confidence page");
    assert_eq!(confidence_page.total, 1);
    let empty_confidence_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            score_confidence: Some(Confidence::High),
            ..Default::default()
        })
        .await
        .expect("empty confidence page");
    assert_eq!(empty_confidence_page.total, 0);
    let before_watermark_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            input_watermark_before: Some(now + Duration::milliseconds(500)),
            ..Default::default()
        })
        .await
        .expect("before-watermark page");
    assert_eq!(before_watermark_page.total, 0);
    let after_watermark_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            input_watermark_before: Some(now + Duration::milliseconds(2_500)),
            ..Default::default()
        })
        .await
        .expect("after-watermark page");
    assert_eq!(after_watermark_page.total, 1);
    let dimension_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            requested_model_key: Some(session.requested_model_key.clone()),
            operation: Some(session.operation.clone()),
            caller_class: Some(session.caller_class.clone()),
            gateway_outcome: Some(analysis.report.gateway_outcome),
            score_maturity: Some(analysis.report.maturity),
            minimum_coverage_percent: Some(analysis.report.coverage.overall_percent),
            normalized_session_id: Some(session_source.normalized_session_id.clone()),
            request_tag_key: Some("environment".to_string()),
            request_tag_value: Some("test".to_string()),
            ..Default::default()
        })
        .await
        .expect("session dimension filters");
    assert_eq!(dimension_page.total, 1);
    let key_only_tag_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            request_tag_key: Some("environment".to_string()),
            ..Default::default()
        })
        .await
        .expect("key-only request tag filter");
    assert_eq!(key_only_tag_page.total, 1);
    let bespoke_tag_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            request_tag_key: Some("customer_tier".to_string()),
            request_tag_value: Some("enterprise".to_string()),
            ..Default::default()
        })
        .await
        .expect("bespoke request tag filter");
    assert_eq!(bespoke_tag_page.total, 1);
    let missing_tag_page = store
        .list_agent_sessions(&AgentSessionListQuery {
            request_tag_key: Some("environment".to_string()),
            request_tag_value: Some("production".to_string()),
            ..Default::default()
        })
        .await
        .expect("missing request tag filter");
    assert_eq!(missing_tag_page.total, 0);

    let queue = AgentAnalysisQueueRecord {
        queue_item_id: Uuid::new_v4(),
        agent_session_id: session.agent_session_id,
        reason: "new_input".to_string(),
        desired_versions: AgentAnalysisDesiredVersions {
            configuration_version: "config-v1".to_string(),
            report_schema_version: agent_session_analysis::REPORT_SCHEMA_VERSION.to_string(),
            boundary_policy_version: agent_session_analysis::SESSION_BOUNDARY_POLICY_VERSION
                .to_string(),
            observation_parser_version: agent_session_analysis::OBSERVATION_PARSER_VERSION
                .to_string(),
            analyzer_version: agent_session_analysis::ANALYZER_VERSION.to_string(),
            score_policy_version: "outcome-cost-time-v1".to_string(),
            pricing_policy_version: "cache-aware-v1".to_string(),
            cohort_version: "successful-boundary-group-v2".to_string(),
            score_maturity: ScoreMaturity::Experimental,
            calibration_approval_id: None,
        },
        status: AgentAnalysisQueueStatus::Pending,
        lease_owner: None,
        lease_expires_at: None,
        attempts: 0,
        max_attempts: 3,
        last_error: None,
        available_at: now,
        created_at: now,
        updated_at: now,
        completed_at: None,
    };
    assert!(store.enqueue_agent_analysis(&queue).await.expect("enqueue"));
    let claimed = store
        .claim_agent_analysis("worker", now, now + Duration::minutes(1))
        .await
        .expect("claim")
        .expect("queue item");
    assert_eq!(claimed.status, AgentAnalysisQueueStatus::Leased);
    assert_eq!(claimed.attempts, 1);
    assert!(
        !store
            .renew_agent_analysis_lease(
                queue.queue_item_id,
                "stale-worker",
                now + Duration::seconds(10),
                now + Duration::seconds(70),
            )
            .await
            .expect("reject foreign lease renewal")
    );
    assert!(
        store
            .renew_agent_analysis_lease(
                queue.queue_item_id,
                "worker",
                now + Duration::seconds(10),
                now + Duration::seconds(70),
            )
            .await
            .expect("renew owned lease")
    );
    assert!(
        store
            .complete_agent_analysis(
                queue.queue_item_id,
                "stale-worker",
                now + Duration::seconds(1),
            )
            .await
            .is_err()
    );
    store
        .complete_agent_analysis(queue.queue_item_id, "worker", now + Duration::seconds(2))
        .await
        .expect("complete");

    let limit_session = AgentSessionRecord {
        agent_session_id: Uuid::new_v4(),
        agent_session_source_id: None,
        boundary_group_key: "sha256:request-limit".to_string(),
        ..session.clone()
    };
    assert!(
        store
            .insert_agent_session_if_absent(&limit_session)
            .await
            .expect("request-limit session")
    );
    store
        .connection()
        .execute(
            "WITH RECURSIVE sequence(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 1000) INSERT INTO agent_session_requests (agent_session_id, request_id, ordinal, correlation_confidence, limitation_codes_json, occurred_at, completed_at) SELECT ?1, 'limit-' || value, value - 1, 'high', '[]', ?2, ?2 FROM sequence",
            libsql::params![
                limit_session.agent_session_id.to_string(),
                i64::try_from(now.unix_timestamp_nanos() / 1_000_000)
                    .expect("current timestamp fits in milliseconds")
            ],
        )
        .await
        .expect("request-limit fixtures");
    let request_limit_error = store
        .append_agent_session_request(&AgentSessionRequestLinkRecord {
            agent_session_id: limit_session.agent_session_id,
            request_id: "limit-overflow".to_string(),
            request_log_id: None,
            usage_event_id: None,
            ordinal: 0,
            execution_id: None,
            parent_execution_id: None,
            normalized_session_id: None,
            correlation_confidence: Confidence::High,
            limitation_codes: vec![],
            occurred_at: now,
            completed_at: Some(now),
            terminal_success: Some(true),
        })
        .await
        .expect_err("request limit must close the current window");
    assert!(matches!(
        request_limit_error,
        StoreError::AgentSessionWindowClosed(id)
            if id == limit_session.agent_session_id.to_string()
    ));
    let closed_limit_session = store
        .load_agent_session_trace(limit_session.agent_session_id)
        .await
        .expect("closed request-limit session")
        .expect("request-limit session trace");
    assert_eq!(
        closed_limit_session.session.lifecycle,
        SessionLifecycleState::Finalized
    );
    assert_eq!(
        closed_limit_session.session.finalized_reason.as_deref(),
        Some("request_limit")
    );

    let mut finalized_session = advanced_session.clone();
    finalized_session.lifecycle = SessionLifecycleState::Finalized;
    finalized_session.ended_at = Some(finalized_session.input_watermark_at);
    finalized_session.finalized_reason = Some("idle_gap".to_string());
    store
        .update_agent_session_window(&finalized_session)
        .await
        .expect("finalize session before retention purge");
    let purged = store
        .purge_agent_analysis_before(now + Duration::seconds(3))
        .await
        .expect("purge analysis facts");
    assert!(purged >= 2);
    let retained_trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .expect("trace after fact retention")
        .expect("retained session and report");
    assert!(retained_trace.requests.is_empty());
    assert!(retained_trace.latest_observation_set.is_none());
    assert!(retained_trace.latest_analysis.is_none());
    let mut retained_report_rows = store
        .connection()
        .query(
            "SELECT COUNT(*) FROM agent_session_analyses WHERE analysis_id = ?1",
            [analysis.analysis_id.to_string()],
        )
        .await
        .expect("retained report query");
    let retained_report = retained_report_rows
        .next()
        .await
        .expect("retained report row")
        .expect("retained report");
    assert_eq!(retained_report.get::<i64>(0).expect("report count"), 0);

    let expired = store
        .purge_expired_agent_analysis(now + Duration::days(91), now + Duration::seconds(3))
        .await
        .expect("purge expired reports and queue");
    assert!(expired >= 1);

    store
        .connection()
        .execute(
            "DELETE FROM users WHERE user_id = ?1",
            [user.user_id.to_string()],
        )
        .await
        .expect("delete user");
    assert!(
        store
            .load_agent_session_trace(session.agent_session_id)
            .await
            .expect("trace after deletion")
            .is_none()
    );
    let mut deleted_report_rows = store
        .connection()
        .query(
            "SELECT COUNT(*) FROM agent_session_analyses WHERE analysis_id = ?1",
            [analysis.analysis_id.to_string()],
        )
        .await
        .expect("deleted report query");
    let deleted_report = deleted_report_rows
        .next()
        .await
        .expect("deleted report row")
        .expect("deleted report");
    assert_eq!(deleted_report.get::<i64>(0).expect("report count"), 0);
}

#[tokio::test]
#[serial]
async fn postgres_agent_session_dimensions_round_trip_and_filter() {
    let Some(test_db) = create_postgres_test_database().await else {
        eprintln!("skipping postgres agent analysis test because TEST_POSTGRES_URL is not set");
        return;
    };
    let options = StoreConnectionOptions::Postgres {
        url: test_db.database_url.clone(),
        max_connections: 2,
    };
    run_migrations_with_options(&options)
        .await
        .expect("postgres migrations");
    let store = PostgresStore::connect(&test_db.database_url, 2)
        .await
        .expect("postgres store");
    let user = store
        .create_identity_user(
            "Postgres Analyst",
            "postgres-analyst@example.com",
            "postgres-analyst@example.com",
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("user");
    let api_key_id = Uuid::new_v4();
    let now = OffsetDateTime::from_unix_timestamp(OffsetDateTime::now_utc().unix_timestamp())
        .expect("current timestamp");
    sqlx::query(
        "INSERT INTO api_keys (id, public_id, secret_hash, name, status, owner_kind, owner_user_id, created_at) VALUES ($1, $2, 'hash', 'analysis', 'active', 'user', $3, $4)",
    )
    .bind(api_key_id.to_string())
    .bind(format!("gw_test_{}", Uuid::new_v4()))
    .bind(user.user_id.to_string())
    .bind(now.unix_timestamp())
    .execute(store.pool())
    .await
    .expect("api key");

    let ownership_scope_key = format!("user:{}", user.user_id);
    let session_source = AgentSessionSourceRecord {
        agent_session_source_id: Uuid::new_v4(),
        ownership_scope_key: ownership_scope_key.clone(),
        api_key_id,
        user_id: Some(user.user_id),
        team_id: None,
        service_account_id: None,
        actor_user_id: None,
        normalized_session_id: "postgres-session".to_string(),
        adapter_namespace: "codex".to_string(),
        adapter_version: "v1".to_string(),
        source_provenance: "session_id_header".to_string(),
        harness_key: "codex".to_string(),
        harness_label: "Codex".to_string(),
        first_seen_at: now,
        last_seen_at: now,
        created_at: now,
        updated_at: now,
    };
    store
        .upsert_agent_session_source(&session_source)
        .await
        .expect("session");

    let session = AgentSessionRecord {
        agent_session_id: Uuid::new_v4(),
        agent_session_source_id: Some(session_source.agent_session_source_id),
        ownership_scope_key,
        api_key_id,
        user_id: Some(user.user_id),
        team_id: None,
        service_account_id: None,
        actor_user_id: None,
        requested_model_key: "claude-opus-4-1".to_string(),
        operation: "chat".to_string(),
        caller_class: "user".to_string(),
        request_tags: serde_json::json!({"environment": "postgres"}),
        boundary_group_key: "sha256:postgres-boundary".to_string(),
        harness_key: "codex".to_string(),
        boundary_policy_version: agent_session_analysis::SESSION_BOUNDARY_POLICY_VERSION
            .to_string(),
        lifecycle: SessionLifecycleState::Finalized,
        boundary_confidence: Confidence::High,
        started_at: now,
        ended_at: Some(now + Duration::seconds(1)),
        input_watermark_at: now + Duration::seconds(1),
        finalized_reason: Some("idle_gap".to_string()),
        created_at: now,
        updated_at: now,
    };
    assert!(
        store
            .insert_agent_session_if_absent(&session)
            .await
            .expect("insert session")
    );
    assert!(
        !store
            .insert_agent_session_if_absent(&session)
            .await
            .expect("idempotent replay")
    );
    assert!(matches!(
        store
            .insert_agent_session_if_absent(&AgentSessionRecord {
                operation: "responses".to_string(),
                ..session.clone()
            })
            .await,
        Err(StoreError::Conflict(_))
    ));

    let page = store
        .list_agent_sessions(&AgentSessionListQuery {
            requested_model_key: Some("claude-opus-4-1".to_string()),
            operation: Some("chat".to_string()),
            caller_class: Some("user".to_string()),
            normalized_session_id: Some("postgres-session".to_string()),
            request_tag_key: Some("environment".to_string()),
            request_tag_value: Some("postgres".to_string()),
            ..AgentSessionListQuery::default()
        })
        .await
        .expect("list sessions");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].session, session);

    store.pool().close().await;
    drop_postgres_test_database(&test_db).await;
}
