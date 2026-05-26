use super::*;
use crate::shared::{json_object_from_str, parse_uuid, serialize_json, unix_to_datetime};

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
        schema_hash: row.get(6).map_err(to_query_error)?,
        schema_version: row.get(7).map_err(to_query_error)?,
        is_active: is_active == 1,
        first_discovered_at: unix_to_datetime(first_discovered_at)?,
        last_discovered_at: unix_to_datetime(last_discovered_at)?,
        deactivated_at: deactivated_at.map(unix_to_datetime).transpose()?,
    })
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

const SERVER_COLUMNS: &str = "mcp_server_id, server_key, display_name, description, transport, server_url, auth_mode, auth_config_json, timeout_ms, status, last_discovery_status, last_discovery_at, last_successful_discovery_at, last_error_summary, last_tool_count, created_at, updated_at, disabled_at";
const SERVER_SELECT_BY_ID: &str = "SELECT mcp_server_id, server_key, display_name, description, transport, server_url, auth_mode, auth_config_json, timeout_ms, status, last_discovery_status, last_discovery_at, last_successful_discovery_at, last_error_summary, last_tool_count, created_at, updated_at, disabled_at FROM external_mcp_servers WHERE mcp_server_id = ?1";
const TOOL_COLUMNS: &str = "mcp_tool_id, mcp_server_id, upstream_name, display_name, description, input_schema_json, schema_hash, schema_version, is_active, first_discovered_at, last_discovered_at, deactivated_at";

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
        self.connection
            .execute(
                r#"
                INSERT INTO external_mcp_servers (
                    mcp_server_id, server_key, display_name, description, transport, server_url,
                    auth_mode, auth_config_json, timeout_ms, status, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?10)
                "#,
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
                ],
            )
            .await
            .map_err(to_write_error)?;
        load_server(&self.connection, id).await
    }

    async fn update_external_mcp_server(
        &self,
        input: &UpdateExternalMcpServerRecord,
    ) -> Result<ExternalMcpServerRecord, StoreError> {
        let auth_config_json = serialize_json(&input.auth_config)?;
        let changed = self
            .connection
            .execute(
                r#"
                UPDATE external_mcp_servers
                SET display_name = ?1, description = ?2, server_url = ?3, auth_mode = ?4,
                    auth_config_json = ?5, timeout_ms = ?6, updated_at = ?7
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
                ],
            )
            .await
            .map_err(to_write_error)?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!(
                "external MCP server `{}` not found",
                input.mcp_server_id
            )));
        }
        load_server(&self.connection, input.mcp_server_id).await
    }

    async fn disable_external_mcp_server(
        &self,
        mcp_server_id: Uuid,
        disabled_at: OffsetDateTime,
    ) -> Result<ExternalMcpServerRecord, StoreError> {
        let changed = self
            .connection
            .execute(
                r#"
                UPDATE external_mcp_servers
                SET status = 'disabled', disabled_at = ?1, updated_at = ?1,
                    last_discovery_status = 'disabled', last_discovery_at = ?1
                WHERE mcp_server_id = ?2
                "#,
                libsql::params![disabled_at.unix_timestamp(), mcp_server_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!(
                "external MCP server `{mcp_server_id}` not found"
            )));
        }
        load_server(&self.connection, mcp_server_id).await
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
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
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
            tx.execute(
                r#"
                INSERT INTO external_mcp_tools (
                    mcp_tool_id, mcp_server_id, upstream_name, display_name, description,
                    input_schema_json, schema_hash, schema_version, is_active,
                    first_discovered_at, last_discovered_at, deactivated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 1, ?8, ?8, NULL)
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
                libsql::params![
                    Uuid::new_v4().to_string(),
                    tool.mcp_server_id.to_string(),
                    tool.upstream_name.as_str(),
                    tool.display_name.as_str(),
                    tool.description.as_deref(),
                    input_schema_json,
                    tool.schema_hash.as_str(),
                    run.finished_at.unix_timestamp(),
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
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
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
