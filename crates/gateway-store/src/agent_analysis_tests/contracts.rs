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
    assert!(store.append_agent_session_request(&request).await.unwrap());
    let finalized = AgentSessionRecord {
        lifecycle: SessionLifecycleState::Finalized,
        ended_at: Some(open.input_watermark_at),
        finalized_reason: Some("idle_gap".to_string()),
        ..open.clone()
    };
    assert!(
        store
            .finalize_agent_session_if_unchanged(&finalized, open.input_watermark_at)
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
            .finalize_agent_session_if_unchanged(&open, open.input_watermark_at)
            .await
            .unwrap()
    );
    let late = AgentSessionRequestLinkRecord {
        request_id: Uuid::new_v4().to_string(),
        completed_at: Some(open.input_watermark_at + Duration::seconds(1)),
        ..request
    };
    assert!(matches!(
        store.append_agent_session_request(&late).await,
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
