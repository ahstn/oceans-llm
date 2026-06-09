use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};

const SESSION_COLUMNS: &str = "session_id, token_hash, api_key_id, owner_kind, owner_user_id, owner_team_id, owner_service_account_id, protocol_version, initialized, expires_at, created_at, updated_at, revoked_at";

fn decode_session(row: &libsql::Row) -> Result<McpAggregateSessionRecord, StoreError> {
    let session_id: String = row.get(0).map_err(to_query_error)?;
    let api_key_id: String = row.get(2).map_err(to_query_error)?;
    let owner_kind: String = row.get(3).map_err(to_query_error)?;
    let owner_user_id: Option<String> = row.get(4).map_err(to_query_error)?;
    let owner_team_id: Option<String> = row.get(5).map_err(to_query_error)?;
    let owner_service_account_id: Option<String> = row.get(6).map_err(to_query_error)?;
    let initialized: i64 = row.get(8).map_err(to_query_error)?;
    let expires_at: i64 = row.get(9).map_err(to_query_error)?;
    let created_at: i64 = row.get(10).map_err(to_query_error)?;
    let updated_at: i64 = row.get(11).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(12).map_err(to_query_error)?;
    Ok(McpAggregateSessionRecord {
        session_id: parse_uuid(&session_id)?,
        token_hash: row.get(1).map_err(to_query_error)?,
        api_key_id: parse_uuid(&api_key_id)?,
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
        protocol_version: row.get(7).map_err(to_query_error)?,
        initialized: initialized == 1,
        expires_at: unix_to_datetime(expires_at)?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

async fn load_session(
    connection: &libsql::Connection,
    session_id: Uuid,
    token_hash: &str,
) -> Result<Option<McpAggregateSessionRecord>, StoreError> {
    let sql = format!(
        "SELECT {SESSION_COLUMNS} FROM mcp_aggregate_sessions WHERE session_id = ?1 AND token_hash = ?2"
    );
    let mut rows = connection
        .query(&sql, libsql::params![session_id.to_string(), token_hash])
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .map(|row| decode_session(&row))
        .transpose()
}

#[async_trait]
impl McpAggregateSessionRepository for LibsqlStore {
    async fn create_mcp_aggregate_session(
        &self,
        session: &NewMcpAggregateSessionRecord,
    ) -> Result<McpAggregateSessionRecord, StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO mcp_aggregate_sessions (
                    session_id, token_hash, api_key_id, owner_kind, owner_user_id, owner_team_id,
                    owner_service_account_id, protocol_version, initialized, expires_at, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, ?10)
                "#,
                libsql::params![
                    session.session_id.to_string(),
                    session.token_hash.as_str(),
                    session.api_key_id.to_string(),
                    session.owner_kind.as_str(),
                    session.owner_user_id.map(|value| value.to_string()),
                    session.owner_team_id.map(|value| value.to_string()),
                    session.owner_service_account_id.map(|value| value.to_string()),
                    session.protocol_version.as_str(),
                    session.expires_at.unix_timestamp(),
                    session.created_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        load_session(&self.connection, session.session_id, &session.token_hash)
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
            format!("SELECT {SESSION_COLUMNS} FROM mcp_aggregate_sessions WHERE token_hash = ?1");
        let mut rows = self
            .connection
            .query(&sql, [token_hash])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_session(&row))
            .transpose()
    }

    async fn update_mcp_aggregate_session_initialized(
        &self,
        session_id: Uuid,
        token_hash: &str,
        initialized_at: OffsetDateTime,
    ) -> Result<Option<McpAggregateSessionRecord>, StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE mcp_aggregate_sessions
                SET initialized = 1, updated_at = ?1
                WHERE session_id = ?2 AND token_hash = ?3 AND revoked_at IS NULL
                "#,
                libsql::params![
                    initialized_at.unix_timestamp(),
                    session_id.to_string(),
                    token_hash
                ],
            )
            .await
            .map_err(to_write_error)?;
        load_session(&self.connection, session_id, token_hash).await
    }

    async fn touch_mcp_aggregate_session(
        &self,
        session_id: Uuid,
        token_hash: &str,
        touched_at: OffsetDateTime,
    ) -> Result<Option<McpAggregateSessionRecord>, StoreError> {
        self.connection
            .execute(
                r#"
                UPDATE mcp_aggregate_sessions
                SET updated_at = ?1
                WHERE session_id = ?2 AND token_hash = ?3 AND revoked_at IS NULL
                "#,
                libsql::params![
                    touched_at.unix_timestamp(),
                    session_id.to_string(),
                    token_hash
                ],
            )
            .await
            .map_err(to_write_error)?;
        load_session(&self.connection, session_id, token_hash).await
    }

    async fn revoke_mcp_aggregate_session(
        &self,
        session_id: Uuid,
        token_hash: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let changed = self
            .connection
            .execute(
                r#"
                UPDATE mcp_aggregate_sessions
                SET revoked_at = ?1, updated_at = ?1
                WHERE session_id = ?2 AND token_hash = ?3 AND revoked_at IS NULL
                "#,
                libsql::params![
                    revoked_at.unix_timestamp(),
                    session_id.to_string(),
                    token_hash
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(changed > 0)
    }
}
