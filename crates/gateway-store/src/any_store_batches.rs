use async_trait::async_trait;
use gateway_core::{
    BatchAccessScope, BatchItemPage, BatchItemQuery, BatchItemRecord, BatchJobRecord, BatchPage,
    BatchPollUpdate, BatchQuery, BatchRepository, BatchStatus, NewBatchJob, ProviderBatchState,
    StoreError,
};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

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
impl BatchRepository for AnyStore {
    async fn insert_batch(&self, batch: &NewBatchJob) -> Result<BatchJobRecord, StoreError> {
        dispatch_store!(self, insert_batch(batch))
    }

    async fn get_batch_by_idempotency_key(
        &self,
        api_key_id: Uuid,
        idempotency_key: &str,
    ) -> Result<Option<BatchJobRecord>, StoreError> {
        dispatch_store!(
            self,
            get_batch_by_idempotency_key(api_key_id, idempotency_key)
        )
    }

    async fn get_batch(
        &self,
        batch_id: Uuid,
        scope: BatchAccessScope,
    ) -> Result<BatchJobRecord, StoreError> {
        dispatch_store!(self, get_batch(batch_id, scope))
    }

    async fn list_batches(
        &self,
        query: &BatchQuery,
        scope: BatchAccessScope,
    ) -> Result<BatchPage, StoreError> {
        dispatch_store!(self, list_batches(query, scope))
    }

    async fn list_batch_items(
        &self,
        batch_id: Uuid,
        query: &BatchItemQuery,
        scope: BatchAccessScope,
    ) -> Result<BatchItemPage, StoreError> {
        dispatch_store!(self, list_batch_items(batch_id, query, scope))
    }

    async fn claim_batch_jobs(
        &self,
        worker_id: &str,
        now: OffsetDateTime,
        lease_expires_at: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<BatchJobRecord>, StoreError> {
        dispatch_store!(
            self,
            claim_batch_jobs(worker_id, now, lease_expires_at, limit)
        )
    }

    async fn mark_stale_batch_submissions_unknown(
        &self,
        now: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        dispatch_store!(self, mark_stale_batch_submissions_unknown(now))
    }

    async fn mark_batch_submitted(
        &self,
        batch_id: Uuid,
        worker_id: &str,
        state: &ProviderBatchState,
        next_poll_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            mark_batch_submitted(batch_id, worker_id, state, next_poll_at)
        )
    }

    async fn mark_batch_submission_failed(
        &self,
        batch_id: Uuid,
        worker_id: &str,
        status: BatchStatus,
        error: &Value,
        completed_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            mark_batch_submission_failed(batch_id, worker_id, status, error, completed_at)
        )
    }

    async fn apply_batch_poll_update(
        &self,
        batch_id: Uuid,
        worker_id: &str,
        update: &BatchPollUpdate,
    ) -> Result<(), StoreError> {
        dispatch_store!(self, apply_batch_poll_update(batch_id, worker_id, update))
    }

    async fn release_batch_lease_after_error(
        &self,
        batch_id: Uuid,
        worker_id: &str,
        error: &Value,
        next_poll_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        dispatch_store!(
            self,
            release_batch_lease_after_error(batch_id, worker_id, error, next_poll_at)
        )
    }

    async fn request_batch_cancel(
        &self,
        batch_id: Uuid,
        scope: BatchAccessScope,
        requested_at: OffsetDateTime,
    ) -> Result<BatchJobRecord, StoreError> {
        dispatch_store!(self, request_batch_cancel(batch_id, scope, requested_at))
    }

    async fn get_batch_items_for_worker(
        &self,
        batch_id: Uuid,
    ) -> Result<Vec<BatchItemRecord>, StoreError> {
        dispatch_store!(self, get_batch_items_for_worker(batch_id))
    }
}
