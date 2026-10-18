//! Access resolution: which tools a caller's grants and tag rules allow, and
//! the queries it runs, including the tag-rule checks folded into them.
use super::*;

pub(super) fn decode_tag_rule(row: &PgRow) -> Result<McpToolTagRuleRecord, StoreError> {
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
    pub(super) async fn allowed_tools_with_rules(
        &self,
        subjects: &[McpGrantSubject],
        mcp_server_id: Option<Uuid>,
        projection: McpToolProjection,
        rules: &McpTagRuleSet,
        upstream_name: Option<&str>,
    ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
        let columns = tool_columns(projection);
        let mut tools_by_id: HashMap<Uuid, ExternalMcpToolRecord> = HashMap::new();

        for subject in subjects {
            collect_direct_tools(
                &self.pool,
                subject,
                mcp_server_id,
                columns,
                upstream_name,
                &mut tools_by_id,
            )
            .await?;
            collect_toolset_tools(
                &self.pool,
                subject,
                mcp_server_id,
                columns,
                upstream_name,
                &mut tools_by_id,
            )
            .await?;
        }
        let selectors = rules.include_selectors(mcp_server_id);
        let matched = include_rule_matches(&self.pool, &selectors, None, upstream_name).await?;
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
pub(super) async fn load_tag_rule_set(
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
pub(super) fn subject_arrays(subjects: &[McpGrantSubject]) -> (Vec<String>, Vec<String>) {
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
pub(super) const RULES_EXIST_SQL: &str = "EXISTS (SELECT 1 FROM mcp_tool_tag_rules WHERE is_active = 1 AND (subject_kind, subject_id) IN (SELECT * FROM UNNEST($2::text[], $3::text[])))";

/// `gateway_core::mcp_tool_policy::include_rule_selects` in SQL: a derived tag
/// selects a tool only when an admin overrode the tag's own hint to `true`, or
/// approved the served version on a server with tool approval.
const TRUSTED_INCLUDE_TAG_SQL: &str = "((s.approve_tool_changes = 1 AND EXISTS (SELECT 1 FROM mcp_tool_versions v WHERE v.mcp_tool_id = t.mcp_tool_id AND v.schema_version = t.approved_schema_version AND v.reviewed_at IS NOT NULL)) OR et.tag NOT IN ('read_only', 'destructive', 'idempotent', 'open_world') OR t.annotation_overrides_json -> (CASE et.tag WHEN 'read_only' THEN 'readOnlyHint' WHEN 'destructive' THEN 'destructiveHint' WHEN 'idempotent' THEN 'idempotentHint' WHEN 'open_world' THEN 'openWorldHint' END) = 'true'::jsonb)";

/// Active, served tools that include rules select, found through the
/// effective tag index: only matching tool ids are read.
pub(super) async fn include_rule_matches(
    pool: &PgPool,
    selectors: &BTreeSet<(Uuid, &str)>,
    server_key: Option<&str>,
    upstream_name: Option<&str>,
) -> Result<Vec<Uuid>, StoreError> {
    if selectors.is_empty() {
        return Ok(Vec::new());
    }
    let (server_ids, tags): (Vec<String>, Vec<String>) = selectors
        .iter()
        .map(|(server_id, tag)| (server_id.to_string(), (*tag).to_string()))
        .unzip();
    let sql = format!(
        "SELECT DISTINCT t.mcp_tool_id FROM mcp_tool_effective_tags et JOIN external_mcp_tools t ON t.mcp_tool_id = et.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE (t.mcp_server_id, et.tag) IN (SELECT * FROM UNNEST($1::text[], $2::text[])) AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND ($3::text IS NULL OR s.server_key = $3) AND ($4::text IS NULL OR t.upstream_name = $4) AND {TRUSTED_INCLUDE_TAG_SQL}"
    );
    let rows = sqlx::query(&sql)
        .bind(server_ids)
        .bind(tags)
        .bind(server_key)
        .bind(upstream_name)
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    rows.iter()
        .map(|row| {
            let mcp_tool_id: String = row.try_get(0).map_err(to_query_error)?;
            parse_uuid(&mcp_tool_id)
        })
        .collect()
}

/// Active tools by id, in one query.
pub(super) async fn tools_by_ids(
    pool: &PgPool,
    tool_ids: &[Uuid],
    columns: &str,
) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
    if tool_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {columns} FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id = ANY($1) AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active'"
    );
    let rows = sqlx::query(&sql)
        .bind(tool_ids.iter().map(Uuid::to_string).collect::<Vec<_>>())
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    rows.iter().map(decode_tool).collect()
}

/// Active catalog tools by id, with their servers, in one query.
pub(super) async fn catalog_tools_by_ids(
    pool: &PgPool,
    tool_ids: &[Uuid],
) -> Result<Vec<McpCatalogToolRecord>, StoreError> {
    if tool_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id = ANY($1) AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active'"
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
pub(super) async fn active_catalog_tool_count(
    pool: &PgPool,
    server_key: Option<&str>,
    subjects: &[McpGrantSubject],
) -> Result<(i64, bool), StoreError> {
    let (kinds, ids) = subject_arrays(subjects);
    let row = sqlx::query(&format!(
        "SELECT (SELECT COUNT(*) FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND ($1::text IS NULL OR s.server_key = $1)), {RULES_EXIST_SQL}"
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
pub(super) async fn active_tool_count(
    pool: &PgPool,
    mcp_server_id: Option<Uuid>,
    subjects: &[McpGrantSubject],
) -> Result<(i64, bool), StoreError> {
    let (kinds, ids) = subject_arrays(subjects);
    let row = sqlx::query(&format!(
        "SELECT (SELECT COUNT(*) FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND ($1::text IS NULL OR t.mcp_server_id = $1)), {RULES_EXIST_SQL}"
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

/// Whether any subject has an active tag rule, without counting tools.
pub(super) async fn rules_exist(
    pool: &PgPool,
    subjects: &[McpGrantSubject],
) -> Result<bool, StoreError> {
    let (kinds, ids) = subject_arrays(subjects);
    // `$1` is unused, so the shared rule check keeps its parameter numbers.
    sqlx::query_scalar(&format!(
        "SELECT {RULES_EXIST_SQL} FROM (SELECT $1::text) unused"
    ))
    .bind(Option::<String>::None)
    .bind(kinds)
    .bind(ids)
    .fetch_one(pool)
    .await
    .map_err(to_query_error)
}

/// Whether any subject holds a grant on the server, and whether any subject
/// has a tag rule, in one query.
pub(super) async fn server_grant_exists(
    pool: &PgPool,
    subjects: &[McpGrantSubject],
    mcp_server_id: Uuid,
) -> Result<(bool, bool), StoreError> {
    let (kinds, ids) = subject_arrays(subjects);
    let row = sqlx::query(&format!(
        "SELECT EXISTS (SELECT 1 FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND (g.subject_kind, g.subject_id) IN (SELECT * FROM UNNEST($2::text[], $3::text[])) AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND t.mcp_server_id = $1) OR EXISTS (SELECT 1 FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND (g.subject_kind, g.subject_id) IN (SELECT * FROM UNNEST($2::text[], $3::text[])) AND ts.status = 'active' AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND t.mcp_server_id = $1), {RULES_EXIST_SQL}"
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

pub(super) fn tool_columns(projection: McpToolProjection) -> &'static str {
    match projection {
        McpToolProjection::Access | McpToolProjection::AccessUncounted => ACCESS_TOOL_COLUMNS,
        McpToolProjection::Full => TOOL_COLUMNS,
    }
}

pub(super) async fn collect_direct_tools(
    pool: &PgPool,
    subject: &McpGrantSubject,
    mcp_server_id: Option<Uuid>,
    columns: &str,
    upstream_name: Option<&str>,
    out: &mut HashMap<Uuid, ExternalMcpToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {columns} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND g.subject_kind = $1 AND g.subject_id = $2 AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND ($3::text IS NULL OR t.mcp_server_id = $3) AND ($4::text IS NULL OR t.upstream_name = $4)"
    );
    let rows = sqlx::query(&sql)
        .bind(subject.subject_kind.as_str())
        .bind(subject.subject_id.to_string())
        .bind(mcp_server_id.map(|value| value.to_string()))
        .bind(upstream_name)
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    for row in rows {
        let tool = decode_tool(&row)?;
        out.insert(tool.mcp_tool_id, tool);
    }
    Ok(())
}

pub(super) async fn collect_toolset_tools(
    pool: &PgPool,
    subject: &McpGrantSubject,
    mcp_server_id: Option<Uuid>,
    columns: &str,
    upstream_name: Option<&str>,
    out: &mut HashMap<Uuid, ExternalMcpToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {columns} FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND g.subject_kind = $1 AND g.subject_id = $2 AND ts.status = 'active' AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND ($3::text IS NULL OR t.mcp_server_id = $3) AND ($4::text IS NULL OR t.upstream_name = $4)"
    );
    let rows = sqlx::query(&sql)
        .bind(subject.subject_kind.as_str())
        .bind(subject.subject_id.to_string())
        .bind(mcp_server_id.map(|value| value.to_string()))
        .bind(upstream_name)
        .fetch_all(pool)
        .await
        .map_err(to_query_error)?;
    for row in rows {
        let tool = decode_tool(&row)?;
        out.insert(tool.mcp_tool_id, tool);
    }
    Ok(())
}

pub(super) async fn collect_catalog_direct_tools(
    pool: &PgPool,
    subject: &McpGrantSubject,
    server_key: Option<&str>,
    out: &mut HashMap<Uuid, McpCatalogToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND g.subject_kind = $1 AND g.subject_id = $2 AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND ($3 IS NULL OR s.server_key = $3)"
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

pub(super) async fn collect_catalog_toolset_tools(
    pool: &PgPool,
    subject: &McpGrantSubject,
    server_key: Option<&str>,
    out: &mut HashMap<Uuid, McpCatalogToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND g.subject_kind = $1 AND g.subject_id = $2 AND ts.status = 'active' AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND ($3 IS NULL OR s.server_key = $3)"
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

#[cfg(test)]
mod tests {
    use gateway_core::mcp_tool_policy::{DERIVED_TAGS, derived_tag_hint};

    use super::TRUSTED_INCLUDE_TAG_SQL;

    /// The SQL repeats `gateway_core`'s derived tags and their hints, so it
    /// must name each one, and nothing else.
    #[test]
    fn the_trusted_include_sql_matches_the_core_derived_tags() {
        let listed = DERIVED_TAGS
            .iter()
            .map(|tag| format!("'{tag}'"))
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            TRUSTED_INCLUDE_TAG_SQL.contains(&format!("et.tag NOT IN ({listed})")),
            "{TRUSTED_INCLUDE_TAG_SQL}"
        );
        for tag in DERIVED_TAGS {
            let hint = derived_tag_hint(tag).expect("derived tag hint");
            let branch = format!("WHEN '{tag}' THEN '{hint}'");
            assert!(
                TRUSTED_INCLUDE_TAG_SQL.contains(&branch),
                "missing {branch}"
            );
        }
        assert_eq!(
            TRUSTED_INCLUDE_TAG_SQL.matches(" WHEN ").count(),
            DERIVED_TAGS.len()
        );
    }
}
