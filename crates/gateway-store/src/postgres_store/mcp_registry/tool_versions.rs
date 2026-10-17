//! Tool version persistence: the discovery snapshot, version rows, and the
//! SQL for each shared `VersionOp`.
use super::*;

pub(super) const VERSION_COLUMNS: &str = "mcp_tool_id, schema_version, schema_hash, description, input_schema_json::text, title, annotations_json::text, output_schema_json::text, status, discovered_at, reviewed_at, reviewed_by_user_id";

pub(super) fn decode_version(row: &PgRow) -> Result<McpToolVersionRecord, StoreError> {
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
pub(super) async fn load_stored_tools(
    conn: &mut sqlx::PgConnection,
    mcp_server_id: Uuid,
) -> Result<HashMap<String, StoredTool>, StoreError> {
    let rows = sqlx::query(&format!(
        "SELECT {TOOL_COLUMNS}, metadata_captured, (SELECT MAX(v.schema_version) FROM mcp_tool_versions v WHERE v.mcp_tool_id = external_mcp_tools.mcp_tool_id) FROM external_mcp_tools WHERE mcp_server_id = $1 FOR UPDATE"
    ))
    .bind(mcp_server_id.to_string())
    .fetch_all(&mut *conn)
    .await
    .map_err(to_query_error)?;
    let mut by_id = HashMap::new();
    for row in &rows {
        let tool = decode_tool(row)?;
        let metadata_captured: i64 = row.try_get(20).map_err(to_query_error)?;
        // The newest version number of any status, read per tool through the
        // `(mcp_tool_id, schema_version)` key rather than the whole history.
        let latest_version: Option<i64> = row.try_get(21).map_err(to_query_error)?;
        by_id.insert(
            tool.mcp_tool_id,
            StoredTool {
                tool,
                metadata_captured: metadata_captured == 1,
                versions: Vec::new(),
                latest_version: latest_version.unwrap_or(0),
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
    Ok(by_id
        .into_values()
        .map(|stored| (stored.tool.upstream_name.clone(), stored))
        .collect())
}

/// Serialized definition columns: input schema, annotations, output schema.
pub(super) fn definition_json(
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
pub(super) async fn write_tool_definition(
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

pub(super) async fn insert_version(
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

pub(super) async fn set_version_status(
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
pub(super) async fn auto_approve_version(
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

/// The newest stored version with each looked-up definition (schema hash,
/// description and title), by tool, in one query, for discovery to reuse
/// instead of creating duplicates.
pub(super) async fn find_versions_by_definition(
    conn: &mut sqlx::PgConnection,
    lookups: &[HistoricalLookup],
) -> Result<HashMap<Uuid, HistoricalVersion>, StoreError> {
    let mut by_tool: HashMap<Uuid, HistoricalVersion> = HashMap::new();
    if lookups.is_empty() {
        return Ok(by_tool);
    }
    let tool_ids = lookups
        .iter()
        .map(|lookup| lookup.mcp_tool_id.to_string())
        .collect::<Vec<_>>();
    let hashes = lookups
        .iter()
        .map(|lookup| lookup.key.schema_hash.clone())
        .collect::<Vec<_>>();
    let descriptions = lookups
        .iter()
        .map(|lookup| lookup.key.description.clone())
        .collect::<Vec<_>>();
    let titles = lookups
        .iter()
        .map(|lookup| lookup.key.title.clone())
        .collect::<Vec<_>>();
    let rows = sqlx::query(
        "SELECT v.mcp_tool_id, v.schema_version, v.status, v.reviewed_by_user_id IS NOT NULL FROM mcp_tool_versions v JOIN UNNEST($1::text[], $2::text[], $3::text[], $4::text[]) AS l(tool_id, schema_hash, description, title) ON v.mcp_tool_id = l.tool_id AND v.schema_hash = l.schema_hash AND v.description IS NOT DISTINCT FROM l.description AND v.title IS NOT DISTINCT FROM l.title",
    )
    .bind(tool_ids)
    .bind(hashes)
    .bind(descriptions)
    .bind(titles)
    .fetch_all(&mut *conn)
    .await
    .map_err(to_query_error)?;
    for row in &rows {
        let mcp_tool_id: String = row.try_get(0).map_err(to_query_error)?;
        let status: String = row.try_get(2).map_err(to_query_error)?;
        keep_newest(
            &mut by_tool,
            parse_uuid(&mcp_tool_id)?,
            HistoricalVersion {
                schema_version: row.try_get(1).map_err(to_query_error)?,
                status: McpToolVersionStatus::from_db(&status).ok_or_else(|| {
                    StoreError::Serialization(format!("invalid MCP tool version status `{status}`"))
                })?,
                reviewed: row.try_get(3).map_err(to_query_error)?,
            },
        );
    }
    Ok(by_tool)
}

/// Marks every pending version other than `keep` as superseded.
pub(super) async fn supersede_pending(
    conn: &mut sqlx::PgConnection,
    mcp_tool_id: Uuid,
    keep: Option<i64>,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE mcp_tool_versions SET status = 'superseded' WHERE mcp_tool_id = $1 AND status = 'pending' AND ($2::bigint IS NULL OR schema_version != $2)",
    )
    .bind(mcp_tool_id.to_string())
    .bind(keep)
    .execute(&mut *conn)
    .await
    .map_err(to_write_error)?;
    Ok(())
}

/// Runs one shared version-write statement for a tool.
pub(super) async fn apply_version_op(
    conn: &mut sqlx::PgConnection,
    mcp_tool_id: Uuid,
    op: &VersionOp<'_>,
) -> Result<(), StoreError> {
    match op {
        VersionOp::WriteDefinition {
            definition,
            version,
        } => write_tool_definition(conn, mcp_tool_id, definition, *version).await,
        VersionOp::RewriteVersion {
            definition,
            version,
        } => {
            let (input_schema_json, annotations_json, output_schema_json) =
                definition_json(definition)?;
            sqlx::query(
                "UPDATE mcp_tool_versions SET schema_hash = $1, description = $2, input_schema_json = $3::jsonb, title = $4, annotations_json = $5::jsonb, output_schema_json = $6::jsonb WHERE mcp_tool_id = $7 AND schema_version = $8",
            )
            .bind(definition.schema_hash.as_str())
            .bind(definition.description.as_deref())
            .bind(input_schema_json)
            .bind(definition.title.as_deref())
            .bind(annotations_json)
            .bind(output_schema_json)
            .bind(mcp_tool_id.to_string())
            .bind(*version)
            .execute(&mut *conn)
            .await
            .map_err(to_write_error)?;
            Ok(())
        }
        VersionOp::InsertVersion {
            definition,
            version,
            status,
            discovered_at,
        } => {
            insert_version(
                conn,
                mcp_tool_id,
                *version,
                definition,
                *status,
                *discovered_at,
            )
            .await
        }
        VersionOp::SetVersionStatus {
            version,
            status,
            reviewed_at,
            reviewed_by_user_id,
        } => {
            set_version_status(
                conn,
                mcp_tool_id,
                *version,
                *status,
                reviewed_at.map(OffsetDateTime::unix_timestamp),
                reviewed_by_user_id.map(|id| id.to_string()).as_deref(),
            )
            .await
        }
        VersionOp::AutoApproveVersion { version } => {
            auto_approve_version(conn, mcp_tool_id, *version).await
        }
        VersionOp::SupersedePending { keep } => supersede_pending(conn, mcp_tool_id, *keep).await,
        VersionOp::SetToolReview {
            status,
            approved_version,
            pending_version,
        } => {
            sqlx::query(
                "UPDATE external_mcp_tools SET approval_status = $1, approved_schema_version = COALESCE($2::bigint, approved_schema_version), pending_schema_version = $3::bigint WHERE mcp_tool_id = $4",
            )
            .bind(status.as_str())
            .bind(*approved_version)
            .bind(*pending_version)
            .bind(mcp_tool_id.to_string())
            .execute(&mut *conn)
            .await
            .map_err(to_write_error)?;
            Ok(())
        }
    }
}

/// Runs what discovery planned for one tool.
pub(super) async fn apply_tool_write(
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
    if matches!(write, ToolWrite::Insert { .. }) {
        return Err(StoreError::Unexpected(
            "an existing MCP tool cannot be inserted".to_string(),
        ));
    }
    for op in tool_write_ops(write, &definition, discovered_at) {
        apply_version_op(conn, id, &op).await?;
    }
    Ok(())
}
