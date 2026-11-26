use super::*;
use std::collections::HashMap;

use gateway_core::{AgentSessionListingRecord, Money9};

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
                listing: AgentSessionListingRecord::default(),
            });
        }
        let ids = items
            .iter()
            .map(|item| item.session.agent_session_id)
            .collect::<Vec<_>>();
        let mut listings = self.session_listings(&ids).await?;
        for item in &mut items {
            if let Some(listing) = listings.remove(&item.session.agent_session_id) {
                item.listing = listing;
            }
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

    /// Listing facts for a page of sessions in one query. Usage is matched on
    /// `(request_id, ownership_scope_key)`, as the analysis report builder matches it. The
    /// provider comes from the latest request that still has a log, matched on
    /// `(request_id, api_key_id)` because a link's `request_log_id` is cleared when its log is
    /// replaced or purged. Owner names come from the same query, so a page needs no per-owner
    /// lookups.
    pub(super) async fn session_listings(
        &self,
        session_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, AgentSessionListingRecord>, StoreError> {
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ids = session_ids.iter().map(Uuid::to_string).collect::<Vec<_>>();
        let rows = sqlx::query(SESSION_LISTING_SQL)
            .bind(&ids)
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;
        let mut listings = HashMap::with_capacity(rows.len());
        for row in rows {
            let session_id: String = row.try_get(0).map_err(to_query_error)?;
            let priced_cost: Option<i64> = row.try_get(1).map_err(to_query_error)?;
            let unpriced_request_count: i64 = row.try_get(2).map_err(to_query_error)?;
            listings.insert(
                Uuid::parse_str(&session_id)
                    .map_err(|error| StoreError::Serialization(error.to_string()))?,
                AgentSessionListingRecord {
                    priced_cost: priced_cost.map(Money9::from_nanos),
                    unpriced_request_count: u64::try_from(unpriced_request_count)
                        .map_err(|error| StoreError::Serialization(error.to_string()))?,
                    latest_provider_key: row.try_get(3).map_err(to_query_error)?,
                    user_name: row.try_get(4).map_err(to_query_error)?,
                    user_email: row.try_get(5).map_err(to_query_error)?,
                    service_account_name: row.try_get(6).map_err(to_query_error)?,
                },
            );
        }
        Ok(listings)
    }
}

const SESSION_LISTING_SQL: &str = r#"
SELECT t.agent_session_id,
       (SUM(ledger.computed_cost_nanos)
          FILTER (WHERE ledger.pricing_status IN ('priced', 'legacy_estimated')))::BIGINT,
       COUNT(r.request_id) FILTER (
         WHERE ledger.usage_event_id IS NULL
            OR ledger.pricing_status NOT IN ('priced', 'legacy_estimated')
       ),
       (SELECT log.provider_key
          FROM agent_session_requests latest
          JOIN request_logs log
            ON log.request_id = latest.request_id
           AND log.api_key_id = t.api_key_id
         WHERE latest.agent_session_id = t.agent_session_id
         ORDER BY latest.ordinal DESC, log.occurred_at DESC
         LIMIT 1),
       u.name, u.email, sa.service_account_name
  FROM agent_sessions t
  LEFT JOIN agent_session_requests r ON r.agent_session_id = t.agent_session_id
  LEFT JOIN usage_cost_events ledger
    ON ledger.request_id = r.request_id
   AND ledger.ownership_scope_key = t.ownership_scope_key
  LEFT JOIN users u ON u.user_id = t.user_id
  LEFT JOIN service_accounts sa ON sa.service_account_id = t.service_account_id
 WHERE t.agent_session_id = ANY($1::text[])
 GROUP BY t.agent_session_id, t.api_key_id, u.name, u.email, sa.service_account_name
"#;
