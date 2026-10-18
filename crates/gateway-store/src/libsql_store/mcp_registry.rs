use super::*;
use std::collections::HashMap;

use crate::mcp_tool_policy::tool_policy_from_json;
use crate::mcp_tool_review::{
    HistoricalLookup, HistoricalVersion, StoredTool, StoredVersion, ToolWrite, VersionOp,
    discovered_definition, historical_lookups, keep_newest, plan_tool_writes,
    reuse_historical_versions, review_ops, reviewable_version, tool_write_ops,
};
use crate::shared::{json_object_from_str, parse_uuid, serialize_json, unix_to_datetime};
use gateway_core::{
    McpAnnotationOverrides, McpToolApprovalStatus, McpToolDefinition, McpToolPolicy,
    McpToolReviewDecision, McpToolReviewState, McpToolVersionQuery, McpToolVersionRecord,
    McpToolVersionStatus, Updated,
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
        && existing.approve_tool_changes == input.approve_tool_changes
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

macro_rules! server_columns {
    () => {
        "mcp_server_id, server_key, display_name, description, transport, server_url, auth_mode, auth_config_json, timeout_ms, status, last_discovery_status, last_discovery_at, last_successful_discovery_at, last_error_summary, last_tool_count, created_at, updated_at, disabled_at, approve_tool_changes"
    };
}

const SERVER_COLUMNS: &str = server_columns!();

const SERVER_SELECT_BY_ID: &str = concat!(
    "SELECT ",
    server_columns!(),
    " FROM external_mcp_servers WHERE mcp_server_id = ?1"
);
const TOOL_COLUMNS: &str = "mcp_tool_id, mcp_server_id, upstream_name, display_name, description, input_schema_json, schema_hash, schema_version, is_active, first_discovered_at, last_discovered_at, deactivated_at, title, annotations_json, output_schema_json, admin_tags_json, annotation_overrides_json, approval_status, approved_schema_version, pending_schema_version";

/// Refuses a discovery result once the server's status has moved on since discovery read it.
async fn ensure_discovery_status_unchanged(
    tx: &libsql::Transaction,
    run: &ExternalMcpDiscoveryRunRecord,
) -> Result<(), StoreError> {
    let Some(expected) = run.expected_server_status else {
        return Ok(());
    };
    let current = load_server(tx, run.mcp_server_id).await?.status;
    if current == expected {
        return Ok(());
    }
    Err(StoreError::Conflict(format!(
        "external MCP server `{}` became {} while discovery ran; refresh discovery again",
        run.mcp_server_id,
        current.as_str()
    )))
}

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
                    approve_tool_changes
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?10, ?11)
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
    ) -> Result<Updated<ExternalMcpServerRecord>, StoreError> {
        let auth_config_json = serialize_json(&input.auth_config)?;
        // An immediate transaction holds the write lock, so the server read here is
        // exactly the one this update replaces.
        let tx = self.begin_transaction().await?;
        let existing = load_server(&tx, input.mcp_server_id).await?;
        if server_settings_match(&existing, input) {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(Updated::unchanged(existing));
        }
        let invalidate_discovery = discovery_config_changed(&existing, input);
        tx.execute(
            r#"
            UPDATE external_mcp_servers
            SET display_name = ?1, description = ?2, server_url = ?3, auth_mode = ?4,
                auth_config_json = ?5, timeout_ms = ?6, updated_at = ?7,
                approve_tool_changes = ?9
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
        }
        // Read inside the transaction, so `current` is the row this update wrote.
        let current = load_server(&tx, input.mcp_server_id).await?;
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

    async fn enable_external_mcp_server(
        &self,
        mcp_server_id: Uuid,
        enabled_at: OffsetDateTime,
    ) -> Result<(ExternalMcpServerRecord, bool), StoreError> {
        // Only a disabled server changes, so a stale request can't wipe an active server's
        // discovery result. With no row, the server was already active or is missing.
        let mut rows = self
            .connection
            .query(
                &format!(
                    "UPDATE external_mcp_servers
                     SET status = 'active', disabled_at = NULL, updated_at = ?1,
                         last_discovery_status = NULL, last_discovery_at = NULL,
                         last_error_summary = NULL
                     WHERE mcp_server_id = ?2
                       AND status = 'disabled'
                     RETURNING {SERVER_COLUMNS}"
                ),
                libsql::params![enabled_at.unix_timestamp(), mcp_server_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        if let Some(row) = rows.next().await.map_err(to_write_error)? {
            return Ok((decode_server(&row)?, true));
        }
        drop(rows);
        Ok((load_server(&self.connection, mcp_server_id).await?, false))
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
        // The transaction holds SQLite's write lock, so the status can't change after this check.
        ensure_discovery_status_unchanged(&tx, run).await?;
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
        // The transaction holds SQLite's write lock, so the status can't change after this check.
        ensure_discovery_status_unchanged(&tx, run).await?;
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
