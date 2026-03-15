use super::*;

#[async_trait]
impl BudgetRepository for LibsqlStore {
    async fn get_active_budget_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserBudgetRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone,
                       is_active, created_at, updated_at
                FROM user_budgets
                WHERE user_id = ?1 AND is_active = 1
                LIMIT 1
                "#,
                [user_id.to_string()],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(None);
        };

        decode_user_budget_record(&row).map(Some)
    }

    async fn get_usage_ledger_by_request_and_scope(
        &self,
        request_id: &str,
        ownership_scope_key: &str,
    ) -> Result<Option<UsageLedgerRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT
                    usage_event_id, request_id, ownership_scope_key, api_key_id, user_id,
                    team_id, actor_user_id, model_id, provider_key, upstream_model,
                    prompt_tokens, completion_tokens, total_tokens, provider_usage_json,
                    pricing_status, unpriced_reason, pricing_row_id, pricing_provider_id,
                    pricing_model_id, pricing_source, pricing_source_etag,
                    pricing_source_fetched_at, pricing_last_updated,
                    input_cost_per_million_tokens_10000,
                    output_cost_per_million_tokens_10000, computed_cost_10000, occurred_at
                FROM usage_cost_events
                WHERE request_id = ?1
                  AND ownership_scope_key = ?2
                LIMIT 1
                "#,
                libsql::params![request_id, ownership_scope_key],
            )
            .await
            .map_err(to_query_error)?;

        let Some(row) = rows.next().await.map_err(to_query_error)? else {
            return Ok(None);
        };

        decode_usage_ledger_record(&row).map(Some)
    }

    async fn sum_usage_cost_for_user_in_window(
        &self,
        user_id: Uuid,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT COALESCE(SUM(computed_cost_10000), 0)
                FROM usage_cost_events
                WHERE user_id = ?1
                  AND pricing_status IN ('priced', 'legacy_estimated')
                  AND occurred_at >= ?2
                  AND occurred_at < ?3
                "#,
                libsql::params![
                    user_id.to_string(),
                    window_start.unix_timestamp(),
                    window_end.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;

        let Some(row) = rows
            .next()
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?
        else {
            return Ok(Money4::ZERO);
        };

        let sum_10000: i64 = row.get(0).map_err(to_query_error)?;
        Ok(Money4::from_scaled(sum_10000))
    }

    async fn insert_usage_ledger_if_absent(
        &self,
        event: &UsageLedgerRecord,
    ) -> Result<bool, StoreError> {
        let provider_usage_json = crate::shared::serialize_json(&event.provider_usage)?;

        let written = self
            .connection
            .execute(
                r#"
                INSERT INTO usage_cost_events (
                    usage_event_id, request_id, ownership_scope_key, api_key_id, user_id,
                    team_id, actor_user_id, model_id, provider_key, upstream_model,
                    prompt_tokens, completion_tokens, total_tokens, provider_usage_json,
                    pricing_status, unpriced_reason, pricing_row_id, pricing_provider_id,
                    pricing_model_id, pricing_source, pricing_source_etag,
                    pricing_source_fetched_at, pricing_last_updated,
                    input_cost_per_million_tokens_10000,
                    output_cost_per_million_tokens_10000, computed_cost_10000, occurred_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                    ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27
                )
                ON CONFLICT(request_id, ownership_scope_key) DO NOTHING
                "#,
                libsql::params![
                    event.usage_event_id.to_string(),
                    event.request_id.as_str(),
                    event.ownership_scope_key.as_str(),
                    event.api_key_id.to_string(),
                    event.user_id.map(|value| value.to_string()),
                    event.team_id.map(|value| value.to_string()),
                    event.actor_user_id.map(|value| value.to_string()),
                    event.model_id.map(|value| value.to_string()),
                    event.provider_key.as_str(),
                    event.upstream_model.as_str(),
                    event.prompt_tokens,
                    event.completion_tokens,
                    event.total_tokens,
                    provider_usage_json,
                    event.pricing_status.as_str(),
                    event.unpriced_reason.as_deref(),
                    event.pricing_row_id.map(|value| value.to_string()),
                    event.pricing_provider_id.as_deref(),
                    event.pricing_model_id.as_deref(),
                    event.pricing_source.as_deref(),
                    event.pricing_source_etag.as_deref(),
                    event
                        .pricing_source_fetched_at
                        .map(OffsetDateTime::unix_timestamp),
                    event.pricing_last_updated.as_deref(),
                    event
                        .input_cost_per_million_tokens
                        .map(Money4::as_scaled_i64),
                    event
                        .output_cost_per_million_tokens
                        .map(Money4::as_scaled_i64),
                    event.computed_cost_usd.as_scaled_i64(),
                    event.occurred_at.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(written > 0)
    }
}
