use super::*;
use std::collections::HashMap;

use gateway_core::{AgentSessionSpendRecord, Money9};

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
                spend: AgentSessionSpendRecord::default(),
            });
        }
        self.hydrate_session_spend(&mut items).await?;
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
        let rows = sqlx::query(SESSION_SPEND_SQL)
            .bind(&session_ids)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        let mut spend = HashMap::with_capacity(rows.len());
        for row in rows {
            let session_id: String = row.try_get(0).map_err(to_query_error)?;
            let priced_cost: Option<i64> = row.try_get(1).map_err(to_query_error)?;
            let unpriced_request_count: i64 = row.try_get(2).map_err(to_query_error)?;
            spend.insert(
                session_id,
                AgentSessionSpendRecord {
                    priced_cost: priced_cost.map(Money9::from_nanos),
                    unpriced_request_count: u64::try_from(unpriced_request_count)
                        .map_err(|error| StoreError::Serialization(error.to_string()))?,
                    latest_provider_key: row.try_get(3).map_err(to_query_error)?,
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
       (SUM(ledger.computed_cost_nanos)
          FILTER (WHERE ledger.pricing_status IN ('priced', 'legacy_estimated')))::BIGINT,
       COUNT(*) FILTER (
         WHERE ledger.usage_event_id IS NULL
            OR ledger.pricing_status NOT IN ('priced', 'legacy_estimated')
       ),
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
 WHERE r.agent_session_id = ANY($1::text[])
 GROUP BY r.agent_session_id
"#;
