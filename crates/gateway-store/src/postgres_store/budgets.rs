use super::*;

#[async_trait]
impl BudgetRepository for PostgresStore {
    async fn get_active_budget_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Option<UserBudgetRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone,
                   is_active, created_at, updated_at
            FROM user_budgets
            WHERE user_id = $1 AND is_active = 1
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_user_budget_record).transpose()
    }

    async fn get_usage_ledger_by_request_and_scope(
        &self,
        request_id: &str,
        ownership_scope_key: &str,
    ) -> Result<Option<UsageLedgerRecord>, StoreError> {
        let row = sqlx::query(
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
            WHERE request_id = $1
              AND ownership_scope_key = $2
            LIMIT 1
            "#,
        )
        .bind(request_id)
        .bind(ownership_scope_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_usage_ledger_record).transpose()
    }

    async fn sum_usage_cost_for_user_in_window(
        &self,
        user_id: Uuid,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT COALESCE(SUM(computed_cost_10000), 0)
            FROM usage_cost_events
            WHERE user_id = $1
              AND pricing_status IN ('priced', 'legacy_estimated')
              AND occurred_at >= $2
              AND occurred_at < $3
            "#,
        )
        .bind(user_id.to_string())
        .bind(window_start.unix_timestamp())
        .bind(window_end.unix_timestamp())
        .fetch_one(&self.pool)
        .await
        .map_err(to_query_error)?;

        Ok(Money4::from_scaled(
            row.try_get::<i64, _>(0).map_err(to_query_error)?,
        ))
    }

    async fn insert_usage_ledger_if_absent(
        &self,
        event: &UsageLedgerRecord,
    ) -> Result<bool, StoreError> {
        let provider_usage_json = crate::shared::serialize_json(&event.provider_usage)?;

        let result = sqlx::query(
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
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
                $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27
            )
            ON CONFLICT (request_id, ownership_scope_key) DO NOTHING
            "#,
        )
        .bind(event.usage_event_id.to_string())
        .bind(event.request_id.as_str())
        .bind(event.ownership_scope_key.as_str())
        .bind(event.api_key_id.to_string())
        .bind(event.user_id.map(|value| value.to_string()))
        .bind(event.team_id.map(|value| value.to_string()))
        .bind(event.actor_user_id.map(|value| value.to_string()))
        .bind(event.model_id.map(|value| value.to_string()))
        .bind(event.provider_key.as_str())
        .bind(event.upstream_model.as_str())
        .bind(event.prompt_tokens)
        .bind(event.completion_tokens)
        .bind(event.total_tokens)
        .bind(provider_usage_json)
        .bind(event.pricing_status.as_str())
        .bind(event.unpriced_reason.as_deref())
        .bind(event.pricing_row_id.map(|value| value.to_string()))
        .bind(event.pricing_provider_id.as_deref())
        .bind(event.pricing_model_id.as_deref())
        .bind(event.pricing_source.as_deref())
        .bind(event.pricing_source_etag.as_deref())
        .bind(
            event
                .pricing_source_fetched_at
                .map(OffsetDateTime::unix_timestamp),
        )
        .bind(event.pricing_last_updated.as_deref())
        .bind(
            event
                .input_cost_per_million_tokens
                .map(Money4::as_scaled_i64),
        )
        .bind(
            event
                .output_cost_per_million_tokens
                .map(Money4::as_scaled_i64),
        )
        .bind(event.computed_cost_usd.as_scaled_i64())
        .bind(event.occurred_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;

        Ok(result.rows_affected() > 0)
    }
}
