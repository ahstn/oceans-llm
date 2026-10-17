//! Access resolution: which tools a caller's grants and tag rules allow, and
//! the queries it runs, including the tag-rule checks folded into them.
use super::*;

pub(super) fn decode_tag_rule(row: &libsql::Row) -> Result<McpToolTagRuleRecord, StoreError> {
    TagRuleRow {
        rule_id: row.get(0).map_err(to_query_error)?,
        subject_kind: row.get(1).map_err(to_query_error)?,
        subject_id: row.get(2).map_err(to_query_error)?,
        server_scope: row.get(3).map_err(to_query_error)?,
        tag: row.get(4).map_err(to_query_error)?,
        effect: row.get(5).map_err(to_query_error)?,
        is_active: row.get(6).map_err(to_query_error)?,
        created_at: row.get(7).map_err(to_query_error)?,
        updated_at: row.get(8).map_err(to_query_error)?,
        revoked_at: row.get(9).map_err(to_query_error)?,
    }
    .decode()
}

impl LibsqlStore {
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
                &self.connection,
                subject,
                mcp_server_id,
                columns,
                upstream_name,
                &mut tools_by_id,
            )
            .await?;
            collect_toolset_tools(
                &self.connection,
                subject,
                mcp_server_id,
                columns,
                upstream_name,
                &mut tools_by_id,
            )
            .await?;
        }
        let selectors = rules.include_selectors(mcp_server_id);
        let matched =
            include_rule_matches(&self.connection, &selectors, None, upstream_name).await?;
        let missing = matched
            .into_iter()
            .filter(|id| !tools_by_id.contains_key(id))
            .collect::<Vec<_>>();
        for tool in tools_by_ids(&self.connection, &missing, columns).await? {
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
    connection: &libsql::Connection,
    subjects: &[McpGrantSubject],
) -> Result<McpTagRuleSet, StoreError> {
    if subjects.is_empty() {
        return Ok(McpTagRuleSet::default());
    }
    let matches = subject_matches("subject_kind", "subject_id", subjects.len(), 1);
    let sql = format!(
        "SELECT {TAG_RULE_COLUMNS} FROM mcp_tool_tag_rules WHERE is_active = 1 AND ({matches})"
    );
    let mut rows = connection
        .query(&sql, libsql::params_from_iter(subject_params(subjects)))
        .await
        .map_err(to_query_error)?;
    let mut rules = Vec::new();
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        rules.push(decode_tag_rule(&row)?);
    }
    Ok(McpTagRuleSet::new(rules))
}

/// `(kind = ?n AND id = ?n+1) OR ...` for `count` subjects, numbering from
/// `first`; `0` when there are none. Bind the values with `subject_params`.
pub(super) fn subject_matches(kind: &str, id: &str, count: usize, first: usize) -> String {
    if count == 0 {
        return "0".to_string();
    }
    (0..count)
        .map(|index| {
            let at = first + index * 2;
            format!("({kind} = ?{at} AND {id} = ?{})", at + 1)
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub(super) fn subject_params(subjects: &[McpGrantSubject]) -> Vec<libsql::Value> {
    subjects
        .iter()
        .flat_map(|subject| {
            [
                libsql::Value::Text(subject.subject_kind.as_str().to_string()),
                libsql::Value::Text(subject.subject_id.to_string()),
            ]
        })
        .collect()
}

/// Whether any of the subjects has an active tag rule, as a SQL expression
/// over subject parameters numbered from `first`.
pub(super) fn rules_exist_sql(count: usize, first: usize) -> String {
    let matches = subject_matches("subject_kind", "subject_id", count, first);
    format!("EXISTS (SELECT 1 FROM mcp_tool_tag_rules WHERE is_active = 1 AND ({matches}))")
}

/// `?1, ?2, ...` placeholders for `count` values.
pub(super) fn id_placeholders(count: usize) -> String {
    (1..=count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Active, served tools that include rules select, found through the
/// effective tag index: only matching tool ids are read.
pub(super) async fn include_rule_matches(
    connection: &libsql::Connection,
    selectors: &BTreeSet<(Uuid, &str)>,
    server_key: Option<&str>,
    upstream_name: Option<&str>,
) -> Result<Vec<Uuid>, StoreError> {
    if selectors.is_empty() {
        return Ok(Vec::new());
    }
    let pairs = (0..selectors.len())
        .map(|index| {
            let at = index * 2 + 1;
            format!("(t.mcp_server_id = ?{at} AND et.tag = ?{})", at + 1)
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    let key_param = selectors.len() * 2 + 1;
    let name_param = key_param + 1;
    let sql = format!(
        "SELECT DISTINCT t.mcp_tool_id FROM mcp_tool_effective_tags et JOIN external_mcp_tools t ON t.mcp_tool_id = et.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE ({pairs}) AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND (?{key_param} IS NULL OR s.server_key = ?{key_param}) AND (?{name_param} IS NULL OR t.upstream_name = ?{name_param})"
    );
    let mut params = selectors
        .iter()
        .flat_map(|(server_id, tag)| {
            [
                libsql::Value::Text(server_id.to_string()),
                libsql::Value::Text((*tag).to_string()),
            ]
        })
        .collect::<Vec<_>>();
    params.push(server_key.map_or(libsql::Value::Null, |key| {
        libsql::Value::Text(key.to_string())
    }));
    params.push(upstream_name.map_or(libsql::Value::Null, |name| {
        libsql::Value::Text(name.to_string())
    }));
    let mut rows = connection
        .query(&sql, libsql::params_from_iter(params))
        .await
        .map_err(to_query_error)?;
    let mut matched = Vec::new();
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        let mcp_tool_id: String = row.get(0).map_err(to_query_error)?;
        matched.push(parse_uuid(&mcp_tool_id)?);
    }
    Ok(matched)
}

/// Active tools by id, in one query.
pub(super) async fn tools_by_ids(
    connection: &libsql::Connection,
    tool_ids: &[Uuid],
    columns: &str,
) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
    if tool_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = id_placeholders(tool_ids.len());
    let sql = format!(
        "SELECT {columns} FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id IN ({placeholders}) AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active'"
    );
    let mut rows = connection
        .query(
            &sql,
            libsql::params_from_iter(tool_ids.iter().map(Uuid::to_string)),
        )
        .await
        .map_err(to_query_error)?;
    let mut tools = Vec::new();
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        tools.push(decode_tool(&row)?);
    }
    Ok(tools)
}

/// Active catalog tools by id, with their servers, in one query.
pub(super) async fn catalog_tools_by_ids(
    connection: &libsql::Connection,
    tool_ids: &[Uuid],
) -> Result<Vec<McpCatalogToolRecord>, StoreError> {
    if tool_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = id_placeholders(tool_ids.len());
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.mcp_tool_id IN ({placeholders}) AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active'"
    );
    let mut rows = connection
        .query(
            &sql,
            libsql::params_from_iter(tool_ids.iter().map(Uuid::to_string)),
        )
        .await
        .map_err(to_query_error)?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await.map_err(to_query_error)? {
        records.push(decode_catalog_tool(&row)?);
    }
    Ok(records)
}

/// Active catalog tools, and whether any subject has a tag rule, so callers
/// without rules skip loading them.
pub(super) async fn active_catalog_tool_count(
    connection: &libsql::Connection,
    server_key: Option<&str>,
    subjects: &[McpGrantSubject],
) -> Result<(i64, bool), StoreError> {
    let sql = format!(
        "SELECT (SELECT COUNT(*) FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND (?1 IS NULL OR s.server_key = ?1)), {}",
        rules_exist_sql(subjects.len(), 2)
    );
    let params = std::iter::once(server_key.map_or(libsql::Value::Null, |key| {
        libsql::Value::Text(key.to_string())
    }))
    .chain(subject_params(subjects));
    count_and_rules(connection, &sql, params, "active MCP catalog tool count").await
}

/// Active tools, and whether any subject has a tag rule, so callers without
/// rules skip loading them.
pub(super) async fn active_tool_count(
    connection: &libsql::Connection,
    mcp_server_id: Option<Uuid>,
    subjects: &[McpGrantSubject],
) -> Result<(i64, bool), StoreError> {
    let sql = format!(
        "SELECT (SELECT COUNT(*) FROM external_mcp_tools t JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND (?1 IS NULL OR t.mcp_server_id = ?1)), {}",
        rules_exist_sql(subjects.len(), 2)
    );
    let params = std::iter::once(mcp_server_id.map_or(libsql::Value::Null, |id| {
        libsql::Value::Text(id.to_string())
    }))
    .chain(subject_params(subjects));
    count_and_rules(connection, &sql, params, "active MCP tool count").await
}

/// Whether any subject holds a grant on the server, and whether any subject
/// has a tag rule, in one query.
pub(super) async fn server_grant_exists(
    connection: &libsql::Connection,
    subjects: &[McpGrantSubject],
    mcp_server_id: Uuid,
) -> Result<(bool, bool), StoreError> {
    let grants = subject_matches("g.subject_kind", "g.subject_id", subjects.len(), 2);
    let sql = format!(
        "SELECT EXISTS (SELECT 1 FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND ({grants}) AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND t.mcp_server_id = ?1) OR EXISTS (SELECT 1 FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND ({grants}) AND ts.status = 'active' AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND t.mcp_server_id = ?1), {}",
        rules_exist_sql(subjects.len(), 2)
    );
    let params = std::iter::once(libsql::Value::Text(mcp_server_id.to_string()))
        .chain(subject_params(subjects));
    let (granted, has_rules) =
        count_and_rules(connection, &sql, params, "MCP server grant check").await?;
    Ok((granted != 0, has_rules))
}

/// Runs a query returning an integer and a rule-existence flag.
pub(super) async fn count_and_rules(
    connection: &libsql::Connection,
    sql: &str,
    params: impl Iterator<Item = libsql::Value>,
    what: &str,
) -> Result<(i64, bool), StoreError> {
    let mut rows = connection
        .query(sql, libsql::params_from_iter(params))
        .await
        .map_err(to_query_error)?;
    let row = rows
        .next()
        .await
        .map_err(to_query_error)?
        .ok_or_else(|| StoreError::Unexpected(format!("{what} returned no row")))?;
    let value: i64 = row.get(0).map_err(to_query_error)?;
    let has_rules: i64 = row.get(1).map_err(to_query_error)?;
    Ok((value, has_rules != 0))
}

pub(super) fn tool_columns(projection: McpToolProjection) -> &'static str {
    match projection {
        McpToolProjection::Access => ACCESS_TOOL_COLUMNS,
        McpToolProjection::Full => TOOL_COLUMNS,
    }
}

pub(super) async fn collect_direct_tools(
    connection: &libsql::Connection,
    subject: &McpGrantSubject,
    mcp_server_id: Option<Uuid>,
    columns: &str,
    upstream_name: Option<&str>,
    out: &mut HashMap<Uuid, ExternalMcpToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {columns} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND g.subject_kind = ?1 AND g.subject_id = ?2 AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND (?3 IS NULL OR t.mcp_server_id = ?3) AND (?4 IS NULL OR t.upstream_name = ?4)"
    );
    let mut rows = connection
        .query(
            &sql,
            libsql::params![
                subject.subject_kind.as_str(),
                subject.subject_id.to_string(),
                mcp_server_id.map(|value| value.to_string()),
                upstream_name,
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

pub(super) async fn collect_toolset_tools(
    connection: &libsql::Connection,
    subject: &McpGrantSubject,
    mcp_server_id: Option<Uuid>,
    columns: &str,
    upstream_name: Option<&str>,
    out: &mut HashMap<Uuid, ExternalMcpToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {columns} FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND g.subject_kind = ?1 AND g.subject_id = ?2 AND ts.status = 'active' AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND (?3 IS NULL OR t.mcp_server_id = ?3) AND (?4 IS NULL OR t.upstream_name = ?4)"
    );
    let mut rows = connection
        .query(
            &sql,
            libsql::params![
                subject.subject_kind.as_str(),
                subject.subject_id.to_string(),
                mcp_server_id.map(|value| value.to_string()),
                upstream_name,
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

pub(super) async fn collect_catalog_direct_tools(
    connection: &libsql::Connection,
    subject: &McpGrantSubject,
    server_key: Option<&str>,
    out: &mut HashMap<Uuid, McpCatalogToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM mcp_tool_grants g JOIN external_mcp_tools t ON t.mcp_tool_id = g.target_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'tool' AND g.subject_kind = ?1 AND g.subject_id = ?2 AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND (?3 IS NULL OR s.server_key = ?3)"
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

pub(super) async fn collect_catalog_toolset_tools(
    connection: &libsql::Connection,
    subject: &McpGrantSubject,
    server_key: Option<&str>,
    out: &mut HashMap<Uuid, McpCatalogToolRecord>,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {TOOL_COLUMNS}, {SERVER_COLUMNS} FROM mcp_tool_grants g JOIN mcp_toolsets ts ON ts.toolset_id = g.target_id JOIN mcp_toolset_tools tst ON tst.toolset_id = ts.toolset_id JOIN external_mcp_tools t ON t.mcp_tool_id = tst.mcp_tool_id JOIN external_mcp_servers s ON s.mcp_server_id = t.mcp_server_id WHERE g.is_active = 1 AND g.target_kind = 'toolset' AND g.subject_kind = ?1 AND g.subject_id = ?2 AND ts.status = 'active' AND t.is_active = 1 AND t.approval_status IN ('approved', 'pending_change') AND s.status = 'active' AND (?3 IS NULL OR s.server_key = ?3)"
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
