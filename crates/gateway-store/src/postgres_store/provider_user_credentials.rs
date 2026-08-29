use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};

const COLUMNS: &str = "credential_id, provider_key, user_id, secret_ciphertext, secret_nonce, secret_key_id, created_at, updated_at, last_used_at";
const STATUS_COLUMNS: &str = "provider_key, user_id, updated_at, last_used_at";

fn decode(row: &PgRow) -> Result<ProviderUserCredentialRecord, StoreError> {
    let last_used_at: Option<i64> = row.try_get(8).map_err(to_query_error)?;
    Ok(ProviderUserCredentialRecord {
        credential_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        provider_key: row.try_get(1).map_err(to_query_error)?,
        user_id: parse_uuid(&row.try_get::<String, _>(2).map_err(to_query_error)?)?,
        secret_ciphertext: row.try_get(3).map_err(to_query_error)?,
        secret_nonce: row.try_get(4).map_err(to_query_error)?,
        secret_key_id: row.try_get(5).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.try_get(6).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(7).map_err(to_query_error)?)?,
        last_used_at: last_used_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_status(row: &PgRow) -> Result<ProviderUserCredentialStatusRecord, StoreError> {
    let last_used_at: Option<i64> = row.try_get(3).map_err(to_query_error)?;
    Ok(ProviderUserCredentialStatusRecord {
        provider_key: row.try_get(0).map_err(to_query_error)?,
        user_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(2).map_err(to_query_error)?)?,
        last_used_at: last_used_at.map(unix_to_datetime).transpose()?,
    })
}

#[async_trait]
impl ProviderUserCredentialRepository for PostgresStore {
    async fn upsert_provider_user_credential(
        &self,
        input: &UpsertProviderUserCredentialRecord,
    ) -> Result<ProviderUserCredentialStatusRecord, StoreError> {
        sqlx::query(r#"INSERT INTO provider_user_credentials (
            credential_id, provider_key, user_id, secret_ciphertext, secret_nonce, secret_key_id, created_at, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
        ON CONFLICT(provider_key, user_id) DO UPDATE SET
            credential_id = excluded.credential_id, secret_ciphertext = excluded.secret_ciphertext,
            secret_nonce = excluded.secret_nonce, secret_key_id = excluded.secret_key_id,
            created_at = excluded.created_at, updated_at = excluded.updated_at, last_used_at = NULL"#)
            .bind(Uuid::new_v4().to_string()).bind(&input.provider_key).bind(input.user_id.to_string())
            .bind(&input.secret_ciphertext).bind(&input.secret_nonce).bind(&input.secret_key_id)
            .bind(input.updated_at.unix_timestamp()).execute(&self.pool).await.map_err(to_write_error)?;
        self.get_provider_user_credential_status(&input.provider_key, input.user_id)
            .await?
            .ok_or_else(|| {
                StoreError::Unexpected(
                    "provider user credential was not found after upsert".to_string(),
                )
            })
    }

    async fn get_provider_user_credential_status(
        &self,
        provider_key: &str,
        user_id: Uuid,
    ) -> Result<Option<ProviderUserCredentialStatusRecord>, StoreError> {
        let sql = format!(
            "SELECT {STATUS_COLUMNS} FROM provider_user_credentials WHERE provider_key = $1 AND user_id = $2"
        );
        let row = sqlx::query(&sql)
            .bind(provider_key)
            .bind(user_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_status).transpose()
    }

    async fn list_provider_user_credential_statuses(
        &self,
        provider_key: &str,
    ) -> Result<Vec<ProviderUserCredentialStatusRecord>, StoreError> {
        let sql = format!(
            "SELECT {STATUS_COLUMNS} FROM provider_user_credentials WHERE provider_key = $1 ORDER BY user_id"
        );
        sqlx::query(&sql)
            .bind(provider_key)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?
            .iter()
            .map(decode_status)
            .collect()
    }

    async fn get_provider_user_credential(
        &self,
        provider_key: &str,
        user_id: Uuid,
    ) -> Result<Option<ProviderUserCredentialRecord>, StoreError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM provider_user_credentials WHERE provider_key = $1 AND user_id = $2"
        );
        let row = sqlx::query(&sql)
            .bind(provider_key)
            .bind(user_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode).transpose()
    }

    async fn delete_provider_user_credential(
        &self,
        provider_key: &str,
        user_id: Uuid,
    ) -> Result<bool, StoreError> {
        sqlx::query(
            "DELETE FROM provider_user_credentials WHERE provider_key = $1 AND user_id = $2",
        )
        .bind(provider_key)
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(to_write_error)
    }

    async fn touch_provider_user_credential(
        &self,
        credential_id: Uuid,
        last_used_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        sqlx::query(
            "UPDATE provider_user_credentials
             SET last_used_at = GREATEST(COALESCE(last_used_at, $1), $1)
             WHERE credential_id = $2",
        )
        .bind(last_used_at.unix_timestamp())
        .bind(credential_id.to_string())
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected() > 0)
        .map_err(to_write_error)
    }
}
