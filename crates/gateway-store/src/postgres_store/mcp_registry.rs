use super::*;
use crate::shared::{json_object_from_str, parse_uuid, serialize_json, unix_to_datetime};

macro_rules! server_columns {
    () => {
        "mcp_server_id, server_key, display_name, description, transport, server_url, auth_mode, auth_config_json::text, timeout_ms, status, last_discovery_status, last_discovery_at, last_successful_discovery_at, last_error_summary, last_tool_count, created_at, updated_at, disabled_at"
    };
}

const SERVER_COLUMNS: &str = server_columns!();
const SERVER_SELECT_BY_ID: &str = concat!(
    "SELECT ",
    server_columns!(),
    " FROM external_mcp_servers WHERE mcp_server_id = $1"
);
const TOOL_COLUMNS: &str = "mcp_tool_id, mcp_server_id, upstream_name, display_name, description, input_schema_json::text, schema_hash, schema_version, is_active, first_discovered_at, last_discovered_at, deactivated_at";

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
        schema_hash: row.try_get(6).map_err(to_query_error)?,
        schema_version: row.try_get(7).map_err(to_query_error)?,
        is_active: is_active == 1,
        first_discovered_at: unix_to_datetime(first_discovered_at)?,
        last_discovered_at: unix_to_datetime(last_discovered_at)?,
        deactivated_at: deactivated_at.map(unix_to_datetime).transpose()?,
    })
}

fn discovery_config_changed(
    existing: &ExternalMcpServerRecord,
    input: &UpdateExternalMcpServerRecord,
) -> bool {
    existing.server_url != input.server_url
        || existing.auth_mode != input.auth_mode
        || existing.auth_config != input.auth_config
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
        sqlx::query(
            r#"
            INSERT INTO external_mcp_servers (
                mcp_server_id, server_key, display_name, description, transport, server_url,
                auth_mode, auth_config_json, timeout_ms, status, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9, 'active', $10, $10)
            "#,
        )
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
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        load_server(&self.pool, id).await
    }

    async fn update_external_mcp_server(
        &self,
        input: &UpdateExternalMcpServerRecord,
    ) -> Result<ExternalMcpServerRecord, StoreError> {
        let existing = load_server(&self.pool, input.mcp_server_id).await?;
        let invalidate_discovery = discovery_config_changed(&existing, input);
        let auth_config_json = serialize_json(&input.auth_config)?;
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        sqlx::query(
            r#"
            UPDATE external_mcp_servers
            SET display_name = $1, description = $2, server_url = $3, auth_mode = $4,
                auth_config_json = $5::jsonb, timeout_ms = $6, updated_at = $7
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
        tx.commit().await.map_err(to_query_error)?;
        load_server(&self.pool, input.mcp_server_id).await
    }

    async fn disable_external_mcp_server(
        &self,
        mcp_server_id: Uuid,
        disabled_at: OffsetDateTime,
    ) -> Result<ExternalMcpServerRecord, StoreError> {
        let changed = sqlx::query(
            r#"
            UPDATE external_mcp_servers
            SET status = 'disabled', disabled_at = $1, updated_at = $1,
                last_discovery_status = 'disabled', last_discovery_at = $1
            WHERE mcp_server_id = $2
            "#,
        )
        .bind(disabled_at.unix_timestamp())
        .bind(mcp_server_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        if changed.rows_affected() == 0 {
            return Err(StoreError::NotFound(format!(
                "external MCP server `{mcp_server_id}` not found"
            )));
        }
        load_server(&self.pool, mcp_server_id).await
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
            sqlx::query(
                r#"
                INSERT INTO external_mcp_tools (
                    mcp_tool_id, mcp_server_id, upstream_name, display_name, description,
                    input_schema_json, schema_hash, schema_version, is_active,
                    first_discovered_at, last_discovered_at, deactivated_at
                ) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, 1, 1, $8, $8, NULL)
                ON CONFLICT(mcp_server_id, upstream_name) DO UPDATE SET
                    display_name = excluded.display_name,
                    description = excluded.description,
                    input_schema_json = excluded.input_schema_json,
                    schema_version = CASE
                        WHEN external_mcp_tools.schema_hash != excluded.schema_hash
                        THEN external_mcp_tools.schema_version + 1
                        ELSE external_mcp_tools.schema_version
                    END,
                    schema_hash = excluded.schema_hash,
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
