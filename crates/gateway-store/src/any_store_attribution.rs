use async_trait::async_trait;
use gateway_core::{
    AttributionAgentSessionRecord, AttributionApplyOutcome, AttributionCommitRecord,
    AttributionCommitWrite, AttributionPullRequestRecord, AttributionPullRequestWrite,
    AttributionReportQuery, AttributionRepository, AttributionRequestFact,
    AttributionSessionCommitTime, AttributionSessionEventWrite, StoreError,
};
use time::OffsetDateTime;

use crate::store::AnyStore;

macro_rules! dispatch_store {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            AnyStore::Libsql(store) => store.$method($($arg),*).await,
            AnyStore::Postgres(store) => store.$method($($arg),*).await,
        }
    };
}

#[async_trait]
impl AttributionRepository for AnyStore {
    async fn apply_attribution_commit(
        &self,
        write: &AttributionCommitWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        dispatch_store!(self, apply_attribution_commit(write))
    }

    async fn apply_attribution_session_event(
        &self,
        write: &AttributionSessionEventWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        dispatch_store!(self, apply_attribution_session_event(write))
    }

    async fn apply_attribution_pull_request(
        &self,
        write: &AttributionPullRequestWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        dispatch_store!(self, apply_attribution_pull_request(write))
    }

    async fn list_attribution_commits(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionCommitRecord>, StoreError> {
        dispatch_store!(self, list_attribution_commits(query))
    }

    async fn list_attribution_pull_requests(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionPullRequestRecord>, StoreError> {
        dispatch_store!(self, list_attribution_pull_requests(query))
    }

    async fn list_attribution_agent_sessions(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionAgentSessionRecord>, StoreError> {
        dispatch_store!(self, list_attribution_agent_sessions(query))
    }

    async fn list_attribution_session_commit_times(
        &self,
        session_hashes: &[String],
        until: OffsetDateTime,
    ) -> Result<Vec<AttributionSessionCommitTime>, StoreError> {
        dispatch_store!(
            self,
            list_attribution_session_commit_times(session_hashes, until)
        )
    }

    async fn list_attribution_request_facts(
        &self,
        session_hashes: &[String],
        until: OffsetDateTime,
    ) -> Result<Vec<AttributionRequestFact>, StoreError> {
        dispatch_store!(self, list_attribution_request_facts(session_hashes, until))
    }

    async fn delete_attribution_for_owner(
        &self,
        ownership_scope_key: &str,
    ) -> Result<u64, StoreError> {
        dispatch_store!(self, delete_attribution_for_owner(ownership_scope_key))
    }
}
