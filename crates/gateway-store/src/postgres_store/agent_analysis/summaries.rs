use super::*;
use std::collections::HashMap;

impl PostgresStore {
    pub(super) async fn hydrate_session_summaries(
        &self,
        rows: Vec<PgRow>,
    ) -> Result<Vec<AgentSessionSummaryRecord>, StoreError> {
        let mut items = Vec::with_capacity(rows.len());
        let mut analysis_ids = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(id) = row
                .try_get::<Option<String>, _>(27)
                .map_err(to_query_error)?
            {
                analysis_ids.push(id);
            }
            items.push(AgentSessionSummaryRecord {
                session: decode_session(&row)?,
                request_count: u64::try_from(row.try_get::<i64, _>(23).map_err(to_query_error)?)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
                input_generation: row.try_get(24).map_err(to_query_error)?,
                session_source_hash: row.try_get(25).map_err(to_query_error)?,
                harness_label: row.try_get(26).map_err(to_query_error)?,
                latest_analysis: None,
            });
        }
        if analysis_ids.is_empty() {
            return Ok(items);
        }
        let sql = format!(
            "SELECT {ANALYSIS_COLUMNS} FROM agent_session_analyses WHERE analysis_id = ANY($1::text[]) AND stale = 0 AND input_generation = (SELECT input_generation FROM agent_sessions s WHERE s.agent_session_id = agent_session_analyses.agent_session_id)"
        );
        let rows = sqlx::query(&sql)
            .bind(&analysis_ids)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        let mut analyses = HashMap::with_capacity(rows.len());
        for row in rows {
            let analysis = decode_analysis(&row)?;
            analyses.insert(analysis.agent_session_id, analysis);
        }
        for item in &mut items {
            item.latest_analysis = analyses.remove(&item.session.agent_session_id);
        }
        Ok(items)
    }
}
