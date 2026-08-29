use async_trait::async_trait;
use gateway_core::{
    ProviderUserCredentialRecord, ProviderUserCredentialRepository, StoreError,
    UpsertProviderUserCredentialRecord,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AnyStore;

#[async_trait]
impl ProviderUserCredentialRepository for AnyStore {
    async fn upsert_provider_user_credential(
        &self,
        input: &UpsertProviderUserCredentialRecord,
    ) -> Result<ProviderUserCredentialRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.upsert_provider_user_credential(input).await,
            Self::Postgres(store) => store.upsert_provider_user_credential(input).await,
        }
    }

    async fn get_provider_user_credential(
        &self,
        provider_key: &str,
        user_id: Uuid,
    ) -> Result<Option<ProviderUserCredentialRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .get_provider_user_credential(provider_key, user_id)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .get_provider_user_credential(provider_key, user_id)
                    .await
            }
        }
    }

    async fn delete_provider_user_credential(
        &self,
        provider_key: &str,
        user_id: Uuid,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .delete_provider_user_credential(provider_key, user_id)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .delete_provider_user_credential(provider_key, user_id)
                    .await
            }
        }
    }

    async fn touch_provider_user_credential(
        &self,
        credential_id: Uuid,
        last_used_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .touch_provider_user_credential(credential_id, last_used_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .touch_provider_user_credential(credential_id, last_used_at)
                    .await
            }
        }
    }
}
