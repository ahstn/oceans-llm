use super::*;
use crate::shared::{
    active_toolset_for_server_sql, foreign_credentials_conflict, foreign_credentials_sql,
    grants_outside_team_sql, json_object_from_str, parse_uuid, serialize_json, stranded_grants,
    stranded_toolset, team_only_toolset_tool, unix_to_datetime,
};
use gateway_core::{
    ExternalMcpServerUpdate, McpServerGuardRow, McpServerOwnership, McpServerOwnershipGuard,
    McpServerVisibility, TransferExternalMcpServerOwnershipRecord, Updated,
};

fn decode_server(row: &libsql::Row) -> Result<ExternalMcpServerRecord, StoreError> {
    let mcp_server_id: String = row.get(0).map_err(to_query_error)?;
    let description: Option<String> = row.get(3).map_err(to_query_error)?;
    let transport: String = row.get(4).map_err(to_query_error)?;
    let auth_mode: String = row.get(6).map_err(to_query_error)?;
    let auth_config_json: String = row.get(7).map_err(to_query_error)?;
    let status: String = row.get(9).map_err(to_query_error)?;
    let last_discovery_status: Option<String> = row.get(10).map_err(to_query_error)?;
    let last_discovery_at: Option<i64> = row.get(11).map_err(to_query_error)?;
    let last_successful_discovery_at: Option<i64> = row.get(12).map_err(to_query_error)?;
    let last_error_summary: Option<String> = row.get(13).map_err(to_query_error)?;
    let last_tool_count: Option<i64> = row.get(14).map_err(to_query_error)?;
    let created_at: i64 = row.get(15).map_err(to_query_error)?;
    let updated_at: i64 = row.get(16).map_err(to_query_error)?;
    let disabled_at: Option<i64> = row.get(17).map_err(to_query_error)?;

    Ok(ExternalMcpServerRecord {
        mcp_server_id: parse_uuid(&mcp_server_id)?,
        server_key: row.get(1).map_err(to_query_error)?,
        display_name: row.get(2).map_err(to_query_error)?,
        description,
        transport: ExternalMcpTransport::from_db(&transport).ok_or_else(|| {
            StoreError::Serialization(format!("invalid external MCP transport `{transport}`"))
        })?,
        server_url: row.get(5).map_err(to_query_error)?,
        auth_mode: ExternalMcpAuthMode::from_db(&auth_mode).ok_or_else(|| {
            StoreError::Serialization(format!("invalid external MCP auth mode `{auth_mode}`"))
        })?,
        auth_config: json_object_from_str(&auth_config_json)?,
        timeout_ms: row.get(8).map_err(to_query_error)?,
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
    row: &libsql::Row,
    offset: i32,
) -> Result<McpServerOwnership, StoreError> {
    let owner_team_id: Option<String> = row.get(offset).map_err(to_query_error)?;
    let visibility: String = row.get(offset + 2).map_err(to_query_error)?;
    Ok(McpServerOwnership {
        owner_team_id: owner_team_id.as_deref().map(parse_uuid).transpose()?,
        owner_contact: row.get(offset + 1).map_err(to_query_error)?,
        visibility: McpServerVisibility::from_db(&visibility).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP server visibility `{visibility}`"))
        })?,
    })
}

fn decode_tool(row: &libsql::Row) -> Result<ExternalMcpToolRecord, StoreError> {
    let mcp_tool_id: String = row.get(0).map_err(to_query_error)?;
    let mcp_server_id: String = row.get(1).map_err(to_query_error)?;
    let description: Option<String> = row.get(4).map_err(to_query_error)?;
    let input_schema_json: String = row.get(5).map_err(to_query_error)?;
    let is_active: i64 = row.get(8).map_err(to_query_error)?;
    let first_discovered_at: i64 = row.get(9).map_err(to_query_error)?;
    let last_discovered_at: i64 = row.get(10).map_err(to_query_error)?;
    let deactivated_at: Option<i64> = row.get(11).map_err(to_query_error)?;

    Ok(ExternalMcpToolRecord {
        mcp_tool_id: parse_uuid(&mcp_tool_id)?,
        mcp_server_id: parse_uuid(&mcp_server_id)?,
        upstream_name: row.get(2).map_err(to_query_error)?,
        display_name: row.get(3).map_err(to_query_error)?,
        description,
        input_schema: serde_json::from_str(&input_schema_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        metadata: decode_tool_metadata(row, 12)?,
        schema_hash: row.get(6).map_err(to_query_error)?,
        schema_version: row.get(7).map_err(to_query_error)?,
        is_active: is_active == 1,
        first_discovered_at: unix_to_datetime(first_discovered_at)?,
        last_discovered_at: unix_to_datetime(last_discovered_at)?,
        deactivated_at: deactivated_at.map(unix_to_datetime).transpose()?,
    })
}

/// Decodes `title, annotations_json, output_schema_json` starting at `offset`.
pub(super) fn decode_tool_metadata(
    row: &libsql::Row,
    offset: i32,
) -> Result<McpToolMetadata, StoreError> {
    let title: Option<String> = row.get(offset).map_err(to_query_error)?;
    let annotations_json: Option<String> = row.get(offset + 1).map_err(to_query_error)?;
    let output_schema_json: Option<String> = row.get(offset + 2).map_err(to_query_error)?;
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
        && input
            .owner_contact
            .as_ref()
            .is_none_or(|contact| *contact == existing.ownership.owner_contact)
        && existing.ownership.visibility == input.visibility
        && !discovery_config_changed(existing, input)
}

async fn load_server(
    connection: &libsql::Connection,
    mcp_server_id: Uuid,
) -> Result<ExternalMcpServerRecord, StoreError> {
    let mut rows = connection
        .query(SERVER_SELECT_BY_ID, [mcp_server_id.to_string()])
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .ok_or_else(|| {
            StoreError::NotFound(format!("external MCP server `{mcp_server_id}` not found"))
        })
        .and_then(|row| decode_server(&row))
}

/// Checks `guard` inside a write transaction, which holds the write lock from
/// its start, so the server cannot change before the caller commits. With
/// `tool_id`, the tool must also belong to the guard's server.
/// Refuses a narrowing to `team_id` while access would be stranded: one of
/// the server's tools in an active tool set, or a grant held outside the team.
/// Run inside the narrowing write, under its write lock.
async fn refuse_stranded_access(
    connection: &libsql::Connection,
    mcp_server_id: Uuid,
    team_id: Uuid,
) -> Result<(), StoreError> {
    let mut rows = connection
        .query(
            &active_toolset_for_server_sql("?1"),
            [mcp_server_id.to_string()],
        )
        .await
        .map_err(to_query_error)?;
    if let Some(row) = rows.next().await.map_err(to_query_error)? {
        let toolset_key: String = row.get(0).map_err(to_query_error)?;
        return Err(stranded_toolset(&toolset_key));
    }
    drop(rows);
    let mut rows = connection
        .query(
            &grants_outside_team_sql("?1", "?2"),
            [mcp_server_id.to_string(), team_id.to_string()],
        )
        .await
        .map_err(to_query_error)?;
    let outside: i64 = match rows.next().await.map_err(to_query_error)? {
        Some(row) => row.get(0).map_err(to_query_error)?,
        None => 0,
    };
    drop(rows);
    if outside > 0 {
        return Err(stranded_grants(outside));
    }
    Ok(())
}

pub(super) async fn check_ownership_guard(
    connection: &libsql::Connection,
    guard: &McpServerOwnershipGuard,
    tool_id: Option<Uuid>,
) -> Result<(), StoreError> {
    let (sql, id) = match tool_id {
        Some(tool_id) => (
            "SELECT s.mcp_server_id, s.owner_team_id, s.owner_contact, s.visibility, s.server_url, s.auth_mode, s.auth_config_json FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id = ?1",
            tool_id,
        ),
        None => (
            "SELECT mcp_server_id, owner_team_id, owner_contact, visibility, server_url, auth_mode, auth_config_json FROM external_mcp_servers WHERE mcp_server_id = ?1",
            guard.mcp_server_id,
        ),
    };
    let mut rows = connection
        .query(sql, [id.to_string()])
        .await
        .map_err(to_query_error)?;
    let holds = match rows.next().await.map_err(to_query_error)? {
        Some(row) => {
            let mcp_server_id: String = row.get(0).map_err(to_query_error)?;
            let ownership = decode_server_ownership(&row, 1)?;
            let server_url: String = row.get(4).map_err(to_query_error)?;
            let auth_mode: String = row.get(5).map_err(to_query_error)?;
            let auth_config_json: String = row.get(6).map_err(to_query_error)?;
            let auth_config = json_object_from_str(&auth_config_json)?;
            guard.matches(&McpServerGuardRow {
                mcp_server_id: parse_uuid(&mcp_server_id)?,
                owner_team_id: ownership.owner_team_id,
                visibility: ownership.visibility,
                server_url: &server_url,
                auth_mode: ExternalMcpAuthMode::from_db(&auth_mode).ok_or_else(|| {
                    StoreError::Serialization(format!("invalid MCP auth mode `{auth_mode}`"))
                })?,
                auth_config: &auth_config,
            })
        }
        None => false,
    };
    while rows.next().await.map_err(to_query_error)?.is_some() {}
    if holds { Ok(()) } else { Err(guard.conflict()) }
}

/// Rejects tools of team-only servers, read inside the tool set write.
/// Tool sets are granted organisation-wide, so they hold only tools of servers
/// published to the organisation.
pub(super) async fn check_no_team_only_tools(
    connection: &libsql::Connection,
    tool_ids: &[Uuid],
) -> Result<(), StoreError> {
    if tool_ids.is_empty() {
        return Ok(());
    }
    let ids = serialize_json(&tool_ids.iter().map(Uuid::to_string).collect::<Vec<_>>())?;
    let mut rows = connection
        .query(
            "SELECT t.upstream_name, s.server_key FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id IN (SELECT value FROM json_each(?1)) AND s.visibility = 'team' ORDER BY s.server_key, t.upstream_name LIMIT 1",
            [ids],
        )
        .await
        .map_err(to_query_error)?;
    let Some(row) = rows.next().await.map_err(to_query_error)? else {
        return Ok(());
    };
    let tool: String = row.get(0).map_err(to_query_error)?;
    let server_key: String = row.get(1).map_err(to_query_error)?;
    while rows.next().await.map_err(to_query_error)?.is_some() {}
    Err(team_only_toolset_tool(&tool, &server_key))
}

macro_rules! server_columns {
    () => {
        "mcp_server_id, server_key, display_name, description, transport, server_url, auth_mode, auth_config_json, timeout_ms, status, last_discovery_status, last_discovery_at, last_successful_discovery_at, last_error_summary, last_tool_count, created_at, updated_at, disabled_at, owner_team_id, owner_contact, visibility"
    };
}

const SERVER_COLUMNS: &str = server_columns!();

const SERVER_SELECT_BY_ID: &str = concat!(
    "SELECT ",
    server_columns!(),
    " FROM external_mcp_servers WHERE mcp_server_id = ?1"
);
/// Grant columns, aliased `g`, in the order `decode_grant` reads them.
const GRANT_COLUMNS: &str = "g.grant_id, g.subject_kind, g.subject_id, g.target_kind, g.target_id, g.is_active, g.created_at, g.updated_at, g.revoked_at";
const TOOL_COLUMNS: &str = "mcp_tool_id, mcp_server_id, upstream_name, display_name, description, input_schema_json, schema_hash, schema_version, is_active, first_discovered_at, last_discovered_at, deactivated_at, title, annotations_json, output_schema_json";

#[async_trait]
impl McpRegistryRepository for LibsqlStore {
    async fn list_external_mcp_servers(
        &self,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE (?1 = 1 OR status != 'disabled') ORDER BY server_key"
        );
        let mut rows = self
            .connection
            .query(&sql, [if include_disabled { 1_i64 } else { 0_i64 }])
            .await
            .map_err(to_query_error)?;
        let mut servers = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            servers.push(decode_server(&row)?);
        }
        Ok(servers)
    }

    async fn list_external_mcp_servers_owned_by(
        &self,
        owner_team_id: Option<Uuid>,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE (?1 = 1 OR status != 'disabled') AND owner_team_id IS ?2 ORDER BY server_key"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    if include_disabled { 1_i64 } else { 0_i64 },
                    owner_team_id.map(|team_id| team_id.to_string())
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut servers = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            servers.push(decode_server(&row)?);
        }
        Ok(servers)
    }

    async fn list_external_mcp_servers_visible_to_team(
        &self,
        team_id: Uuid,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE (?1 = 1 OR status != 'disabled') AND (owner_team_id = ?2 OR visibility = 'organisation') ORDER BY server_key"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    if include_disabled { 1_i64 } else { 0_i64 },
                    team_id.to_string()
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut servers = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            servers.push(decode_server(&row)?);
        }
        Ok(servers)
    }

    async fn list_external_mcp_servers_by_ids(
        &self,
        mcp_server_ids: &[Uuid],
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        if mcp_server_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids = serialize_json(
            &mcp_server_ids
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>(),
        )?;
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE mcp_server_id IN (SELECT value FROM json_each(?1)) ORDER BY server_key"
        );
        let mut rows = self
            .connection
            .query(&sql, [ids])
            .await
            .map_err(to_query_error)?;
        let mut servers = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            servers.push(decode_server(&row)?);
        }
        Ok(servers)
    }

    async fn get_external_mcp_server(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<Option<ExternalMcpServerRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(SERVER_SELECT_BY_ID, [mcp_server_id.to_string()])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_server(&row))
            .transpose()
    }

    async fn get_external_mcp_server_by_key(
        &self,
        server_key: &str,
    ) -> Result<Option<ExternalMcpServerRecord>, StoreError> {
        let sql =
            format!("SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE server_key = ?1");
        let mut rows = self
            .connection
            .query(&sql, [server_key])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_server(&row))
            .transpose()
    }

    async fn create_external_mcp_server(
        &self,
        input: &NewExternalMcpServerRecord,
    ) -> Result<ExternalMcpServerRecord, StoreError> {
        let id = Uuid::new_v4();
        let auth_config_json = serialize_json(&input.auth_config)?;
        // `RETURNING` gives the row this insert wrote, before any later update.
        let mut rows = self
            .connection
            .query(
                &format!(
                    r#"
                INSERT INTO external_mcp_servers (
                    mcp_server_id, server_key, display_name, description, transport, server_url,
                    auth_mode, auth_config_json, timeout_ms, status, created_at, updated_at,
                    owner_team_id, owner_contact, visibility
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?10, ?11, ?12, ?13)
                RETURNING {SERVER_COLUMNS}
                "#
                ),
                libsql::params![
                    id.to_string(),
                    input.server_key.as_str(),
                    input.display_name.as_str(),
                    input.description.as_deref(),
                    input.transport.as_str(),
                    input.server_url.as_str(),
                    input.auth_mode.as_str(),
                    auth_config_json,
                    input.timeout_ms,
                    input.created_at.unix_timestamp(),
                    input.ownership.owner_team_id.map(|id| id.to_string()),
                    input.ownership.owner_contact.as_deref(),
                    input.ownership.visibility.as_str(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        let row = rows
            .next()
            .await
            .map_err(to_write_error)?
            .ok_or_else(|| StoreError::Unexpected("insert returned no row".to_string()))?;
        decode_server(&row)
    }

    async fn update_external_mcp_server(
        &self,
        input: &UpdateExternalMcpServerRecord,
    ) -> Result<ExternalMcpServerUpdate, StoreError> {
        let auth_config_json = serialize_json(&input.auth_config)?;
        // An immediate transaction holds the write lock, so the server read here is
        // exactly the one this update replaces.
        let tx = self.begin_transaction().await?;
        let existing = load_server(&tx, input.mcp_server_id).await?;
        if existing.ownership.owner_team_id != input.expected_owner_team_id
            || existing.ownership.visibility != input.expected_visibility
        {
            tx.commit().await.map_err(to_write_error)?;
            return Err(owner_changed(input.mcp_server_id));
        }
        if let Some(guard) = &input.expected_server
            && !guard.matches(&McpServerGuardRow::of(&existing))
        {
            tx.commit().await.map_err(to_write_error)?;
            return Err(guard.conflict());
        }
        // An omitted contact keeps the one read under the write lock, so a
        // concurrent contact change is never undone by a stale copy.
        let owner_contact = input
            .owner_contact
            .clone()
            .unwrap_or_else(|| existing.ownership.owner_contact.clone());
        if input.retarget_owner_credentials_only && existing.server_url != input.server_url {
            let mut rows = tx
                .query(
                    &foreign_credentials_sql("?1", "?2"),
                    libsql::params![
                        input.mcp_server_id.to_string(),
                        existing
                            .ownership
                            .owner_team_id
                            .map(|team_id| team_id.to_string())
                            .unwrap_or_default()
                    ],
                )
                .await
                .map_err(to_query_error)?;
            let foreign: i64 = match rows.next().await.map_err(to_query_error)? {
                Some(row) => row.get(0).map_err(to_query_error)?,
                None => 0,
            };
            drop(rows);
            if foreign > 0 {
                tx.commit().await.map_err(to_write_error)?;
                return Err(foreign_credentials_conflict(foreign));
            }
        }
        if server_settings_match(&existing, input) {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(ExternalMcpServerUpdate::unchanged(existing));
        }
        // Narrowing is checked here, in the same write as the settings, so a
        // refused narrowing saves nothing and a saved one needs no undo.
        if input.visibility == McpServerVisibility::Team
            && existing.ownership.visibility != McpServerVisibility::Team
            && let Some(team_id) = existing.ownership.owner_team_id
        {
            refuse_stranded_access(&tx, input.mcp_server_id, team_id).await?;
        }
        let invalidate_discovery = discovery_config_changed(&existing, input);
        let mut revoked_oauth_bindings = Vec::new();
        tx.execute(
            r#"
            UPDATE external_mcp_servers
            SET display_name = ?1, description = ?2, server_url = ?3, auth_mode = ?4,
                auth_config_json = ?5, timeout_ms = ?6, updated_at = ?7,
                owner_contact = ?9, visibility = ?10
            WHERE mcp_server_id = ?8
            "#,
            libsql::params![
                input.display_name.as_str(),
                input.description.as_deref(),
                input.server_url.as_str(),
                input.auth_mode.as_str(),
                auth_config_json,
                input.timeout_ms,
                input.updated_at.unix_timestamp(),
                input.mcp_server_id.to_string(),
                owner_contact.as_deref(),
                input.visibility.as_str(),
            ],
        )
        .await
        .map_err(to_write_error)?;
        if invalidate_discovery {
            tx.execute(
                r#"
                UPDATE external_mcp_tools
                SET is_active = 0, deactivated_at = ?1
                WHERE mcp_server_id = ?2 AND is_active = 1
                "#,
                libsql::params![
                    input.updated_at.unix_timestamp(),
                    input.mcp_server_id.to_string()
                ],
            )
            .await
            .map_err(to_write_error)?;
            tx.execute(
                r#"
                UPDATE external_mcp_servers
                SET last_discovery_status = NULL, last_discovery_at = NULL,
                    last_successful_discovery_at = NULL, last_error_summary = NULL,
                    last_tool_count = 0
                WHERE mcp_server_id = ?1
                "#,
                libsql::params![input.mcp_server_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
            // A dynamically registered OAuth client belongs to the endpoints it
            // was registered against. Drop it so the next connection registers
            // again, and its secret and users' codes never go to a new endpoint.
            tx.execute(
                "DELETE FROM mcp_oauth_clients WHERE mcp_server_id = ?1",
                libsql::params![input.mcp_server_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
            // A flow started against the old settings must not finish against
            // the new ones; its callback finds no state and is refused.
            tx.execute(
                "DELETE FROM mcp_oauth_states WHERE mcp_server_id = ?1",
                libsql::params![input.mcp_server_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
            // Users' OAuth tokens were issued by the old endpoints. Revoke them,
            // so a refresh token is never posted to a token endpoint that did
            // not issue it; each user connects again.
            // The revoked rows are returned so each revoke is audited.
            let mut rows = tx
                .query(
                    &format!(
                        "UPDATE mcp_upstream_credential_bindings SET revoked_at = ?2, updated_at = ?2 WHERE mcp_server_id = ?1 AND material_kind = 'oauth_tokens' AND revoked_at IS NULL RETURNING {}",
                        super::mcp_credentials::CREDENTIAL_COLUMNS
                    ),
                    libsql::params![
                        input.mcp_server_id.to_string(),
                        input.updated_at.unix_timestamp()
                    ],
                )
                .await
                .map_err(to_write_error)?;
            while let Some(row) = rows.next().await.map_err(to_write_error)? {
                revoked_oauth_bindings.push(super::mcp_credentials::decode_credential(&row)?);
            }
            drop(rows);
        }
        // Read inside the transaction, so `current` is the row this update wrote.
        let current = load_server(&tx, input.mcp_server_id).await?;
        tx.commit().await.map_err(to_query_error)?;
        Ok(ExternalMcpServerUpdate {
            current,
            previous: Some(existing),
            revoked_oauth_bindings,
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
        let mut rows = self
            .connection
            .query(
                &format!(
                    "UPDATE external_mcp_servers
                     SET status = 'disabled', disabled_at = ?1, updated_at = ?1,
                         last_discovery_status = 'disabled', last_discovery_at = ?1
                     WHERE mcp_server_id = ?2
                       AND status <> 'disabled'
                     RETURNING {SERVER_COLUMNS}"
                ),
                libsql::params![disabled_at.unix_timestamp(), mcp_server_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        if let Some(row) = rows.next().await.map_err(to_write_error)? {
            return Ok((decode_server(&row)?, true));
        }
        drop(rows);
        Ok((load_server(&self.connection, mcp_server_id).await?, false))
    }

    async fn disable_external_mcp_server_for_owner(
        &self,
        mcp_server_id: Uuid,
        expected_owner_team_id: Option<Uuid>,
        disabled_at: OffsetDateTime,
    ) -> Result<(ExternalMcpServerRecord, bool), StoreError> {
        // `IS` matches a NULL (platform) owner too, so the owner check and the
        // disable are one statement.
        let mut rows = self
            .connection
            .query(
                &format!(
                    "UPDATE external_mcp_servers
                     SET status = 'disabled', disabled_at = ?1, updated_at = ?1,
                         last_discovery_status = 'disabled', last_discovery_at = ?1
                     WHERE mcp_server_id = ?2
                       AND status <> 'disabled'
                       AND owner_team_id IS ?3
                     RETURNING {SERVER_COLUMNS}"
                ),
                libsql::params![
                    disabled_at.unix_timestamp(),
                    mcp_server_id.to_string(),
                    expected_owner_team_id.map(|id| id.to_string()),
                ],
            )
            .await
            .map_err(to_write_error)?;
        if let Some(row) = rows.next().await.map_err(to_write_error)? {
            return Ok((decode_server(&row)?, true));
        }
        drop(rows);
        let server = load_server(&self.connection, mcp_server_id).await?;
        if server.ownership.owner_team_id != expected_owner_team_id {
            return Err(owner_changed(mcp_server_id));
        }
        Ok((server, false))
    }

    async fn transfer_external_mcp_server_ownership(
        &self,
        input: &TransferExternalMcpServerOwnershipRecord,
    ) -> Result<Updated<ExternalMcpServerRecord>, StoreError> {
        // Read under the write lock, so `previous` is the owner this call replaced.
        let tx = self.begin_transaction().await?;
        let existing = load_server(&tx, input.mcp_server_id).await?;
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
        // Only a narrowing (to team-only, or to another team) can strand access.
        if input.require_no_stranded_access
            && input.ownership.visibility == McpServerVisibility::Team
            && let Some(team_id) = input.ownership.owner_team_id
            && (existing.ownership.visibility != McpServerVisibility::Team
                || existing.ownership.owner_team_id != Some(team_id))
        {
            refuse_stranded_access(&tx, input.mcp_server_id, team_id).await?;
        }
        tx.execute(
            r#"
            UPDATE external_mcp_servers
            SET owner_team_id = ?1, owner_contact = ?2, visibility = ?3, updated_at = ?4
            WHERE mcp_server_id = ?5
            "#,
            libsql::params![
                input.ownership.owner_team_id.map(|id| id.to_string()),
                input.ownership.owner_contact.as_deref(),
                input.ownership.visibility.as_str(),
                input.updated_at.unix_timestamp(),
                input.mcp_server_id.to_string(),
            ],
        )
        .await
        .map_err(to_write_error)?;
        let current = load_server(&tx, input.mcp_server_id).await?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(Updated {
            current,
            previous: Some(existing),
        })
    }

    async fn first_active_mcp_toolset_key_for_server(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<Option<String>, StoreError> {
        let mut rows = self
            .connection
            .query(
                &active_toolset_for_server_sql("?1"),
                [mcp_server_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| row.get(0).map_err(to_query_error))
            .transpose()
    }

    async fn count_mcp_tool_grants_outside_team(
        &self,
        mcp_server_id: Uuid,
        team_id: Uuid,
    ) -> Result<u64, StoreError> {
        let mut rows = self
            .connection
            .query(
                &grants_outside_team_sql("?1", "?2"),
                [mcp_server_id.to_string(), team_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        let count: i64 = match rows.next().await.map_err(to_query_error)? {
            Some(row) => row.get(0).map_err(to_query_error)?,
            None => 0,
        };
        Ok(count.max(0) as u64)
    }

    async fn list_mcp_tool_grants_for_owner_team(
        &self,
        team_id: Uuid,
        subject_kind: Option<McpToolGrantSubjectKind>,
        subject_id: Option<Uuid>,
    ) -> Result<Vec<McpToolGrantRecord>, StoreError> {
        let sql = format!(
            "SELECT {GRANT_COLUMNS} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.target_kind = 'tool' AND s.owner_team_id = ?1 AND (?2 IS NULL OR g.subject_kind = ?2) AND (?3 IS NULL OR g.subject_id = ?3) ORDER BY g.created_at, g.grant_id"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    team_id.to_string(),
                    subject_kind.map(|kind| kind.as_str()),
                    subject_id.map(|id| id.to_string())
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut grants = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            grants.push(super::mcp_access::decode_grant(&row)?);
        }
        Ok(grants)
    }

    async fn get_external_mcp_tool(
        &self,
        mcp_tool_id: Uuid,
    ) -> Result<Option<ExternalMcpToolRecord>, StoreError> {
        let sql = format!("SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_tool_id = ?1");
        let mut rows = self
            .connection
            .query(&sql, [mcp_tool_id.to_string()])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_tool(&row))
            .transpose()
    }

    async fn list_external_mcp_tools(
        &self,
        mcp_server_id: Uuid,
        include_inactive: bool,
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        let sql = format!(
            "SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_server_id = ?1 AND (?2 = 1 OR is_active = 1) ORDER BY upstream_name"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    mcp_server_id.to_string(),
                    if include_inactive { 1_i64 } else { 0_i64 }
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut tools = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            tools.push(decode_tool(&row)?);
        }
        Ok(tools)
    }

    async fn record_external_mcp_discovery_success(
        &self,
        run: &ExternalMcpDiscoveryRunRecord,
        tools: &[UpsertExternalMcpToolRecord],
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        let details_json = serialize_json(&run.details)?;
        let tx = self.begin_transaction().await?;
        if let Some(guard) = &run.expected_server {
            check_ownership_guard(&tx, guard, None).await?;
        }
        tx.execute(
            r#"
            INSERT INTO external_mcp_discovery_runs (
                discovery_run_id, mcp_server_id, status, started_at, finished_at,
                discovered_tool_count, active_tool_count, schema_set_hash, error_summary, details_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            libsql::params![
                run.discovery_run_id.to_string(),
                run.mcp_server_id.to_string(),
                run.status.as_str(),
                run.started_at.unix_timestamp(),
                run.finished_at.unix_timestamp(),
                run.discovered_tool_count,
                run.active_tool_count,
                run.schema_set_hash.as_deref(),
                run.error_summary.as_deref(),
                details_json,
            ],
        )
        .await
        .map_err(to_query_error)?;

        tx.execute(
            "UPDATE external_mcp_tools SET is_active = 0, deactivated_at = ?1 WHERE mcp_server_id = ?2",
            libsql::params![run.finished_at.unix_timestamp(), run.mcp_server_id.to_string()],
        )
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
            tx.execute(
                r#"
                INSERT INTO external_mcp_tools (
                    mcp_tool_id, mcp_server_id, upstream_name, display_name, description,
                    input_schema_json, schema_hash, schema_version, is_active,
                    first_discovered_at, last_discovered_at, deactivated_at,
                    title, annotations_json, output_schema_json, metadata_captured
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 1, ?8, ?8, NULL, ?9, ?10, ?11, 1)
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
                libsql::params![
                    Uuid::new_v4().to_string(),
                    tool.mcp_server_id.to_string(),
                    tool.upstream_name.as_str(),
                    tool.display_name.as_str(),
                    tool.description.as_deref(),
                    input_schema_json,
                    tool.schema_hash.as_str(),
                    run.finished_at.unix_timestamp(),
                    tool.metadata.title.as_deref(),
                    annotations_json,
                    output_schema_json,
                ],
            )
            .await
            .map_err(to_query_error)?;
        }

        tx.execute(
            r#"
            UPDATE external_mcp_servers
            SET last_discovery_status = ?1, last_discovery_at = ?2,
                last_successful_discovery_at = ?2, last_error_summary = NULL,
                last_tool_count = ?3, updated_at = ?2
            WHERE mcp_server_id = ?4
            "#,
            libsql::params![
                run.status.as_str(),
                run.finished_at.unix_timestamp(),
                run.active_tool_count,
                run.mcp_server_id.to_string(),
            ],
        )
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
        let tx = self.begin_transaction().await?;
        if let Some(guard) = &run.expected_server {
            check_ownership_guard(&tx, guard, None).await?;
        }
        tx.execute(
            r#"
            INSERT INTO external_mcp_discovery_runs (
                discovery_run_id, mcp_server_id, status, started_at, finished_at,
                discovered_tool_count, active_tool_count, schema_set_hash, error_summary, details_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            libsql::params![
                run.discovery_run_id.to_string(),
                run.mcp_server_id.to_string(),
                run.status.as_str(),
                run.started_at.unix_timestamp(),
                run.finished_at.unix_timestamp(),
                run.discovered_tool_count,
                run.active_tool_count,
                run.schema_set_hash.as_deref(),
                run.error_summary.as_deref(),
                details_json,
            ],
        )
        .await
        .map_err(to_query_error)?;
        tx.execute(
            r#"
            UPDATE external_mcp_servers
            SET last_discovery_status = ?1, last_discovery_at = ?2,
                last_error_summary = ?3, updated_at = ?2
            WHERE mcp_server_id = ?4
            "#,
            libsql::params![
                run.status.as_str(),
                run.finished_at.unix_timestamp(),
                run.error_summary.as_deref(),
                run.mcp_server_id.to_string(),
            ],
        )
        .await
        .map_err(to_query_error)?;
        tx.commit().await.map_err(to_query_error)?;
        Ok(())
    }
}
