use super::contracts::{CounterStore, observation_fixture, open_fixture, request_fixture};
use gateway_core::{
    AgentAnalysisDesiredVersions, AgentAnalysisLease, AgentAnalysisQueueRecord,
    AgentAnalysisQueueStatus, AgentSessionAnalysisRecord, AgentSessionAnalysisRepository,
    AgentSessionInput, AgentSessionRecord, AgentSessionTraceRecord, SessionLifecycleState,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub(super) async fn generation_jobs(
    store: &(impl CounterStore + AgentSessionAnalysisRepository),
    template: &AgentSessionRecord,
) {
    let session = open_fixture(template);
    store
        .insert_agent_session_if_absent(&session)
        .await
        .unwrap();
    let request = request_fixture(&session);
    let observations = observation_fixture(&request, 0);
    let job = job_fixture(&session);
    let absent = AgentAnalysisQueueRecord {
        agent_session_id: Uuid::new_v4(),
        ..job.clone()
    };
    assert!(matches!(
        store.enqueue_agent_analysis(&absent).await,
        Err(gateway_core::StoreError::NotFound(_))
    ));
    store
        .append_agent_session_input(AgentSessionInput {
            request: Some(&request),
            observations: AgentSessionInput::observations(&observations).observations,
            analysis: Some(&job),
        })
        .await
        .unwrap();
    let claimed = claim(store, session.input_watermark_at).await;
    assert_eq!(claimed.agent_session_id, session.agent_session_id);
    let trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    let report = analysis_fixture(
        &trace,
        &claimed.desired_versions,
        session.input_watermark_at,
    );
    assert!(
        store
            .append_agent_session_analysis(&report, Some(lease(&claimed)))
            .await
            .unwrap()
    );
    schedule_failure_rolls_back(store, &session, &job).await;
    late_input_reruns_a_fenced_lease(store, &session, &claimed, &report).await;
    manual_requests_and_finalization(store, &session, &job).await;
    expired_lease_is_fenced(store, &session, &job).await;
    finalization_schedules_initial_job(store, template).await;
}

async fn schedule_failure_rolls_back(
    store: &(impl CounterStore + AgentSessionAnalysisRepository),
    session: &AgentSessionRecord,
    job: &AgentAnalysisQueueRecord,
) {
    let invalid_job = AgentAnalysisQueueRecord {
        queue_item_id: Uuid::new_v4(),
        max_attempts: 0,
        ..job.clone()
    };
    let request = request_fixture(session);
    let observations = observation_fixture(&request, 1);
    assert!(
        store
            .append_agent_session_input(AgentSessionInput {
                request: Some(&request),
                observations: AgentSessionInput::observations(&observations).observations,
                analysis: Some(&invalid_job),
            })
            .await
            .is_err()
    );
    assert_eq!(store.counters(session.agent_session_id).await, (1, 1, 0, 1));
    let trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(trace.requests.len(), 1);
    assert!(trace.latest_analysis.is_some());
    assert_eq!(
        store
            .load_agent_observation_sets(session.agent_session_id)
            .await
            .unwrap()
            .len(),
        1
    );
}

async fn late_input_reruns_a_fenced_lease(
    store: &(impl CounterStore + AgentSessionAnalysisRepository),
    session: &AgentSessionRecord,
    claimed: &AgentAnalysisQueueRecord,
    previous_report: &AgentSessionAnalysisRecord,
) {
    let now = session.input_watermark_at;
    let mut request = request_fixture(session);
    request.occurred_at -= Duration::seconds(2);
    request.completed_at = Some(now - Duration::seconds(1));
    let observations = observation_fixture(&request, 0);
    store
        .append_agent_session_input(AgentSessionInput {
            request: Some(&request),
            observations: AgentSessionInput::observations(&observations).observations,
            analysis: None,
        })
        .await
        .unwrap();
    let trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        trace.session.input_watermark_at,
        previous_report.input_watermark_at
    );
    assert_eq!(trace.input_generation, previous_report.input_generation + 1);
    assert!(
        trace.latest_analysis.is_none(),
        "late facts hide the obsolete report before replacement"
    );
    assert!(
        !store
            .append_agent_session_analysis(previous_report, Some(lease(claimed)))
            .await
            .unwrap()
    );
    store
        .complete_agent_analysis(lease(claimed), now + Duration::seconds(1))
        .await
        .unwrap();
    let rerun = claim(store, now + Duration::seconds(1)).await;
    assert_eq!(
        rerun.queue_item_id, claimed.queue_item_id,
        "one durable job per session"
    );
    assert!(rerun.lease_epoch > claimed.lease_epoch);
    assert_eq!(
        rerun.attempts, 1,
        "a new generation has a fresh retry budget"
    );
    assert!(
        !store
            .renew_agent_analysis_lease(
                lease(claimed),
                now + Duration::seconds(2),
                now + Duration::minutes(2)
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .complete_agent_analysis(lease(claimed), now + Duration::seconds(2))
            .await
            .is_err()
    );
    assert!(
        store
            .fail_agent_analysis(
                lease(claimed),
                "old failure",
                None,
                now + Duration::seconds(2)
            )
            .await
            .is_err()
    );
    let report = analysis_fixture(&trace, &rerun.desired_versions, now + Duration::seconds(2));
    assert_eq!(
        report.observation_set_id,
        previous_report.observation_set_id
    );
    assert!(
        !store
            .append_agent_session_analysis(&report, Some(lease(claimed)))
            .await
            .unwrap()
    );
    assert!(
        store
            .append_agent_session_analysis(&report, Some(lease(&rerun)))
            .await
            .unwrap()
    );
    let sets = store
        .load_agent_observation_sets(session.agent_session_id)
        .await
        .unwrap();
    assert_eq!(sets.len(), 2);
    assert!(sets[0].source_watermark_at < sets[1].source_watermark_at);
    assert_eq!(
        trace
            .requests
            .iter()
            .map(|request| request.ordinal)
            .collect::<Vec<_>>(),
        [1, 0]
    );
    store
        .complete_agent_analysis(lease(&rerun), now + Duration::seconds(3))
        .await
        .unwrap();
    let replay = store
        .append_agent_session_input(AgentSessionInput::observations(&observations))
        .await
        .unwrap();
    assert!(!replay.observations_inserted);
    assert!(
        store
            .claim_agent_analysis(
                "worker",
                now + Duration::seconds(3),
                now + Duration::minutes(1)
            )
            .await
            .unwrap()
            .is_none()
    );
}

async fn manual_requests_and_finalization(
    store: &(impl CounterStore + AgentSessionAnalysisRepository),
    session: &AgentSessionRecord,
    job: &AgentAnalysisQueueRecord,
) {
    let now = session.input_watermark_at;
    let mut manual = AgentAnalysisQueueRecord {
        queue_item_id: Uuid::new_v4(),
        reason: "manual_recompute".to_string(),
        ..job.clone()
    };
    manual.desired_versions.configuration_version = "manual-v2".to_string();
    assert!(store.enqueue_agent_analysis(&manual).await.unwrap());
    assert!(!store.enqueue_agent_analysis(&manual).await.unwrap());
    let claimed = claim(store, now + Duration::seconds(4)).await;
    assert_eq!(claimed.desired_versions, manual.desired_versions);
    manual.queue_item_id = Uuid::new_v4();
    manual.desired_versions.configuration_version = "manual-v3".to_string();
    assert!(store.enqueue_agent_analysis(&manual).await.unwrap());
    let trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    let stale_policy_report = analysis_fixture(&trace, &claimed.desired_versions, now);
    assert!(
        !store
            .append_agent_session_analysis(&stale_policy_report, Some(lease(&claimed)))
            .await
            .unwrap()
    );
    store
        .fail_agent_analysis(
            lease(&claimed),
            "old generation failed",
            None,
            now + Duration::seconds(5),
        )
        .await
        .unwrap();
    let newest = claim(store, now + Duration::seconds(5)).await;
    assert_eq!(newest.desired_versions, manual.desired_versions);
    assert_eq!(newest.attempts, 1);
    store
        .complete_agent_analysis(lease(&newest), now + Duration::seconds(6))
        .await
        .unwrap();
    let mut finalized = trace.session;
    finalized.lifecycle = SessionLifecycleState::Finalized;
    finalized.ended_at = Some(finalized.input_watermark_at);
    finalized.finalized_reason = Some("idle_gap".to_string());
    finalized.updated_at = now + Duration::seconds(7);
    assert!(
        store
            .finalize_agent_session_if_unchanged(&finalized, finalized.input_watermark_at, None)
            .await
            .unwrap()
    );
    let claimed = claim(store, now + Duration::seconds(7)).await;
    let trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(trace.session.lifecycle, SessionLifecycleState::Finalized);
    assert_eq!(trace.input_generation, 3);
    assert!(trace.latest_analysis.is_none());
    let report = analysis_fixture(
        &trace,
        &claimed.desired_versions,
        now + Duration::seconds(8),
    );
    assert!(
        store
            .append_agent_session_analysis(&report, Some(lease(&claimed)))
            .await
            .unwrap()
    );
    store
        .complete_agent_analysis(lease(&claimed), now + Duration::seconds(8))
        .await
        .unwrap();
    assert!(
        store
            .load_agent_session_trace(session.agent_session_id)
            .await
            .unwrap()
            .unwrap()
            .latest_analysis
            .is_some()
    );
}

async fn expired_lease_is_fenced(
    store: &(impl CounterStore + AgentSessionAnalysisRepository),
    session: &AgentSessionRecord,
    job: &AgentAnalysisQueueRecord,
) {
    let now = session.input_watermark_at + Duration::minutes(2);
    let request = AgentAnalysisQueueRecord {
        queue_item_id: Uuid::new_v4(),
        ..job.clone()
    };
    store.enqueue_agent_analysis(&request).await.unwrap();
    let expired = claim(store, now).await;
    let reclaimed = claim(store, now + Duration::minutes(1)).await;
    assert_eq!(expired.lease_owner, reclaimed.lease_owner);
    assert!(reclaimed.lease_epoch > expired.lease_epoch);
    assert_eq!(reclaimed.attempts, 2);
    let trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    let report = analysis_fixture(
        &trace,
        &reclaimed.desired_versions,
        now + Duration::minutes(1),
    );
    assert!(
        !store
            .append_agent_session_analysis(&report, Some(lease(&expired)))
            .await
            .unwrap()
    );
    assert!(
        store
            .complete_agent_analysis(lease(&expired), now + Duration::minutes(1))
            .await
            .is_err()
    );
    assert!(
        store
            .append_agent_session_analysis(&report, Some(lease(&reclaimed)))
            .await
            .unwrap()
    );
    store
        .complete_agent_analysis(lease(&reclaimed), now + Duration::minutes(1))
        .await
        .unwrap();
}

async fn claim(
    store: &(impl AgentSessionAnalysisRepository + Sync),
    now: OffsetDateTime,
) -> AgentAnalysisQueueRecord {
    store
        .claim_agent_analysis("worker", now, now + Duration::minutes(1))
        .await
        .unwrap()
        .unwrap()
}

fn lease(job: &AgentAnalysisQueueRecord) -> AgentAnalysisLease<'_> {
    AgentAnalysisLease {
        queue_item_id: job.queue_item_id,
        owner: job.lease_owner.as_deref().unwrap(),
        epoch: job.lease_epoch,
    }
}

fn job_fixture(session: &AgentSessionRecord) -> AgentAnalysisQueueRecord {
    let policy = agent_session_analysis::AnalysisPolicy::default();
    AgentAnalysisQueueRecord {
        queue_item_id: Uuid::new_v4(),
        agent_session_id: session.agent_session_id,
        reason: "new_input".to_string(),
        desired_versions: AgentAnalysisDesiredVersions {
            report_schema_version: policy.report_schema_version,
            boundary_policy_version: session.boundary_policy_version.clone(),
            observation_parser_version: "contract".to_string(),
            analyzer_version: policy.analyzer_version,
            score_policy_version: policy.score_policy_version,
            pricing_policy_version: "contract".to_string(),
            cohort_version: "contract".to_string(),
            configuration_version: policy.configuration_version,
            score_maturity: policy.maturity,
            calibration_approval_id: None,
        },
        status: AgentAnalysisQueueStatus::Pending,
        lease_owner: None,
        lease_epoch: 0,
        lease_expires_at: None,
        attempts: 0,
        max_attempts: 3,
        last_error: None,
        available_at: session.input_watermark_at,
        created_at: session.input_watermark_at,
        updated_at: session.input_watermark_at,
        completed_at: None,
    }
}

fn analysis_fixture(
    trace: &AgentSessionTraceRecord,
    versions: &AgentAnalysisDesiredVersions,
    now: OffsetDateTime,
) -> AgentSessionAnalysisRecord {
    let report = agent_session_analysis::analyze_session(
        &agent_session_analysis::SessionTrace {
            requests: vec![],
            activity_intervals: vec![],
            observations: vec![],
            lifecycle: trace.session.lifecycle,
            boundary_confidence: trace.session.boundary_confidence,
            evidence: agent_session_analysis::TraceEvidence::default(),
        },
        &agent_session_analysis::AnalysisPolicy {
            configuration_version: versions.configuration_version.clone(),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    AgentSessionAnalysisRecord {
        analysis_id: Uuid::new_v4(),
        agent_session_id: trace.session.agent_session_id,
        configuration_version: report.configuration_version.clone(),
        boundary_policy_version: trace.session.boundary_policy_version.clone(),
        input_watermark_at: trace.session.input_watermark_at,
        input_generation: trace.input_generation,
        observation_set_id: trace
            .latest_observation_set
            .as_ref()
            .unwrap()
            .observation_set_id,
        observation_parser_version: versions.observation_parser_version.clone(),
        pricing_policy_version: versions.pricing_policy_version.clone(),
        cohort_version: versions.cohort_version.clone(),
        cohort_fallback_level: 7,
        cohort_sample_size: 0,
        cohort_snapshot_digest: "none".to_string(),
        direct_mcp_snapshot_digest: "none".to_string(),
        analyzed_at: now,
        report,
        stale: false,
        superseded_by_analysis_id: None,
        expires_at: now + Duration::days(90),
        ownership_scope_key: trace.session.ownership_scope_key.clone(),
        user_id: trace.session.user_id,
        service_account_id: trace.session.service_account_id,
    }
}

async fn finalization_schedules_initial_job(
    store: &(impl CounterStore + AgentSessionAnalysisRepository),
    template: &AgentSessionRecord,
) {
    let session = open_fixture(template);
    store
        .insert_agent_session_if_absent(&session)
        .await
        .unwrap();
    let request = request_fixture(&session);
    let observations = observation_fixture(&request, 0);
    store
        .append_agent_session_input(AgentSessionInput {
            request: Some(&request),
            observations: AgentSessionInput::observations(&observations).observations,
            analysis: None,
        })
        .await
        .unwrap();
    let mut finalized = session.clone();
    finalized.lifecycle = SessionLifecycleState::Finalized;
    finalized.ended_at = Some(session.input_watermark_at);
    finalized.finalized_reason = Some("idle_gap".to_string());
    let job = job_fixture(&session);
    let invalid_job = AgentAnalysisQueueRecord {
        max_attempts: 0,
        ..job.clone()
    };
    assert!(
        store
            .finalize_agent_session_if_unchanged(
                &finalized,
                session.input_watermark_at,
                Some(&invalid_job)
            )
            .await
            .is_err()
    );
    let unchanged = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.session.lifecycle, SessionLifecycleState::Open);
    assert_eq!(unchanged.input_generation, 1);
    assert!(
        store
            .finalize_agent_session_if_unchanged(&finalized, session.input_watermark_at, Some(&job))
            .await
            .unwrap()
    );
    assert!(
        !store
            .finalize_agent_session_if_unchanged(&finalized, session.input_watermark_at, Some(&job))
            .await
            .unwrap()
    );
    let claimed = claim(store, session.input_watermark_at).await;
    assert_eq!(claimed.agent_session_id, session.agent_session_id);
    let trace = store
        .load_agent_session_trace(session.agent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(trace.session.lifecycle, SessionLifecycleState::Finalized);
    assert_eq!(trace.input_generation, 2);
    let report = analysis_fixture(
        &trace,
        &claimed.desired_versions,
        session.input_watermark_at,
    );
    assert!(
        store
            .append_agent_session_analysis(&report, Some(lease(&claimed)))
            .await
            .unwrap()
    );
    store
        .complete_agent_analysis(lease(&claimed), session.input_watermark_at)
        .await
        .unwrap();
}
