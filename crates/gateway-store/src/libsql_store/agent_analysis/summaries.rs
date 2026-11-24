use super::*;
use std::collections::HashMap;

use gateway_core::{AgentSessionSpendRecord, Money9};

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
                spend: AgentSessionSpendRecord::default(),
            });
        }
        drop(rows);
        self.hydrate_session_spend(&mut items).await?;
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

    /// Ledger spend and the latest provider for a page of sessions. Usage is matched on
    /// `(request_id, ownership_scope_key)`, as the analysis report builder matches it. The
    /// provider comes from the latest request's log, matched on `(request_id, api_key_id)`
    /// because a link's `request_log_id` is cleared when its log is replaced or purged.
    async fn hydrate_session_spend(
        &self,
        items: &mut [AgentSessionSummaryRecord],
    ) -> Result<(), StoreError> {
        if items.is_empty() {
            return Ok(());
        }
        let session_ids = items
            .iter()
            .map(|item| item.session.agent_session_id.to_string())
            .collect::<Vec<_>>();
        let session_ids_json = serde_json::to_string(&session_ids)
            .map_err(|error| StoreError::Serialization(error.to_string()))?;
        let mut rows = self
            .connection
            .query(SESSION_SPEND_SQL, libsql::params![session_ids_json])
            .await
            .map_err(to_query_error)?;
        let mut spend = HashMap::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            let session_id: String = row.get(0).map_err(to_query_error)?;
            let priced_cost: Option<i64> = row.get(1).map_err(to_query_error)?;
            let unpriced_request_count: i64 = row.get(2).map_err(to_query_error)?;
            spend.insert(
                session_id,
                AgentSessionSpendRecord {
                    priced_cost: priced_cost.map(Money9::from_nanos),
                    unpriced_request_count: u64::try_from(unpriced_request_count)
                        .map_err(|error| StoreError::Serialization(error.to_string()))?,
                    latest_provider_key: row.get(3).map_err(to_query_error)?,
                },
            );
        }
        for item in items {
            if let Some(found) = spend.remove(&item.session.agent_session_id.to_string()) {
                item.spend = found;
            }
        }
        Ok(())
    }
}

const SESSION_SPEND_SQL: &str = r#"
SELECT r.agent_session_id,
       SUM(CASE WHEN ledger.pricing_status IN ('priced', 'legacy_estimated')
                THEN ledger.computed_cost_nanos END),
       SUM(CASE WHEN ledger.pricing_status IN ('priced', 'legacy_estimated') THEN 0 ELSE 1 END),
       (SELECT log.provider_key
          FROM agent_session_requests latest
          JOIN agent_sessions owner ON owner.agent_session_id = latest.agent_session_id
          JOIN request_logs log
            ON log.request_id = latest.request_id
           AND log.api_key_id = owner.api_key_id
         WHERE latest.agent_session_id = r.agent_session_id
         ORDER BY latest.ordinal DESC, log.occurred_at DESC
         LIMIT 1)
  FROM agent_session_requests r
  JOIN agent_sessions t ON t.agent_session_id = r.agent_session_id
  LEFT JOIN usage_cost_events ledger
    ON ledger.request_id = r.request_id
   AND ledger.ownership_scope_key = t.ownership_scope_key
 WHERE r.agent_session_id IN (SELECT value FROM json_each(?1))
 GROUP BY r.agent_session_id
"#;
