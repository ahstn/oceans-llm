use std::collections::{BTreeSet, HashMap};

use gateway_core::McpToolProjection;

use gateway_core::{McpTagRuleSet, McpToolTagRuleKey, McpToolTagRuleRecord};

use super::mcp_registry::{decode_tool_metadata, decode_tool_policy};
use super::*;
use crate::mcp_tool_policy::{TAG_RULE_COLUMNS, TagRuleRow};
use crate::shared::{json_object_from_str, parse_uuid, unix_to_datetime};
use gateway_core::Updated;

const TOOLSET_COLUMNS: &str = "toolset_id, toolset_key, display_name, description, status, created_at, updated_at, disabled_at";
const TOOL_COLUMNS: &str = "t.mcp_tool_id, t.mcp_server_id, t.upstream_name, t.display_name, t.description, t.input_schema_json::text, t.schema_hash, t.schema_version, t.is_active, t.first_discovered_at, t.last_discovered_at, t.deactivated_at, t.title, t.annotations_json::text, t.output_schema_json::text, t.admin_tags_json::text, t.annotation_overrides_json::text";
/// `TOOL_COLUMNS` with the title and output schema replaced by NULLs. The row
/// keeps the same shape, so `decode_tool` reads it. Annotations and the admin
/// policy stay, because tag rules are evaluated from them.
const ACCESS_TOOL_COLUMNS: &str = "t.mcp_tool_id, t.mcp_server_id, t.upstream_name, t.display_name, t.description, t.input_schema_json::text, t.schema_hash, t.schema_version, t.is_active, t.first_discovered_at, t.last_discovered_at, t.deactivated_at, NULL::text, t.annotations_json::text, NULL::text, t.admin_tags_json::text, t.annotation_overrides_json::text";
/// Number of columns in `TOOL_COLUMNS`; server columns follow them.
/// `TOOL_COLUMNS` with only what tag rules read: identity, annotations and
/// the admin policy. Schemas and text are left out, so scanning a server's
/// tools for include-rule matches stays small.
const RULE_TOOL_COLUMNS: &str = "t.mcp_tool_id, t.mcp_server_id, t.upstream_name, t.display_name, NULL::text, '{}'::text, t.schema_hash, t.schema_version, t.is_active, t.first_discovered_at, t.last_discovered_at, t.deactivated_at, NULL::text, t.annotations_json::text, NULL::text, t.admin_tags_json::text, t.annotation_overrides_json::text";
const TOOL_COLUMN_COUNT: usize = 17;
const SERVER_COLUMNS: &str = "s.mcp_server_id, s.server_key, s.display_name, s.description, s.transport, s.server_url, s.auth_mode, s.auth_config_json::text, s.timeout_ms, s.status, s.last_discovery_status, s.last_discovery_at, s.last_successful_discovery_at, s.last_error_summary, s.last_tool_count, s.created_at, s.updated_at, s.disabled_at";

fn decode_toolset(row: &PgRow) -> Result<McpToolsetRecord, StoreError> {
    let status: String = row.try_get(4).map_err(to_query_error)?;
    let disabled_at: Option<i64> = row.try_get(7).map_err(to_query_error)?;
    Ok(McpToolsetRecord {
        toolset_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        toolset_key: row.try_get(1).map_err(to_query_error)?,
        display_name: row.try_get(2).map_err(to_query_error)?,
        description: row.try_get(3).map_err(to_query_error)?,
        status: McpToolsetStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP toolset status `{status}`"))
        })?,
        created_at: unix_to_datetime(row.try_get(5).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(6).map_err(to_query_error)?)?,
        disabled_at: disabled_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_toolset_tool(row: &PgRow) -> Result<McpToolsetToolRecord, StoreError> {
    Ok(McpToolsetToolRecord {
        toolset_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        mcp_tool_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        created_at: unix_to_datetime(row.try_get(2).map_err(to_query_error)?)?,
    })
}

fn decode_grant(row: &PgRow) -> Result<McpToolGrantRecord, StoreError> {
    let subject_kind: String = row.try_get(1).map_err(to_query_error)?;
    let target_kind: String = row.try_get(3).map_err(to_query_error)?;
    let revoked_at: Option<i64> = row.try_get(8).map_err(to_query_error)?;
    Ok(McpToolGrantRecord {
        grant_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        subject_kind: McpToolGrantSubjectKind::from_db(&subject_kind).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP grant subject `{subject_kind}`"))
        })?,
        subject_id: parse_uuid(&row.try_get::<String, _>(2).map_err(to_query_error)?)?,
        target_kind: McpToolGrantTargetKind::from_db(&target_kind).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP grant target `{target_kind}`"))
        })?,
        target_id: parse_uuid(&row.try_get::<String, _>(4).map_err(to_query_error)?)?,
        is_active: row.try_get::<i64, _>(5).map_err(to_query_error)? == 1,
        created_at: unix_to_datetime(row.try_get(6).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(7).map_err(to_query_error)?)?,
        revoked_at: revoked_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_tool(row: &PgRow) -> Result<ExternalMcpToolRecord, StoreError> {
    let input_schema_json: String = row.try_get(5).map_err(to_query_error)?;
    let deactivated_at: Option<i64> = row.try_get(11).map_err(to_query_error)?;
    Ok(ExternalMcpToolRecord {
        mcp_tool_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
        mcp_server_id: parse_uuid(&row.try_get::<String, _>(1).map_err(to_query_error)?)?,
        upstream_name: row.try_get(2).map_err(to_query_error)?,
        display_name: row.try_get(3).map_err(to_query_error)?,
        description: row.try_get(4).map_err(to_query_error)?,
        input_schema: serde_json::from_str(&input_schema_json)
            .map_err(|error| StoreError::Serialization(error.to_string()))?,
        metadata: decode_tool_metadata(row, 12)?,
        policy: decode_tool_policy(row, 15)?,
        schema_hash: row.try_get(6).map_err(to_query_error)?,
        schema_version: row.try_get(7).map_err(to_query_error)?,
        is_active: row.try_get::<i64, _>(8).map_err(to_query_error)? == 1,
        first_discovered_at: unix_to_datetime(row.try_get(9).map_err(to_query_error)?)?,
        last_discovered_at: unix_to_datetime(row.try_get(10).map_err(to_query_error)?)?,
        deactivated_at: deactivated_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_server_at(row: &PgRow, offset: usize) -> Result<ExternalMcpServerRecord, StoreError> {
    let transport: String = row.try_get(offset + 4).map_err(to_query_error)?;
    let auth_mode: String = row.try_get(offset + 6).map_err(to_query_error)?;
    let auth_config_json: String = row.try_get(offset + 7).map_err(to_query_error)?;
    let status: String = row.try_get(offset + 9).map_err(to_query_error)?;
    let last_discovery_status: Option<String> = row.try_get(offset + 10).map_err(to_query_error)?;
    let last_discovery_at: Option<i64> = row.try_get(offset + 11).map_err(to_query_error)?;
    let last_successful_discovery_at: Option<i64> =
        row.try_get(offset + 12).map_err(to_query_error)?;
    let disabled_at: Option<i64> = row.try_get(offset + 17).map_err(to_query_error)?;

    Ok(ExternalMcpServerRecord {
        mcp_server_id: parse_uuid(&row.try_get::<String, _>(offset).map_err(to_query_error)?)?,
        server_key: row.try_get(offset + 1).map_err(to_query_error)?,
        display_name: row.try_get(offset + 2).map_err(to_query_error)?,
        description: row.try_get(offset + 3).map_err(to_query_error)?,
        transport: ExternalMcpTransport::from_db(&transport).ok_or_else(|| {
            StoreError::Serialization(format!("invalid external MCP transport `{transport}`"))
        })?,
        server_url: row.try_get(offset + 5).map_err(to_query_error)?,
        auth_mode: ExternalMcpAuthMode::from_db(&auth_mode).ok_or_else(|| {
            StoreError::Serialization(format!("invalid external MCP auth mode `{auth_mode}`"))
        })?,
        auth_config: json_object_from_str(&auth_config_json)?,
        timeout_ms: row.try_get(offset + 8).map_err(to_query_error)?,
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
        last_error_summary: row.try_get(offset + 13).map_err(to_query_error)?,
        last_tool_count: row.try_get(offset + 14).map_err(to_query_error)?,
        created_at: unix_to_datetime(row.try_get(offset + 15).map_err(to_query_error)?)?,
        updated_at: unix_to_datetime(row.try_get(offset + 16).map_err(to_query_error)?)?,
        disabled_at: disabled_at.map(unix_to_datetime).transpose()?,
    })
}

fn decode_catalog_tool(row: &PgRow) -> Result<McpCatalogToolRecord, StoreError> {
    Ok(McpCatalogToolRecord {
        tool: decode_tool(row)?,
        server: decode_server_at(row, TOOL_COLUMN_COUNT)?,
    })
}

async fn find_toolset(
    pool: &PgPool,
    toolset_id: Uuid,
) -> Result<Option<McpToolsetRecord>, StoreError> {
    let sql = format!("SELECT {TOOLSET_COLUMNS} FROM mcp_toolsets WHERE toolset_id = $1");
    let row = sqlx::query(&sql)
        .bind(toolset_id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(to_query_error)?;
    row.as_ref().map(decode_toolset).transpose()
}

async fn toolset_tools_on(
    executor: impl sqlx::PgExecutor<'_>,
    toolset_id: Uuid,
) -> Result<Vec<McpToolsetToolRecord>, StoreError> {
    let rows = sqlx::query(
        "SELECT toolset_id, mcp_tool_id, created_at FROM mcp_toolset_tools WHERE toolset_id = $1 ORDER BY mcp_tool_id",
    )
    .bind(toolset_id.to_string())
    .fetch_all(executor)
    .await
    .map_err(to_query_error)?;
    rows.iter().map(decode_toolset_tool).collect()
}

async fn load_toolset(pool: &PgPool, toolset_id: Uuid) -> Result<McpToolsetRecord, StoreError> {
    find_toolset(pool, toolset_id)
        .await?
        .ok_or_else(|| StoreError::NotFound(format!("MCP toolset `{toolset_id}` was not found")))
}

#[async_trait]
impl McpAccessRepository for PostgresStore {
    async fn list_mcp_toolsets(
        &self,
        include_disabled: bool,
    ) -> Result<Vec<McpToolsetRecord>, StoreError> {
        let sql = format!(
            "SELECT {TOOLSET_COLUMNS} FROM mcp_toolsets WHERE ($1 = 1 OR status != 'disabled') ORDER BY toolset_key"
        );
        let rows = sqlx::query(&sql)
            .bind(if include_disabled { 1_i64 } else { 0_i64 })
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        rows.iter().map(decode_toolset).collect()
    }

    async fn get_mcp_toolset(
        &self,
        toolset_id: Uuid,
    ) -> Result<Option<McpToolsetRecord>, StoreError> {
        find_toolset(&self.pool, toolset_id).await
    }

    async fn create_mcp_toolset(
        &self,
        input: &NewMcpToolsetRecord,
    ) -> Result<McpToolsetRecord, StoreError> {
        let id = Uuid::new_v4();
        // `RETURNING` gives the row this insert wrote, before any later update.
        let row = sqlx::query(&format!(
            "INSERT INTO mcp_toolsets (
                toolset_id, toolset_key, display_name, description, status, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, 'active', $5, $5)
            RETURNING {TOOLSET_COLUMNS}"
        ))
        .bind(id.to_string())
        .bind(&input.toolset_key)
        .bind(&input.display_name)
        .bind(&input.description)
        .bind(input.created_at.unix_timestamp())
        .fetch_one(&self.pool)
        .await
        .map_err(to_write_error)?;
        decode_toolset(&row)
    }

    async fn update_mcp_toolset(
        &self,
        input: &UpdateMcpToolsetRecord,
    ) -> Result<Updated<McpToolsetRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        let sql =
            format!("SELECT {TOOLSET_COLUMNS} FROM mcp_toolsets WHERE toolset_id = $1 FOR UPDATE");
        let existing = sqlx::query(&sql)
            .bind(input.toolset_id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_query_error)?
            .as_ref()
            .map(decode_toolset)
            .transpose()?
            .filter(|toolset| toolset.status != McpToolsetStatus::Disabled)
            .ok_or_else(|| {
                StoreError::NotFound(format!(
                    "active MCP toolset `{}` was not found",
                    input.toolset_id
                ))
            })?;
        if existing.display_name == input.display_name && existing.description == input.description
        {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(Updated::unchanged(existing));
        }
        sqlx::query(
            r#"
            UPDATE mcp_toolsets
            SET display_name = $1, description = $2, updated_at = $3
            WHERE toolset_id = $4
            "#,
        )
        .bind(&input.display_name)
        .bind(&input.description)
        .bind(input.updated_at.unix_timestamp())
        .bind(input.toolset_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(to_write_error)?;
        // Read inside the transaction, so `current` is the row this update wrote.
        let current = sqlx::query(&format!(
            "SELECT {TOOLSET_COLUMNS} FROM mcp_toolsets WHERE toolset_id = $1"
        ))
        .bind(input.toolset_id.to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(to_query_error)
        .and_then(|row| decode_toolset(&row))?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(Updated {
            current,
            previous: Some(existing),
        })
    }

    async fn disable_mcp_toolset(
        &self,
        toolset_id: Uuid,
        disabled_at: OffsetDateTime,
    ) -> Result<McpToolsetRecord, StoreError> {
        let result = sqlx::query(
            "UPDATE mcp_toolsets SET status = 'disabled', updated_at = $1, disabled_at = $1 WHERE toolset_id = $2 AND status != 'disabled'",
        )
        .bind(disabled_at.unix_timestamp())
        .bind(toolset_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_write_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound(format!(
                "active MCP toolset `{toolset_id}` was not found"
            )));
        }
        load_toolset(&self.pool, toolset_id).await
    }

    async fn replace_mcp_toolset_tools(
        &self,
        toolset_id: Uuid,
        tool_ids: &[Uuid],
        updated_at: OffsetDateTime,
    ) -> Result<Updated<Vec<McpToolsetToolRecord>>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(to_query_error)?;
        // Locking the tool set row serializes replacements, so the tools read here
        // are the ones this replacement removes.
        sqlx::query("SELECT 1 FROM mcp_toolsets WHERE toolset_id = $1 FOR UPDATE")
            .bind(toolset_id.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| {
                StoreError::NotFound(format!("MCP toolset `{toolset_id}` was not found"))
            })?;
        let existing = toolset_tools_on(&mut *tx, toolset_id).await?;
        if same_tool_ids(&existing, tool_ids) {
            tx.commit().await.map_err(to_write_error)?;
            return Ok(Updated::unchanged(existing));
        }
        sqlx::query("DELETE FROM mcp_toolset_tools WHERE toolset_id = $1")
            .bind(toolset_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
        for tool_id in tool_ids {
            sqlx::query(
                "INSERT INTO mcp_toolset_tools (toolset_id, mcp_tool_id, created_at) VALUES ($1, $2, $3)",
            )
            .bind(toolset_id.to_string())
            .bind(tool_id.to_string())
            .bind(updated_at.unix_timestamp())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
        }
        sqlx::query("UPDATE mcp_toolsets SET updated_at = $1 WHERE toolset_id = $2")
            .bind(updated_at.unix_timestamp())
            .bind(toolset_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(to_write_error)?;
        // Read inside the transaction, so `current` is the membership this call wrote.
        let current = toolset_tools_on(&mut *tx, toolset_id).await?;
        tx.commit().await.map_err(to_write_error)?;
        Ok(Updated {
            current,
            previous: Some(existing),
        })
    }

    async fn list_mcp_toolset_tools(
        &self,
        toolset_id: Uuid,
    ) -> Result<Vec<McpToolsetToolRecord>, StoreError> {
        load_toolset(&self.pool, toolset_id).await?;
        toolset_tools_on(&self.pool, toolset_id).await
    }

    async fn upsert_mcp_tool_grant(
        &self,
        grant: &UpsertMcpToolGrantRecord,
    ) -> Result<(McpToolGrantRecord, bool), StoreError> {
        let id = Uuid::new_v4();
        // `RETURNING` gives the row this statement wrote, so `created` cannot be
        // confused by a concurrent revoke and re-grant.
        let row = sqlx::query(
            r#"
            INSERT INTO mcp_tool_grants (
                grant_id, subject_kind, subject_id, target_kind, target_id, is_active, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, 1, $6, $6)
            ON CONFLICT(subject_kind, subject_id, target_kind, target_id) WHERE is_active = 1
            DO UPDATE SET updated_at = excluded.updated_at, revoked_at = NULL
            RETURNING grant_id, subject_kind, subject_id, target_kind, target_id, is_active, created_at, updated_at, revoked_at
            "#,
        )
        .bind(id.to_string())
        .bind(grant.subject_kind.as_str())
        .bind(grant.subject_id.to_string())
        .bind(grant.target_kind.as_str())
        .bind(grant.target_id.to_string())
        .bind(grant.updated_at.unix_timestamp())
        .fetch_one(&self.pool)
        .await
        .map_err(to_write_error)?;
        let record = decode_grant(&row)?;
        // The insert keeps this call's id; an already-active grant keeps its own.
        let created = record.grant_id == id;
        Ok((record, created))
    }

    async fn revoke_mcp_tool_grant(
        &self,
        subject_kind: McpToolGrantSubjectKind,
        subject_id: Uuid,
        target_kind: McpToolGrantTargetKind,
        target_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<Option<Uuid>, StoreError> {
        // `RETURNING` names the row this update revoked, even if another request
        // revoked and re-created the grant in between.
        let row = sqlx::query(
            r#"
            UPDATE mcp_tool_grants
            SET is_active = 0, revoked_at = $1, updated_at = $1
            WHERE subject_kind = $2 AND subject_id = $3 AND target_kind = $4 AND target_id = $5 AND is_active = 1
            RETURNING grant_id
            "#,
        )
        .bind(revoked_at.unix_timestamp())
        .bind(subject_kind.as_str())
        .bind(subject_id.to_string())
        .bind(target_kind.as_str())
        .bind(target_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_write_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let grant_id: String = row.try_get("grant_id").map_err(to_query_error)?;
        Uuid::parse_str(&grant_id)
            .map(Some)
            .map_err(|error| StoreError::Unexpected(error.to_string()))
    }

    async fn list_mcp_tool_grants(
        &self,
        subject_kind: Option<McpToolGrantSubjectKind>,
        subject_id: Option<Uuid>,
    ) -> Result<Vec<McpToolGrantRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT grant_id, subject_kind, subject_id, target_kind, target_id, is_active, created_at, updated_at, revoked_at
            FROM mcp_tool_grants
            WHERE ($1 IS NULL OR subject_kind = $1) AND ($2 IS NULL OR subject_id = $2)
            ORDER BY subject_kind, subject_id, target_kind, target_id
            "#,
        )
        .bind(subject_kind.map(|value| value.as_str()))
        .bind(subject_id.map(|value| value.to_string()))
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter().map(decode_grant).collect()
    }

    async fn resolve_mcp_access_for_subjects(
        &self,
        subjects: &[McpGrantSubject],
        mcp_server_id: Option<Uuid>,
        projection: McpToolProjection,
    ) -> Result<McpAccessResolution, StoreError> {
        let (exposed_tool_count, has_rules) =
            active_tool_count(&self.pool, mcp_server_id, subjects).await?;
        let rules = if has_rules {
            load_tag_rule_set(&self.pool, subjects).await?
        } else {
            McpTagRuleSet::default()
        };
        let allowed_tools = self
            .allowed_tools_with_rules(subjects, mcp_server_id, projection, &rules)
            .await?;
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

    async fn has_mcp_server_grant(
        &self,
        subjects: &[McpGrantSubject],
        mcp_server_id: Uuid,
    ) -> Result<bool, StoreError> {
        // One query answers both questions. Rules can add or remove tools, so
        // a caller with any rule gets the full resolution instead.
        let (granted, has_rules) = server_grant_exists(&self.pool, subjects, mcp_server_id).await?;
        if !has_rules {
            return Ok(granted);
        }
        let rules = load_tag_rule_set(&self.pool, subjects).await?;
        let allowed_tools = self
            .allowed_tools_with_rules(
                subjects,
                Some(mcp_server_id),
                McpToolProjection::Access,
                &rules,
            )
            .await?;
        Ok(!allowed_tools.is_empty())
    }

    async fn resolve_mcp_catalog_access_for_subjects(
        &self,
        subjects: &[McpGrantSubject],
        server_key: Option<&str>,
    ) -> Result<McpCatalogAccessResolution, StoreError> {
        let (exposed_tool_count, has_rules) =
            active_catalog_tool_count(&self.pool, server_key, subjects).await?;
        let rules = if has_rules {
            load_tag_rule_set(&self.pool, subjects).await?
        } else {
            McpTagRuleSet::default()
        };
        let mut tools_by_id: HashMap<Uuid, McpCatalogToolRecord> = HashMap::new();

        for subject in subjects {
            collect_catalog_direct_tools(&self.pool, subject, server_key, &mut tools_by_id).await?;
            collect_catalog_toolset_tools(&self.pool, subject, server_key, &mut tools_by_id)
                .await?;
        }
        let include_servers = rules.include_server_ids(None);
        let matched =
            include_rule_matches(&self.pool, &include_servers, server_key, &rules).await?;
        let missing = matched
            .into_iter()
            .filter(|id| !tools_by_id.contains_key(id))
            .collect::<Vec<_>>();
        for record in catalog_tools_by_ids(&self.pool, &missing).await? {
            tools_by_id.insert(record.tool.mcp_tool_id, record);
        }
        tools_by_id.retain(|_, record| !rules.excludes(&record.tool));

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
            "SELECT {TOOL_COLUMNS} FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_server_id = $1 AND t.upstream_name = $2 AND t.is_active = 1 AND s.status = 'active'"
        );
        let row = sqlx::query(&sql)
            .bind(mcp_server_id.to_string())
            .bind(upstream_name)
            .fetch_optional(&self.pool)
            .await
            .map_err(to_query_error)?;
        row.as_ref().map(decode_tool).transpose()
    }

    async fn list_active_mcp_tools(
        &self,
        mcp_server_id: Option<Uuid>,
        after: Option<(Uuid, &str)>,
        limit: u32,
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        let (after_server, after_name) = after.unzip();
        sqlx::query(&format!(
            "SELECT {TOOL_COLUMNS} FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.is_active = 1 AND s.status = 'active' AND ($1::text IS NULL OR t.mcp_server_id = $1) AND ($2::text IS NULL OR (t.mcp_server_id, t.upstream_name) > ($2, $3::text)) ORDER BY t.mcp_server_id, t.upstream_name LIMIT $4"
        ))
        .bind(mcp_server_id.map(|id| id.to_string()))
        .bind(after_server.map(|id| id.to_string()))
        .bind(after_name)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?
        .iter()
        .map(decode_tool)
        .collect()
    }

    async fn list_mcp_tool_tag_rules(
        &self,
        subject_kind: Option<McpToolGrantSubjectKind>,
        subject_id: Option<Uuid>,
    ) -> Result<Vec<McpToolTagRuleRecord>, StoreError> {
        let rows = sqlx::query(&format!(
            "SELECT {TAG_RULE_COLUMNS} FROM mcp_tool_tag_rules WHERE ($1::text IS NULL OR subject_kind = $1) AND ($2::text IS NULL OR subject_id = $2) ORDER BY subject_kind, subject_id, server_scope, tag, effect, created_at"
        ))
        .bind(subject_kind.map(|value| value.as_str()))
        .bind(subject_id.map(|value| value.to_string()))
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;
        rows.iter().map(decode_tag_rule).collect()
    }

    async fn upsert_mcp_tool_tag_rule(
        &self,
        key: &McpToolTagRuleKey,
        updated_at: OffsetDateTime,
    ) -> Result<(McpToolTagRuleRecord, bool), StoreError> {
        let id = Uuid::new_v4();
        // `RETURNING` gives the row this statement wrote, so `created` cannot be
        // confused by a concurrent revoke and re-create.
        let row = sqlx::query(&format!(
            "INSERT INTO mcp_tool_tag_rules (
                rule_id, subject_kind, subject_id, server_scope, tag, effect, is_active, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $7)
            ON CONFLICT(subject_kind, subject_id, server_scope, tag, effect) WHERE is_active = 1
            DO UPDATE SET updated_at = excluded.updated_at
            RETURNING {TAG_RULE_COLUMNS}"
        ))
        .bind(id.to_string())
        .bind(key.subject_kind.as_str())
        .bind(key.subject_id.to_string())
        .bind(key.server_scope())
        .bind(key.tag.as_str())
        .bind(key.effect.as_str())
        .bind(updated_at.unix_timestamp())
        .fetch_one(&self.pool)
        .await
        .map_err(to_write_error)?;
        let record = decode_tag_rule(&row)?;
        let created = record.rule_id == id;
        Ok((record, created))
    }

    async fn get_mcp_tool_tag_rule(
        &self,
        rule_id: Uuid,
    ) -> Result<Option<McpToolTagRuleRecord>, StoreError> {
        sqlx::query(&format!(
            "SELECT {TAG_RULE_COLUMNS} FROM mcp_tool_tag_rules WHERE rule_id = $1"
        ))
        .bind(rule_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?
        .as_ref()
        .map(decode_tag_rule)
        .transpose()
    }

    async fn revoke_mcp_tool_tag_rule(
        &self,
        rule_id: Uuid,
        revoked_at: OffsetDateTime,
    ) -> Result<Option<McpToolTagRuleRecord>, StoreError> {
        sqlx::query(&format!(
            "UPDATE mcp_tool_tag_rules
             SET is_active = 0, revoked_at = $1, updated_at = $1
             WHERE rule_id = $2 AND is_active = 1
             RETURNING {TAG_RULE_COLUMNS}"
        ))
        .bind(revoked_at.unix_timestamp())
        .bind(rule_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_write_error)?
        .as_ref()
        .map(decode_tag_rule)
        .transpose()
    }
}

fn decode_tag_rule(row: &PgRow) -> Result<McpToolTagRuleRecord, StoreError> {
    TagRuleRow {
        rule_id: row.try_get(0).map_err(to_query_error)?,
        subject_kind: row.try_get(1).map_err(to_query_error)?,
        subject_id: row.try_get(2).map_err(to_query_error)?,
        server_scope: row.try_get(3).map_err(to_query_error)?,
        tag: row.try_get(4).map_err(to_query_error)?,
        effect: row.try_get(5).map_err(to_query_error)?,
        is_active: row.try_get(6).map_err(to_query_error)?,
        created_at: row.try_get(7).map_err(to_query_error)?,
        updated_at: row.try_get(8).map_err(to_query_error)?,
        revoked_at: row.try_get(9).map_err(to_query_error)?,
    }
    .decode()
}

impl PostgresStore {
    /// The subjects' allowed tools, with their tag rules already loaded.
    async fn allowed_tools_with_rules(
        &self,
        subjects: &[McpGrantSubject],
        mcp_server_id: Option<Uuid>,
        projection: McpToolProjection,
        rules: &McpTagRuleSet,
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        let columns = tool_columns(projection);
        let mut tools_by_id: HashMap<Uuid, ExternalMcpToolRecord> = HashMap::new();

        for subject in subjects {
            collect_direct_tools(
                &self.pool,
                subject,
                mcp_server_id,
                columns,
                &mut tools_by_id,
            )
            .await?;
            collect_toolset_tools(
                &self.pool,
                subject,
                mcp_server_id,
                columns,
                &mut tools_by_id,
            )
            .await?;
        }
        let include_servers = rules.include_server_ids(mcp_server_id);
        let matched = include_rule_matches(&self.pool, &include_servers, None, rules).await?;
        let missing = matched
            .into_iter()
            .filter(|id| !tools_by_id.contains_key(id))
            .collect::<Vec<_>>();
        for tool in tools_by_ids(&self.pool, &missing, columns).await? {
            tools_by_id.insert(tool.mcp_tool_id, tool);
        }
        tools_by_id.retain(|_, tool| !rules.excludes(tool));

        let mut allowed_tools: Vec<_> = tools_by_id.into_values().collect();
        allowed_tools.sort_by(|a, b| {
            a.mcp_server_id
                .cmp(&b.mcp_server_id)
                .then_with(|| a.upstream_name.cmp(&b.upstream_name))
        });
        Ok(allowed_tools)
    }
}

/// The active tag rules of every subject, in one query.
async fn load_tag_rule_set(
    pool: &PgPool,
    subjects: &[McpGrantSubject],
) -> Result<McpTagRuleSet, StoreError> {
    if subjects.is_empty() {
        return Ok(McpTagRuleSet::default());
    }
    let (kinds, ids) = subject_arrays(subjects);
    let rows = sqlx::query(&format!(
        "SELECT {TAG_RULE_COLUMNS} FROM mcp_tool_tag_rules WHERE is_active = 1 AND (subject_kind, subject_id) IN (SELECT * FROM UNNEST($1::text[], $2::text[]))"
    ))
    .bind(kinds)
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(to_query_error)?;
    let rules = rows
        .iter()
        .map(decode_tag_rule)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(McpTagRuleSet::new(rules))
}

/// Subject kinds and ids as parallel arrays for `UNNEST`.
fn subject_arrays(subjects: &[McpGrantSubject]) -> (Vec<String>, Vec<String>) {
    subjects
        .iter()
        .map(|subject| {
            (
                subject.subject_kind.as_str().to_string(),
                subject.subject_id.to_string(),
            )
        })
        .unzip()
}

/// Whether any subject in the `$2`/`$3` arrays has an active tag rule.
const RULES_EXIST_SQL: &str = "EXISTS (SELECT 1 FROM mcp_tool_tag_rules WHERE is_active = 1 AND (subject_kind, subject_id) IN (SELECT * FROM UNNEST($2::text[], $3::text[])))";

/// Active tools on the servers that an include rule selects. Only the
/// columns tag rules read are scanned; callers load full rows for the
/// matches alone.
async fn include_rule_matches(
    pool: &PgPool,
    server_ids: &BTreeSet<Uuid>,
    server_key: Option<&str>,
    rules: &McpTagRuleSet,
) -> Result<Vec<Uuid>, StoreError> {
    if server_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {RULE_TOOL_COLUMNS} FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_server_id = ANY($1) AND t.is_active = 1 AND s.status = 'active' AND ($2::text IS NULL OR s.server_key = $2)"
    );
    let rows = sqlx::query(&sql)
        .bind(server_ids.iter().map(Uuid::to_string).collect::<Vec<_>>())
        .bind(server_key)
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    let mut matched = Vec::new();
    for row in &rows {
        let tool = decode_tool(row)?;
        if rules.includes(&tool) {
            matched.push(tool.mcp_tool_id);
        }
    }
    Ok(matched)
}

/// Active tools by id, in one query.
async fn tools_by_ids(
    pool: &PgPool,
    tool_ids: &[Uuid],
    columns: &str,
) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
    if tool_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {columns} FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id = ANY($1) AND t.is_active = 1 AND s.status = 'active'"
    );
    let rows = sqlx::query(&sql)
        .bind(tool_ids.iter().map(Uuid::to_string).collect::<Vec<_>>())
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    rows.iter().map(decode_tool).collect()
}

/// Active catalog tools by id, with their servers, in one query.
async fn catalog_tools_by_ids(
    pool: &PgPool,
    tool_ids: &[Uuid],
) -> Result<Vec<McpCatalogToolRecord>, StoreError> {
    if tool_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id = ANY($1) AND t.is_active = 1 AND s.status = 'active'"
    );
    let rows = sqlx::query(&sql)
        .bind(tool_ids.iter().map(Uuid::to_string).collect::<Vec<_>>())
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    rows.iter().map(decode_catalog_tool).collect()
}

/// Active catalog tools, and whether any subject has a tag rule, so callers
/// without rules skip loading them.
async fn active_catalog_tool_count(
    pool: &PgPool,
    server_key: Option<&str>,
    subjects: &[McpGrantSubject],
) -> Result<(i64, bool), StoreError> {
    let (kinds, ids) = subject_arrays(subjects);
    let row = sqlx::query(&format!(
        "SELECT (SELECT COUNT(*) FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.is_active = 1 AND s.status = 'active' AND ($1::text IS NULL OR s.server_key = $1)), {RULES_EXIST_SQL}"
    ))
    .bind(server_key)
    .bind(kinds)
    .bind(ids)
    .fetch_one(pool)
    .await
    .map_err(to_query_error)?;
    Ok((
        row.try_get(0).map_err(to_query_error)?,
        row.try_get(1).map_err(to_query_error)?,
    ))
}

/// Active tools, and whether any subject has a tag rule, so callers without
/// rules skip loading them.
async fn active_tool_count(
    pool: &PgPool,
    mcp_server_id: Option<Uuid>,
    subjects: &[McpGrantSubject],
) -> Result<(i64, bool), StoreError> {
    let (kinds, ids) = subject_arrays(subjects);
    let row = sqlx::query(&format!(
        "SELECT (SELECT COUNT(*) FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.is_active = 1 AND s.status = 'active' AND ($1::text IS NULL OR t.mcp_server_id = $1)), {RULES_EXIST_SQL}"
    ))
    .bind(mcp_server_id.map(|value| value.to_string()))
    .bind(kinds)
    .bind(ids)
    .fetch_one(pool)
    .await
    .map_err(to_query_error)?;
    Ok((
        row.try_get(0).map_err(to_query_error)?,
        row.try_get(1).map_err(to_query_error)?,
    ))
}

/// Whether any subject holds a grant on the server, and whether any subject
/// has a tag rule, in one query.
async fn server_grant_exists(
    pool: &PgPool,
    subjects: &[McpGrantSubject],
    mcp_server_id: Uuid,
) -> Result<(bool, bool), StoreError> {
    let (kinds, ids) = subject_arrays(subjects);
    let row = sqlx::query(&format!(
        "SELECT EXISTS (SELECT 1 FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND (g.subject_kind, g.subject_id) IN (SELECT * FROM UNNEST($2::text[], $3::text[])) AND t.is_active = 1 AND s.status = 'active' AND t.mcp_server_id = $1) OR EXISTS (SELECT 1 FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND (g.subject_kind, g.subject_id) IN (SELECT * FROM UNNEST($2::text[], $3::text[])) AND ts.status = 'active' AND t.is_active = 1 AND s.status = 'active' AND t.mcp_server_id = $1), {RULES_EXIST_SQL}"
    ))
    .bind(mcp_server_id.to_string())
    .bind(kinds)
    .bind(ids)
    .fetch_one(pool)
    .await
    .map_err(to_query_error)?;
    Ok((
        row.try_get(0).map_err(to_query_error)?,
        row.try_get(1).map_err(to_query_error)?,
    ))
}

fn tool_columns(projection: McpToolProjection) -> &'static str {
    match projection {
        McpToolProjection::Access => ACCESS_TOOL_COLUMNS,
        McpToolProjection::Full => TOOL_COLUMNS,
    }
}

async fn collect_direct_tools(
    pool: &PgPool,
    subject: &McpGrantSubject,
    mcp_server_id: Option<Uuid>,
    columns: &str,
    out: &mut HashMap<Uuid, ExternalMcpToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {columns} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND g.subject_kind = $1 AND g.subject_id = $2 AND t.is_active = 1 AND s.status = 'active' AND ($3 IS NULL OR t.mcp_server_id = $3)"
    );
    let rows = sqlx::query(&sql)
        .bind(subject.subject_kind.as_str())
        .bind(subject.subject_id.to_string())
        .bind(mcp_server_id.map(|value| value.to_string()))
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    for row in rows {
        let tool = decode_tool(&row)?;
        out.insert(tool.mcp_tool_id, tool);
    }
    Ok(())
}

async fn collect_toolset_tools(
    pool: &PgPool,
    subject: &McpGrantSubject,
    mcp_server_id: Option<Uuid>,
    columns: &str,
    out: &mut HashMap<Uuid, ExternalMcpToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {columns} FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND g.subject_kind = $1 AND g.subject_id = $2 AND ts.status = 'active' AND t.is_active = 1 AND s.status = 'active' AND ($3 IS NULL OR t.mcp_server_id = $3)"
    );
    let rows = sqlx::query(&sql)
        .bind(subject.subject_kind.as_str())
        .bind(subject.subject_id.to_string())
        .bind(mcp_server_id.map(|value| value.to_string()))
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    for row in rows {
        let tool = decode_tool(&row)?;
        out.insert(tool.mcp_tool_id, tool);
    }
    Ok(())
}

async fn collect_catalog_direct_tools(
    pool: &PgPool,
    subject: &McpGrantSubject,
    server_key: Option<&str>,
    out: &mut HashMap<Uuid, McpCatalogToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND g.subject_kind = $1 AND g.subject_id = $2 AND t.is_active = 1 AND s.status = 'active' AND ($3 IS NULL OR s.server_key = $3)"
    );
    let rows = sqlx::query(&sql)
        .bind(subject.subject_kind.as_str())
        .bind(subject.subject_id.to_string())
        .bind(server_key)
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    for row in rows {
        let record = decode_catalog_tool(&row)?;
        out.insert(record.tool.mcp_tool_id, record);
    }
    Ok(())
}

async fn collect_catalog_toolset_tools(
    pool: &PgPool,
    subject: &McpGrantSubject,
    server_key: Option<&str>,
    out: &mut HashMap<Uuid, McpCatalogToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND g.subject_kind = $1 AND g.subject_id = $2 AND ts.status = 'active' AND t.is_active = 1 AND s.status = 'active' AND ($3 IS NULL OR s.server_key = $3)"
    );
    let rows = sqlx::query(&sql)
        .bind(subject.subject_kind.as_str())
        .bind(subject.subject_id.to_string())
        .bind(server_key)
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    for row in rows {
        let record = decode_catalog_tool(&row)?;
        out.insert(record.tool.mcp_tool_id, record);
    }
    Ok(())
}

/// Whether `requested` names exactly the tools already in the set, ignoring order
/// and repeats.
fn same_tool_ids(existing: &[McpToolsetToolRecord], requested: &[Uuid]) -> bool {
    let existing: std::collections::BTreeSet<Uuid> =
        existing.iter().map(|tool| tool.mcp_tool_id).collect();
    let requested: std::collections::BTreeSet<Uuid> = requested.iter().copied().collect();
    existing == requested
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{create_postgres_test_database, drop_postgres_test_database};
    use crate::{StoreConnectionOptions, run_migrations_with_options};

    #[tokio::test]
    #[serial_test::serial]
    async fn postgres_toolset_membership_distinguishes_missing_and_empty_sets() {
        let Some(test_db) = create_postgres_test_database().await else {
            eprintln!("skipping PostgreSQL membership test: TEST_POSTGRES_URL is not set");
            return;
        };
        run_migrations_with_options(&StoreConnectionOptions::Postgres {
            url: test_db.database_url.clone(),
            max_connections: 2,
        })
        .await
        .expect("migrations");
        let store = PostgresStore::connect(&test_db.database_url, 2)
            .await
            .expect("store");
        let now = OffsetDateTime::now_utc();
        let toolset = store
            .create_mcp_toolset(&NewMcpToolsetRecord {
                toolset_key: "empty".into(),
                display_name: "Empty".into(),
                description: None,
                created_at: now,
            })
            .await
            .expect("create set");
        assert!(
            store
                .list_mcp_toolset_tools(toolset.toolset_id)
                .await
                .expect("empty set")
                .is_empty()
        );
        store
            .disable_mcp_toolset(toolset.toolset_id, now)
            .await
            .expect("disable set");
        assert!(
            store
                .list_mcp_toolset_tools(toolset.toolset_id)
                .await
                .expect("disabled set")
                .is_empty()
        );
        assert!(matches!(
            store.list_mcp_toolset_tools(Uuid::new_v4()).await,
            Err(StoreError::NotFound(_))
        ));
        drop(store);
        drop_postgres_test_database(&test_db).await;
    }
}
