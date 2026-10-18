use super::*;
use std::collections::HashMap;

use crate::mcp_tool_policy::tool_policy_from_json;
use crate::mcp_tool_review::{
    HistoricalLookup, HistoricalVersion, StoredTool, StoredVersion, ToolWrite, VersionOp,
    discovered_definition, historical_lookups, keep_newest, plan_tool_writes,
    reuse_historical_versions, review_ops, reviewable_version, stale_discovery_check,
    tool_write_ops, unix_millis,
};
use crate::shared::{
    active_toolset_for_server_sql, foreign_credentials_conflict, foreign_credentials_sql,
    grants_outside_team_sql, json_object_from_str, parse_uuid, serialize_json, stranded_grants,
    stranded_toolset, team_only_toolset_tool, unix_to_datetime,
};
use gateway_core::{
    ExternalMcpServerUpdate, McpAnnotationOverrides, McpServerGuardRow, McpServerOwnership,
    McpServerOwnershipGuard, McpServerVisibility, McpToolApprovalStatus, McpToolDefinition,
    McpToolPolicy, McpToolReviewDecision, McpToolReviewState, McpToolVersionQuery,
    McpToolVersionRecord, McpToolVersionStatus, TransferExternalMcpServerOwnershipRecord, Updated,
};

mod tool_versions;
use tool_versions::*;

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
        approve_tool_changes: row.get::<i64>(18).map_err(to_query_error)? == 1,
        ownership: decode_server_ownership(row, 19)?,
        team_destinations: row.get::<i64>(22).map_err(to_query_error)? != 0,
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
        policy: decode_tool_policy(row, 15)?,
        review: decode_tool_review(row, 17)?,
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

/// Decodes `approval_status, approved_schema_version, pending_schema_version`
/// starting at `offset`.
pub(super) fn decode_tool_review(
    row: &libsql::Row,
    offset: i32,
) -> Result<McpToolReviewState, StoreError> {
    let status: String = row.get(offset).map_err(to_query_error)?;
    Ok(McpToolReviewState {
        approval_status: McpToolApprovalStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP tool approval status `{status}`"))
        })?,
        approved_schema_version: row.get(offset + 1).map_err(to_query_error)?,
        pending_schema_version: row.get(offset + 2).map_err(to_query_error)?,
    })
}

/// Decodes `admin_tags_json, annotation_overrides_json` starting at `offset`.
pub(super) fn decode_tool_policy(
    row: &libsql::Row,
    offset: i32,
) -> Result<McpToolPolicy, StoreError> {
    let tags_json: String = row.get(offset).map_err(to_query_error)?;
    let overrides_json: String = row.get(offset + 1).map_err(to_query_error)?;
    tool_policy_from_json(&tags_json, &overrides_json)
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
        && existing.approve_tool_changes == input.approve_tool_changes
        && !discovery_config_changed(existing, input)
        // A platform admin's save vouches for destinations a team set.
        && !(input.vouch_for_destinations && existing.team_destinations)
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
        "mcp_server_id, server_key, display_name, description, transport, server_url, auth_mode, auth_config_json, timeout_ms, status, last_discovery_status, last_discovery_at, last_successful_discovery_at, last_error_summary, last_tool_count, created_at, updated_at, disabled_at, approve_tool_changes, owner_team_id, owner_contact, visibility, team_destinations"
    };
}

/// Whether a discovery of the server that started after this one already
/// committed. Reads `external_mcp_discovery_runs_server_status_ms_idx`. A
/// gateway older than V72, still serving during a rolling upgrade, records
/// runs without `started_at_ms`; those compare by their whole-second start.
const NEWER_DISCOVERY_COMMITTED_SQL: &str = "SELECT EXISTS (SELECT 1 FROM external_mcp_discovery_runs WHERE mcp_server_id = ?1 AND status = ?2 AND (started_at_ms > ?3 OR (started_at_ms IS NULL AND started_at * 1000 > ?3)))";

const SERVER_COLUMNS: &str = server_columns!();

const SERVER_SELECT_BY_ID: &str = concat!(
    "SELECT ",
    server_columns!(),
    " FROM external_mcp_servers WHERE mcp_server_id = ?1"
);
/// Grant columns, aliased `g`, in the order `decode_grant` reads them.
const GRANT_COLUMNS: &str = "g.grant_id, g.subject_kind, g.subject_id, g.target_kind, g.target_id, g.is_active, g.created_at, g.updated_at, g.revoked_at";
const TOOL_COLUMNS: &str = "mcp_tool_id, mcp_server_id, upstream_name, display_name, description, input_schema_json, schema_hash, schema_version, is_active, first_discovered_at, last_discovered_at, deactivated_at, title, annotations_json, output_schema_json, admin_tags_json, annotation_overrides_json, approval_status, approved_schema_version, pending_schema_version";

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
        // Two branches rather than `owner_team_id = ?2 OR visibility = ...`:
        // SQLite would scan the whole table in key order for the OR, while
        // each branch searches its own index, and both come out in key order,
        // so they merge without a sort. The second branch leaves out the
        // team's own servers, which the first already returned.
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE (?1 = 1 OR status != 'disabled') AND owner_team_id = ?2 \
             UNION ALL \
             SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE (?1 = 1 OR status != 'disabled') AND visibility = 'organisation' AND owner_team_id IS NOT ?2 \
             ORDER BY server_key"
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

    async fn list_mcp_server_ownerships(
        &self,
        mcp_server_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, McpServerOwnership)>, StoreError> {
        if mcp_server_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids = serialize_json(
            &mcp_server_ids
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>(),
        )?;
        let mut rows = self
            .connection
            .query(
                "SELECT mcp_server_id, owner_team_id, owner_contact, visibility FROM external_mcp_servers WHERE mcp_server_id IN (SELECT value FROM json_each(?1))",
                [ids],
            )
            .await
            .map_err(to_query_error)?;
        let mut ownerships = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            let mcp_server_id: String = row.get(0).map_err(to_query_error)?;
            ownerships.push((
                parse_uuid(&mcp_server_id)?,
                decode_server_ownership(&row, 1)?,
            ));
        }
        Ok(ownerships)
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

    async fn list_external_mcp_servers_by_keys(
        &self,
        server_keys: &[String],
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        if server_keys.is_empty() {
            return Ok(Vec::new());
        }
        let keys = serialize_json(&server_keys)?;
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE server_key IN (SELECT value FROM json_each(?1)) ORDER BY server_key"
        );
        let mut rows = self
            .connection
            .query(&sql, [keys])
            .await
            .map_err(to_query_error)?;
        let mut servers = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            servers.push(decode_server(&row)?);
        }
        Ok(servers)
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
                    approve_tool_changes, owner_team_id, owner_contact, visibility
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?10, ?11, ?12, ?13, ?14)
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
                    i64::from(input.approve_tool_changes),
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
                    &foreign_credentials_sql("?1", "?2", "?3"),
                    libsql::params![
                        input.mcp_server_id.to_string(),
                        existing
                            .ownership
                            .owner_team_id
                            .map(|team_id| team_id.to_string())
                            .unwrap_or_default(),
                        input.updated_at.unix_timestamp()
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
                owner_contact = ?9, visibility = ?10,
                team_destinations = CASE WHEN ?11 = 1 THEN 0 ELSE team_destinations END,
                approve_tool_changes = ?12
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
                i64::from(input.vouch_for_destinations),
                i64::from(input.approve_tool_changes),
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
            SET owner_team_id = ?1, owner_contact = ?2, visibility = ?3, updated_at = ?4,
                -- A server leaving a team keeps the destinations it set
                -- unvouched, and so keeps the public-address guard.
                team_destinations = CASE WHEN owner_team_id IS NOT NULL THEN 1 ELSE team_destinations END
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
            let server = load_server(&tx, run.mcp_server_id).await?;
            let mut rows = tx
                .query(
                    NEWER_DISCOVERY_COMMITTED_SQL,
                    libsql::params![
                        run.mcp_server_id.to_string(),
                        ExternalMcpDiscoveryStatus::Success.as_str(),
                        unix_millis(run.started_at),
                    ],
                )
                .await
                .map_err(to_query_error)?;
            let newer_run_committed: i64 = rows
                .next()
                .await
                .map_err(to_query_error)?
                .ok_or_else(|| StoreError::Unexpected("EXISTS returned no row".to_string()))?
                .get(0)
                .map_err(to_query_error)?;
            stale_discovery_check(
                server.status == ExternalMcpServerStatus::Active,
                newer_run_committed != 0,
            )?;
        }
        tx.execute(
            r#"
            INSERT INTO external_mcp_discovery_runs (
                discovery_run_id, mcp_server_id, status, started_at, finished_at,
                discovered_tool_count, active_tool_count, schema_set_hash, error_summary, details_json,
                started_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                unix_millis(run.started_at),
            ],
        )
        .await
        .map_err(to_query_error)?;

        let approve_tool_changes = load_server(&tx, run.mcp_server_id)
            .await?
            .approve_tool_changes;
        let stored = load_stored_tools(&tx, run.mcp_server_id).await?;
        tx.execute(
            "UPDATE external_mcp_tools SET is_active = 0, deactivated_at = ?1 WHERE mcp_server_id = ?2",
            libsql::params![run.finished_at.unix_timestamp(), run.mcp_server_id.to_string()],
        )
        .await
        .map_err(to_query_error)?;

        let mut planned = plan_tool_writes(&stored, tools, approve_tool_changes);
        let historical = find_versions_by_definition(&tx, &historical_lookups(&planned)).await?;
        reuse_historical_versions(&mut planned, &historical);
        for plan in &planned {
            apply_tool_write(&tx, run.finished_at, plan.tool, plan.existing, &plan.write).await?;
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
                discovered_tool_count, active_tool_count, schema_set_hash, error_summary, details_json,
                started_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                unix_millis(run.started_at),
            ],
        )
        .await
        .map_err(to_query_error)?;
        // The run stays in the history, but a success that started later
        // remains the server's last discovery.
        tx.execute(
            r#"
            UPDATE external_mcp_servers
            SET last_discovery_status = ?1, last_discovery_at = ?2,
                last_error_summary = ?3, updated_at = ?2
            WHERE mcp_server_id = ?4
              AND NOT EXISTS (
                SELECT 1 FROM external_mcp_discovery_runs
                WHERE mcp_server_id = ?4 AND status = ?5
                  AND (started_at_ms > ?6 OR (started_at_ms IS NULL AND started_at * 1000 > ?6))
              )
            "#,
            libsql::params![
                run.status.as_str(),
                run.finished_at.unix_timestamp(),
                run.error_summary.as_deref(),
                run.mcp_server_id.to_string(),
                ExternalMcpDiscoveryStatus::Success.as_str(),
                unix_millis(run.started_at),
            ],
        )
        .await
        .map_err(to_query_error)?;
        tx.commit().await.map_err(to_query_error)?;
        Ok(())
    }

    async fn get_external_mcp_tool(
        &self,
        mcp_tool_id: Uuid,
    ) -> Result<Option<ExternalMcpToolRecord>, StoreError> {
        find_tool(&self.connection, mcp_tool_id).await
    }

    async fn get_external_mcp_tools(
        &self,
        mcp_tool_ids: &[Uuid],
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        let mut tools = Vec::with_capacity(mcp_tool_ids.len());
        // Batches stay well under SQLite's bound-variable limit.
        for batch in mcp_tool_ids.chunks(500) {
            let placeholders = (1..=batch.len())
                .map(|index| format!("?{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut rows = self
                .connection
                .query(
                    &format!(
                        "SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_tool_id IN ({placeholders})"
                    ),
                    libsql::params_from_iter(batch.iter().map(Uuid::to_string)),
                )
                .await
                .map_err(to_query_error)?;
            while let Some(row) = rows.next().await.map_err(to_query_error)? {
                tools.push(decode_tool(&row)?);
            }
        }
        Ok(tools)
    }

    async fn set_mcp_tool_tags(
        &self,
        mcp_tool_id: Uuid,
        tags: &[String],
    ) -> Result<Updated<ExternalMcpToolRecord>, StoreError> {
        let tags_json = serialize_json(tags)?;
        self.update_tool_policy_column(mcp_tool_id, "admin_tags_json", &tags_json, |tool| {
            tool.policy.tags == tags
        })
        .await
    }

    async fn set_mcp_tool_annotation_overrides(
        &self,
        mcp_tool_id: Uuid,
        overrides: &McpAnnotationOverrides,
    ) -> Result<Updated<ExternalMcpToolRecord>, StoreError> {
        let overrides_json = serialize_json(&overrides.to_object())?;
        self.update_tool_policy_column(
            mcp_tool_id,
            "annotation_overrides_json",
            &overrides_json,
            |tool| tool.policy.annotation_overrides == *overrides,
        )
        .await
    }

    async fn reviewed_mcp_tool_versions(
        &self,
        versions: &[(Uuid, i64)],
    ) -> Result<Vec<(Uuid, i64)>, StoreError> {
        let mut reviewed = Vec::new();
        // 500 pairs bind 1,000 variables, well under SQLite's limit.
        for batch in versions.chunks(500) {
            let pairs = (0..batch.len())
                .map(|index| format!("(?{}, ?{})", 2 * index + 1, 2 * index + 2))
                .collect::<Vec<_>>()
                .join(", ");
            let params = batch
                .iter()
                .flat_map(|(mcp_tool_id, schema_version)| {
                    [
                        libsql::Value::Text(mcp_tool_id.to_string()),
                        libsql::Value::Integer(*schema_version),
                    ]
                })
                .collect::<Vec<_>>();
            let mut rows = self
                .connection
                .query(
                    &format!(
                        "SELECT mcp_tool_id, schema_version FROM mcp_tool_versions WHERE reviewed_at IS NOT NULL AND (mcp_tool_id, schema_version) IN (VALUES {pairs})"
                    ),
                    libsql::params_from_iter(params),
                )
                .await
                .map_err(to_query_error)?;
            while let Some(row) = rows.next().await.map_err(to_query_error)? {
                let mcp_tool_id: String = row.get(0).map_err(to_query_error)?;
                reviewed.push((
                    parse_uuid(&mcp_tool_id)?,
                    row.get(1).map_err(to_query_error)?,
                ));
            }
        }
        Ok(reviewed)
    }

    async fn list_mcp_tool_versions(
        &self,
        mcp_tool_id: Uuid,
        query: &McpToolVersionQuery,
    ) -> Result<Vec<McpToolVersionRecord>, StoreError> {
        let mut params = vec![libsql::Value::Text(mcp_tool_id.to_string())];
        let filter = match query {
            McpToolVersionQuery::Versions(versions) if versions.is_empty() => {
                return Ok(Vec::new());
            }
            McpToolVersionQuery::Versions(versions) => {
                params.extend(versions.iter().copied().map(libsql::Value::Integer));
                let placeholders = (2..=versions.len() + 1)
                    .map(|index| format!("?{index}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("AND schema_version IN ({placeholders}) ORDER BY schema_version DESC")
            }
            McpToolVersionQuery::Page { before, limit } => {
                params.push(before.map_or(libsql::Value::Null, libsql::Value::Integer));
                params.push(libsql::Value::Integer(i64::from(*limit)));
                "AND (?2 IS NULL OR schema_version < ?2) ORDER BY schema_version DESC LIMIT ?3"
                    .to_string()
            }
        };
        let mut rows = self
            .connection
            .query(
                &format!(
                    "SELECT {VERSION_COLUMNS} FROM mcp_tool_versions WHERE mcp_tool_id = ?1 {filter}"
                ),
                libsql::params_from_iter(params),
            )
            .await
            .map_err(to_query_error)?;
        let mut versions = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            versions.push(decode_version(&row)?);
        }
        Ok(versions)
    }

    async fn review_mcp_tool_version(
        &self,
        mcp_tool_id: Uuid,
        schema_version: i64,
        decision: McpToolReviewDecision,
        reviewed_by_user_id: Option<Uuid>,
        reviewed_at: OffsetDateTime,
    ) -> Result<Updated<ExternalMcpToolRecord>, StoreError> {
        // The write lock makes the pending check and the review one step, so two
        // admins reviewing at once cannot both apply.
        let tx = self.begin_transaction().await?;
        let existing = find_tool(&tx, mcp_tool_id).await?.ok_or_else(|| {
            StoreError::NotFound(format!("MCP tool `{mcp_tool_id}` was not found"))
        })?;
        let version = reviewable_version(
            mcp_tool_id,
            schema_version,
            find_version(&tx, mcp_tool_id, schema_version).await?,
        )?;
        for op in review_ops(
            decision,
            &existing,
            &version,
            reviewed_at,
            reviewed_by_user_id,
        ) {
            apply_version_op(&tx, mcp_tool_id, &op).await?;
        }
        let current = find_tool(&tx, mcp_tool_id).await?.ok_or_else(|| {
            StoreError::Unexpected("MCP tool disappeared during review".to_string())
        })?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(Updated {
            current,
            previous: Some(existing),
        })
    }
}

async fn find_tool(
    connection: &libsql::Connection,
    mcp_tool_id: Uuid,
) -> Result<Option<ExternalMcpToolRecord>, StoreError> {
    let sql = format!("SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_tool_id = ?1");
    let mut rows = connection
        .query(&sql, [mcp_tool_id.to_string()])
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .map(|row| decode_tool(&row))
        .transpose()
}

impl LibsqlStore {
    /// Writes one admin policy column under the write lock, only when `matches`
    /// says the stored value differs.
    async fn update_tool_policy_column(
        &self,
        mcp_tool_id: Uuid,
        column: &'static str,
        value_json: &str,
        matches: impl FnOnce(&ExternalMcpToolRecord) -> bool,
    ) -> Result<Updated<ExternalMcpToolRecord>, StoreError> {
        let tx = self.begin_transaction().await?;
        let existing = find_tool(&tx, mcp_tool_id).await?.ok_or_else(|| {
            StoreError::NotFound(format!("MCP tool `{mcp_tool_id}` was not found"))
        })?;
        if matches(&existing) {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(Updated::unchanged(existing));
        }
        tx.execute(
            &format!("UPDATE external_mcp_tools SET {column} = ?1 WHERE mcp_tool_id = ?2"),
            libsql::params![value_json, mcp_tool_id.to_string()],
        )
        .await
        .map_err(to_write_error)?;
        let current = find_tool(&tx, mcp_tool_id).await?.ok_or_else(|| {
            StoreError::Unexpected("MCP tool disappeared during update".to_string())
        })?;
        sync_effective_tags(&tx, &current).await?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(Updated {
            current,
            previous: Some(existing),
        })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::NEWER_DISCOVERY_COMMITTED_SQL;
    use crate::migration_registry::{MIGRATION_REGISTRY, MigrationBackend};

    #[tokio::test]
    async fn the_stale_discovery_check_reads_backfilled_runs_by_index() {
        let temporary = tempdir().expect("tempdir");
        let database_path = temporary.path().join("gateway.db");
        let database = libsql::Builder::new_local(database_path.to_str().expect("database path"))
            .build()
            .await
            .expect("database");
        let connection = database.connect().expect("connection");
        let apply = |below: u32, at_least: u32| {
            MIGRATION_REGISTRY
                .iter()
                .filter(move |migration| migration.version >= at_least && migration.version < below)
                .map(|migration| migration.sql_for(MigrationBackend::Libsql))
        };
        for sql in apply(72, 0) {
            connection
                .execute_batch(sql)
                .await
                .expect("earlier migration");
        }
        connection
            .execute("PRAGMA foreign_keys = OFF", ())
            .await
            .expect("pragma");
        connection
            .execute(
                "INSERT INTO external_mcp_discovery_runs \
                 (discovery_run_id, mcp_server_id, status, started_at, finished_at) \
                 VALUES ('run', 'server', 'success', 1700000000, 1700000001)",
                (),
            )
            .await
            .expect("older run");
        for sql in apply(73, 72) {
            connection.execute_batch(sql).await.expect("V72");
        }

        let mut rows = connection
            .query(
                NEWER_DISCOVERY_COMMITTED_SQL,
                libsql::params!["server", "success", 1_699_999_999_500_i64],
            )
            .await
            .expect("stale check");
        let newer: bool = rows
            .next()
            .await
            .expect("row")
            .expect("row")
            .get(0)
            .expect("value");
        assert!(newer, "the older run was backfilled in milliseconds");

        // A gateway older than V72 records a run without `started_at_ms`.
        connection
            .execute(
                "INSERT INTO external_mcp_discovery_runs \
                 (discovery_run_id, mcp_server_id, status, started_at, finished_at) \
                 VALUES ('legacy', 'server', 'success', 1700000100, 1700000101)",
                (),
            )
            .await
            .expect("legacy run");
        let mut rows = connection
            .query(
                NEWER_DISCOVERY_COMMITTED_SQL,
                libsql::params!["server", "success", 1_700_000_050_000_i64],
            )
            .await
            .expect("stale check");
        let newer: bool = rows
            .next()
            .await
            .expect("row")
            .expect("row")
            .get(0)
            .expect("value");
        assert!(newer, "a run without milliseconds compares by its start");

        let mut rows = connection
            .query(
                &format!("EXPLAIN QUERY PLAN {NEWER_DISCOVERY_COMMITTED_SQL}"),
                libsql::params!["server", "success", 0_i64],
            )
            .await
            .expect("query plan");
        let mut plan = String::new();
        while let Some(row) = rows.next().await.expect("plan row") {
            let detail: String = row.get(3).expect("plan detail");
            plan.push_str(&detail);
            plan.push('\n');
        }
        assert!(
            plan.contains("external_mcp_discovery_runs_server_status_ms_idx"),
            "plan:\n{plan}"
        );
    }
}
