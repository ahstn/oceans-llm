use super::contracts::{CounterStore, observation_fixture, open_fixture, request_fixture};
use gateway_core::{
    AgentSessionAnalysisRepository, AgentSessionInput, AgentSessionListQuery, AgentSessionRecord,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub(super) async fn bulk_hydration(
    store: &(impl CounterStore + AgentSessionAnalysisRepository),
    template: &AgentSessionRecord,
) {
    let mut session = open_fixture(template);
    session.agent_session_source_id = None;
    session.harness_key = format!("bulk-contract-{}", session.agent_session_id);
    let base = OffsetDateTime::from_unix_timestamp(session.started_at.unix_timestamp()).unwrap();
    session.started_at = base + Duration::seconds(501);
    session.input_watermark_at = base + Duration::seconds(502);
    store
        .insert_agent_session_if_absent(&session)
        .await
        .unwrap();
    for index in 0_i64..501 {
        let mut request = request_fixture(&session);
        request.occurred_at = base + Duration::seconds(501 - index);
        request.completed_at = Some(request.occurred_at + Duration::seconds(1));
        let mut set = observation_fixture(&request, 0);
        set.coverage["index"] = index.into();
        if index == 0 {
            let observation = set.observations[0].clone();
            set.observations
                .extend((0..100).map(|_| gateway_core::InferredObservation {
                    observation_id: Uuid::new_v4(),
                    ..observation.clone()
                }));
        }
        store
            .append_agent_session_input(AgentSessionInput {
                request: Some(&request),
                observations: AgentSessionInput::observations(&set).observations,
                analysis: None,
            })
            .await
            .unwrap();
    }
    let sets = store
        .load_agent_observation_sets(session.agent_session_id)
        .await
        .unwrap();
    assert_eq!(
        sets.len(),
        501,
        "include both sides of the libSQL bulk-ID boundary"
    );
    assert_eq!(sets.first().unwrap().coverage["index"], 500);
    assert_eq!(sets.last().unwrap().coverage["index"], 0);
    assert_eq!(
        sets.last().unwrap().observations.len(),
        101,
        "include both write batches"
    );
    assert_eq!(
        sets.iter().map(|set| set.observations.len()).sum::<usize>(),
        601
    );
    assert!(
        sets.iter()
            .all(|set| set.agent_session_id == session.agent_session_id)
    );
    let page = store
        .list_agent_sessions(&AgentSessionListQuery {
            harness_key: Some(session.harness_key),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    let summary = &page.items[0];
    assert_eq!(summary.request_count, 501);
    assert_eq!(summary.input_generation, 501);
    assert_eq!(summary.session.started_at, base + Duration::seconds(1));
    assert_eq!(
        summary.session.input_watermark_at,
        base + Duration::seconds(502)
    );
    assert!(summary.session_source_hash.is_none() && summary.harness_label.is_none());
    let at_watermark = store
        .list_idle_agent_sessions(base + Duration::seconds(502), 200)
        .await
        .unwrap();
    assert!(
        !at_watermark
            .iter()
            .any(|candidate| candidate.agent_session_id == session.agent_session_id)
    );
    let idle = store
        .list_idle_agent_sessions(base + Duration::seconds(503), 200)
        .await
        .unwrap();
    assert!(
        idle.iter()
            .any(|candidate| candidate.agent_session_id == session.agent_session_id)
    );
    let mut finalized = summary.session.clone();
    finalized.lifecycle = gateway_core::SessionLifecycleState::Finalized;
    finalized.ended_at = Some(finalized.input_watermark_at);
    finalized.finalized_reason = Some("idle_gap".to_string());
    assert!(
        store
            .finalize_agent_session_if_unchanged(&finalized, finalized.input_watermark_at, None)
            .await
            .unwrap()
    );
    let idle = store
        .list_idle_agent_sessions(base + Duration::seconds(503), 200)
        .await
        .unwrap();
    assert!(
        !idle
            .iter()
            .any(|candidate| candidate.agent_session_id == session.agent_session_id)
    );
}
