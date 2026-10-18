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
    grants_outside_team_sql, json_object_from_str, newer_discovery_run_sql, parse_uuid,
    serialize_json, stranded_grants, stranded_toolset, team_only_toolset_tool, unix_to_datetime,
};
use gateway_core::{
    ExternalMcpServerUpdate, McpAnnotationOverrides, McpServerGuardRow, McpServerOwnership,
    McpServerOwnershipGuard, McpServerVisibility, McpToolApprovalStatus, McpToolDefinition,
    McpToolPolicy, McpToolReviewDecision, McpToolReviewState, McpToolVersionQuery,
    McpToolVersionRecord, McpToolVersionStatus, TransferExternalMcpServerOwnershipRecord, Updated,
};

mod tool_versions;
use tool_versions::*;

macro_rules! server_columns {
    () => {
        "mcp_server_id, server_key, display_name, description, transport, server_url, auth_mode, auth_config_json::text, timeout_ms, status, last_discovery_status, last_discovery_at, last_successful_discovery_at, last_error_summary, last_tool_count, created_at, updated_at, disabled_at, approve_tool_changes, owner_team_id, owner_contact, visibility, team_destinations"
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
const TOOL_COLUMNS: &str = "mcp_tool_id, mcp_server_id, upstream_name, display_name, description, input_schema_json::text, schema_hash, schema_version, is_active, first_discovered_at, last_discovered_at, deactivated_at, title, annotations_json::text, output_schema_json::text, admin_tags_json::text, annotation_overrides_json::text, approval_status, approved_schema_version, pending_schema_version";

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
        approve_tool_changes: row.try_get::<i64, _>(18).map_err(to_query_error)? == 1,
        ownership: decode_server_ownership(row, 19)?,
        team_destinations: row.try_get::<i64, _>(22).map_err(to_query_error)? != 0,
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
        policy: decode_tool_policy(row, 15)?,
        schema_hash: row.try_get(6).map_err(to_query_error)?,
        schema_version: row.try_get(7).map_err(to_query_error)?,
        is_active: is_active == 1,
        first_discovered_at: unix_to_datetime(first_discovered_at)?,
        last_discovered_at: unix_to_datetime(last_discovered_at)?,
        deactivated_at: deactivated_at.map(unix_to_datetime).transpose()?,
        review: decode_tool_review(row, 17)?,
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

/// Decodes `approval_status, approved_schema_version, pending_schema_version`
/// starting at `offset`.
pub(super) fn decode_tool_review(
    row: &PgRow,
    offset: usize,
) -> Result<McpToolReviewState, StoreError> {
    let status: String = row.try_get(offset).map_err(to_query_error)?;
    Ok(McpToolReviewState {
        approval_status: McpToolApprovalStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP tool approval status `{status}`"))
        })?,
        approved_schema_version: row.try_get(offset + 1).map_err(to_query_error)?,
        pending_schema_version: row.try_get(offset + 2).map_err(to_query_error)?,
    })
}

/// Decodes `admin_tags_json, annotation_overrides_json` starting at `offset`.
pub(super) fn decode_tool_policy(row: &PgRow, offset: usize) -> Result<McpToolPolicy, StoreError> {
    let tags_json: String = row.try_get(offset).map_err(to_query_error)?;
    let overrides_json: String = row.try_get(offset + 1).map_err(to_query_error)?;
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
/// Refuses a narrowing to `team_id` while access would be stranded: one of
/// the server's tools in an active tool set, or a grant held outside the team.
/// Run inside the narrowing write, with the server row locked `FOR UPDATE`: a
/// grant write holds it `FOR SHARE` until it commits, so one in flight is
/// counted here.
async fn refuse_stranded_access(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    mcp_server_id: Uuid,
    team_id: Uuid,
) -> Result<(), StoreError> {
    let toolset_key: Option<String> = sqlx::query_scalar(&active_toolset_for_server_sql("$1"))
        .bind(mcp_server_id.to_string())
        .fetch_optional(&mut **tx)
        .await
        .map_err(to_query_error)?;
    if let Some(toolset_key) = toolset_key {
        return Err(stranded_toolset(&toolset_key));
    }
    let outside: i64 = sqlx::query_scalar(&grants_outside_team_sql("$1", "$2"))
        .bind(mcp_server_id.to_string())
        .bind(team_id.to_string())
        .fetch_one(&mut **tx)
        .await
        .map_err(to_query_error)?;
    if outside > 0 {
        return Err(stranded_grants(outside));
    }
    Ok(())
}

pub(super) async fn check_ownership_guard(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    guard: &McpServerOwnershipGuard,
    tool_id: Option<Uuid>,
) -> Result<(), StoreError> {
    let (sql, id) = match tool_id {
        Some(tool_id) => (
            "SELECT s.mcp_server_id, s.owner_team_id, s.owner_contact, s.visibility, s.server_url, s.auth_mode, s.auth_config_json::text FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id = $1 FOR SHARE OF s",
            tool_id,
        ),
        None => (
            "SELECT mcp_server_id, owner_team_id, owner_contact, visibility, server_url, auth_mode, auth_config_json::text FROM external_mcp_servers WHERE mcp_server_id = $1 FOR SHARE",
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
            let server_url: String = row.try_get(4).map_err(to_query_error)?;
            let auth_mode: String = row.try_get(5).map_err(to_query_error)?;
            let auth_config_json: String = row.try_get(6).map_err(to_query_error)?;
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

    async fn list_external_mcp_servers_owned_by(
        &self,
        owner_team_id: Option<Uuid>,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE ($1::bigint = 1 OR status != 'disabled') AND owner_team_id IS NOT DISTINCT FROM $2::text ORDER BY server_key"
        );
        let rows = sqlx::query(&sql)
            .bind(if include_disabled { 1_i64 } else { 0_i64 })
            .bind(owner_team_id.map(|team_id| team_id.to_string()))
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_server).collect()
    }

    async fn list_external_mcp_servers_visible_to_team(
        &self,
        team_id: Uuid,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE ($1::bigint = 1 OR status != 'disabled') AND (owner_team_id = $2 OR visibility = 'organisation') ORDER BY server_key"
        );
        let rows = sqlx::query(&sql)
            .bind(if include_disabled { 1_i64 } else { 0_i64 })
            .bind(team_id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_server).collect()
    }

    async fn list_external_mcp_servers_by_ids(
        &self,
        mcp_server_ids: &[Uuid],
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        if mcp_server_ids.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE mcp_server_id = ANY($1) ORDER BY server_key"
        );
        let rows = sqlx::query(&sql)
            .bind(
                mcp_server_ids
                    .iter()
                    .map(Uuid::to_string)
                    .collect::<Vec<_>>(),
            )
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_server).collect()
    }

    async fn list_mcp_server_ownerships(
        &self,
        mcp_server_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, McpServerOwnership)>, StoreError> {
        if mcp_server_ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT mcp_server_id, owner_team_id, owner_contact, visibility FROM external_mcp_servers WHERE mcp_server_id = ANY($1)",
        )
        .bind(
            mcp_server_ids
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>(),
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter()
            .map(|row| {
                let mcp_server_id: String = row.try_get(0).map_err(to_query_error)?;
                Ok((
                    parse_uuid(&mcp_server_id)?,
                    decode_server_ownership(row, 1)?,
                ))
            })
            .collect()
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

    async fn list_external_mcp_servers_by_keys(
        &self,
        server_keys: &[String],
    ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
        if server_keys.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT {SERVER_COLUMNS} FROM external_mcp_servers WHERE server_key = ANY($1) ORDER BY server_key"
        );
        sqlx::query(&sql)
            .bind(server_keys)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?
            .iter()
            .map(decode_server)
            .collect()
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
                approve_tool_changes, owner_team_id, owner_contact, visibility
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9, 'active', $10, $10, $11, $12, $13, $14)
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
        .bind(i64::from(input.approve_tool_changes))
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
    ) -> Result<ExternalMcpServerUpdate, StoreError> {
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
            // Counted under the row lock: a credential write holds the row
            // `FOR SHARE` until it commits, so one that landed first is seen.
            let foreign: i64 = sqlx::query_scalar(&foreign_credentials_sql("$1", "$2", "$3"))
                .bind(input.mcp_server_id.to_string())
                .bind(
                    existing
                        .ownership
                        .owner_team_id
                        .map(|team_id| team_id.to_string())
                        .unwrap_or_default(),
                )
                .bind(input.updated_at.unix_timestamp())
                .fetch_one(&mut *tx)
                .await
                .map_err(to_query_error)?;
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
            refuse_stranded_access(&mut tx, input.mcp_server_id, team_id).await?;
        }
        let invalidate_discovery = discovery_config_changed(&existing, input);
        let mut revoked_oauth_bindings = Vec::new();
        sqlx::query(
            r#"
            UPDATE external_mcp_servers
            SET display_name = $1, description = $2, server_url = $3, auth_mode = $4,
                auth_config_json = $5::jsonb, timeout_ms = $6, updated_at = $7,
                owner_contact = $9, visibility = $10,
                team_destinations = CASE WHEN $11 THEN 0 ELSE team_destinations END,
                approve_tool_changes = $12
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
        .bind(owner_contact.as_deref())
        .bind(input.visibility.as_str())
        .bind(input.vouch_for_destinations)
        .bind(i64::from(input.approve_tool_changes))
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
            // A flow started against the old settings must not finish against
            // the new ones; its callback finds no state and is refused.
            sqlx::query("DELETE FROM mcp_oauth_states WHERE mcp_server_id = $1")
                .bind(input.mcp_server_id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(to_write_error)?;
            // Users' OAuth tokens were issued by the old endpoints. Revoke them,
            // so a refresh token is never posted to a token endpoint that did
            // not issue it; each user connects again.
            // The revoked rows are returned so each revoke is audited.
            revoked_oauth_bindings = sqlx::query(&format!(
                "UPDATE mcp_upstream_credential_bindings SET revoked_at = $2, updated_at = $2 WHERE mcp_server_id = $1 AND material_kind = 'oauth_tokens' AND revoked_at IS NULL RETURNING {}",
                super::mcp_credentials::CREDENTIAL_COLUMNS
            ))
            .bind(input.mcp_server_id.to_string())
            .bind(input.updated_at.unix_timestamp())
            .fetch_all(&mut *tx)
            .await
            .map_err(to_write_error)?
            .iter()
            .map(super::mcp_credentials::decode_credential)
            .collect::<Result<_, _>>()?;
        }
        // Read inside the transaction, so `current` is the row this update wrote.
        let current = sqlx::query(SERVER_SELECT_BY_ID)
            .bind(input.mcp_server_id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(to_query_error)
            .and_then(|row| decode_server(&row))?;
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
        // Read with the row locked `FOR UPDATE`: a grant write holds it `FOR
        // SHARE` until it commits, so one in flight is counted here.
        // Only a narrowing (to team-only, or to another team) can strand access.
        if input.require_no_stranded_access
            && input.ownership.visibility == McpServerVisibility::Team
            && let Some(team_id) = input.ownership.owner_team_id
            && (existing.ownership.visibility != McpServerVisibility::Team
                || existing.ownership.owner_team_id != Some(team_id))
        {
            refuse_stranded_access(&mut tx, input.mcp_server_id, team_id).await?;
        }
        let current = sqlx::query(&format!(
            r#"
            UPDATE external_mcp_servers
            SET owner_team_id = $1, owner_contact = $2, visibility = $3, updated_at = $4,
                -- A server leaving a team keeps the destinations it set
                -- unvouched, and so keeps the public-address guard.
                team_destinations = CASE WHEN owner_team_id IS NOT NULL THEN 1 ELSE team_destinations END
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

    async fn first_active_mcp_toolset_key_for_server(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<Option<String>, StoreError> {
        sqlx::query_scalar(&active_toolset_for_server_sql("$1"))
            .bind(mcp_server_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)
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
        subject_kind: Option<McpToolGrantSubjectKind>,
        subject_id: Option<Uuid>,
    ) -> Result<Vec<McpToolGrantRecord>, StoreError> {
        let sql = format!(
            "SELECT {GRANT_COLUMNS} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.target_kind = 'tool' AND s.owner_team_id = $1 AND ($2::text IS NULL OR g.subject_kind = $2) AND ($3::text IS NULL OR g.subject_id = $3) ORDER BY g.created_at, g.grant_id"
        );
        let rows = sqlx::query(&sql)
            .bind(team_id.to_string())
            .bind(subject_kind.map(|kind| kind.as_str()))
            .bind(subject_id.map(|id| id.to_string()))
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(super::mcp_access::decode_grant).collect()
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
        // Lock the server, then its tools, so concurrent discoveries of one
        // server run one at a time and a review cannot change a tool between
        // the snapshot below and the writes planned from it. The server is
        // locked `FOR UPDATE` before the guard's `FOR SHARE` read: taking the
        // share lock first and upgrading it later deadlocks two discoveries.
        let server = sqlx::query(&format!("{SERVER_SELECT_BY_ID} FOR UPDATE"))
            .bind(run.mcp_server_id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_server)
            .transpose()?;
        let approve_tool_changes = server
            .as_ref()
            .is_some_and(|server| server.approve_tool_changes);
        if let Some(guard) = &run.expected_server {
            check_ownership_guard(&mut tx, guard, None).await?;
            let newer_run_committed: bool = sqlx::query_scalar(&format!(
                "SELECT {}",
                newer_discovery_run_sql("$1", Some("$2"), "$3", "$4")
            ))
            .bind(run.mcp_server_id.to_string())
            .bind(ExternalMcpDiscoveryStatus::Success.as_str())
            .bind(unix_millis(run.started_at))
            .bind(run.discovery_run_id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(to_query_error)?;
            stale_discovery_check(
                server
                    .as_ref()
                    .is_some_and(|server| server.status == ExternalMcpServerStatus::Active),
                newer_run_committed,
            )?;
        }
        sqlx::query(
            r#"
            INSERT INTO external_mcp_discovery_runs (
                discovery_run_id, mcp_server_id, status, started_at, finished_at,
                discovered_tool_count, active_tool_count, schema_set_hash, error_summary, details_json,
                started_at_ms
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb, $11)
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
        .bind(unix_millis(run.started_at))
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;

        let stored = load_stored_tools(&mut tx, run.mcp_server_id).await?;
        sqlx::query(
            "UPDATE external_mcp_tools SET is_active = 0, deactivated_at = $1 WHERE mcp_server_id = $2",
        )
        .bind(run.finished_at.unix_timestamp())
        .bind(run.mcp_server_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;

        let mut planned = plan_tool_writes(&stored, tools, approve_tool_changes);
        let historical =
            find_versions_by_definition(&mut tx, &historical_lookups(&planned)).await?;
        reuse_historical_versions(&mut planned, &historical);
        for plan in &planned {
            apply_tool_write(
                &mut tx,
                run.finished_at,
                plan.tool,
                plan.existing,
                &plan.write,
            )
            .await?;
        }

        // A newer run of any status, such as a failure that committed while
        // this one ran, stays the server's last discovery.
        let newer = newer_discovery_run_sql("$4", None, "$5", "$6");
        sqlx::query(&format!(
            "UPDATE external_mcp_servers
             SET last_successful_discovery_at = $2, last_tool_count = $3, updated_at = $2,
                 last_discovery_status = CASE WHEN {newer} THEN last_discovery_status ELSE $1 END,
                 last_discovery_at = CASE WHEN {newer} THEN last_discovery_at ELSE $2 END,
                 last_error_summary = CASE WHEN {newer} THEN last_error_summary ELSE NULL END
             WHERE mcp_server_id = $4"
        ))
        .bind(run.status.as_str())
        .bind(run.finished_at.unix_timestamp())
        .bind(run.active_tool_count)
        .bind(run.mcp_server_id.to_string())
        .bind(unix_millis(run.started_at))
        .bind(run.discovery_run_id.to_string())
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
        // `FOR UPDATE` before the guard's `FOR SHARE` read, as on success: the
        // update below would otherwise upgrade a share lock and deadlock with a
        // concurrent discovery of this server.
        sqlx::query("SELECT 1 FROM external_mcp_servers WHERE mcp_server_id = $1 FOR UPDATE")
            .bind(run.mcp_server_id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_query_error)?;
        if let Some(guard) = &run.expected_server {
            check_ownership_guard(&mut tx, guard, None).await?;
        }
        sqlx::query(
            r#"
            INSERT INTO external_mcp_discovery_runs (
                discovery_run_id, mcp_server_id, status, started_at, finished_at,
                discovered_tool_count, active_tool_count, schema_set_hash, error_summary, details_json,
                started_at_ms
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb, $11)
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
        .bind(unix_millis(run.started_at))
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;
        // The run stays in the history, but a run of any status that started
        // later remains the server's last discovery.
        sqlx::query(&format!(
            "UPDATE external_mcp_servers
             SET last_discovery_status = $1, last_discovery_at = $2,
                 last_error_summary = $3, updated_at = $2
             WHERE mcp_server_id = $4 AND NOT {}",
            newer_discovery_run_sql("$4", None, "$5", "$6")
        ))
        .bind(run.status.as_str())
        .bind(run.finished_at.unix_timestamp())
        .bind(run.error_summary.as_deref())
        .bind(run.mcp_server_id.to_string())
        .bind(unix_millis(run.started_at))
        .bind(run.discovery_run_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(to_query_error)?;
        tx.commit().await.map_err(to_query_error)?;
        Ok(())
    }

    async fn get_external_mcp_tool(
        &self,
        mcp_tool_id: Uuid,
    ) -> Result<Option<ExternalMcpToolRecord>, StoreError> {
        sqlx::query(&format!(
            "SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_tool_id = $1"
        ))
        .bind(mcp_tool_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?
        .as_ref()
        .map(decode_tool)
        .transpose()
    }

    async fn get_external_mcp_tools(
        &self,
        mcp_tool_ids: &[Uuid],
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        if mcp_tool_ids.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query(&format!(
            "SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_tool_id = ANY($1)"
        ))
        .bind(mcp_tool_ids.iter().map(Uuid::to_string).collect::<Vec<_>>())
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?
        .iter()
        .map(decode_tool)
        .collect()
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
        if versions.is_empty() {
            return Ok(Vec::new());
        }
        let (tool_ids, schema_versions): (Vec<String>, Vec<i64>) = versions
            .iter()
            .map(|(mcp_tool_id, schema_version)| (mcp_tool_id.to_string(), *schema_version))
            .unzip();
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT mcp_tool_id, schema_version FROM mcp_tool_versions WHERE reviewed_at IS NOT NULL AND (mcp_tool_id, schema_version) IN (SELECT * FROM UNNEST($1::text[], $2::bigint[]))",
        )
        .bind(tool_ids)
        .bind(schema_versions)
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.into_iter()
            .map(|(mcp_tool_id, schema_version)| Ok((parse_uuid(&mcp_tool_id)?, schema_version)))
            .collect()
    }

    async fn list_mcp_tool_versions(
        &self,
        mcp_tool_id: Uuid,
        query: &McpToolVersionQuery,
    ) -> Result<Vec<McpToolVersionRecord>, StoreError> {
        let named_sql = format!(
            "SELECT {VERSION_COLUMNS} FROM mcp_tool_versions WHERE mcp_tool_id = $1 AND schema_version = ANY($2) ORDER BY schema_version DESC"
        );
        let page_sql = format!(
            "SELECT {VERSION_COLUMNS} FROM mcp_tool_versions WHERE mcp_tool_id = $1 AND ($2::bigint IS NULL OR schema_version < $2) ORDER BY schema_version DESC LIMIT $3"
        );
        let statement = match query {
            McpToolVersionQuery::Versions(versions) => sqlx::query(&named_sql)
                .bind(mcp_tool_id.to_string())
                .bind(versions.clone()),
            McpToolVersionQuery::Page { before, limit } => sqlx::query(&page_sql)
                .bind(mcp_tool_id.to_string())
                .bind(*before)
                .bind(i64::from(*limit)),
        };
        statement
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?
            .iter()
            .map(decode_version)
            .collect()
    }

    async fn review_mcp_tool_version(
        &self,
        mcp_tool_id: Uuid,
        schema_version: i64,
        decision: McpToolReviewDecision,
        reviewed_by_user_id: Option<Uuid>,
        reviewed_at: OffsetDateTime,
    ) -> Result<Updated<ExternalMcpToolRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        // Locking the tool row makes the pending check and the review one step,
        // so two admins reviewing at once cannot both apply.
        let existing = sqlx::query(&format!(
            "SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_tool_id = $1 FOR UPDATE"
        ))
        .bind(mcp_tool_id.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(to_query_error)?
        .ok_or_else(|| StoreError::NotFound(format!("MCP tool `{mcp_tool_id}` was not found")))
        .and_then(|row| decode_tool(&row))?;
        let version = sqlx::query(&format!(
            "SELECT {VERSION_COLUMNS} FROM mcp_tool_versions WHERE mcp_tool_id = $1 AND schema_version = $2"
        ))
        .bind(mcp_tool_id.to_string())
        .bind(schema_version)
        .fetch_optional(&mut *tx)
        .await
        .map_err(to_query_error)?
        .as_ref()
        .map(decode_version)
        .transpose()?;
        let version = reviewable_version(mcp_tool_id, schema_version, version)?;
        for op in review_ops(
            decision,
            &existing,
            &version,
            reviewed_at,
            reviewed_by_user_id,
        ) {
            apply_version_op(&mut tx, mcp_tool_id, &op).await?;
        }
        let current = sqlx::query(&format!(
            "SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_tool_id = $1"
        ))
        .bind(mcp_tool_id.to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(to_query_error)
        .and_then(|row| decode_tool(&row))?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(Updated {
            current,
            previous: Some(existing),
        })
    }
}

impl PostgresStore {
    /// Writes one admin policy column with the tool row locked, only when
    /// `matches` says the stored value differs.
    async fn update_tool_policy_column(
        &self,
        mcp_tool_id: Uuid,
        column: &'static str,
        value_json: &str,
        matches: impl FnOnce(&ExternalMcpToolRecord) -> bool,
    ) -> Result<Updated<ExternalMcpToolRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let existing = sqlx::query(&format!(
            "SELECT {TOOL_COLUMNS} FROM external_mcp_tools WHERE mcp_tool_id = $1 FOR UPDATE"
        ))
        .bind(mcp_tool_id.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(to_query_error)?
        .ok_or_else(|| StoreError::NotFound(format!("MCP tool `{mcp_tool_id}` was not found")))
        .and_then(|row| decode_tool(&row))?;
        if matches(&existing) {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(Updated::unchanged(existing));
        }
        let row = sqlx::query(&format!(
            "UPDATE external_mcp_tools SET {column} = $1::jsonb WHERE mcp_tool_id = $2 RETURNING {TOOL_COLUMNS}"
        ))
        .bind(value_json)
        .bind(mcp_tool_id.to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(to_write_error)?;
        let current = decode_tool(&row)?;
        sync_effective_tags(&mut tx, &current).await?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(Updated {
            current,
            previous: Some(existing),
        })
    }
}
