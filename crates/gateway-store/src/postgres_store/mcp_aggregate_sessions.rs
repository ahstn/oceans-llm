use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};

const SESSION_COLUMNS: &str = "session_id, token_hash, api_key_id, owner_kind, owner_user_id, owner_team_id, owner_service_account_id, protocol_version, initialized, expires_at, created_at, updated_at, revoked_at";

fn decode_session(row: &PgRow) -> Result<McpAggregateSessionRecord, StoreError> {
    let owner_kind: String = row.try_get(3).map_err(to_query_error)?;
    let owner_user_id: Option<String> = row.try_get(4).map_err(to_query_error)?;
    let owner_team_id: Option<String> = row.try_get(5).map_err(to_query_error)?;
    let owner_service_account_id: Option<String> = row.try_get(6).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.try_get(12).map_err(to_query_error)?;
    Ok(McpAggregateSessionRecord {
        session_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        token_hash: row.try_get(1).map_err(to_query_error)?,
        api_key_id: parse_uuid(&row.try_get::<String, _>(2).map_err(to_query_error)?)?,
        owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
            StoreError::Serialization(format!(
                "invalid MCP aggregate session owner `{owner_kind}`"
            ))
        })?,
        owner_user_id: owner_user_id.as_deref().map(parse_uuid).transpose()?,
        owner_team_id: owner_team_id.as_deref().map(parse_uuid).transpose()?,
        owner_service_account_id: owner_service_account_id
            .as_deref()
            .map(parse_uuid)
            .transpose()?,
        protocol_version: row.try_get(7).map_err(to_query_error)?,
        initialized: row.try_get::<i64, _>(8).map_err(to_query_error)? == 1,
        expires_at: unix_to_datetime(row.try_get(9).map_err(to_query_error)?)?,
        created_at: unix_to_datetime(row.try_get(10).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(11).map_err(to_query_error)?)?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

async fn load_session(
    pool: &PgPool,
    session_id: Uuid,
    token_hash: &str,
) -> Result<Option<McpAggregateSessionRecord>, StoreError> {
    let sql = format!(
        "SELECT {SESSION_COLUMNS} FROM mcp_aggregate_sessions WHERE session_id = $1 AND token_hash = $2"
    );
    let row = sqlx::query(&sql)
        .bind(session_id.to_string())
        .bind(token_hash)
        .fetch_optional(pool)
        .await
        .map_err(to_query_error)?;
    row.as_ref().map(decode_session).transpose()
}

#[async_trait]
impl McpAggregateSessionRepository for PostgresStore {
    async fn create_mcp_aggregate_session(
        &self,
        session: &NewMcpAggregateSessionRecord,
    ) -> Result<McpAggregateSessionRecord, StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mcp_aggregate_sessions (
                session_id, token_hash, api_key_id, owner_kind, owner_user_id, owner_team_id,
                owner_service_account_id, protocol_version, initialized, expires_at, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9, $10, $10)
            "#,
        )
        .bind(session.session_id.to_string())
        .bind(&session.token_hash)
        .bind(session.api_key_id.to_string())
        .bind(session.owner_kind.as_str())
        .bind(session.owner_user_id.map(|value| value.to_string()))
        .bind(session.owner_team_id.map(|value| value.to_string()))
        .bind(session.owner_service_account_id.map(|value| value.to_string()))
        .bind(&session.protocol_version)
        .bind(session.expires_at.unix_timestamp())
        .bind(session.created_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        load_session(&self.pool, session.session_id, &session.token_hash)
            .await?
            .ok_or_else(|| {
                StoreError::Unexpected("created MCP aggregate session was not found".to_string())
            })
    }

    async fn get_mcp_aggregate_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<McpAggregateSessionRecord>, StoreError> {
        let sql =
            format!("SELECT {SESSION_COLUMNS} FROM mcp_aggregate_sessions WHERE token_hash = $1");
        let row = sqlx::query(&sql)
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_session).transpose()
    }

    async fn update_mcp_aggregate_session_initialized(
        &self,
        session_id: Uuid,
        token_hash: &str,
        initialized_at: OffsetDateTime,
    ) -> Result<Option<McpAggregateSessionRecord>, StoreError> {
        sqlx::query(
            r#"
            UPDATE mcp_aggregate_sessions
            SET initialized = 1, updated_at = $1
            WHERE session_id = $2 AND token_hash = $3 AND revoked_at IS NULL
            "#,
        )
        .bind(initialized_at.unix_timestamp())
        .bind(session_id.to_string())
        .bind(token_hash)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        load_session(&self.pool, session_id, token_hash).await
    }

    async fn touch_mcp_aggregate_session(
        &self,
        session_id: Uuid,
        token_hash: &str,
        touched_at: OffsetDateTime,
    ) -> Result<Option<McpAggregateSessionRecord>, StoreError> {
        sqlx::query(
            r#"
            UPDATE mcp_aggregate_sessions
            SET updated_at = $1
            WHERE session_id = $2 AND token_hash = $3 AND revoked_at IS NULL
            "#,
        )
        .bind(touched_at.unix_timestamp())
        .bind(session_id.to_string())
        .bind(token_hash)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        load_session(&self.pool, session_id, token_hash).await
    }

    async fn revoke_mcp_aggregate_session(
        &self,
        session_id: Uuid,
        token_hash: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE mcp_aggregate_sessions
            SET revoked_at = $1, updated_at = $1
            WHERE session_id = $2 AND token_hash = $3 AND revoked_at IS NULL
            "#,
        )
        .bind(revoked_at.unix_timestamp())
        .bind(session_id.to_string())
        .bind(token_hash)
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        Ok(result.rows_affected() > 0)
    }
}
