use super::*;
use crate::shared::{
    grants_outside_team_sql, json_object_from_str, parse_uuid, serialize_json,
    team_only_toolset_tool, unix_to_datetime,
};
use gateway_core::{
    McpServerOwnership, McpServerOwnershipGuard, McpServerVisibility,
    TransferExternalMcpServerOwnershipRecord, Updated,
};

macro_rules! server_columns {
    () => {
        "mcp_server_id, server_key, display_name, description, transport, server_url, auth_mode, auth_config_json::text, timeout_ms, status, last_discovery_status, last_discovery_at, last_successful_discovery_at, last_error_summary, last_tool_count, created_at, updated_at, disabled_at, owner_team_id, owner_contact, visibility"
    };
}

const SERVER_COLUMNS: &str = server_columns!();

const SERVER_SELECT_BY_ID: &str = concat!(
    "SELECT ",
    server_columns!(),
    " FROM external_mcp_servers WHERE mcp_server_id = $1"
);
/// Grant columns, aliased `g`, in the order `decode_grant` reads them.
const GRANT_COLUMNS: &str = "g.grant_id, g.subject_kind, g.subject_id, g.target_kind, g.target_id, g.is_active, g.created_at, g.updated_at, g.revoked_at";
const TOOL_COLUMNS: &str = "mcp_tool_id, mcp_server_id, upstream_name, display_name, description, input_schema_json::text, schema_hash, schema_version, is_active, first_discovered_at, last_discovered_at, deactivated_at, title, annotations_json::text, output_schema_json::text";

fn decode_server(row: &PgRow) -> Result<ExternalMcpServerRecord, StoreError> {
    let mcp_server_id: String = row.try_get(0).map_err(to_query_error)?;
    let description: Option<String> = row.try_get(3).map_err(to_query_error)?;
    let transport: String = row.try_get(4).map_err(to_query_error)?;
    let auth_mode: String = row.try_get(6).map_err(to_query_error)?;
    let auth_config_json: String = row.try_get(7).map_err(to_query_error)?;
    let status: String = row.try_get(9).map_err(to_query_error)?;
    let last_discovery_status: Option<String> = row.try_get(10).map_err(to_query_error)?;
    let last_discovery_at: Option<i64> = row.try_get(11).map_err(to_query_error)?;
    let last_successful_discovery_at: Option<i64> = row.try_get(12).map_err(to_query_error)?;
    let last_error_summary: Option<String> = row.try_get(13).map_err(to_query_error)?;
    let last_tool_count: Option<i64> = row.try_get(14).map_err(to_query_error)?;
    let created_at: i64 = row.try_get(15).map_err(to_query_error)?;
    let updated_at: i64 = row.try_get(16).map_err(to_query_error)?;
    let disabled_at: Option<i64> = row.try_get(17).map_err(to_query_error)?;

    Ok(ExternalMcpServerRecord {
        mcp_server_id: parse_uuid(&mcp_server_id)?,
        server_key: row.try_get(1).map_err(to_query_error)?,
        display_name: row.try_get(2).map_err(to_query_error)?,
        description,
        transport: ExternalMcpTransport::from_db(&transport).ok_or_else(|| {
            StoreError::Serialization(format!("invalid external MCP transport `{transport}`"))
        })?,
        server_url: row.try_get(5).map_err(to_query_error)?,
        auth_mode: ExternalMcpAuthMode::from_db(&auth_mode).ok_or_else(|| {
            StoreError::Serialization(format!("invalid external MCP auth mode `{auth_mode}`"))
        })?,
        auth_config: json_object_from_str(&auth_config_json)?,
        timeout_ms: row.try_get(8).map_err(to_query_error)?,
        status: ExternalMcpServerStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid external MCP server status `{status}`"))
        })?,
        last_discovery_status: last_discovery_status
            .as_deref()
            .map(|value| {
                ExternalMcpDiscoveryStatus::from_db(value).ok_or_else(|| {
                    StoreError::Serialization(format!(
                        "invalid external MCP discovery status `{value}`"
                    ))
                })
            })
            .transpose()?,
        last_discovery_at: last_discovery_at.map(unix_to_datetime).transpose()?,
        last_successful_discovery_at: last_successful_discovery_at
            .map(unix_to_datetime)
            .transpose()?,
        last_error_summary,
        last_tool_count,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
        disabled_at: disabled_at.map(unix_to_datetime).transpose()?,
        ownership: decode_server_ownership(row, 18)?,
    })
}

/// Decodes `owner_team_id, owner_contact, visibility` starting at `offset`.
pub(super) fn decode_server_ownership(
    row: &PgRow,
    offset: usize,
) -> Result<McpServerOwnership, StoreError> {
    let owner_team_id: Option<String> = row.try_get(offset).map_err(to_query_error)?;
    let visibility: String = row.try_get(offset + 2).map_err(to_query_error)?;
    Ok(McpServerOwnership {
        owner_team_id: owner_team_id.as_deref().map(parse_uuid).transpose()?,
        owner_contact: row.try_get(offset + 1).map_err(to_query_error)?,
        visibility: McpServerVisibility::from_db(&visibility).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP server visibility `{visibility}`"))
        })?,
    })
}

fn decode_tool(row: &PgRow) -> Result<ExternalMcpToolRecord, StoreError> {
    let mcp_tool_id: String = row.try_get(0).map_err(to_query_error)?;
    let mcp_server_id: String = row.try_get(1).map_err(to_query_error)?;
    let description: Option<String> = row.try_get(4).map_err(to_query_error)?;
    let input_schema_json: String = row.try_get(5).map_err(to_query_error)?;
    let is_active: i64 = row.try_get(8).map_err(to_query_error)?;
    let first_discovered_at: i64 = row.try_get(9).map_err(to_query_error)?;
    let last_discovered_at: i64 = row.try_get(10).map_err(to_query_error)?;
    let deactivated_at: Option<i64> = row.try_get(11).map_err(to_query_error)?;

    Ok(ExternalMcpToolRecord {
        mcp_tool_id: parse_uuid(&mcp_tool_id)?,
        mcp_server_id: parse_uuid(&mcp_server_id)?,
        upstream_name: row.try_get(2).map_err(to_query_error)?,
        display_name: row.try_get(3).map_err(to_query_error)?,
        description,
        input_schema: serde_json::from_str(&input_schema_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        metadata: decode_tool_metadata(row, 12)?,
        schema_hash: row.try_get(6).map_err(to_query_error)?,
        schema_version: row.try_get(7).map_err(to_query_error)?,
        is_active: is_active == 1,
        first_discovered_at: unix_to_datetime(first_discovered_at)?,
        last_discovered_at: unix_to_datetime(last_discovered_at)?,
        deactivated_at: deactivated_at.map(unix_to_datetime).transpose()?,
    })
}

/// Decodes `title, annotations_json, output_schema_json` starting at `offset`.
pub(super) fn decode_tool_metadata(
    row: &PgRow,
    offset: usize,
) -> Result<McpToolMetadata, StoreError> {
    let title: Option<String> = row.try_get(offset).map_err(to_query_error)?;
    let annotations_json: Option<String> = row.try_get(offset + 1).map_err(to_query_error)?;
    let output_schema_json: Option<String> = row.try_get(offset + 2).map_err(to_query_error)?;
    Ok(McpToolMetadata {
        title,
        annotations: annotations_json
            .as_deref()
            .map(json_object_from_str)
            .transpose()?,
        output_schema: output_schema_json
            .as_deref()
            .map(|value| {
                serde_json::from_str(value)
                    .map_err(|error| StoreError::Serialization(error.to_string()))
            })
            .transpose()?,
    })
}

/// A settings save authorized against an owner the server no longer has.
fn owner_changed(mcp_server_id: Uuid) -> StoreError {
    StoreError::Conflict(format!(
        "external MCP server `{mcp_server_id}` changed owner or visibility; reload it and try again"
    ))
}

fn discovery_config_changed(
    existing: &ExternalMcpServerRecord,
    input: &UpdateExternalMcpServerRecord,
) -> bool {
    existing.server_url != input.server_url
        || existing.auth_mode != input.auth_mode
        || existing.auth_config != input.auth_config
}

/// Whether saving `input` would leave the server's settings as they are.
fn server_settings_match(
    existing: &ExternalMcpServerRecord,
    input: &UpdateExternalMcpServerRecord,
) -> bool {
    existing.display_name == input.display_name
        && existing.description == input.description
        && existing.timeout_ms == input.timeout_ms
        && existing.ownership.owner_contact == input.owner_contact
        && existing.ownership.visibility == input.visibility
        && !discovery_config_changed(existing, input)
}

async fn load_server(
    pool: &PgPool,
    mcp_server_id: Uuid,
) -> Result<ExternalMcpServerRecord, StoreError> {
    let row = sqlx::query(SERVER_SELECT_BY_ID)
        .bind(mcp_server_id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(to_query_error)?
        .ok_or_else(|| {
            StoreError::NotFound(format!("external MCP server `{mcp_server_id}` not found"))
        })?;
    decode_server(&row)
}

/// Checks `guard` inside a write transaction. `FOR SHARE` holds the server row
/// until the caller commits, so a transfer or narrowing waits for this write,
/// and one that committed first is the row read here. With `tool_id`, the tool
/// must also belong to the guard's server.
pub(super) async fn check_ownership_guard(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    guard: &McpServerOwnershipGuard,
    tool_id: Option<Uuid>,
) -> Result<(), StoreError> {
    let (sql, id) = match tool_id {
        Some(tool_id) => (
            "SELECT s.mcp_server_id, s.owner_team_id, s.owner_contact, s.visibility FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id = $1 FOR SHARE OF s",
            tool_id,
        ),
        None => (
            "SELECT mcp_server_id, owner_team_id, owner_contact, visibility FROM external_mcp_servers WHERE mcp_server_id = $1 FOR SHARE",
            guard.mcp_server_id,
        ),
    };
    let row = sqlx::query(sql)
        .bind(id.to_string())
        .fetch_optional(&mut **tx)
        .await
        .map_err(to_query_error)?;
    let holds = match row {
        Some(row) => {
            let mcp_server_id: String = row.try_get(0).map_err(to_query_error)?;
            let ownership = decode_server_ownership(&row, 1)?;
            guard.matches(
                parse_uuid(&mcp_server_id)?,
                ownership.owner_team_id,
                ownership.visibility,
            )
        }
        None => false,
    };
    if holds { Ok(()) } else { Err(guard.conflict()) }
}

/// Rejects tools of team-only servers, read inside the tool set write. Every
/// named tool's server is held `FOR SHARE` until commit, so none can be
/// narrowed to its team while the set is written. Tool sets are granted
/// organisation-wide, so they hold only tools of servers published to the
/// organisation.
pub(super) async fn check_no_team_only_tools(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tool_ids: &[Uuid],
) -> Result<(), StoreError> {
    if tool_ids.is_empty() {
        return Ok(());
    }
    let rows = sqlx::query(
        "SELECT t.upstream_name, s.server_key, s.visibility FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id = ANY($1) ORDER BY s.mcp_server_id, t.upstream_name FOR SHARE OF s",
    )
    .bind(tool_ids.iter().map(Uuid::to_string).collect::<Vec<_>>())
    .fetch_all(&mut **tx)
    .await
    .map_err(to_query_error)?;
    for row in rows {
        let visibility: String = row.try_get(2).map_err(to_query_error)?;
        if McpServerVisibility::from_db(&visibility) == Some(McpServerVisibility::Team) {
            let tool: String = row.try_get(0).map_err(to_query_error)?;
            let server_key: String = row.try_get(1).map_err(to_query_error)?;
            return Err(team_only_toolset_tool(&tool, &server_key));
        }
    }
    Ok(())
}

#[async_trait]
impl McpRegistryRepository for PostgresStore {
    async fn list_external_mcp_servers(
        &self,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE ($1::bigint = 1 OR status != 'disabled') ORDER BY server_key"
        );
        let rows = sqlx::query(&sql)
            .bind(if include_disabled { 1_i64 } else { 0_i64 })
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_server).collect()
    }

    async fn get_external_mcp_server(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<Option<ExternalMcpServerRecord>, StoreError> {
        sqlx::query(SERVER_SELECT_BY_ID)
            .bind(mcp_server_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_server)
            .transpose()
    }

    async fn get_external_mcp_server_by_key(
        &self,
        server_key: &str,
    ) -> Result<Option<ExternalMcpServerRecord>, StoreError> {
        let sql =
            format!("SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE server_key = $1");
        sqlx::query(&sql)
            .bind(server_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_server)
            .transpose()
    }

    async fn create_external_mcp_server(
        &self,
        input: &NewExternalMcpServerRecord,
    ) -> Result<ExternalMcpServerRecord, StoreError> {
        let id = Uuid::new_v4();
        let auth_config_json = serialize_json(&input.auth_config)?;
        // `RETURNING` gives the row this insert wrote, before any later update.
        let row = sqlx::query(&format!(
            r#"
            INSERT INTO external_mcp_servers (
                mcp_server_id, server_key, display_name, description, transport, server_url,
                auth_mode, auth_config_json, timeout_ms, status, created_at, updated_at,
                owner_team_id, owner_contact, visibility
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9, 'active', $10, $10, $11, $12, $13)
            RETURNING {SERVER_COLUMNS}
            "#
        ))
        .bind(id.to_string())
        .bind(input.server_key.as_str())
        .bind(input.display_name.as_str())
        .bind(input.description.as_deref())
        .bind(input.transport.as_str())
        .bind(input.server_url.as_str())
        .bind(input.auth_mode.as_str())
        .bind(auth_config_json)
        .bind(input.timeout_ms)
        .bind(input.created_at.unix_timestamp())
        .bind(input.ownership.owner_team_id.map(|id| id.to_string()))
        .bind(input.ownership.owner_contact.as_deref())
        .bind(input.ownership.visibility.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(to_write_error)?;
        decode_server(&row)
    }

    async fn update_external_mcp_server(
        &self,
        input: &UpdateExternalMcpServerRecord,
    ) -> Result<Updated<ExternalMcpServerRecord>, StoreError> {
        let auth_config_json = serialize_json(&input.auth_config)?;
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        // Locking the row makes the server read here the one this update replaces.
        let existing = sqlx::query(&format!("{SERVER_SELECT_BY_ID} FOR UPDATE"))
            .bind(input.mcp_server_id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| {
                StoreError::NotFound(format!(
                    "external MCP server `{}` not found",
                    input.mcp_server_id
                ))
            })
            .and_then(|row| decode_server(&row))?;
        if existing.ownership.owner_team_id != input.expected_owner_team_id
            || existing.ownership.visibility != input.expected_visibility
        {
            tx.commit().await.map_err(to_write_error)?;
            return Err(owner_changed(input.mcp_server_id));
        }
        if server_settings_match(&existing, input) {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(Updated::unchanged(existing));
        }
        let invalidate_discovery = discovery_config_changed(&existing, input);
        sqlx::query(
            r#"
            UPDATE external_mcp_servers
            SET display_name = $1, description = $2, server_url = $3, auth_mode = $4,
                auth_config_json = $5::jsonb, timeout_ms = $6, updated_at = $7,
                owner_contact = $9, visibility = $10
            WHERE mcp_server_id = $8
            "#,
        )
        .bind(input.display_name.as_str())
        .bind(input.description.as_deref())
        .bind(input.server_url.as_str())
        .bind(input.auth_mode.as_str())
        .bind(auth_config_json)
        .bind(input.timeout_ms)
        .bind(input.updated_at.unix_timestamp())
        .bind(input.mcp_server_id.to_string())
        .bind(input.owner_contact.as_deref())
        .bind(input.visibility.as_str())
        .execute(&mut *tx)
        .await
        .map_err(to_write_error)?;
        if invalidate_discovery {
            sqlx::query(
                r#"
                UPDATE external_mcp_tools
                SET is_active = 0, deactivated_at = $1
                WHERE mcp_server_id = $2 AND is_active = 1
                "#,
            )
            .bind(input.updated_at.unix_timestamp())
            .bind(input.mcp_server_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
            sqlx::query(
                r#"
                UPDATE external_mcp_servers
                SET last_discovery_status = NULL, last_discovery_at = NULL,
                    last_successful_discovery_at = NULL, last_error_summary = NULL,
                    last_tool_count = 0
                WHERE mcp_server_id = $1
                "#,
            )
            .bind(input.mcp_server_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
            // A dynamically registered OAuth client belongs to the endpoints it
            // was registered against. Drop it so the next connection registers
            // again, and its secret and users' codes never go to a new endpoint.
            sqlx::query("DELETE FROM mcp_oauth_clients WHERE mcp_server_id = $1")
                .bind(input.mcp_server_id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(to_write_error)?;
        }
        // Read inside the transaction, so `current` is the row this update wrote.
        let current = sqlx::query(SERVER_SELECT_BY_ID)
            .bind(input.mcp_server_id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(to_query_error)
            .and_then(|row| decode_server(&row))?;
        tx.commit().await.map_err(to_query_error)?;
        Ok(Updated {
            current,
            previous: Some(existing),
        })
    }

    async fn disable_external_mcp_server(
        &self,
        mcp_server_id: Uuid,
        disabled_at: OffsetDateTime,
    ) -> Result<(ExternalMcpServerRecord, bool), StoreError> {
        // Only an enabled server changes. `RETURNING` gives the row this call
        // disabled; with no row, the server was already disabled or is missing,
        // which `load_server` reports.
        let disabled = sqlx::query(&format!(
            "UPDATE external_mcp_servers
             SET status = 'disabled', disabled_at = $1, updated_at = $1,
                 last_discovery_status = 'disabled', last_discovery_at = $1
             WHERE mcp_server_id = $2
               AND status <> 'disabled'
             RETURNING {SERVER_COLUMNS}"
        ))
        .bind(disabled_at.unix_timestamp())
        .bind(mcp_server_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_write_error)?;
        match disabled {
            Some(row) => Ok((decode_server(&row)?, true)),
            None => Ok((load_server(&self.pool, mcp_server_id).await?, false)),
        }
    }

    async fn disable_external_mcp_server_for_owner(
        &self,
        mcp_server_id: Uuid,
        expected_owner_team_id: Option<Uuid>,
        disabled_at: OffsetDateTime,
    ) -> Result<(ExternalMcpServerRecord, bool), StoreError> {
        // The owner check and the disable are one statement.
        let row = sqlx::query(&format!(
            "UPDATE external_mcp_servers
             SET status = 'disabled', disabled_at = $1, updated_at = $1,
                 last_discovery_status = 'disabled', last_discovery_at = $1
             WHERE mcp_server_id = $2
               AND status <> 'disabled'
               AND owner_team_id IS NOT DISTINCT FROM $3
             RETURNING {SERVER_COLUMNS}"
        ))
        .bind(disabled_at.unix_timestamp())
        .bind(mcp_server_id.to_string())
        .bind(expected_owner_team_id.map(|id| id.to_string()))
        .fetch_optional(&self.pool)
        .await
        .map_err(to_write_error)?;
        if let Some(row) = row {
            return Ok((decode_server(&row)?, true));
        }
        let server = load_server(&self.pool, mcp_server_id).await?;
        if server.ownership.owner_team_id != expected_owner_team_id {
            return Err(owner_changed(mcp_server_id));
        }
        Ok((server, false))
    }

    async fn transfer_external_mcp_server_ownership(
        &self,
        input: &TransferExternalMcpServerOwnershipRecord,
    ) -> Result<Updated<ExternalMcpServerRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        // Locking the row makes `previous` the owner this call replaced.
        let existing = sqlx::query(&format!("{SERVER_SELECT_BY_ID} FOR UPDATE"))
            .bind(input.mcp_server_id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| {
                StoreError::NotFound(format!(
                    "external MCP server `{}` not found",
                    input.mcp_server_id
                ))
            })
            .and_then(|row| decode_server(&row))?;
        if input
            .expected_ownership
            .as_ref()
            .is_some_and(|expected| *expected != existing.ownership)
        {
            tx.commit().await.map_err(to_write_error)?;
            return Err(owner_changed(input.mcp_server_id));
        }
        if existing.ownership == input.ownership {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(Updated::unchanged(existing));
        }
        let current = sqlx::query(&format!(
            r#"
            UPDATE external_mcp_servers
            SET owner_team_id = $1, owner_contact = $2, visibility = $3, updated_at = $4
            WHERE mcp_server_id = $5
            RETURNING {SERVER_COLUMNS}
            "#
        ))
        .bind(input.ownership.owner_team_id.map(|id| id.to_string()))
        .bind(input.ownership.owner_contact.as_deref())
        .bind(input.ownership.visibility.as_str())
        .bind(input.updated_at.unix_timestamp())
        .bind(input.mcp_server_id.to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(to_write_error)
        .and_then(|row| decode_server(&row))?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(Updated {
            current,
            previous: Some(existing),
        })
    }

    async fn list_mcp_toolset_keys_for_server(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<Vec<String>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT ts.toolset_key
            FROM mcp_toolset_tools tst
            JOIN mcp_toolsets ts ON ts.toolset_id = tst.toolset_id
            JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id
            WHERE t.mcp_server_id = $1
            ORDER BY ts.toolset_key
            "#,
        )
        .bind(mcp_server_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter()
            .map(|row| row.try_get(0).map_err(to_query_error))
            .collect()
    }

    async fn count_mcp_tool_grants_outside_team(
        &self,
        mcp_server_id: Uuid,
        team_id: Uuid,
    ) -> Result<u64, StoreError> {
        let count: i64 = sqlx::query_scalar(&grants_outside_team_sql("$1", "$2"))
            .bind(mcp_server_id.to_string())
            .bind(team_id.to_string())
            .fetch_one(&self.pool)
            .await
            .map_err(to_query_error)?;
        Ok(count.max(0) as u64)
    }

    async fn list_mcp_tool_grants_for_owner_team(
        &self,
        team_id: Uuid,
    ) -> Result<Vec<McpToolGrantRecord>, StoreError> {
        let sql = format!(
            "SELECT {GRANT_COLUMNS} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.target_kind = 'tool' AND s.owner_team_id = $1 ORDER BY g.created_at, g.grant_id"
        );
        let rows = sqlx::query(&sql)
            .bind(team_id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(super::mcp_access::decode_grant).collect()
    }

    async fn get_external_mcp_tool(
        &self,
        mcp_tool_id: Uuid,
    ) -> Result<Option<ExternalMcpToolRecord>, StoreError> {
        let sql = format!("SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_tool_id = $1");
        sqlx::query(&sql)
            .bind(mcp_tool_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_tool)
            .transpose()
    }

    async fn list_external_mcp_tools(
        &self,
        mcp_server_id: Uuid,
        include_inactive: bool,
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        let sql = format!(
            "SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_server_id = $1 AND ($2::bigint = 1 OR is_active = 1) ORDER BY upstream_name"
        );
        let rows = sqlx::query(&sql)
            .bind(mcp_server_id.to_string())
            .bind(if include_inactive { 1_i64 } else { 0_i64 })
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_tool).collect()
    }

    async fn record_external_mcp_discovery_success(
        &self,
        run: &ExternalMcpDiscoveryRunRecord,
        tools: &[UpsertExternalMcpToolRecord],
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        let details_json = serialize_json(&run.details)?;
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        sqlx::query(
            r#"
            INSERT INTO external_mcp_discovery_runs (
                discovery_run_id, mcp_server_id, status, started_at, finished_at,
                discovered_tool_count, active_tool_count, schema_set_hash, error_summary, details_json
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb)
            "#,
        )
        .bind(run.discovery_run_id.to_string())
        .bind(run.mcp_server_id.to_string())
        .bind(run.status.as_str())
        .bind(run.started_at.unix_timestamp())
        .bind(run.finished_at.unix_timestamp())
        .bind(run.discovered_tool_count)
        .bind(run.active_tool_count)
        .bind(run.schema_set_hash.as_deref())
        .bind(run.error_summary.as_deref())
        .bind(details_json)
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;

        sqlx::query(
            "UPDATE external_mcp_tools SET is_active = 0, deactivated_at = $1 WHERE mcp_server_id = $2",
        )
        .bind(run.finished_at.unix_timestamp())
        .bind(run.mcp_server_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;

        for tool in tools {
            let input_schema_json = serialize_json(&tool.input_schema)?;
            let annotations_json = tool
                .metadata
                .annotations
                .as_ref()
                .map(serialize_json)
                .transpose()?;
            let output_schema_json = tool
                .metadata
                .output_schema
                .as_ref()
                .map(serialize_json)
                .transpose()?;
            // A row discovered before metadata was stored keeps its schema
            // version when only the newly captured metadata moved the hash.
            sqlx::query(
                r#"
                INSERT INTO external_mcp_tools (
                    mcp_tool_id, mcp_server_id, upstream_name, display_name, description,
                    input_schema_json, schema_hash, schema_version, is_active,
                    first_discovered_at, last_discovered_at, deactivated_at,
                    title, annotations_json, output_schema_json, metadata_captured
                ) VALUES (
                    $1, $2, $3, $4, $5, $6::jsonb, $7, 1, 1, $8, $8, NULL,
                    $9, $10::jsonb, $11::jsonb, 1
                )
                ON CONFLICT(mcp_server_id, upstream_name) DO UPDATE SET
                    display_name = excluded.display_name,
                    description = excluded.description,
                    input_schema_json = excluded.input_schema_json,
                    schema_version = CASE
                        WHEN external_mcp_tools.schema_hash != excluded.schema_hash
                            AND NOT (
                                external_mcp_tools.metadata_captured = 0
                                AND external_mcp_tools.input_schema_json = excluded.input_schema_json
                            )
                        THEN external_mcp_tools.schema_version + 1
                        ELSE external_mcp_tools.schema_version
                    END,
                    schema_hash = excluded.schema_hash,
                    title = excluded.title,
                    annotations_json = excluded.annotations_json,
                    output_schema_json = excluded.output_schema_json,
                    metadata_captured = 1,
                    is_active = 1,
                    last_discovered_at = excluded.last_discovered_at,
                    deactivated_at = NULL
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(tool.mcp_server_id.to_string())
            .bind(tool.upstream_name.as_str())
            .bind(tool.display_name.as_str())
            .bind(tool.description.as_deref())
            .bind(input_schema_json)
            .bind(tool.schema_hash.as_str())
            .bind(run.finished_at.unix_timestamp())
            .bind(tool.metadata.title.as_deref())
            .bind(annotations_json)
            .bind(output_schema_json)
            .execute(&mut *tx)
            .await
            .map_err(to_query_error)?;
        }

        sqlx::query(
            r#"
            UPDATE external_mcp_servers
            SET last_discovery_status = $1, last_discovery_at = $2,
                last_successful_discovery_at = $2, last_error_summary = NULL,
                last_tool_count = $3, updated_at = $2
            WHERE mcp_server_id = $4
            "#,
        )
        .bind(run.status.as_str())
        .bind(run.finished_at.unix_timestamp())
        .bind(run.active_tool_count)
        .bind(run.mcp_server_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;

        tx.commit().await.map_err(to_query_error)?;
        self.list_external_mcp_tools(run.mcp_server_id, false).await
    }

    async fn record_external_mcp_discovery_failure(
        &self,
        run: &ExternalMcpDiscoveryRunRecord,
    ) -> Result<(), StoreError> {
        let details_json = serialize_json(&run.details)?;
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        sqlx::query(
            r#"
            INSERT INTO external_mcp_discovery_runs (
                discovery_run_id, mcp_server_id, status, started_at, finished_at,
                discovered_tool_count, active_tool_count, schema_set_hash, error_summary, details_json
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb)
            "#,
        )
        .bind(run.discovery_run_id.to_string())
        .bind(run.mcp_server_id.to_string())
        .bind(run.status.as_str())
        .bind(run.started_at.unix_timestamp())
        .bind(run.finished_at.unix_timestamp())
        .bind(run.discovered_tool_count)
        .bind(run.active_tool_count)
        .bind(run.schema_set_hash.as_deref())
        .bind(run.error_summary.as_deref())
        .bind(details_json)
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;
        sqlx::query(
            r#"
            UPDATE external_mcp_servers
            SET last_discovery_status = $1, last_discovery_at = $2,
                last_error_summary = $3, updated_at = $2
            WHERE mcp_server_id = $4
            "#,
        )
        .bind(run.status.as_str())
        .bind(run.finished_at.unix_timestamp())
        .bind(run.error_summary.as_deref())
        .bind(run.mcp_server_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;
        tx.commit().await.map_err(to_query_error)?;
        Ok(())
    }
}
