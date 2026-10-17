use super::*;
use std::collections::HashMap;

use crate::mcp_tool_policy::tool_policy_from_json;
use crate::mcp_tool_review::{
    StoredTool, StoredVersion, ToolWrite, discovered_definition, historical_lookups,
    plan_tool_writes, reuse_historical_versions,
};
use crate::shared::{json_object_from_str, parse_uuid, serialize_json, unix_to_datetime};
use gateway_core::mcp_tool_review::status_after_rejection;
use gateway_core::{
    McpAnnotationOverrides, McpToolApprovalStatus, McpToolDefinition, McpToolPolicy,
    McpToolReviewDecision, McpToolReviewState, McpToolVersionQuery, McpToolVersionRecord,
    McpToolVersionStatus, Updated,
};

macro_rules! server_columns {
    () => {
        "mcp_server_id, server_key, display_name, description, transport, server_url, auth_mode, auth_config_json::text, timeout_ms, status, last_discovery_status, last_discovery_at, last_successful_discovery_at, last_error_summary, last_tool_count, created_at, updated_at, disabled_at, approve_tool_changes"
    };
}

const SERVER_COLUMNS: &str = server_columns!();

const SERVER_SELECT_BY_ID: &str = concat!(
    "SELECT ",
    server_columns!(),
    " FROM external_mcp_servers WHERE mcp_server_id = $1"
);
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
        && existing.approve_tool_changes == input.approve_tool_changes
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
                approve_tool_changes
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9, 'active', $10, $10, $11)
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
                approve_tool_changes = $9
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

        // Lock the server, then its tools, so concurrent discoveries of one
        // server run one at a time and a review cannot change a tool between
        // the snapshot below and the writes planned from it.
        let approve_tool_changes = sqlx::query(&format!("{SERVER_SELECT_BY_ID} FOR UPDATE"))
            .bind(run.mcp_server_id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_server)
            .transpose()?
            .is_some_and(|server| server.approve_tool_changes);
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
        let historical = find_versions_by_hash(&mut tx, &historical_lookups(&planned)).await?;
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
        .ok_or_else(|| {
            StoreError::NotFound(format!(
                "MCP tool `{mcp_tool_id}` has no version {schema_version}"
            ))
        })
        .and_then(|row| decode_version(&row))?;
        if version.status != McpToolVersionStatus::Pending {
            return Err(StoreError::Conflict(format!(
                "version {schema_version} of MCP tool `{mcp_tool_id}` is {}, not pending",
                version.status.as_str()
            )));
        }
        let at = reviewed_at.unix_timestamp();
        let reviewer = reviewed_by_user_id.map(|id| id.to_string());
        match decision {
            McpToolReviewDecision::Approve => {
                write_tool_definition(&mut tx, mcp_tool_id, &version.definition, schema_version)
                    .await?;
                sqlx::query("UPDATE external_mcp_tools SET approval_status = 'approved', approved_schema_version = $1, pending_schema_version = NULL WHERE mcp_tool_id = $2")
                    .bind(schema_version)
                    .bind(mcp_tool_id.to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(to_write_error)?;
                set_version_status(
                    &mut tx,
                    mcp_tool_id,
                    schema_version,
                    McpToolVersionStatus::Approved,
                    Some(at),
                    reviewer.as_deref(),
                )
                .await?;
                supersede_pending(&mut tx, mcp_tool_id, schema_version).await?;
            }
            McpToolReviewDecision::Reject => {
                let status = status_after_rejection(
                    existing.review.approval_status,
                    existing.review.approved_schema_version,
                );
                sqlx::query("UPDATE external_mcp_tools SET approval_status = $1, pending_schema_version = NULL WHERE mcp_tool_id = $2")
                    .bind(status.as_str())
                    .bind(mcp_tool_id.to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(to_write_error)?;
                set_version_status(
                    &mut tx,
                    mcp_tool_id,
                    schema_version,
                    McpToolVersionStatus::Rejected,
                    Some(at),
                    reviewer.as_deref(),
                )
                .await?;
            }
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
        tx.commit().await.map_err(to_write_error)?;
        Ok(Updated {
            current,
            previous: Some(existing),
        })
    }
}

const VERSION_COLUMNS: &str = "mcp_tool_id, schema_version, schema_hash, description, input_schema_json::text, title, annotations_json::text, output_schema_json::text, status, discovered_at, reviewed_at, reviewed_by_user_id";

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

fn decode_version(row: &PgRow) -> Result<McpToolVersionRecord, StoreError> {
    let input_schema_json: String = row.try_get(4).map_err(to_query_error)?;
    let status: String = row.try_get(8).map_err(to_query_error)?;
    let reviewed_at: Option<i64> = row.try_get(10).map_err(to_query_error)?;
    let reviewed_by: Option<String> = row.try_get(11).map_err(to_query_error)?;
    let metadata = decode_tool_metadata(row, 5)?;
    Ok(McpToolVersionRecord {
        mcp_tool_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        schema_version: row.try_get(1).map_err(to_query_error)?,
        definition: McpToolDefinition {
            description: row.try_get(3).map_err(to_query_error)?,
            input_schema: serde_json::from_str(&input_schema_json)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            title: metadata.title,
            annotations: metadata.annotations,
            output_schema: metadata.output_schema,
            schema_hash: row.try_get(2).map_err(to_query_error)?,
        },
        status: McpToolVersionStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP tool version status `{status}`"))
        })?,
        discovered_at: unix_to_datetime(row.try_get(9).map_err(to_query_error)?)?,
        reviewed_at: reviewed_at.map(unix_to_datetime).transpose()?,
        reviewed_by_user_id: reviewed_by.as_deref().map(parse_uuid).transpose()?,
    })
}

/// The server's tools with their versions, by upstream name.
async fn load_stored_tools(
    conn: &mut sqlx::PgConnection,
    mcp_server_id: Uuid,
) -> Result<HashMap<String, StoredTool>, StoreError> {
    let rows = sqlx::query(&format!(
        "SELECT {TOOL_COLUMNS}, metadata_captured FROM external_mcp_tools WHERE mcp_server_id = $1 FOR UPDATE"
    ))
    .bind(mcp_server_id.to_string())
    .fetch_all(&mut *conn)
    .await
    .map_err(to_query_error)?;
    let mut by_id = HashMap::new();
    for row in &rows {
        let tool = decode_tool(row)?;
        let metadata_captured: i64 = row.try_get(20).map_err(to_query_error)?;
        by_id.insert(
            tool.mcp_tool_id,
            StoredTool {
                tool,
                metadata_captured: metadata_captured == 1,
                versions: Vec::new(),
                latest_version: 0,
            },
        );
    }
    let rows = sqlx::query(
        "SELECT v.mcp_tool_id, v.schema_version, v.schema_hash, v.status, v.description, v.title FROM mcp_tool_versions v JOIN external_mcp_tools t ON t.mcp_tool_id = v.mcp_tool_id WHERE t.mcp_server_id = $1 AND v.status IN ('pending', 'rejected')",
    )
    .bind(mcp_server_id.to_string())
    .fetch_all(&mut *conn)
    .await
    .map_err(to_query_error)?;
    for row in &rows {
        let mcp_tool_id: String = row.try_get(0).map_err(to_query_error)?;
        let status: String = row.try_get(3).map_err(to_query_error)?;
        if let Some(stored) = by_id.get_mut(&parse_uuid(&mcp_tool_id)?) {
            stored.versions.push(StoredVersion {
                schema_version: row.try_get(1).map_err(to_query_error)?,
                schema_hash: row.try_get(2).map_err(to_query_error)?,
                description: row.try_get(4).map_err(to_query_error)?,
                title: row.try_get(5).map_err(to_query_error)?,
                status: McpToolVersionStatus::from_db(&status).ok_or_else(|| {
                    StoreError::Serialization(format!("invalid MCP tool version status `{status}`"))
                })?,
            });
        }
    }
    let rows = sqlx::query(
        "SELECT v.mcp_tool_id, MAX(v.schema_version) FROM mcp_tool_versions v JOIN external_mcp_tools t ON t.mcp_tool_id = v.mcp_tool_id WHERE t.mcp_server_id = $1 GROUP BY v.mcp_tool_id",
    )
    .bind(mcp_server_id.to_string())
    .fetch_all(&mut *conn)
    .await
    .map_err(to_query_error)?;
    for row in &rows {
        let mcp_tool_id: String = row.try_get(0).map_err(to_query_error)?;
        if let Some(stored) = by_id.get_mut(&parse_uuid(&mcp_tool_id)?) {
            stored.latest_version = row.try_get(1).map_err(to_query_error)?;
        }
    }
    Ok(by_id
        .into_values()
        .map(|stored| (stored.tool.upstream_name.clone(), stored))
        .collect())
}

/// Serialized definition columns: input schema, annotations, output schema.
fn definition_json(
    definition: &McpToolDefinition,
) -> Result<(String, Option<String>, Option<String>), StoreError> {
    Ok((
        serialize_json(&definition.input_schema)?,
        definition
            .annotations
            .as_ref()
            .map(serialize_json)
            .transpose()?,
        definition
            .output_schema
            .as_ref()
            .map(serialize_json)
            .transpose()?,
    ))
}

/// Writes a definition into the tool row as `schema_version`.
async fn write_tool_definition(
    conn: &mut sqlx::PgConnection,
    mcp_tool_id: Uuid,
    definition: &McpToolDefinition,
    schema_version: i64,
) -> Result<(), StoreError> {
    let (input_schema_json, annotations_json, output_schema_json) = definition_json(definition)?;
    sqlx::query(
        r#"
        UPDATE external_mcp_tools
        SET description = $1, input_schema_json = $2::jsonb, title = $3, annotations_json = $4::jsonb,
            output_schema_json = $5::jsonb, schema_hash = $6, schema_version = $7, metadata_captured = 1
        WHERE mcp_tool_id = $8
        "#,
    )
    .bind(definition.description.as_deref())
    .bind(input_schema_json)
    .bind(definition.title.as_deref())
    .bind(annotations_json)
    .bind(output_schema_json)
    .bind(definition.schema_hash.as_str())
    .bind(schema_version)
    .bind(mcp_tool_id.to_string())
    .execute(&mut *conn)
    .await
    .map_err(to_write_error)?;
    Ok(())
}

async fn insert_version(
    conn: &mut sqlx::PgConnection,
    mcp_tool_id: Uuid,
    schema_version: i64,
    definition: &McpToolDefinition,
    status: McpToolVersionStatus,
    discovered_at: OffsetDateTime,
) -> Result<(), StoreError> {
    let (input_schema_json, annotations_json, output_schema_json) = definition_json(definition)?;
    sqlx::query(
        r#"
        INSERT INTO mcp_tool_versions (
            mcp_tool_id, schema_version, schema_hash, description, input_schema_json, title,
            annotations_json, output_schema_json, status, discovered_at
        ) VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7::jsonb, $8::jsonb, $9, $10)
        "#,
    )
    .bind(mcp_tool_id.to_string())
    .bind(schema_version)
    .bind(definition.schema_hash.as_str())
    .bind(definition.description.as_deref())
    .bind(input_schema_json)
    .bind(definition.title.as_deref())
    .bind(annotations_json)
    .bind(output_schema_json)
    .bind(status.as_str())
    .bind(discovered_at.unix_timestamp())
    .execute(&mut *conn)
    .await
    .map_err(to_write_error)?;
    Ok(())
}

async fn set_version_status(
    conn: &mut sqlx::PgConnection,
    mcp_tool_id: Uuid,
    schema_version: i64,
    status: McpToolVersionStatus,
    reviewed_at: Option<i64>,
    reviewed_by_user_id: Option<&str>,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE mcp_tool_versions SET status = $1, reviewed_at = $2, reviewed_by_user_id = $3 WHERE mcp_tool_id = $4 AND schema_version = $5",
    )
    .bind(status.as_str())
    .bind(reviewed_at)
    .bind(reviewed_by_user_id)
    .bind(mcp_tool_id.to_string())
    .bind(schema_version)
    .execute(&mut *conn)
    .await
    .map_err(to_write_error)?;
    Ok(())
}

/// Approves a version automatically, because the server does not require
/// review. A version an admin already approved keeps its reviewer; one an
/// admin rejected loses the rejection's attribution.
async fn auto_approve_version(
    conn: &mut sqlx::PgConnection,
    mcp_tool_id: Uuid,
    schema_version: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE mcp_tool_versions SET status = 'approved', reviewed_at = NULL, reviewed_by_user_id = NULL WHERE mcp_tool_id = $1 AND schema_version = $2 AND status <> 'approved'",
    )
    .bind(mcp_tool_id.to_string())
    .bind(schema_version)
    .execute(&mut *conn)
    .await
    .map_err(to_write_error)?;
    Ok(())
}

/// Stored versions of the given tools with the given schema hashes, in one
/// query, for discovery to reuse instead of creating duplicates.
async fn find_versions_by_hash(
    conn: &mut sqlx::PgConnection,
    lookups: &[(Uuid, &str)],
) -> Result<HashMap<Uuid, Vec<StoredVersion>>, StoreError> {
    let mut by_tool: HashMap<Uuid, Vec<StoredVersion>> = HashMap::new();
    if lookups.is_empty() {
        return Ok(by_tool);
    }
    let (tool_ids, hashes): (Vec<String>, Vec<String>) = lookups
        .iter()
        .map(|(tool_id, hash)| (tool_id.to_string(), (*hash).to_string()))
        .unzip();
    let rows = sqlx::query(
        "SELECT mcp_tool_id, schema_version, schema_hash, status, description, title FROM mcp_tool_versions WHERE (mcp_tool_id, schema_hash) IN (SELECT * FROM UNNEST($1::text[], $2::text[]))",
    )
    .bind(tool_ids)
    .bind(hashes)
    .fetch_all(&mut *conn)
    .await
    .map_err(to_query_error)?;
    for row in &rows {
        let mcp_tool_id: String = row.try_get(0).map_err(to_query_error)?;
        let status: String = row.try_get(3).map_err(to_query_error)?;
        by_tool
            .entry(parse_uuid(&mcp_tool_id)?)
            .or_default()
            .push(StoredVersion {
                schema_version: row.try_get(1).map_err(to_query_error)?,
                schema_hash: row.try_get(2).map_err(to_query_error)?,
                description: row.try_get(4).map_err(to_query_error)?,
                title: row.try_get(5).map_err(to_query_error)?,
                status: McpToolVersionStatus::from_db(&status).ok_or_else(|| {
                    StoreError::Serialization(format!("invalid MCP tool version status `{status}`"))
                })?,
            });
    }
    Ok(by_tool)
}

/// Marks every pending version other than `keep` as superseded.
async fn supersede_pending(
    conn: &mut sqlx::PgConnection,
    mcp_tool_id: Uuid,
    keep: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE mcp_tool_versions SET status = 'superseded' WHERE mcp_tool_id = $1 AND status = 'pending' AND schema_version != $2",
    )
    .bind(mcp_tool_id.to_string())
    .bind(keep)
    .execute(&mut *conn)
    .await
    .map_err(to_write_error)?;
    Ok(())
}

/// Runs what discovery planned for one tool.
async fn apply_tool_write(
    conn: &mut sqlx::PgConnection,
    discovered_at: OffsetDateTime,
    tool: &UpsertExternalMcpToolRecord,
    existing: Option<&StoredTool>,
    write: &ToolWrite,
) -> Result<(), StoreError> {
    let definition = discovered_definition(tool);
    let at = discovered_at.unix_timestamp();
    let Some(existing) = existing else {
        let ToolWrite::Insert { status } = write else {
            return Err(StoreError::Unexpected(
                "a new MCP tool must be inserted".to_string(),
            ));
        };
        let id = Uuid::new_v4();
        let approved = *status == McpToolApprovalStatus::Approved;
        let (input_schema_json, annotations_json, output_schema_json) =
            definition_json(&definition)?;
        sqlx::query(
            r#"
            INSERT INTO external_mcp_tools (
                mcp_tool_id, mcp_server_id, upstream_name, display_name, description,
                input_schema_json, schema_hash, schema_version, is_active,
                first_discovered_at, last_discovered_at, deactivated_at,
                title, annotations_json, output_schema_json, metadata_captured,
                approval_status, approved_schema_version, pending_schema_version
            ) VALUES (
                $1, $2, $3, $4, $5, $6::jsonb, $7, 1, 1, $8, $8, NULL,
                $9, $10::jsonb, $11::jsonb, 1, $12, $13, $14
            )
            "#,
        )
        .bind(id.to_string())
        .bind(tool.mcp_server_id.to_string())
        .bind(tool.upstream_name.as_str())
        .bind(tool.display_name.as_str())
        .bind(tool.description.as_deref())
        .bind(input_schema_json)
        .bind(tool.schema_hash.as_str())
        .bind(at)
        .bind(tool.metadata.title.as_deref())
        .bind(annotations_json)
        .bind(output_schema_json)
        .bind(status.as_str())
        .bind(approved.then_some(1_i64))
        .bind((!approved).then_some(1_i64))
        .execute(&mut *conn)
        .await
        .map_err(to_write_error)?;
        let version_status = if approved {
            McpToolVersionStatus::Approved
        } else {
            McpToolVersionStatus::Pending
        };
        return insert_version(conn, id, 1, &definition, version_status, discovered_at).await;
    };
    let id = existing.tool.mcp_tool_id;
    // Every discovered tool is active again and keeps its latest display name.
    sqlx::query(
        "UPDATE external_mcp_tools SET display_name = $1, is_active = 1, last_discovered_at = $2, deactivated_at = NULL WHERE mcp_tool_id = $3",
    )
    .bind(tool.display_name.as_str())
    .bind(at)
    .bind(id.to_string())
    .execute(&mut *conn)
    .await
    .map_err(to_write_error)?;
    match write {
        ToolWrite::Insert { .. } => {
            return Err(StoreError::Unexpected(
                "an existing MCP tool cannot be inserted".to_string(),
            ));
        }
        ToolWrite::Touch => {}
        ToolWrite::CaptureMetadata { version } => {
            write_tool_definition(conn, id, &definition, *version).await?;
            let (input_schema_json, annotations_json, output_schema_json) =
                definition_json(&definition)?;
            sqlx::query(
                "UPDATE mcp_tool_versions SET schema_hash = $1, description = $2, input_schema_json = $3::jsonb, title = $4, annotations_json = $5::jsonb, output_schema_json = $6::jsonb WHERE mcp_tool_id = $7 AND schema_version = $8",
            )
            .bind(definition.schema_hash.as_str())
            .bind(definition.description.as_deref())
            .bind(input_schema_json)
            .bind(definition.title.as_deref())
            .bind(annotations_json)
            .bind(output_schema_json)
            .bind(id.to_string())
            .bind(*version)
            .execute(&mut *conn)
            .await
            .map_err(to_write_error)?;
        }
        ToolWrite::Serve {
            version,
            insert_version: insert,
        } => {
            write_tool_definition(conn, id, &definition, *version).await?;
            sqlx::query("UPDATE external_mcp_tools SET approval_status = 'approved', approved_schema_version = $1, pending_schema_version = NULL WHERE mcp_tool_id = $2")
                .bind(*version)
                .bind(id.to_string())
                .execute(&mut *conn)
                .await
                .map_err(to_write_error)?;
            supersede_pending(conn, id, *version).await?;
            if *insert {
                insert_version(
                    conn,
                    id,
                    *version,
                    &definition,
                    McpToolVersionStatus::Approved,
                    discovered_at,
                )
                .await?;
            } else {
                auto_approve_version(conn, id, *version).await?;
            }
        }
        ToolWrite::Hold {
            version,
            status,
            write_definition,
            insert_version: insert,
        } => {
            supersede_pending(conn, id, *version).await?;
            if *insert {
                insert_version(
                    conn,
                    id,
                    *version,
                    &definition,
                    McpToolVersionStatus::Pending,
                    discovered_at,
                )
                .await?;
            } else {
                set_version_status(
                    conn,
                    id,
                    *version,
                    McpToolVersionStatus::Pending,
                    None,
                    None,
                )
                .await?;
            }
            if *write_definition {
                write_tool_definition(conn, id, &definition, *version).await?;
            }
            sqlx::query("UPDATE external_mcp_tools SET approval_status = $1, pending_schema_version = $2 WHERE mcp_tool_id = $3")
                .bind(status.as_str())
                .bind(*version)
                .bind(id.to_string())
                .execute(&mut *conn)
                .await
                .map_err(to_write_error)?;
        }
        ToolWrite::Settle {
            status,
            restore_version,
        } => {
            supersede_pending(conn, id, -1).await?;
            if let Some(version) = restore_version {
                write_tool_definition(conn, id, &definition, *version).await?;
            }
            sqlx::query("UPDATE external_mcp_tools SET approval_status = $1, pending_schema_version = NULL WHERE mcp_tool_id = $2")
                .bind(status.as_str())
                .bind(id.to_string())
                .execute(&mut *conn)
                .await
                .map_err(to_write_error)?;
        }
        ToolWrite::Revert => {
            supersede_pending(conn, id, -1).await?;
            sqlx::query("UPDATE external_mcp_tools SET approval_status = 'approved', pending_schema_version = NULL WHERE mcp_tool_id = $1")
                .bind(id.to_string())
                .execute(&mut *conn)
                .await
                .map_err(to_write_error)?;
        }
    }
    Ok(())
}
