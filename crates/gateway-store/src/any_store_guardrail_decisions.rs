use async_trait::async_trait;
use gateway_core::{
    GuardrailDecisionEventRecord, GuardrailDecisionPage, GuardrailDecisionQuery,
    GuardrailDecisionRepository, StoreError,
};

use crate::store::AnyStore;

#[async_trait]
impl GuardrailDecisionRepository for AnyStore {
    async fn insert_guardrail_decision(
        &self,
        decision: &GuardrailDecisionEventRecord,
    ) -> Result<(), StoreError> {
        match self {
            Self::Libsql(store) => store.insert_guardrail_decision(decision).await,
            Self::Postgres(store) => store.insert_guardrail_decision(decision).await,
        }
    }

    async fn list_guardrail_decisions(
        &self,
        query: &GuardrailDecisionQuery,
    ) -> Result<GuardrailDecisionPage, StoreError> {
        match self {
            Self::Libsql(store) => store.list_guardrail_decisions(query).await,
            Self::Postgres(store) => store.list_guardrail_decisions(query).await,
        }
    }
}
