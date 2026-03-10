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
                SELECT COALESCE(SUM(estimated_cost_10000), 0)
                FROM usage_cost_events
                WHERE user_id = ?1
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

    async fn insert_usage_cost_event(
        &self,
        event: &UsageCostEventRecord,
    ) -> Result<(), StoreError> {
        self.connection
            .execute(
                r#"
                INSERT INTO usage_cost_events (
                    usage_event_id, request_id, api_key_id, user_id, team_id, model_id,
                    estimated_cost_10000, occurred_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
                libsql::params![
                    event.usage_event_id.to_string(),
                    event.request_id.as_str(),
                    event.api_key_id.to_string(),
                    event.user_id.map(|value| value.to_string()),
                    event.team_id.map(|value| value.to_string()),
                    event.model_id.map(|value| value.to_string()),
                    event.estimated_cost_usd.as_scaled_i64(),
                    event.occurred_at.unix_timestamp()
                ],
            )
            .await
            .map_err(|error| StoreError::Query(error.to_string()))?;
        Ok(())
    }
}
