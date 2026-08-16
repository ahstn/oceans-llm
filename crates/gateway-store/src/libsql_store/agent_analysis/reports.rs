use super::*;

#[async_trait]
impl AgentSessionReportRepository for LibsqlStore {
    async fn append_agent_session_analysis(
        &self,
        analysis: &AgentSessionAnalysisRecord,
    ) -> Result<bool, StoreError> {
        let written = self.connection.execute(
            "INSERT INTO agent_session_analyses (analysis_id, agent_session_id, report_schema_version, boundary_policy_version, input_watermark_at, observation_set_id, observation_parser_version, analyzer_version, score_policy_version, pricing_policy_version, cohort_version, cohort_fallback_level, cohort_sample_size, cohort_snapshot_digest, direct_mcp_snapshot_digest, analyzed_at, report_json, stale, superseded_by_analysis_id, expires_at, ownership_scope_key, user_id, service_account_id, configuration_version) SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24 WHERE EXISTS (SELECT 1 FROM agent_sessions WHERE agent_session_id = ?2 AND input_watermark_at = ?5) AND EXISTS (SELECT 1 FROM agent_inferred_observation_sets WHERE observation_set_id = ?6 AND agent_session_id = ?2) ON CONFLICT DO NOTHING",
            libsql::params![analysis.analysis_id.to_string(), analysis.agent_session_id.to_string(), analysis.report.report_schema_version.as_str(), analysis.boundary_policy_version.as_str(), datetime_to_unix_millis(analysis.input_watermark_at)?, analysis.observation_set_id.to_string(), analysis.observation_parser_version.as_str(), analysis.report.analyzer_version.as_str(), analysis.report.score_policy_version.as_str(), analysis.pricing_policy_version.as_str(), analysis.cohort_version.as_str(), i64::from(analysis.cohort_fallback_level), i64::try_from(analysis.cohort_sample_size).map_err(|error| StoreError::Serialization(error.to_string()))?, analysis.cohort_snapshot_digest.as_str(), analysis.direct_mcp_snapshot_digest.as_str(), analysis.analyzed_at.unix_timestamp(), crate::shared::serialize_json(&analysis.report)?, i64::from(analysis.stale), analysis.superseded_by_analysis_id.map(|value| value.to_string()), analysis.expires_at.unix_timestamp(), analysis.ownership_scope_key.as_str(), analysis.user_id.map(|value| value.to_string()), analysis.service_account_id.map(|value| value.to_string()), analysis.configuration_version.as_str()],
        ).await.map_err(to_query_error)?;
        if written > 0 {
            return Ok(true);
        }
        let sql = format!(
            "SELECT {ANALYSIS_COLUMNS} FROM agent_session_analyses WHERE agent_session_id = ?1 AND report_schema_version = ?2 AND boundary_policy_version = ?3 AND input_watermark_at = ?4 AND observation_set_id = ?5 AND observation_parser_version = ?6 AND analyzer_version = ?7 AND score_policy_version = ?8 AND pricing_policy_version = ?9 AND cohort_version = ?10 AND cohort_fallback_level = ?11 AND cohort_sample_size = ?12 AND cohort_snapshot_digest = ?13 AND direct_mcp_snapshot_digest = ?14 AND configuration_version = ?15"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    analysis.agent_session_id.to_string(),
                    analysis.report.report_schema_version.as_str(),
                    analysis.boundary_policy_version.as_str(),
                    datetime_to_unix_millis(analysis.input_watermark_at)?,
                    analysis.observation_set_id.to_string(),
                    analysis.observation_parser_version.as_str(),
                    analysis.report.analyzer_version.as_str(),
                    analysis.report.score_policy_version.as_str(),
                    analysis.pricing_policy_version.as_str(),
                    analysis.cohort_version.as_str(),
                    i64::from(analysis.cohort_fallback_level),
                    i64::try_from(analysis.cohort_sample_size)
                        .map_err(|error| StoreError::Serialization(error.to_string()))?,
                    analysis.cohort_snapshot_digest.as_str(),
                    analysis.direct_mcp_snapshot_digest.as_str(),
                    analysis.configuration_version.as_str()
                ],
            )
            .await
            .map_err(to_query_error)?;
        if let Some(row) = rows.next().await.map_err(to_query_error)? {
            let existing = decode_analysis(&row)?;
            return if agent_session_analysis_matches(&existing, analysis) {
                Ok(false)
            } else {
                Err(StoreError::Conflict(format!(
                    "agent session analysis for session `{}` conflicts with the existing record",
                    analysis.agent_session_id
                )))
            };
        }
        let mut id_rows = self
            .connection
            .query(
                "SELECT 1 FROM agent_session_analyses WHERE analysis_id = ?1",
                [analysis.analysis_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        if id_rows.next().await.map_err(to_query_error)?.is_some() {
            return Err(StoreError::Conflict(format!(
                "agent session analysis `{}` conflicts with the existing record",
                analysis.analysis_id
            )));
        }
        Err(StoreError::Conflict(format!(
            "agent session `{}` changed while its analysis was being stored",
            analysis.agent_session_id
        )))
    }

    async fn list_agent_sessions(
        &self,
        query: &AgentSessionListQuery,
    ) -> Result<AgentSessionListPage, StoreError> {
        let page = query.page.max(1);
        let page_size = query.page_size.clamp(1, MAX_AGENT_SESSION_PAGE_SIZE);
        let offset = i64::from(page.saturating_sub(1).saturating_mul(page_size));
        let lifecycle = query.lifecycle.map(enum_name).transpose()?;
        let score_confidence = query.score_confidence.map(enum_name).transpose()?;
        let gateway_outcome = query.gateway_outcome.map(enum_name).transpose()?;
        let score_maturity = query.score_maturity.map(enum_name).transpose()?;
        let from_sql = "agent_sessions t LEFT JOIN agent_session_sources s ON s.agent_session_source_id = t.agent_session_source_id LEFT JOIN agent_session_analyses latest_analysis ON latest_analysis.analysis_id = (SELECT a.analysis_id FROM agent_session_analyses a WHERE a.agent_session_id = t.agent_session_id AND a.stale = 0 ORDER BY a.analyzed_at DESC, a.analysis_id DESC LIMIT 1)";
        let where_sql = concat!(
            "(?1 IS NULL OR t.ownership_scope_key = ?1)",
            " AND (?2 IS NULL OR t.user_id = ?2)",
            " AND (?3 IS NULL OR t.team_id = ?3)",
            " AND (?4 IS NULL OR t.service_account_id = ?4)",
            " AND (?5 IS NULL OR t.harness_key = ?5)",
            " AND (?6 IS NULL OR t.lifecycle = ?6)",
            " AND (?7 IS NULL OR t.started_at >= ?7)",
            " AND (?8 IS NULL OR t.started_at < ?8)",
            " AND (?9 IS NULL OR t.input_watermark_at < ?9)",
            " AND (?10 IS NULL OR json_extract(latest_analysis.report_json, '$.confidence') = ?10)",
            " AND (?11 IS NULL OR t.agent_session_source_id = ?11)",
            " AND (?12 IS NULL OR t.requested_model_key = ?12)",
            " AND (?13 IS NULL OR t.operation = ?13)",
            " AND (?14 IS NULL OR t.caller_class = ?14)",
            " AND (?15 IS NULL OR json_extract(latest_analysis.report_json, '$.gateway_outcome') = ?15)",
            " AND (?16 IS NULL OR json_extract(latest_analysis.report_json, '$.maturity') = ?16)",
            " AND (?17 IS NULL OR CAST(json_extract(latest_analysis.report_json, '$.coverage.overall_percent') AS INTEGER) >= ?17)",
            " AND (?18 IS NULL OR s.normalized_session_hash = ?18)",
            " AND ((?19 IS NULL AND ?20 IS NULL) OR (?19 IS NOT NULL AND (",
            "EXISTS (SELECT 1 FROM json_each(t.request_tags_json) tag",
            " WHERE tag.key = ?19 AND tag.type = 'text' AND (?20 IS NULL OR CAST(tag.value AS TEXT) = ?20))",
            " OR EXISTS (SELECT 1 FROM json_each(json_extract(t.request_tags_json, '$.bespoke')) tag",
            " WHERE json_extract(tag.value, '$.key') = ?19",
            " AND (?20 IS NULL OR json_extract(tag.value, '$.value') = ?20)))))",
        );
        let count_sql = format!("SELECT COUNT(*) FROM {from_sql} WHERE {where_sql}");
        let mut count_rows = self
            .connection
            .query(
                &count_sql,
                libsql::params![
                    query.ownership_scope_key.as_deref(),
                    query.user_id.map(|value| value.to_string()),
                    query.team_id.map(|value| value.to_string()),
                    query.service_account_id.map(|value| value.to_string()),
                    query.harness_key.as_deref(),
                    lifecycle.as_deref(),
                    query.started_after.map(OffsetDateTime::unix_timestamp),
                    query.started_before.map(OffsetDateTime::unix_timestamp),
                    query
                        .input_watermark_before
                        .map(datetime_to_unix_millis)
                        .transpose()?,
                    score_confidence.as_deref(),
                    query.agent_session_source_id.map(|value| value.to_string()),
                    query.requested_model_key.as_deref(),
                    query.operation.as_deref(),
                    query.caller_class.as_deref(),
                    gateway_outcome.as_deref(),
                    score_maturity.as_deref(),
                    query.minimum_coverage_percent.map(i64::from),
                    query.normalized_session_id.as_deref(),
                    query.request_tag_key.as_deref(),
                    query.request_tag_value.as_deref(),
                ],
            )
            .await
            .map_err(to_query_error)?;
        let total = u64::try_from(
            count_rows
                .next()
                .await
                .map_err(to_query_error)?
                .ok_or_else(|| StoreError::Unexpected("agent session count missing".to_string()))?
                .get::<i64>(0)
                .map_err(to_query_error)?,
        )
        .map_err(|error| StoreError::Serialization(error.to_string()))?;
        let list_sql = format!(
            "SELECT t.agent_session_id FROM {from_sql} WHERE {where_sql} ORDER BY t.started_at DESC, t.agent_session_id LIMIT ?21 OFFSET ?22"
        );
        let mut rows = self
            .connection
            .query(
                &list_sql,
                libsql::params![
                    query.ownership_scope_key.as_deref(),
                    query.user_id.map(|value| value.to_string()),
                    query.team_id.map(|value| value.to_string()),
                    query.service_account_id.map(|value| value.to_string()),
                    query.harness_key.as_deref(),
                    lifecycle.as_deref(),
                    query.started_after.map(OffsetDateTime::unix_timestamp),
                    query.started_before.map(OffsetDateTime::unix_timestamp),
                    query
                        .input_watermark_before
                        .map(datetime_to_unix_millis)
                        .transpose()?,
                    score_confidence.as_deref(),
                    query.agent_session_source_id.map(|value| value.to_string()),
                    query.requested_model_key.as_deref(),
                    query.operation.as_deref(),
                    query.caller_class.as_deref(),
                    gateway_outcome.as_deref(),
                    score_maturity.as_deref(),
                    query.minimum_coverage_percent.map(i64::from),
                    query.normalized_session_id.as_deref(),
                    query.request_tag_key.as_deref(),
                    query.request_tag_value.as_deref(),
                    i64::from(page_size),
                    offset,
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            ids.push(parse_uuid(&row.get::<String>(0).map_err(to_query_error)?)?);
        }
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(trace) = self.load_agent_session_trace(id).await? {
                items.push(trace);
            }
        }
        Ok(AgentSessionListPage {
            items,
            page,
            page_size,
            total,
        })
    }

    async fn mark_agent_session_analyses_stale(
        &self,
        agent_session_id: Uuid,
        superseded_by: Option<Uuid>,
    ) -> Result<u64, StoreError> {
        self.connection.execute("UPDATE agent_session_analyses SET stale = 1, superseded_by_analysis_id = ?2 WHERE agent_session_id = ?1 AND stale = 0 AND (?2 IS NULL OR analysis_id <> ?2)", libsql::params![agent_session_id.to_string(), superseded_by.map(|value| value.to_string())]).await.map_err(to_query_error)
    }
}
