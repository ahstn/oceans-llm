use super::*;

#[async_trait]
impl AgentSessionReportRepository for PostgresStore {
    async fn append_agent_session_analysis(
        &self,
        analysis: &AgentSessionAnalysisRecord,
        lease: Option<AgentAnalysisLease<'_>>,
    ) -> Result<bool, StoreError> {
        let report_json = crate::shared::serialize_json(&analysis.report)?;
        let session_id = analysis.agent_session_id.to_string();
        let analysis_id = analysis.analysis_id.to_string();
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let generation = sqlx::query_scalar::<_, i64>(LOAD_ANALYSIS_GENERATION_SQL)
            .bind(&session_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(to_query_error)?
            .ok_or_else(|| StoreError::NotFound("agent session not found".to_string()))?;
        if generation != analysis.input_generation {
            return Ok(false);
        }
        if let Some(lease) = lease {
            let valid = sqlx::query(LOCK_ANALYSIS_LEASE_SQL)
                .bind(&session_id)
                .bind(lease.queue_item_id.to_string())
                .bind(lease.owner)
                .bind(lease.epoch)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(to_query_error)?
                .is_some();
            if !valid {
                return Ok(false);
            }
        }
        let result = sqlx::query(INSERT_ANALYSIS_SQL)
            .bind(&analysis_id)
            .bind(&session_id)
            .bind(&analysis.report.report_schema_version)
            .bind(&analysis.boundary_policy_version)
            .bind(datetime_to_unix_millis(analysis.input_watermark_at)?)
            .bind(analysis.observation_set_id.to_string())
            .bind(&analysis.observation_parser_version)
            .bind(&analysis.report.analyzer_version)
            .bind(&analysis.report.score_policy_version)
            .bind(&analysis.pricing_policy_version)
            .bind(&analysis.cohort_version)
            .bind(i32::from(analysis.cohort_fallback_level))
            .bind(
                i64::try_from(analysis.cohort_sample_size)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
            )
            .bind(&analysis.cohort_snapshot_digest)
            .bind(&analysis.direct_mcp_snapshot_digest)
            .bind(analysis.analyzed_at.unix_timestamp())
            .bind(report_json)
            .bind(i32::from(analysis.stale))
            .bind(
                analysis
                    .superseded_by_analysis_id
                    .map(|value| value.to_string()),
            )
            .bind(analysis.expires_at.unix_timestamp())
            .bind(&analysis.ownership_scope_key)
            .bind(analysis.user_id.map(|value| value.to_string()))
            .bind(analysis.service_account_id.map(|value| value.to_string()))
            .bind(&analysis.configuration_version)
            .bind(analysis.input_generation)
            .execute(&mut *transaction)
            .await
            .map_err(to_query_error)?;
        if result.rows_affected() > 0 {
            if !analysis.stale {
                sqlx::query(SUPERSEDE_ANALYSES_SQL)
                    .bind(&session_id)
                    .bind(&analysis_id)
                    .bind(analysis.input_generation)
                    .bind(analysis.analyzed_at.unix_timestamp())
                    .execute(&mut *transaction)
                    .await
                    .map_err(to_query_error)?;
            }
            transaction.commit().await.map_err(to_query_error)?;
            return Ok(true);
        }
        check_analysis_replay(&mut transaction, analysis).await
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
        let from_sql = SESSION_LIST_FROM_SQL;
        let where_sql = concat!(
            "($1::text IS NULL OR t.ownership_scope_key = $1)",
            " AND ($2::text IS NULL OR t.user_id = $2)",
            " AND ($3::text IS NULL OR t.team_id = $3)",
            " AND ($4::text IS NULL OR t.service_account_id = $4)",
            " AND ($5::text IS NULL OR t.harness_key = $5)",
            " AND ($6::text IS NULL OR t.lifecycle = $6)",
            " AND ($7::bigint IS NULL OR t.started_at >= $7)",
            " AND ($8::bigint IS NULL OR t.started_at < $8)",
            " AND ($9::bigint IS NULL OR t.input_watermark_at < $9)",
            " AND ($10::text IS NULL OR (latest_report.fields ->> 'confidence') = $10)",
            " AND ($11::text IS NULL OR t.agent_session_source_id = $11)",
            " AND ($12::text IS NULL OR t.requested_model_key = $12)",
            " AND ($13::text IS NULL OR t.operation = $13)",
            " AND ($14::text IS NULL OR t.caller_class = $14)",
            " AND ($15::text IS NULL OR (latest_report.fields ->> 'gateway_outcome') = $15)",
            " AND ($16::text IS NULL OR (latest_report.fields ->> 'maturity') = $16)",
            " AND ($17::smallint IS NULL OR (latest_report.fields #>> '{coverage,overall_percent}')::smallint >= $17)",
            " AND ($18::text IS NULL OR s.normalized_session_hash = $18)",
            " AND (($19::text IS NULL AND $20::text IS NULL) OR ($19::text IS NOT NULL AND (",
            "((t.request_tags_json::jsonb ->> $19) IS NOT NULL",
            " AND ($20::text IS NULL OR t.request_tags_json::jsonb ->> $19 = $20))",
            " OR EXISTS (SELECT 1 FROM jsonb_array_elements(COALESCE(t.request_tags_json::jsonb -> 'bespoke', '[]'::jsonb)) tag",
            " WHERE tag ->> 'key' = $19 AND ($20::text IS NULL OR tag ->> 'value' = $20)))))",
            // C collation matches SQLite's ASCII-only lowercasing, as in the request log search.
            " AND ($21::text IS NULL OR strpos(lower(t.requested_model_key COLLATE \"C\"), lower($21 COLLATE \"C\")) > 0",
            " OR EXISTS (SELECT 1 FROM users u WHERE u.user_id = t.user_id",
            " AND (strpos(lower(u.name COLLATE \"C\"), lower($21 COLLATE \"C\")) > 0",
            " OR strpos(lower(u.email COLLATE \"C\"), lower($21 COLLATE \"C\")) > 0)))",
        );
        let search = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
        let count_sql = format!("SELECT COUNT(*) FROM {from_sql} WHERE {where_sql}");
        let count_row = sqlx::query(&count_sql)
            .bind(query.ownership_scope_key.as_deref())
            .bind(query.user_id.map(|value| value.to_string()))
            .bind(query.team_id.map(|value| value.to_string()))
            .bind(query.service_account_id.map(|value| value.to_string()))
            .bind(query.harness_key.as_deref())
            .bind(lifecycle.as_deref())
            .bind(query.started_after.map(OffsetDateTime::unix_timestamp))
            .bind(query.started_before.map(OffsetDateTime::unix_timestamp))
            .bind(
                query
                    .input_watermark_before
                    .map(datetime_to_unix_millis)
                    .transpose()?,
            )
            .bind(score_confidence.as_deref())
            .bind(query.agent_session_source_id.map(|value| value.to_string()))
            .bind(query.requested_model_key.as_deref())
            .bind(query.operation.as_deref())
            .bind(query.caller_class.as_deref())
            .bind(gateway_outcome.as_deref())
            .bind(score_maturity.as_deref())
            .bind(query.minimum_coverage_percent.map(i16::from))
            .bind(query.normalized_session_id.as_deref())
            .bind(query.request_tag_key.as_deref())
            .bind(query.request_tag_value.as_deref())
            .bind(search)
            .fetch_one(&self.pool)
            .await
            .map_err(to_query_error)?;
        let total = u64::try_from(count_row.try_get::<i64, _>(0).map_err(to_query_error)?)
            .map_err(|error| StoreError::Serialization(error.to_string()))?;
        let mut list_sql =
            String::with_capacity(SESSION_COLUMNS.len() + from_sql.len() + where_sql.len() + 512);
        list_sql.push_str("SELECT ");
        for (index, column) in SESSION_COLUMNS.split(", ").enumerate() {
            if index > 0 {
                list_sql.push_str(", ");
            }
            list_sql.push_str("t.");
            list_sql.push_str(column);
        }
        list_sql.push_str(", t.request_count, t.input_generation, s.normalized_session_hash, s.harness_label, latest_analysis.analysis_id FROM ");
        list_sql.push_str(from_sql);
        list_sql.push_str(" WHERE ");
        list_sql.push_str(where_sql);
        list_sql.push_str(" ORDER BY t.started_at DESC, t.agent_session_id LIMIT $22 OFFSET $23");
        let rows = sqlx::query(&list_sql)
            .bind(query.ownership_scope_key.as_deref())
            .bind(query.user_id.map(|value| value.to_string()))
            .bind(query.team_id.map(|value| value.to_string()))
            .bind(query.service_account_id.map(|value| value.to_string()))
            .bind(query.harness_key.as_deref())
            .bind(lifecycle.as_deref())
            .bind(query.started_after.map(OffsetDateTime::unix_timestamp))
            .bind(query.started_before.map(OffsetDateTime::unix_timestamp))
            .bind(
                query
                    .input_watermark_before
                    .map(datetime_to_unix_millis)
                    .transpose()?,
            )
            .bind(score_confidence.as_deref())
            .bind(query.agent_session_source_id.map(|value| value.to_string()))
            .bind(query.requested_model_key.as_deref())
            .bind(query.operation.as_deref())
            .bind(query.caller_class.as_deref())
            .bind(gateway_outcome.as_deref())
            .bind(score_maturity.as_deref())
            .bind(query.minimum_coverage_percent.map(i16::from))
            .bind(query.normalized_session_id.as_deref())
            .bind(query.request_tag_key.as_deref())
            .bind(query.request_tag_value.as_deref())
            .bind(search)
            .bind(i64::from(page_size))
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        let items = self.hydrate_session_summaries(rows).await?;
        Ok(AgentSessionListPage {
            items,
            page,
            page_size,
            total,
        })
    }
}

async fn check_analysis_replay(
    transaction: &mut sqlx::PgConnection,
    analysis: &AgentSessionAnalysisRecord,
) -> Result<bool, StoreError> {
    let sql = format!(
        "SELECT {ANALYSIS_COLUMNS} FROM agent_session_analyses WHERE agent_session_id = $1 AND report_schema_version = $2 AND boundary_policy_version = $3 AND input_watermark_at = $4 AND observation_set_id = $5 AND observation_parser_version = $6 AND analyzer_version = $7 AND score_policy_version = $8 AND pricing_policy_version = $9 AND cohort_version = $10 AND cohort_fallback_level = $11 AND cohort_sample_size = $12 AND cohort_snapshot_digest = $13 AND direct_mcp_snapshot_digest = $14 AND configuration_version = $15 AND input_generation = $16"
    );
    let existing = sqlx::query(&sql)
        .bind(analysis.agent_session_id.to_string())
        .bind(&analysis.report.report_schema_version)
        .bind(&analysis.boundary_policy_version)
        .bind(datetime_to_unix_millis(analysis.input_watermark_at)?)
        .bind(analysis.observation_set_id.to_string())
        .bind(&analysis.observation_parser_version)
        .bind(&analysis.report.analyzer_version)
        .bind(&analysis.report.score_policy_version)
        .bind(&analysis.pricing_policy_version)
        .bind(&analysis.cohort_version)
        .bind(i32::from(analysis.cohort_fallback_level))
        .bind(
            i64::try_from(analysis.cohort_sample_size)
                .map_err(|error| StoreError::Serialization(error.to_string()))?,
        )
        .bind(&analysis.cohort_snapshot_digest)
        .bind(&analysis.direct_mcp_snapshot_digest)
        .bind(&analysis.configuration_version)
        .bind(analysis.input_generation)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(to_query_error)?
        .as_ref()
        .map(decode_analysis)
        .transpose()?;
    if let Some(existing) = existing {
        return if agent_session_analysis_matches(&existing, analysis) {
            Ok(false)
        } else {
            Err(StoreError::Conflict(format!(
                "agent session analysis for session `{}` conflicts with the existing record",
                analysis.agent_session_id
            )))
        };
    }
    if sqlx::query("SELECT 1 FROM agent_session_analyses WHERE analysis_id = $1")
        .bind(analysis.analysis_id.to_string())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(to_query_error)?
        .is_some()
    {
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

const LOAD_ANALYSIS_GENERATION_SQL: &str = r"
SELECT input_generation FROM agent_sessions
WHERE agent_session_id = $1 FOR NO KEY UPDATE
";

const LOCK_ANALYSIS_LEASE_SQL: &str = r"
SELECT 1 FROM agent_analysis_recompute_queue
WHERE agent_session_id = $1 AND queue_item_id = $2 AND status = 'leased'
  AND lease_owner = $3 AND lease_epoch = $4 AND request_revision = leased_revision
FOR UPDATE
";

const INSERT_ANALYSIS_SQL: &str = r"
INSERT INTO agent_session_analyses (
    analysis_id, agent_session_id, report_schema_version, boundary_policy_version,
    input_watermark_at, observation_set_id, observation_parser_version, analyzer_version,
    score_policy_version, pricing_policy_version, cohort_version, cohort_fallback_level,
    cohort_sample_size, cohort_snapshot_digest, direct_mcp_snapshot_digest, analyzed_at,
    report_json, stale, superseded_by_analysis_id, expires_at, ownership_scope_key,
    user_id, service_account_id, configuration_version, input_generation
)
SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
       $16, $17, $18, $19, $20, $21, $22, $23, $24, $25
WHERE EXISTS (
    SELECT 1 FROM agent_sessions WHERE agent_session_id = $2 AND input_watermark_at = $5
) AND EXISTS (
    SELECT 1 FROM agent_inferred_observation_sets WHERE observation_set_id = $6 AND agent_session_id = $2
)
ON CONFLICT DO NOTHING
";

const SUPERSEDE_ANALYSES_SQL: &str = r"
UPDATE agent_session_analyses SET stale = 1, superseded_by_analysis_id = $2
WHERE agent_session_id = $1 AND analysis_id <> $2 AND stale = 0
  AND (input_generation < $3 OR (input_generation = $3 AND analyzed_at <= $4))
";

// Diagnostics retain JSON strings from client metadata, including escaped NUL.
// Only the filtering projection replaces NUL escapes; literal "\\u0000" stays
// unchanged, and decoding report_json still returns every original string.
const SESSION_LIST_FROM_SQL: &str = r"
agent_sessions t
LEFT JOIN agent_session_sources s ON s.agent_session_source_id = t.agent_session_source_id
LEFT JOIN agent_session_analyses latest_analysis ON latest_analysis.analysis_id = (
    SELECT a.analysis_id FROM agent_session_analyses a
    WHERE a.agent_session_id = t.agent_session_id
      AND a.stale = 0 AND a.input_generation = t.input_generation
    ORDER BY a.analyzed_at DESC, a.analysis_id DESC LIMIT 1
)
LEFT JOIN LATERAL (
    SELECT (CASE WHEN strpos(latest_analysis.report_json, '\u0000') = 0
        THEN latest_analysis.report_json
        ELSE regexp_replace(
            latest_analysis.report_json, '(?<!\\)((?:\\\\)*)\\u0000', '\1\\ufffd', 'g'
        )
    END)::jsonb AS fields
) latest_report ON TRUE
";
