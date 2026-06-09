use std::collections::HashMap;

use super::*;
use crate::shared::{json_object_from_str, parse_uuid, unix_to_datetime};

const TOOLSET_COLUMNS: &str = "toolset_id, toolset_key, display_name, description, status, created_at, updated_at, disabled_at";
const TOOL_COLUMNS: &str = "t.mcp_tool_id, t.mcp_server_id, t.upstream_name, t.display_name, t.description, t.input_schema_json, t.schema_hash, t.schema_version, t.is_active, t.first_discovered_at, t.last_discovered_at, t.deactivated_at";
const SERVER_COLUMNS: &str = "s.mcp_server_id, s.server_key, s.display_name, s.description, s.transport, s.server_url, s.auth_mode, s.auth_config_json, s.timeout_ms, s.status, s.last_discovery_status, s.last_discovery_at, s.last_successful_discovery_at, s.last_error_summary, s.last_tool_count, s.created_at, s.updated_at, s.disabled_at";

fn decode_toolset(row: &libsql::Row) -> Result<McpToolsetRecord, StoreError> {
    let toolset_id: String = row.get(0).map_err(to_query_error)?;
    let status: String = row.get(4).map_err(to_query_error)?;
    let created_at: i64 = row.get(5).map_err(to_query_error)?;
    let updated_at: i64 = row.get(6).map_err(to_query_error)?;
    let disabled_at: Option<i64> = row.get(7).map_err(to_query_error)?;
    Ok(McpToolsetRecord {
        toolset_id: parse_uuid(&toolset_id)?,
        toolset_key: row.get(1).map_err(to_query_error)?,
        display_name: row.get(2).map_err(to_query_error)?,
        description: row.get(3).map_err(to_query_error)?,
        status: McpToolsetStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP toolset status `{status}`"))
        })?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
        disabled_at: disabled_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_toolset_tool(row: &libsql::Row) -> Result<McpToolsetToolRecord, StoreError> {
    let toolset_id: String = row.get(0).map_err(to_query_error)?;
    let mcp_tool_id: String = row.get(1).map_err(to_query_error)?;
    let created_at: i64 = row.get(2).map_err(to_query_error)?;
    Ok(McpToolsetToolRecord {
        toolset_id: parse_uuid(&toolset_id)?,
        mcp_tool_id: parse_uuid(&mcp_tool_id)?,
        created_at: unix_to_datetime(created_at)?,
    })
}

fn decode_grant(row: &libsql::Row) -> Result<McpToolGrantRecord, StoreError> {
    let grant_id: String = row.get(0).map_err(to_query_error)?;
    let subject_kind: String = row.get(1).map_err(to_query_error)?;
    let subject_id: String = row.get(2).map_err(to_query_error)?;
    let target_kind: String = row.get(3).map_err(to_query_error)?;
    let target_id: String = row.get(4).map_err(to_query_error)?;
    let is_active: i64 = row.get(5).map_err(to_query_error)?;
    let created_at: i64 = row.get(6).map_err(to_query_error)?;
    let updated_at: i64 = row.get(7).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.get(8).map_err(to_query_error)?;
    Ok(McpToolGrantRecord {
        grant_id: parse_uuid(&grant_id)?,
        subject_kind: McpToolGrantSubjectKind::from_db(&subject_kind).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP grant subject `{subject_kind}`"))
        })?,
        subject_id: parse_uuid(&subject_id)?,
        target_kind: McpToolGrantTargetKind::from_db(&target_kind).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP grant target `{target_kind}`"))
        })?,
        target_id: parse_uuid(&target_id)?,
        is_active: is_active == 1,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_tool(row: &libsql::Row) -> Result<ExternalMcpToolRecord, StoreError> {
    let mcp_tool_id: String = row.get(0).map_err(to_query_error)?;
    let mcp_server_id: String = row.get(1).map_err(to_query_error)?;
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
        description: row.get(4).map_err(to_query_error)?,
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

fn libsql_idx(offset: usize, column: usize) -> i32 {
    (offset + column) as i32
}

fn decode_server_at(
    row: &libsql::Row,
    offset: usize,
) -> Result<ExternalMcpServerRecord, StoreError> {
    let mcp_server_id: String = row.get(libsql_idx(offset, 0)).map_err(to_query_error)?;
    let transport: String = row.get(libsql_idx(offset, 4)).map_err(to_query_error)?;
    let auth_mode: String = row.get(libsql_idx(offset, 6)).map_err(to_query_error)?;
    let auth_config_json: String = row.get(libsql_idx(offset, 7)).map_err(to_query_error)?;
    let status: String = row.get(libsql_idx(offset, 9)).map_err(to_query_error)?;
    let last_discovery_status: Option<String> =
        row.get(libsql_idx(offset, 10)).map_err(to_query_error)?;
    let last_discovery_at: Option<i64> = row.get(libsql_idx(offset, 11)).map_err(to_query_error)?;
    let last_successful_discovery_at: Option<i64> =
        row.get(libsql_idx(offset, 12)).map_err(to_query_error)?;
    let created_at: i64 = row.get(libsql_idx(offset, 15)).map_err(to_query_error)?;
    let updated_at: i64 = row.get(libsql_idx(offset, 16)).map_err(to_query_error)?;
    let disabled_at: Option<i64> = row.get(libsql_idx(offset, 17)).map_err(to_query_error)?;
    Ok(ExternalMcpServerRecord {
        mcp_server_id: parse_uuid(&mcp_server_id)?,
        server_key: row.get(libsql_idx(offset, 1)).map_err(to_query_error)?,
        display_name: row.get(libsql_idx(offset, 2)).map_err(to_query_error)?,
        description: row.get(libsql_idx(offset, 3)).map_err(to_query_error)?,
        transport: ExternalMcpTransport::from_db(&transport).ok_or_else(|| {
            StoreError::Serialization(format!("invalid external MCP transport `{transport}`"))
        })?,
        server_url: row.get(libsql_idx(offset, 5)).map_err(to_query_error)?,
        auth_mode: ExternalMcpAuthMode::from_db(&auth_mode).ok_or_else(|| {
            StoreError::Serialization(format!("invalid external MCP auth mode `{auth_mode}`"))
        })?,
        auth_config: json_object_from_str(&auth_config_json)?,
        timeout_ms: row.get(libsql_idx(offset, 8)).map_err(to_query_error)?,
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
        last_error_summary: row.get(libsql_idx(offset, 13)).map_err(to_query_error)?,
        last_tool_count: row.get(libsql_idx(offset, 14)).map_err(to_query_error)?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
        disabled_at: disabled_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_catalog_tool(row: &libsql::Row) -> Result<McpCatalogToolRecord, StoreError> {
    Ok(McpCatalogToolRecord {
        tool: decode_tool(row)?,
        server: decode_server_at(row, 12)?,
    })
}

async fn load_toolset(
    connection: &libsql::Connection,
    toolset_id: Uuid,
) -> Result<McpToolsetRecord, StoreError> {
    let sql = format!("SELECT {TOOLSET_COLUMNS} FROM mcp_toolsets WHERE toolset_id = ?1");
    let mut rows = connection
        .query(&sql, [toolset_id.to_string()])
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .map(|row| decode_toolset(&row))
        .transpose()?
        .ok_or_else(|| StoreError::NotFound(format!("MCP toolset `{toolset_id}` was not found")))
}

#[async_trait]
impl McpAccessRepository for LibsqlStore {
    async fn list_mcp_toolsets(
        &self,
        include_disabled: bool,
    ) -> Result<Vec<McpToolsetRecord>, StoreError> {
        let sql = format!(
            "SELECT {TOOLSET_COLUMNS} FROM mcp_toolsets WHERE (?1 = 1 OR status != 'disabled') ORDER BY toolset_key"
        );
        let mut rows = self
            .connection
            .query(&sql, [if include_disabled { 1_i64 } else { 0_i64 }])
            .await
            .map_err(to_query_error)?;
        let mut toolsets = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            toolsets.push(decode_toolset(&row)?);
        }
        Ok(toolsets)
    }

    async fn create_mcp_toolset(
        &self,
        input: &NewMcpToolsetRecord,
    ) -> Result<McpToolsetRecord, StoreError> {
        let id = Uuid::new_v4();
        self.connection
            .execute(
                r#"
                INSERT INTO mcp_toolsets (
                    toolset_id, toolset_key, display_name, description, status, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?5)
                "#,
                libsql::params![
                    id.to_string(),
                    input.toolset_key.as_str(),
                    input.display_name.as_str(),
                    input.description.as_deref(),
                    input.created_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        load_toolset(&self.connection, id).await
    }

    async fn update_mcp_toolset(
        &self,
        input: &UpdateMcpToolsetRecord,
    ) -> Result<McpToolsetRecord, StoreError> {
        let changed = self
            .connection
            .execute(
                r#"
                UPDATE mcp_toolsets
                SET display_name = ?1, description = ?2, updated_at = ?3
                WHERE toolset_id = ?4 AND status != 'disabled'
                "#,
                libsql::params![
                    input.display_name.as_str(),
                    input.description.as_deref(),
                    input.updated_at.unix_timestamp(),
                    input.toolset_id.to_string(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!(
                "active MCP toolset `{}` was not found",
                input.toolset_id
            )));
        }
        load_toolset(&self.connection, input.toolset_id).await
    }

    async fn disable_mcp_toolset(
        &self,
        toolset_id: Uuid,
        disabled_at: OffsetDateTime,
    ) -> Result<McpToolsetRecord, StoreError> {
        let changed = self
            .connection
            .execute(
                "UPDATE mcp_toolsets SET status = 'disabled', updated_at = ?1, disabled_at = ?1 WHERE toolset_id = ?2 AND status != 'disabled'",
                libsql::params![disabled_at.unix_timestamp(), toolset_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!(
                "active MCP toolset `{toolset_id}` was not found"
            )));
        }
        load_toolset(&self.connection, toolset_id).await
    }

    async fn replace_mcp_toolset_tools(
        &self,
        toolset_id: Uuid,
        tool_ids: &[Uuid],
        updated_at: OffsetDateTime,
    ) -> Result<Vec<McpToolsetToolRecord>, StoreError> {
        let tx = self
            .connection
            .transaction()
            .await
            .map_err(to_query_error)?;
        tx.execute(
            "DELETE FROM mcp_toolset_tools WHERE toolset_id = ?1",
            [toolset_id.to_string()],
        )
        .await
        .map_err(to_write_error)?;
        for tool_id in tool_ids {
            tx.execute(
                "INSERT INTO mcp_toolset_tools (toolset_id, mcp_tool_id, created_at) VALUES (?1, ?2, ?3)",
                libsql::params![
                    toolset_id.to_string(),
                    tool_id.to_string(),
                    updated_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        }
        tx.execute(
            "UPDATE mcp_toolsets SET updated_at = ?1 WHERE toolset_id = ?2",
            libsql::params![updated_at.unix_timestamp(), toolset_id.to_string()],
        )
        .await
        .map_err(to_write_error)?;
        tx.commit().await.map_err(to_write_error)?;
        self.list_mcp_toolset_tools(toolset_id).await
    }

    async fn list_mcp_toolset_tools(
        &self,
        toolset_id: Uuid,
    ) -> Result<Vec<McpToolsetToolRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                "SELECT toolset_id, mcp_tool_id, created_at FROM mcp_toolset_tools WHERE toolset_id = ?1 ORDER BY mcp_tool_id",
                [toolset_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        let mut tools = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            tools.push(decode_toolset_tool(&row)?);
        }
        Ok(tools)
    }

    async fn upsert_mcp_tool_grant(
        &self,
        grant: &UpsertMcpToolGrantRecord,
    ) -> Result<McpToolGrantRecord, StoreError> {
        let id = Uuid::new_v4();
        self.connection
            .execute(
                r#"
                INSERT INTO mcp_tool_grants (
                    grant_id, subject_kind, subject_id, target_kind, target_id, is_active, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)
                ON CONFLICT(subject_kind, subject_id, target_kind, target_id) WHERE is_active = 1
                DO UPDATE SET updated_at = excluded.updated_at, revoked_at = NULL
                "#,
                libsql::params![
                    id.to_string(),
                    grant.subject_kind.as_str(),
                    grant.subject_id.to_string(),
                    grant.target_kind.as_str(),
                    grant.target_id.to_string(),
                    grant.updated_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        self.list_mcp_tool_grants(Some(grant.subject_kind), Some(grant.subject_id))
            .await?
            .into_iter()
            .find(|record| {
                record.target_kind == grant.target_kind
                    && record.target_id == grant.target_id
                    && record.is_active
            })
            .ok_or_else(|| StoreError::Unexpected("failed loading upserted MCP grant".to_string()))
    }

    async fn revoke_mcp_tool_grant(
        &self,
        subject_kind: McpToolGrantSubjectKind,
        subject_id: Uuid,
        target_kind: McpToolGrantTargetKind,
        target_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let changed = self
            .connection
            .execute(
                r#"
                UPDATE mcp_tool_grants
                SET is_active = 0, revoked_at = ?1, updated_at = ?1
                WHERE subject_kind = ?2 AND subject_id = ?3 AND target_kind = ?4 AND target_id = ?5 AND is_active = 1
                "#,
                libsql::params![
                    revoked_at.unix_timestamp(),
                    subject_kind.as_str(),
                    subject_id.to_string(),
                    target_kind.as_str(),
                    target_id.to_string(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        Ok(changed > 0)
    }

    async fn list_mcp_tool_grants(
        &self,
        subject_kind: Option<McpToolGrantSubjectKind>,
        subject_id: Option<Uuid>,
    ) -> Result<Vec<McpToolGrantRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT grant_id, subject_kind, subject_id, target_kind, target_id, is_active, created_at, updated_at, revoked_at
                FROM mcp_tool_grants
                WHERE (?1 IS NULL OR subject_kind = ?1) AND (?2 IS NULL OR subject_id = ?2)
                ORDER BY subject_kind, subject_id, target_kind, target_id
                "#,
                libsql::params![
                    subject_kind.map(McpToolGrantSubjectKind::as_str),
                    subject_id.map(|value| value.to_string()),
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut grants = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            grants.push(decode_grant(&row)?);
        }
        Ok(grants)
    }

    async fn resolve_mcp_access_for_subjects(
        &self,
        subjects: &[McpGrantSubject],
        mcp_server_id: Option<Uuid>,
    ) -> Result<McpAccessResolution, StoreError> {
        let exposed_tool_count = active_tool_count(&self.connection, mcp_server_id).await?;
        let mut tools_by_id: HashMap<Uuid, ExternalMcpToolRecord> = HashMap::new();

        for subject in subjects {
            collect_direct_tools(&self.connection, subject, mcp_server_id, &mut tools_by_id)
                .await?;
            collect_toolset_tools(&self.connection, subject, mcp_server_id, &mut tools_by_id)
                .await?;
        }

        let mut allowed_tools: Vec<_> = tools_by_id.into_values().collect();
        allowed_tools.sort_by(|a, b| {
            a.mcp_server_id
                .cmp(&b.mcp_server_id)
                .then_with(|| a.upstream_name.cmp(&b.upstream_name))
        });
        let referenced_server_count = allowed_tools
            .iter()
            .map(|tool| tool.mcp_server_id)
            .collect::<std::collections::HashSet<_>>()
            .len() as i64;
        let filtered_tool_count = exposed_tool_count.saturating_sub(allowed_tools.len() as i64);

        Ok(McpAccessResolution {
            allowed_tools,
            referenced_server_count,
            exposed_tool_count,
            filtered_tool_count,
        })
    }

    async fn resolve_mcp_catalog_access_for_subjects(
        &self,
        subjects: &[McpGrantSubject],
        server_key: Option<&str>,
    ) -> Result<McpCatalogAccessResolution, StoreError> {
        let exposed_tool_count = active_catalog_tool_count(&self.connection, server_key).await?;
        let mut tools_by_id: HashMap<Uuid, McpCatalogToolRecord> = HashMap::new();

        for subject in subjects {
            collect_catalog_direct_tools(&self.connection, subject, server_key, &mut tools_by_id)
                .await?;
            collect_catalog_toolset_tools(&self.connection, subject, server_key, &mut tools_by_id)
                .await?;
        }

        let mut allowed_tools: Vec<_> = tools_by_id.into_values().collect();
        allowed_tools.sort_by(|a, b| {
            a.server
                .server_key
                .cmp(&b.server.server_key)
                .then_with(|| a.tool.upstream_name.cmp(&b.tool.upstream_name))
                .then_with(|| a.tool.mcp_tool_id.cmp(&b.tool.mcp_tool_id))
        });
        let referenced_server_count = allowed_tools
            .iter()
            .map(|record| record.server.mcp_server_id)
            .collect::<std::collections::HashSet<_>>()
            .len() as i64;
        let filtered_tool_count = exposed_tool_count.saturating_sub(allowed_tools.len() as i64);

        Ok(McpCatalogAccessResolution {
            allowed_tools,
            referenced_server_count,
            exposed_tool_count,
            filtered_tool_count,
        })
    }

    async fn get_active_mcp_tool_by_name(
        &self,
        mcp_server_id: Uuid,
        upstream_name: &str,
    ) -> Result<Option<ExternalMcpToolRecord>, StoreError> {
        let sql = format!(
            "SELECT {TOOL_COLUMNS} FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_server_id = ?1 AND t.upstream_name = ?2 AND t.is_active = 1 AND s.status = 'active'"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![mcp_server_id.to_string(), upstream_name],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_tool(&row))
            .transpose()
    }
}

async fn active_catalog_tool_count(
    connection: &libsql::Connection,
    server_key: Option<&str>,
) -> Result<i64, StoreError> {
    let mut rows = connection
        .query(
            r#"
            SELECT COUNT(*)
            FROM external_mcp_tools t
            JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id
            WHERE t.is_active = 1 AND s.status = 'active' AND (?1 IS NULL OR s.server_key = ?1)
            "#,
            libsql::params![server_key],
        )
        .await
        .map_err(to_query_error)?;
    let row = rows.next().await.map_err(to_query_error)?.ok_or_else(|| {
        StoreError::Unexpected("active MCP catalog tool count returned no row".to_string())
    })?;
    row.get(0).map_err(to_query_error)
}

async fn active_tool_count(
    connection: &libsql::Connection,
    mcp_server_id: Option<Uuid>,
) -> Result<i64, StoreError> {
    let mut rows = connection
        .query(
            r#"
            SELECT COUNT(*)
            FROM external_mcp_tools t
            JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id
            WHERE t.is_active = 1 AND s.status = 'active' AND (?1 IS NULL OR t.mcp_server_id = ?1)
            "#,
            libsql::params![mcp_server_id.map(|value| value.to_string())],
        )
        .await
        .map_err(to_query_error)?;
    let row = rows.next().await.map_err(to_query_error)?.ok_or_else(|| {
        StoreError::Unexpected("active MCP tool count returned no row".to_string())
    })?;
    row.get(0).map_err(to_query_error)
}

async fn collect_direct_tools(
    connection: &libsql::Connection,
    subject: &McpGrantSubject,
    mcp_server_id: Option<Uuid>,
    out: &mut HashMap<Uuid, ExternalMcpToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {TOOL_COLUMNS} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND g.subject_kind = ?1 AND g.subject_id = ?2 AND t.is_active = 1 AND s.status = 'active' AND (?3 IS NULL OR t.mcp_server_id = ?3)"
    );
    let mut rows = connection
        .query(
            &sql,
            libsql::params![
                subject.subject_kind.as_str(),
                subject.subject_id.to_string(),
                mcp_server_id.map(|value| value.to_string()),
            ],
        )
        .await
        .map_err(to_query_error)?;
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        let tool = decode_tool(&row)?;
        out.insert(tool.mcp_tool_id, tool);
    }
    Ok(())
}

async fn collect_toolset_tools(
    connection: &libsql::Connection,
    subject: &McpGrantSubject,
    mcp_server_id: Option<Uuid>,
    out: &mut HashMap<Uuid, ExternalMcpToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {TOOL_COLUMNS} FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND g.subject_kind = ?1 AND g.subject_id = ?2 AND ts.status = 'active' AND t.is_active = 1 AND s.status = 'active' AND (?3 IS NULL OR t.mcp_server_id = ?3)"
    );
    let mut rows = connection
        .query(
            &sql,
            libsql::params![
                subject.subject_kind.as_str(),
                subject.subject_id.to_string(),
                mcp_server_id.map(|value| value.to_string()),
            ],
        )
        .await
        .map_err(to_query_error)?;
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        let tool = decode_tool(&row)?;
        out.insert(tool.mcp_tool_id, tool);
    }
    Ok(())
}

async fn collect_catalog_direct_tools(
    connection: &libsql::Connection,
    subject: &McpGrantSubject,
    server_key: Option<&str>,
    out: &mut HashMap<Uuid, McpCatalogToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND g.subject_kind = ?1 AND g.subject_id = ?2 AND t.is_active = 1 AND s.status = 'active' AND (?3 IS NULL OR s.server_key = ?3)"
    );
    let mut rows = connection
        .query(
            &sql,
            libsql::params![
                subject.subject_kind.as_str(),
                subject.subject_id.to_string(),
                server_key,
            ],
        )
        .await
        .map_err(to_query_error)?;
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        let record = decode_catalog_tool(&row)?;
        out.insert(record.tool.mcp_tool_id, record);
    }
    Ok(())
}

async fn collect_catalog_toolset_tools(
    connection: &libsql::Connection,
    subject: &McpGrantSubject,
    server_key: Option<&str>,
    out: &mut HashMap<Uuid, McpCatalogToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND g.subject_kind = ?1 AND g.subject_id = ?2 AND ts.status = 'active' AND t.is_active = 1 AND s.status = 'active' AND (?3 IS NULL OR s.server_key = ?3)"
    );
    let mut rows = connection
        .query(
            &sql,
            libsql::params![
                subject.subject_kind.as_str(),
                subject.subject_id.to_string(),
                server_key,
            ],
        )
        .await
        .map_err(to_query_error)?;
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        let record = decode_catalog_tool(&row)?;
        out.insert(record.tool.mcp_tool_id, record);
    }
    Ok(())
}
