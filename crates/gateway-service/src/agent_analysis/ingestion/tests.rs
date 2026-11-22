use std::sync::Mutex;

use async_trait::async_trait;
use gateway_core::{
    AgentSessionInput, AgentSessionInputAppendResult, AgentSessionTraceRepository,
    ApiKeyModelGrantMode, ApiKeyOwnerKind, RequestAttemptRecord,
};

use super::*;

#[derive(Default)]
struct WindowStore {
    state: Mutex<WindowState>,
}

#[derive(Default)]
struct WindowState {
    open: Option<AgentSessionRecord>,
    replacement_on_append: Option<AgentSessionRecord>,
    failed_finalizations: usize,
    finalized: Vec<AgentSessionRecord>,
    appended: Vec<(Uuid, Uuid)>,
}

#[async_trait]
impl AgentSessionTraceRepository for WindowStore {
    async fn get_open_agent_session(
        &self,
        _scope: &str,
        _source: Option<Uuid>,
        _harness: &str,
        _boundary: &str,
    ) -> Result<Option<AgentSessionRecord>, StoreError> {
        Ok(self.state.lock().unwrap().open.clone())
    }

    async fn insert_agent_session_if_absent(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<bool, StoreError> {
        let mut state = self.state.lock().unwrap();
        if state.open.is_some() {
            return Ok(false);
        }
        state.open = Some(session.clone());
        Ok(true)
    }

    async fn finalize_agent_session_if_unchanged(
        &self,
        session: &AgentSessionRecord,
        watermark: OffsetDateTime,
        analysis: Option<&AgentAnalysisQueueRecord>,
    ) -> Result<bool, StoreError> {
        let mut state = self.state.lock().unwrap();
        assert_eq!(analysis.unwrap().agent_session_id, session.agent_session_id);
        if state.failed_finalizations > 0 {
            state.failed_finalizations -= 1;
            return Ok(false);
        }
        let current = state.open.as_ref().unwrap();
        assert_eq!(current.agent_session_id, session.agent_session_id);
        assert_eq!(current.input_watermark_at, watermark);
        state.finalized.push(session.clone());
        state.open = None;
        Ok(true)
    }

    async fn append_agent_session_input(
        &self,
        input: AgentSessionInput<'_>,
    ) -> Result<AgentSessionInputAppendResult, StoreError> {
        let mut state = self.state.lock().unwrap();
        let request = input.request.unwrap();
        if let Some(replacement) = state.replacement_on_append.take() {
            state.open = Some(replacement);
            return Err(StoreError::AgentSessionWindowClosed(
                request.agent_session_id.to_string(),
            ));
        }
        let set = input.observations.unwrap().set;
        assert_eq!(
            state.open.as_ref().unwrap().agent_session_id,
            request.agent_session_id
        );
        assert_eq!(
            input.analysis.unwrap().agent_session_id,
            request.agent_session_id
        );
        assert_eq!(set.agent_session_id, request.agent_session_id);
        assert_eq!(
            set.observation_set_id,
            observation_set_id(
                request.agent_session_id,
                &request.request_id,
                set.source_watermark_at
            )
        );
        state
            .appended
            .push((request.agent_session_id, set.agent_session_id));
        Ok(AgentSessionInputAppendResult {
            request_inserted: true,
            observations_inserted: true,
            nested_facts_truncated: false,
        })
    }

    async fn upsert_agent_session_source(
        &self,
        _source: &AgentSessionSourceRecord,
    ) -> Result<AgentSessionSourceRecord, StoreError> {
        panic!("source resolution is outside the retry test")
    }

    async fn load_agent_session_source(
        &self,
        _id: Uuid,
    ) -> Result<Option<AgentSessionSourceRecord>, StoreError> {
        panic!("source resolution is outside the retry test")
    }

    async fn list_idle_agent_sessions(
        &self,
        _cutoff: OffsetDateTime,
        _limit: u32,
    ) -> Result<Vec<AgentSessionRecord>, StoreError> {
        panic!("idle worker is outside the retry test")
    }

    async fn list_agent_session_request_attempts(
        &self,
        _id: Uuid,
        _limit: u32,
    ) -> Result<Vec<RequestAttemptRecord>, StoreError> {
        panic!("report generation is outside the retry test")
    }

    async fn load_agent_observation_sets(
        &self,
        _id: Uuid,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError> {
        panic!("report generation is outside the retry test")
    }

    async fn load_agent_observation_sets_for_parser(
        &self,
        _id: Uuid,
        _parser_version: &str,
    ) -> Result<Vec<AgentObservationSetRecord>, StoreError> {
        panic!("report generation is outside the retry test")
    }

    async fn link_request_log_to_agent_session(
        &self,
        _link: &AgentRequestLogLinkRecord,
    ) -> Result<(), StoreError> {
        panic!("request has no retained log")
    }

    async fn load_agent_session_trace(
        &self,
        _id: Uuid,
    ) -> Result<Option<AgentSessionTraceRecord>, StoreError> {
        panic!("report generation is outside the retry test")
    }
}

fn request_input() -> PreparedPassiveRequestRecord {
    let auth = AuthenticatedApiKey {
        id: Uuid::from_u128(1),
        public_id: "key".into(),
        name: "test".into(),
        model_grant_mode: ApiKeyModelGrantMode::Explicit,
        owner_kind: ApiKeyOwnerKind::User,
        owner_user_id: Some(Uuid::from_u128(2)),
        owner_team_id: None,
        owner_service_account_id: None,
        access: Default::default(),
        oauth: None,
    };
    let metadata = agent_session_analysis::observations::request::extract_request_metadata(
        &json!({"messages": [{"role": "user", "content": "hello"}]}),
        &BTreeMap::new(),
        true,
        "omp",
    );
    let occurred_at = OffsetDateTime::UNIX_EPOCH + Duration::hours(2);
    PassiveRequestRecord {
        auth: &auth,
        request_id: "late-request",
        request_log_id: None,
        harness_key: "omp",
        harness_label: "OMP",
        metadata: &metadata,
        response_body: None,
        occurred_at,
        completed_at: occurred_at + Duration::seconds(1),
        terminal_success: Some(true),
        payload_truncated: false,
        requested_model_key: "model",
        operation: "responses",
        request_tags: json!({}),
        boundary_group_key: "boundary",
    }
    .prepare()
}

async fn assert_retry_respects_idle_gap(failed_finalizations: usize) {
    let store = WindowStore::default();
    let input = request_input();
    let ownership = PassiveRequestOwnership {
        ownership_scope_key: "scope".into(),
        analytics_team_id: None,
        session_source_id: None,
        normalized_session_id: None,
    };
    let versions = desired_versions_for_policy(&AnalysisPolicy::default());
    let mut initial = acquire_session_window(&store, &input, &ownership, &versions)
        .await
        .unwrap();
    // A concurrent capacity rollover lets an older, delayed request create the replacement.
    initial.agent_session_id = Uuid::from_u128(3);
    let mut replacement = initial.clone();
    replacement.agent_session_id = Uuid::from_u128(4);
    replacement.input_watermark_at = input.occurred_at - SESSION_IDLE_GAP;
    {
        let mut state = store.state.lock().unwrap();
        state.open = Some(initial.clone());
        state.replacement_on_append = Some(replacement.clone());
        state.failed_finalizations = failed_finalizations;
    }
    let request_link = AgentSessionRequestLinkRecord {
        agent_session_id: initial.agent_session_id,
        request_id: input.request_id.clone(),
        request_log_id: None,
        usage_event_id: None,
        ordinal: 0,
        execution_id: None,
        parent_execution_id: None,
        normalized_session_id: None,
        correlation_confidence: Confidence::Medium,
        limitation_codes: Vec::new(),
        occurred_at: input.occurred_at,
        completed_at: Some(input.completed_at),
        terminal_success: Some(true),
    };
    let mut observations = input.observations.clone();
    assign_observation_ids(initial.agent_session_id, &mut observations);
    let observation_set = AgentObservationSetRecord {
        observation_set_id: observation_set_id(
            initial.agent_session_id,
            &input.request_id,
            input.completed_at,
        ),
        agent_session_id: initial.agent_session_id,
        parser_version: OBSERVATION_PARSER_VERSION.into(),
        source_watermark_at: input.completed_at,
        coverage: json!({}),
        created_at: input.completed_at,
        observations,
    };
    let prepared = PreparedSessionInput {
        request_link,
        truncated_observation_set: observation_set.clone(),
        observation_set,
    };
    let result = append_passive_request(&store, &input, &ownership, initial, prepared, &versions)
        .await
        .unwrap();
    assert_ne!(result, replacement.agent_session_id);
    let state = store.state.lock().unwrap();
    assert_eq!(state.finalized.len(), 1);
    assert_eq!(
        state.finalized[0].agent_session_id,
        replacement.agent_session_id
    );
    assert_eq!(
        state.finalized[0].finalized_reason.as_deref(),
        Some("idle_gap")
    );
    assert_eq!(
        state.finalized[0].ended_at,
        Some(replacement.input_watermark_at)
    );
    assert_eq!(state.appended, vec![(result, result)]);
}

#[tokio::test]
async fn closed_window_retry_finalizes_an_idle_replacement() {
    assert_retry_respects_idle_gap(0).await;
}

#[tokio::test]
async fn closed_window_retry_rechecks_idle_gap_after_lost_finalization() {
    assert_retry_respects_idle_gap(1).await;
}
