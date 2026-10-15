use async_trait::async_trait;
use gateway_core::{
    RateLimitConsumeResult, RateLimitCounterCharge, RateLimitCounterIncrement,
    RateLimitPolicyRecord, RateLimitRepository, StoreError,
};
use uuid::Uuid;

use crate::store::AnyStore;

#[async_trait]
impl RateLimitRepository for AnyStore {
    async fn list_rate_limit_policies(&self) -> Result<Vec<RateLimitPolicyRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.list_rate_limit_policies().await,
            Self::Postgres(store) => store.list_rate_limit_policies().await,
        }
    }

    async fn get_rate_limit_policy(
        &self,
        policy_id: Uuid,
    ) -> Result<Option<RateLimitPolicyRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.get_rate_limit_policy(policy_id).await,
            Self::Postgres(store) => store.get_rate_limit_policy(policy_id).await,
        }
    }

    async fn insert_rate_limit_policy(
        &self,
        policy: &RateLimitPolicyRecord,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => store.insert_rate_limit_policy(policy).await,
            Self::Postgres(store) => store.insert_rate_limit_policy(policy).await,
        }
    }

    async fn update_rate_limit_policy(
        &self,
        policy: &RateLimitPolicyRecord,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => store.update_rate_limit_policy(policy).await,
            Self::Postgres(store) => store.update_rate_limit_policy(policy).await,
        }
    }

    async fn delete_rate_limit_policy(&self, policy_id: Uuid) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => store.delete_rate_limit_policy(policy_id).await,
            Self::Postgres(store) => store.delete_rate_limit_policy(policy_id).await,
        }
    }

    async fn consume_rate_limits(
        &self,
        charges: &[RateLimitCounterCharge],
    ) -> Result<RateLimitConsumeResult, StoreError> {
        match self {
            Self::Libsql(store) => store.consume_rate_limits(charges).await,
            Self::Postgres(store) => store.consume_rate_limits(charges).await,
        }
    }

    async fn add_rate_limit_usage(
        &self,
        increments: &[RateLimitCounterIncrement],
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => store.add_rate_limit_usage(increments).await,
            Self::Postgres(store) => store.add_rate_limit_usage(increments).await,
        }
    }

    async fn purge_expired_rate_limit_counters(
        &self,
        now: i64,
        limit: i64,
    ) -> Result<u64, StoreError> {
        match self {
            Self::Libsql(store) => store.purge_expired_rate_limit_counters(now, limit).await,
            Self::Postgres(store) => store.purge_expired_rate_limit_counters(now, limit).await,
        }
    }
}
