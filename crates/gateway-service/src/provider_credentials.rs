use std::sync::Arc;

use async_trait::async_trait;
use gateway_core::{
    GatewayError, ProviderError, ProviderUserCredentialRepository, ProviderUserTokenResolver,
    UpsertProviderUserCredentialRecord, decrypt_secret_with_key_and_aad,
    encrypt_secret_with_key_and_aad, validate_secret_key_env,
};
use time::OffsetDateTime;
use uuid::Uuid;

pub const PROVIDER_CREDENTIAL_KEY_ENV: &str = "OCEANS_PROVIDER_CREDENTIAL_ENCRYPTION_KEY";
pub const PROVIDER_CREDENTIAL_KEY_ID: &str = "env/OCEANS_PROVIDER_CREDENTIAL_ENCRYPTION_KEY";

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderCredentialStatus {
    pub provider_key: String,
    pub configured: bool,
    pub updated_at: Option<OffsetDateTime>,
    pub last_used_at: Option<OffsetDateTime>,
}

#[derive(Clone)]
pub struct ProviderCredentialService<S> {
    store: Arc<S>,
}

impl<S> ProviderCredentialService<S>
where
    S: ProviderUserCredentialRepository,
{
    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    pub fn validate_runtime_configuration() -> Result<(), GatewayError> {
        validate_secret_key_env(PROVIDER_CREDENTIAL_KEY_ENV)
    }

    pub async fn status(
        &self,
        provider_key: &str,
        user_id: Uuid,
    ) -> Result<ProviderCredentialStatus, GatewayError> {
        let credential = self
            .store
            .get_provider_user_credential(provider_key, user_id)
            .await?;
        Ok(ProviderCredentialStatus {
            provider_key: provider_key.to_string(),
            configured: credential.is_some(),
            updated_at: credential.as_ref().map(|value| value.updated_at),
            last_used_at: credential.and_then(|value| value.last_used_at),
        })
    }

    pub async fn upsert(
        &self,
        provider_key: &str,
        user_id: Uuid,
        token: &str,
    ) -> Result<ProviderCredentialStatus, GatewayError> {
        let token = token.trim();
        if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_whitespace) {
            return Err(GatewayError::InvalidRequest(
                "GitHub token must be a non-empty token without whitespace".to_string(),
            ));
        }
        let associated_data = credential_associated_data(provider_key, user_id);
        let encrypted = encrypt_secret_with_key_and_aad(
            token,
            associated_data.as_bytes(),
            PROVIDER_CREDENTIAL_KEY_ENV,
            PROVIDER_CREDENTIAL_KEY_ID,
            "provider user credential",
        )?;
        let record = self
            .store
            .upsert_provider_user_credential(&UpsertProviderUserCredentialRecord {
                provider_key: provider_key.to_string(),
                user_id,
                secret_ciphertext: encrypted.ciphertext,
                secret_nonce: encrypted.nonce,
                secret_key_id: encrypted.key_id.to_string(),
                updated_at: OffsetDateTime::now_utc(),
            })
            .await?;
        Ok(ProviderCredentialStatus {
            provider_key: record.provider_key,
            configured: true,
            updated_at: Some(record.updated_at),
            last_used_at: record.last_used_at,
        })
    }

    pub async fn delete(&self, provider_key: &str, user_id: Uuid) -> Result<bool, GatewayError> {
        self.store
            .delete_provider_user_credential(provider_key, user_id)
            .await
            .map_err(Into::into)
    }
}

#[async_trait]
impl<S> ProviderUserTokenResolver for ProviderCredentialService<S>
where
    S: ProviderUserCredentialRepository + 'static,
{
    async fn resolve_provider_user_token(
        &self,
        provider_key: &str,
        user_id: Uuid,
    ) -> Result<String, ProviderError> {
        let credential = self
            .store
            .get_provider_user_credential(provider_key, user_id)
            .await
            .map_err(|error| {
                ProviderError::Transport(format!(
                    "failed loading provider user credential: {error}"
                ))
            })?
            .ok_or_else(|| {
                ProviderError::InvalidRequest(format!(
                    "user `{user_id}` has no credential configured for provider `{provider_key}`"
                ))
            })?;
        let associated_data = credential_associated_data(provider_key, user_id);
        let token = decrypt_secret_with_key_and_aad(
            &credential.secret_ciphertext,
            &credential.secret_nonce,
            &credential.secret_key_id,
            associated_data.as_bytes(),
            PROVIDER_CREDENTIAL_KEY_ENV,
            PROVIDER_CREDENTIAL_KEY_ID,
            "provider user credential",
        )
        .map_err(|error| {
            ProviderError::Transport(format!(
                "failed decrypting provider user credential: {error}"
            ))
        })?;
        self.store
            .touch_provider_user_credential(credential.credential_id, OffsetDateTime::now_utc())
            .await
            .map_err(|error| {
                ProviderError::Transport(format!(
                    "failed updating provider user credential usage: {error}"
                ))
            })?;
        Ok(token)
    }
}

fn credential_associated_data(provider_key: &str, user_id: Uuid) -> String {
    format!(
        "provider-user-credential:v1:{}:{provider_key}:{user_id}",
        provider_key.len()
    )
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use gateway_core::{
        ProviderUserCredentialRecord, ProviderUserCredentialRepository, StoreError,
    };

    use super::*;

    #[derive(Default)]
    struct TestCredentialStore {
        records: Mutex<HashMap<(String, Uuid), ProviderUserCredentialRecord>>,
    }

    impl TestCredentialStore {
        fn copy_ciphertext_to_user(&self, provider_key: &str, from: Uuid, to: Uuid) {
            let mut records = self.records.lock().expect("credential records");
            let mut record = records
                .get(&(provider_key.to_string(), from))
                .expect("source credential")
                .clone();
            record.credential_id = Uuid::new_v4();
            record.user_id = to;
            records.insert((provider_key.to_string(), to), record);
        }
    }

    #[async_trait]
    impl ProviderUserCredentialRepository for TestCredentialStore {
        async fn upsert_provider_user_credential(
            &self,
            input: &UpsertProviderUserCredentialRecord,
        ) -> Result<ProviderUserCredentialRecord, StoreError> {
            let record = ProviderUserCredentialRecord {
                credential_id: Uuid::new_v4(),
                provider_key: input.provider_key.clone(),
                user_id: input.user_id,
                secret_ciphertext: input.secret_ciphertext.clone(),
                secret_nonce: input.secret_nonce.clone(),
                secret_key_id: input.secret_key_id.clone(),
                created_at: input.updated_at,
                updated_at: input.updated_at,
                last_used_at: None,
            };
            self.records
                .lock()
                .expect("credential records")
                .insert((input.provider_key.clone(), input.user_id), record.clone());
            Ok(record)
        }

        async fn get_provider_user_credential(
            &self,
            provider_key: &str,
            user_id: Uuid,
        ) -> Result<Option<ProviderUserCredentialRecord>, StoreError> {
            Ok(self
                .records
                .lock()
                .expect("credential records")
                .get(&(provider_key.to_string(), user_id))
                .cloned())
        }

        async fn delete_provider_user_credential(
            &self,
            provider_key: &str,
            user_id: Uuid,
        ) -> Result<bool, StoreError> {
            Ok(self
                .records
                .lock()
                .expect("credential records")
                .remove(&(provider_key.to_string(), user_id))
                .is_some())
        }

        async fn touch_provider_user_credential(
            &self,
            credential_id: Uuid,
            last_used_at: OffsetDateTime,
        ) -> Result<bool, StoreError> {
            let mut records = self.records.lock().expect("credential records");
            let Some(record) = records
                .values_mut()
                .find(|record| record.credential_id == credential_id)
            else {
                return Ok(false);
            };
            record.last_used_at = Some(last_used_at);
            Ok(true)
        }
    }

    #[tokio::test]
    async fn rejects_ciphertext_copied_to_another_user() {
        unsafe {
            std::env::set_var(
                PROVIDER_CREDENTIAL_KEY_ENV,
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            );
        }

        let store = Arc::new(TestCredentialStore::default());
        let service = ProviderCredentialService::new(store.clone());
        let provider_key = "github-copilot-user";
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();
        service
            .upsert(provider_key, user_a, "github-token-a")
            .await
            .expect("store token");
        store.copy_ciphertext_to_user(provider_key, user_a, user_b);

        assert_eq!(
            service
                .resolve_provider_user_token(provider_key, user_a)
                .await
                .expect("original owner token"),
            "github-token-a"
        );
        let error = service
            .resolve_provider_user_token(provider_key, user_b)
            .await
            .expect_err("copied ciphertext must fail");
        assert!(error.to_string().contains("could not be decrypted"));
    }
}
