use super::*;
use std::collections::HashMap;

impl LibsqlStore {
    pub(super) async fn hydrate_session_summaries(
        &self,
        mut rows: libsql::Rows,
    ) -> Result<Vec<AgentSessionSummaryRecord>, StoreError> {
        let mut items = Vec::new();
        let mut analysis_ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            if let Some(id) = row.get::<Option<String>>(27).map_err(to_query_error)? {
                analysis_ids.push(id);
            }
            items.push(AgentSessionSummaryRecord {
                session: decode_session(&row)?,
                request_count: u64::try_from(row.get::<i64>(23).map_err(to_query_error)?)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
                input_generation: row.get(24).map_err(to_query_error)?,
                session_source_hash: row.get(25).map_err(to_query_error)?,
                harness_label: row.get(26).map_err(to_query_error)?,
                latest_analysis: None,
            });
        }
        drop(rows);
        if analysis_ids.is_empty() {
            return Ok(items);
        }
        let mut sql =
            format!("SELECT {ANALYSIS_COLUMNS} FROM agent_session_analyses WHERE analysis_id IN (");
        let suffix = ") AND stale = 0 AND input_generation = (SELECT input_generation FROM agent_sessions s WHERE s.agent_session_id = agent_session_analyses.agent_session_id)";
        sql.reserve(analysis_ids.len() * 2 + suffix.len());
        for index in 0..analysis_ids.len() {
            if index > 0 {
                sql.push(',');
            }
            sql.push('?');
        }
        sql.push_str(suffix);
        let mut rows = self
            .connection
            .query(&sql, libsql::params_from_iter(analysis_ids))
            .await
            .map_err(to_query_error)?;
        let mut analyses = HashMap::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            let analysis = decode_analysis(&row)?;
            analyses.insert(analysis.agent_session_id, analysis);
        }
        for item in &mut items {
            item.latest_analysis = analyses.remove(&item.session.agent_session_id);
        }
        Ok(items)
    }
}
