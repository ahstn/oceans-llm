use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};

const COLUMNS: &str = "credential_id, provider_key, user_id, secret_ciphertext, secret_nonce, secret_key_id, created_at, updated_at, last_used_at";

fn decode(row: &libsql::Row) -> Result<ProviderUserCredentialRecord, StoreError> {
    let last_used_at: Option<i64> = row.get(8).map_err(to_query_error)?;
    Ok(ProviderUserCredentialRecord {
        credential_id: parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?,
        provider_key: row.get(1).map_err(to_query_error)?,
        user_id: parse_uuid(&row.get::<String>(2).map_err(to_query_error)?)?,
        secret_ciphertext: row.get(3).map_err(to_query_error)?,
        secret_nonce: row.get(4).map_err(to_query_error)?,
        secret_key_id: row.get(5).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.get(6).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.get(7).map_err(to_query_error)?)?,
        last_used_at: last_used_at.map(unix_to_datetime).transpose()?,
    })
}

#[async_trait]
impl ProviderUserCredentialRepository for LibsqlStore {
    async fn upsert_provider_user_credential(
        &self,
        input: &UpsertProviderUserCredentialRecord,
    ) -> Result<ProviderUserCredentialRecord, StoreError> {
        let credential_id = Uuid::new_v4();
        self.connection
            .execute(
                r#"INSERT INTO provider_user_credentials (
                credential_id, provider_key, user_id, secret_ciphertext, secret_nonce,
                secret_key_id, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
            ON CONFLICT(provider_key, user_id) DO UPDATE SET
                secret_ciphertext = excluded.secret_ciphertext,
                secret_nonce = excluded.secret_nonce,
                secret_key_id = excluded.secret_key_id,
                updated_at = excluded.updated_at,
                last_used_at = NULL"#,
                libsql::params![
                    credential_id.to_string(),
                    input.provider_key.as_str(),
                    input.user_id.to_string(),
                    input.secret_ciphertext.as_str(),
                    input.secret_nonce.as_str(),
                    input.secret_key_id.as_str(),
                    input.updated_at.unix_timestamp()
                ],
            )
            .await
            .map_err(to_write_error)?;
        self.get_provider_user_credential(&input.provider_key, input.user_id)
            .await?
            .ok_or_else(|| {
                StoreError::Unexpected(
                    "provider user credential was not found after upsert".to_string(),
                )
            })
    }

    async fn get_provider_user_credential(
        &self,
        provider_key: &str,
        user_id: Uuid,
    ) -> Result<Option<ProviderUserCredentialRecord>, StoreError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM provider_user_credentials WHERE provider_key = ?1 AND user_id = ?2"
        );
        let mut rows = self
            .connection
            .query(&sql, libsql::params![provider_key, user_id.to_string()])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode(&row))
            .transpose()
    }

    async fn delete_provider_user_credential(
        &self,
        provider_key: &str,
        user_id: Uuid,
    ) -> Result<bool, StoreError> {
        self.connection
            .execute(
                "DELETE FROM provider_user_credentials WHERE provider_key = ?1 AND user_id = ?2",
                libsql::params![provider_key, user_id.to_string()],
            )
            .await
            .map(|count| count > 0)
            .map_err(to_write_error)
    }

    async fn touch_provider_user_credential(
        &self,
        credential_id: Uuid,
        last_used_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        self.connection
            .execute(
                "UPDATE provider_user_credentials SET last_used_at = ?1 WHERE credential_id = ?2",
                libsql::params![last_used_at.unix_timestamp(), credential_id.to_string()],
            )
            .await
            .map(|count| count > 0)
            .map_err(to_write_error)
    }
}
