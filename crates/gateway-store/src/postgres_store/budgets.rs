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

    async fn sum_usage_cost_for_user_in_window(
        &self,
        user_id: Uuid,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT COALESCE(SUM(estimated_cost_10000), 0)
            FROM usage_cost_events
            WHERE user_id = $1
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

    async fn insert_usage_cost_event(
        &self,
        event: &UsageCostEventRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO usage_cost_events (
                usage_event_id, request_id, api_key_id, user_id, team_id, model_id,
                estimated_cost_10000, occurred_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(event.usage_event_id.to_string())
        .bind(event.request_id.as_str())
        .bind(event.api_key_id.to_string())
        .bind(event.user_id.map(|value| value.to_string()))
        .bind(event.team_id.map(|value| value.to_string()))
        .bind(event.model_id.map(|value| value.to_string()))
        .bind(event.estimated_cost_usd.as_scaled_i64())
        .bind(event.occurred_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;
        Ok(())
    }
}
