use async_trait::async_trait;
use gateway_core::{
    AgentAnalysisQueueRecord, AgentAnalysisQueueRepository, AgentObservationSetAppendResult,
    AgentRequestLogLinkRecord, AgentSessionAnalysisRecord, AgentSessionListPage,
    AgentSessionListQuery, AgentSessionRecord, AgentSessionReportRepository,
    AgentSessionRequestLinkRecord, AgentSessionSourceRecord, AgentSessionTraceRecord,
    AgentSessionTraceRepository, RequestAttemptRecord, StoreError,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::store::AnyStore;

#[async_trait]
impl AgentSessionTraceRepository for AnyStore {
    async fn upsert_agent_session_source(
        &self,
        session: &AgentSessionSourceRecord,
    ) -> Result<AgentSessionSourceRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.upsert_agent_session_source(session).await,
            Self::Postgres(store) => store.upsert_agent_session_source(session).await,
        }
    }

    async fn load_agent_session_source(
        &self,
        session_id: Uuid,
    ) -> Result<Option<AgentSessionSourceRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.load_agent_session_source(session_id).await,
            Self::Postgres(store) => store.load_agent_session_source(session_id).await,
        }
    }

    async fn get_open_agent_session(
        &self,
        scope: &str,
        session_id: Option<Uuid>,
        harness_key: &str,
        boundary_group_key: &str,
    ) -> Result<Option<AgentSessionRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .get_open_agent_session(scope, session_id, harness_key, boundary_group_key)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .get_open_agent_session(scope, session_id, harness_key, boundary_group_key)
                    .await
            }
        }
    }

    async fn insert_agent_session_if_absent(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => store.insert_agent_session_if_absent(session).await,
            Self::Postgres(store) => store.insert_agent_session_if_absent(session).await,
        }
    }

    async fn update_agent_session_window(
        &self,
        session: &AgentSessionRecord,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => store.update_agent_session_window(session).await,
            Self::Postgres(store) => store.update_agent_session_window(session).await,
        }
    }

    async fn finalize_agent_session_if_unchanged(
        &self,
        session: &AgentSessionRecord,
        expected_input_watermark_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .finalize_agent_session_if_unchanged(session, expected_input_watermark_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .finalize_agent_session_if_unchanged(session, expected_input_watermark_at)
                    .await
            }
        }
    }

    async fn append_agent_session_request(
        &self,
        link: &AgentSessionRequestLinkRecord,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => store.append_agent_session_request(link).await,
            Self::Postgres(store) => store.append_agent_session_request(link).await,
        }
    }

    async fn count_agent_session_requests(&self, session_id: Uuid) -> Result<u64, StoreError> {
        match self {
            Self::Libsql(store) => store.count_agent_session_requests(session_id).await,
            Self::Postgres(store) => store.count_agent_session_requests(session_id).await,
        }
    }
    async fn list_agent_session_request_attempts(
        &self,
        session_id: Uuid,
        limit: u32,
    ) -> Result<Vec<RequestAttemptRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .list_agent_session_request_attempts(session_id, limit)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .list_agent_session_request_attempts(session_id, limit)
                    .await
            }
        }
    }

    async fn append_agent_observation_set(
        &self,
        set: &gateway_core::AgentObservationSetRecord,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => store.append_agent_observation_set(set).await,
            Self::Postgres(store) => store.append_agent_observation_set(set).await,
        }
    }

    async fn append_bounded_agent_observation_set(
        &self,
        set: &gateway_core::AgentObservationSetRecord,
        truncated_set: &gateway_core::AgentObservationSetRecord,
        maximum_nested_facts: usize,
    ) -> Result<AgentObservationSetAppendResult, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .append_bounded_agent_observation_set(set, truncated_set, maximum_nested_facts)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .append_bounded_agent_observation_set(set, truncated_set, maximum_nested_facts)
                    .await
            }
        }
    }

    async fn load_agent_observation_sets(
        &self,
        session_id: Uuid,
    ) -> Result<Vec<gateway_core::AgentObservationSetRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.load_agent_observation_sets(session_id).await,
            Self::Postgres(store) => store.load_agent_observation_sets(session_id).await,
        }
    }

    async fn link_request_log_to_agent_session(
        &self,
        link: &AgentRequestLogLinkRecord,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => store.link_request_log_to_agent_session(link).await,
            Self::Postgres(store) => store.link_request_log_to_agent_session(link).await,
        }
    }

    async fn load_agent_session_trace(
        &self,
        session_id: Uuid,
    ) -> Result<Option<AgentSessionTraceRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.load_agent_session_trace(session_id).await,
            Self::Postgres(store) => store.load_agent_session_trace(session_id).await,
        }
    }
}

#[async_trait]
impl AgentSessionReportRepository for AnyStore {
    async fn append_agent_session_analysis(
        &self,
        analysis: &AgentSessionAnalysisRecord,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => store.append_agent_session_analysis(analysis).await,
            Self::Postgres(store) => store.append_agent_session_analysis(analysis).await,
        }
    }

    async fn list_agent_sessions(
        &self,
        query: &AgentSessionListQuery,
    ) -> Result<AgentSessionListPage, StoreError> {
        match self {
            Self::Libsql(store) => store.list_agent_sessions(query).await,
            Self::Postgres(store) => store.list_agent_sessions(query).await,
        }
    }

    async fn mark_agent_session_analyses_stale(
        &self,
        session_id: Uuid,
        superseded_by: Option<Uuid>,
    ) -> Result<u64, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .mark_agent_session_analyses_stale(session_id, superseded_by)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .mark_agent_session_analyses_stale(session_id, superseded_by)
                    .await
            }
        }
    }
}

#[async_trait]
impl AgentAnalysisQueueRepository for AnyStore {
    async fn enqueue_agent_analysis(
        &self,
        item: &AgentAnalysisQueueRecord,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => store.enqueue_agent_analysis(item).await,
            Self::Postgres(store) => store.enqueue_agent_analysis(item).await,
        }
    }

    async fn claim_agent_analysis(
        &self,
        owner: &str,
        now: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<Option<AgentAnalysisQueueRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.claim_agent_analysis(owner, now, expires_at).await,
            Self::Postgres(store) => store.claim_agent_analysis(owner, now, expires_at).await,
        }
    }

    async fn renew_agent_analysis_lease(
        &self,
        item_id: Uuid,
        owner: &str,
        updated_at: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .renew_agent_analysis_lease(item_id, owner, updated_at, expires_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .renew_agent_analysis_lease(item_id, owner, updated_at, expires_at)
                    .await
            }
        }
    }

    async fn complete_agent_analysis(
        &self,
        item_id: Uuid,
        owner: &str,
        completed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .complete_agent_analysis(item_id, owner, completed_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .complete_agent_analysis(item_id, owner, completed_at)
                    .await
            }
        }
    }

    async fn fail_agent_analysis(
        &self,
        item_id: Uuid,
        owner: &str,
        error: &str,
        retry_at: Option<OffsetDateTime>,
        updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .fail_agent_analysis(item_id, owner, error, retry_at, updated_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .fail_agent_analysis(item_id, owner, error, retry_at, updated_at)
                    .await
            }
        }
    }

    async fn purge_expired_agent_analysis(
        &self,
        report_cutoff: OffsetDateTime,
        queue_cutoff: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .purge_expired_agent_analysis(report_cutoff, queue_cutoff)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .purge_expired_agent_analysis(report_cutoff, queue_cutoff)
                    .await
            }
        }
    }

    async fn purge_agent_analysis_before(
        &self,
        request_cutoff: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        match self {
            Self::Libsql(store) => store.purge_agent_analysis_before(request_cutoff).await,
            Self::Postgres(store) => store.purge_agent_analysis_before(request_cutoff).await,
        }
    }

    async fn delete_agent_analysis_for_owner(&self, scope: &str) -> Result<u64, StoreError> {
        match self {
            Self::Libsql(store) => store.delete_agent_analysis_for_owner(scope).await,
            Self::Postgres(store) => store.delete_agent_analysis_for_owner(scope).await,
        }
    }
}
