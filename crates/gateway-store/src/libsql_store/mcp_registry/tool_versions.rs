//! Tool version persistence: the discovery snapshot, version rows, and the
//! SQL for each shared `VersionOp`.
use super::*;

pub(super) const VERSION_COLUMNS: &str = "mcp_tool_id, schema_version, schema_hash, description, input_schema_json, title, annotations_json, output_schema_json, status, discovered_at, reviewed_at, reviewed_by_user_id";

pub(super) fn decode_version(row: &libsql::Row) -> Result<McpToolVersionRecord, StoreError> {
    let mcp_tool_id: String = row.get(0).map_err(to_query_error)?;
    let input_schema_json: String = row.get(4).map_err(to_query_error)?;
    let status: String = row.get(8).map_err(to_query_error)?;
    let discovered_at: i64 = row.get(9).map_err(to_query_error)?;
    let reviewed_at: Option<i64> = row.get(10).map_err(to_query_error)?;
    let reviewed_by: Option<String> = row.get(11).map_err(to_query_error)?;
    let metadata = decode_tool_metadata(row, 5)?;
    Ok(McpToolVersionRecord {
        mcp_tool_id: parse_uuid(&mcp_tool_id)?,
        schema_version: row.get(1).map_err(to_query_error)?,
        definition: McpToolDefinition {
            description: row.get(3).map_err(to_query_error)?,
            input_schema: serde_json::from_str(&input_schema_json)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
            title: metadata.title,
            annotations: metadata.annotations,
            output_schema: metadata.output_schema,
            schema_hash: row.get(2).map_err(to_query_error)?,
        },
        status: McpToolVersionStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid MCP tool version status `{status}`"))
        })?,
        discovered_at: unix_to_datetime(discovered_at)?,
        reviewed_at: reviewed_at.map(unix_to_datetime).transpose()?,
        reviewed_by_user_id: reviewed_by.as_deref().map(parse_uuid).transpose()?,
    })
}

pub(super) async fn find_version(
    connection: &libsql::Connection,
    mcp_tool_id: Uuid,
    schema_version: i64,
) -> Result<Option<McpToolVersionRecord>, StoreError> {
    let mut rows = connection
        .query(
            &format!(
                "SELECT {VERSION_COLUMNS} FROM mcp_tool_versions WHERE mcp_tool_id = ?1 AND schema_version = ?2"
            ),
            libsql::params![mcp_tool_id.to_string(), schema_version],
        )
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .map(|row| decode_version(&row))
        .transpose()
}

/// The server's tools with their versions, by upstream name.
pub(super) async fn load_stored_tools(
    connection: &libsql::Connection,
    mcp_server_id: Uuid,
) -> Result<HashMap<String, StoredTool>, StoreError> {
    let mut rows = connection
        .query(
            &format!(
                "SELECT {TOOL_COLUMNS}, metadata_captured, (SELECT MAX(v.schema_version) FROM mcp_tool_versions v WHERE v.mcp_tool_id = external_mcp_tools.mcp_tool_id) FROM external_mcp_tools WHERE mcp_server_id = ?1"
            ),
            [mcp_server_id.to_string()],
        )
        .await
        .map_err(to_query_error)?;
    let mut by_id = HashMap::new();
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        let tool = decode_tool(&row)?;
        let metadata_captured: i64 = row.get(20).map_err(to_query_error)?;
        // The newest version number of any status, read per tool through the
        // `(mcp_tool_id, schema_version)` key rather than the whole history.
        let latest_version: Option<i64> = row.get(21).map_err(to_query_error)?;
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
    drop(rows);
    let mut rows = connection
        .query(
            "SELECT v.mcp_tool_id, v.schema_version, v.schema_hash, v.status, v.description, v.title FROM mcp_tool_versions v JOIN external_mcp_tools t ON t.mcp_tool_id = v.mcp_tool_id WHERE t.mcp_server_id = ?1 AND v.status IN ('pending', 'rejected')",
            [mcp_server_id.to_string()],
        )
        .await
        .map_err(to_query_error)?;
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        let mcp_tool_id: String = row.get(0).map_err(to_query_error)?;
        let status: String = row.get(3).map_err(to_query_error)?;
        if let Some(stored) = by_id.get_mut(&parse_uuid(&mcp_tool_id)?) {
            stored.versions.push(StoredVersion {
                schema_version: row.get(1).map_err(to_query_error)?,
                schema_hash: row.get(2).map_err(to_query_error)?,
                description: row.get(4).map_err(to_query_error)?,
                title: row.get(5).map_err(to_query_error)?,
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
    connection: &libsql::Connection,
    mcp_tool_id: Uuid,
    definition: &McpToolDefinition,
    schema_version: i64,
) -> Result<(), StoreError> {
    let (input_schema_json, annotations_json, output_schema_json) = definition_json(definition)?;
    connection
        .execute(
            r#"
            UPDATE external_mcp_tools
            SET description = ?1, input_schema_json = ?2, title = ?3, annotations_json = ?4,
                output_schema_json = ?5, schema_hash = ?6, schema_version = ?7, metadata_captured = 1
            WHERE mcp_tool_id = ?8
            "#,
            libsql::params![
                definition.description.as_deref(),
                input_schema_json,
                definition.title.as_deref(),
                annotations_json,
                output_schema_json,
                definition.schema_hash.as_str(),
                schema_version,
                mcp_tool_id.to_string(),
            ],
        )
        .await
        .map_err(to_write_error)?;
    Ok(())
}

pub(super) async fn insert_version(
    connection: &libsql::Connection,
    mcp_tool_id: Uuid,
    schema_version: i64,
    definition: &McpToolDefinition,
    status: McpToolVersionStatus,
    discovered_at: OffsetDateTime,
) -> Result<(), StoreError> {
    let (input_schema_json, annotations_json, output_schema_json) = definition_json(definition)?;
    connection
        .execute(
            r#"
            INSERT INTO mcp_tool_versions (
                mcp_tool_id, schema_version, schema_hash, description, input_schema_json, title,
                annotations_json, output_schema_json, status, discovered_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            libsql::params![
                mcp_tool_id.to_string(),
                schema_version,
                definition.schema_hash.as_str(),
                definition.description.as_deref(),
                input_schema_json,
                definition.title.as_deref(),
                annotations_json,
                output_schema_json,
                status.as_str(),
                discovered_at.unix_timestamp(),
            ],
        )
        .await
        .map_err(to_write_error)?;
    Ok(())
}

pub(super) async fn set_version_status(
    connection: &libsql::Connection,
    mcp_tool_id: Uuid,
    schema_version: i64,
    status: McpToolVersionStatus,
    reviewed_at: Option<i64>,
    reviewed_by_user_id: Option<&str>,
) -> Result<(), StoreError> {
    connection
        .execute(
            "UPDATE mcp_tool_versions SET status = ?1, reviewed_at = ?2, reviewed_by_user_id = ?3 WHERE mcp_tool_id = ?4 AND schema_version = ?5",
            libsql::params![
                status.as_str(),
                reviewed_at,
                reviewed_by_user_id,
                mcp_tool_id.to_string(),
                schema_version,
            ],
        )
        .await
        .map_err(to_write_error)?;
    Ok(())
}

/// Approves a version automatically, because the server does not require
/// review. A version an admin already approved keeps its reviewer; one an
/// admin rejected loses the rejection's attribution.
pub(super) async fn auto_approve_version(
    connection: &libsql::Connection,
    mcp_tool_id: Uuid,
    schema_version: i64,
) -> Result<(), StoreError> {
    connection
        .execute(
            "UPDATE mcp_tool_versions SET status = 'approved', reviewed_at = NULL, reviewed_by_user_id = NULL WHERE mcp_tool_id = ?1 AND schema_version = ?2 AND status <> 'approved'",
            libsql::params![mcp_tool_id.to_string(), schema_version],
        )
        .await
        .map_err(to_write_error)?;
    Ok(())
}

/// Lookups per historical version query: 500 lookups bind 2,000 variables,
/// below SQLite's limit of 32,766.
pub(super) const HISTORICAL_LOOKUP_BATCH: usize = 500;

/// The newest stored version with each looked-up definition (schema hash,
/// description and title), by tool, in a few batched queries, for discovery
/// to reuse instead of creating duplicates.
pub(super) async fn find_versions_by_definition(
    connection: &libsql::Connection,
    lookups: &[HistoricalLookup],
) -> Result<HashMap<Uuid, HistoricalVersion>, StoreError> {
    let mut by_tool: HashMap<Uuid, HistoricalVersion> = HashMap::new();
    for batch in lookups.chunks(HISTORICAL_LOOKUP_BATCH) {
        let matches = (0..batch.len())
            .map(|index| {
                let at = index * 4 + 1;
                format!(
                    "(mcp_tool_id = ?{at} AND schema_hash = ?{} AND description IS ?{} AND title IS ?{})",
                    at + 1,
                    at + 2,
                    at + 3
                )
            })
            .collect::<Vec<_>>()
            .join(" OR ");
        let text = |value: Option<&String>| {
            value.map_or(libsql::Value::Null, |value| {
                libsql::Value::Text(value.clone())
            })
        };
        let params = batch
            .iter()
            .flat_map(|lookup| {
                [
                    libsql::Value::Text(lookup.mcp_tool_id.to_string()),
                    libsql::Value::Text(lookup.key.schema_hash.clone()),
                    text(lookup.key.description.as_ref()),
                    text(lookup.key.title.as_ref()),
                ]
            })
            .collect::<Vec<_>>();
        let mut rows = connection
            .query(
                &format!(
                    "SELECT mcp_tool_id, schema_version, status, reviewed_by_user_id IS NOT NULL FROM mcp_tool_versions WHERE {matches}"
                ),
                libsql::params_from_iter(params),
            )
            .await
            .map_err(to_query_error)?;
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            let mcp_tool_id: String = row.get(0).map_err(to_query_error)?;
            let status: String = row.get(2).map_err(to_query_error)?;
            let reviewed: i64 = row.get(3).map_err(to_query_error)?;
            keep_newest(
                &mut by_tool,
                parse_uuid(&mcp_tool_id)?,
                HistoricalVersion {
                    schema_version: row.get(1).map_err(to_query_error)?,
                    status: McpToolVersionStatus::from_db(&status).ok_or_else(|| {
                        StoreError::Serialization(format!(
                            "invalid MCP tool version status `{status}`"
                        ))
                    })?,
                    reviewed: reviewed != 0,
                },
            );
        }
    }
    Ok(by_tool)
}

/// Marks every pending version other than `keep` as superseded.
pub(super) async fn supersede_pending(
    connection: &libsql::Connection,
    mcp_tool_id: Uuid,
    keep: Option<i64>,
) -> Result<(), StoreError> {
    connection
        .execute(
            "UPDATE mcp_tool_versions SET status = 'superseded' WHERE mcp_tool_id = ?1 AND status = 'pending' AND (?2 IS NULL OR schema_version != ?2)",
            libsql::params![mcp_tool_id.to_string(), keep],
        )
        .await
        .map_err(to_write_error)?;
    Ok(())
}

/// Runs one shared version-write statement for a tool.
pub(super) async fn apply_version_op(
    connection: &libsql::Connection,
    mcp_tool_id: Uuid,
    op: &VersionOp<'_>,
) -> Result<(), StoreError> {
    match op {
        VersionOp::WriteDefinition {
            definition,
            version,
        } => write_tool_definition(connection, mcp_tool_id, definition, *version).await,
        VersionOp::RewriteVersion {
            definition,
            version,
        } => {
            let (input_schema_json, annotations_json, output_schema_json) =
                definition_json(definition)?;
            connection
                .execute(
                    "UPDATE mcp_tool_versions SET schema_hash = ?1, description = ?2, input_schema_json = ?3, title = ?4, annotations_json = ?5, output_schema_json = ?6 WHERE mcp_tool_id = ?7 AND schema_version = ?8",
                    libsql::params![
                        definition.schema_hash.as_str(),
                        definition.description.as_deref(),
                        input_schema_json,
                        definition.title.as_deref(),
                        annotations_json,
                        output_schema_json,
                        mcp_tool_id.to_string(),
                        *version,
                    ],
                )
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
                connection,
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
                connection,
                mcp_tool_id,
                *version,
                *status,
                reviewed_at.map(OffsetDateTime::unix_timestamp),
                reviewed_by_user_id.map(|id| id.to_string()).as_deref(),
            )
            .await
        }
        VersionOp::AutoApproveVersion { version } => {
            auto_approve_version(connection, mcp_tool_id, *version).await
        }
        VersionOp::SupersedePending { keep } => {
            supersede_pending(connection, mcp_tool_id, *keep).await
        }
        VersionOp::SetToolReview {
            status,
            approved_version,
            pending_version,
        } => {
            connection
                .execute(
                    "UPDATE external_mcp_tools SET approval_status = ?1, approved_schema_version = COALESCE(?2, approved_schema_version), pending_schema_version = ?3 WHERE mcp_tool_id = ?4",
                    libsql::params![
                        status.as_str(),
                        *approved_version,
                        *pending_version,
                        mcp_tool_id.to_string(),
                    ],
                )
                .await
                .map_err(to_write_error)?;
            Ok(())
        }
    }
}

/// Runs what discovery planned for one tool.
pub(super) async fn apply_tool_write(
    connection: &libsql::Connection,
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
        connection
            .execute(
                r#"
                INSERT INTO external_mcp_tools (
                    mcp_tool_id, mcp_server_id, upstream_name, display_name, description,
                    input_schema_json, schema_hash, schema_version, is_active,
                    first_discovered_at, last_discovered_at, deactivated_at,
                    title, annotations_json, output_schema_json, metadata_captured,
                    approval_status, approved_schema_version, pending_schema_version
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 1, ?8, ?8, NULL, ?9, ?10, ?11, 1, ?12, ?13, ?14)
                "#,
                libsql::params![
                    id.to_string(),
                    tool.mcp_server_id.to_string(),
                    tool.upstream_name.as_str(),
                    tool.display_name.as_str(),
                    tool.description.as_deref(),
                    input_schema_json,
                    tool.schema_hash.as_str(),
                    at,
                    tool.metadata.title.as_deref(),
                    annotations_json,
                    output_schema_json,
                    status.as_str(),
                    approved.then_some(1_i64),
                    (!approved).then_some(1_i64),
                ],
            )
            .await
            .map_err(to_write_error)?;
        let version_status = if approved {
            McpToolVersionStatus::Approved
        } else {
            McpToolVersionStatus::Pending
        };
        return insert_version(
            connection,
            id,
            1,
            &definition,
            version_status,
            discovered_at,
        )
        .await;
    };
    let id = existing.tool.mcp_tool_id;
    // Every discovered tool is active again and keeps its latest display name.
    connection
        .execute(
            "UPDATE external_mcp_tools SET display_name = ?1, is_active = 1, last_discovered_at = ?2, deactivated_at = NULL WHERE mcp_tool_id = ?3",
            libsql::params![tool.display_name.as_str(), at, id.to_string()],
        )
        .await
        .map_err(to_write_error)?;
    if matches!(write, ToolWrite::Insert { .. }) {
        return Err(StoreError::Unexpected(
            "an existing MCP tool cannot be inserted".to_string(),
        ));
    }
    for op in tool_write_ops(write, &definition, discovered_at) {
        apply_version_op(connection, id, &op).await?;
    }
    Ok(())
}
