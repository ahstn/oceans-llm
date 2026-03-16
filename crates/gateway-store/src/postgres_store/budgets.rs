use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};

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

    async fn get_active_budget_for_team(
        &self,
        team_id: Uuid,
    ) -> Result<Option<TeamBudgetRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone,
                   is_active, created_at, updated_at
            FROM team_budgets
            WHERE team_id = $1 AND is_active = 1
            LIMIT 1
            "#,
        )
        .bind(team_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_team_budget_record).transpose()
    }

    async fn upsert_active_budget_for_user(
        &self,
        user_id: Uuid,
        cadence: BudgetCadence,
        amount_usd: Money4,
        hard_limit: bool,
        timezone: &str,
        updated_at: OffsetDateTime,
    ) -> Result<UserBudgetRecord, StoreError> {
        let updated = sqlx::query(
            r#"
            UPDATE user_budgets
            SET cadence = $1,
                amount_10000 = $2,
                hard_limit = $3,
                timezone = $4,
                updated_at = $5
            WHERE user_id = $6
              AND is_active = 1
            "#,
        )
        .bind(cadence.as_str())
        .bind(amount_usd.as_scaled_i64())
        .bind(if hard_limit { 1_i64 } else { 0_i64 })
        .bind(timezone)
        .bind(updated_at.unix_timestamp())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?
        .rows_affected();

        if updated == 0 {
            sqlx::query(
                r#"
                INSERT INTO user_budgets (
                    user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone,
                    is_active, created_at, updated_at
                ) VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $8)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(user_id.to_string())
            .bind(cadence.as_str())
            .bind(amount_usd.as_scaled_i64())
            .bind(if hard_limit { 1_i64 } else { 0_i64 })
            .bind(timezone)
            .bind(updated_at.unix_timestamp())
            .bind(updated_at.unix_timestamp())
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
        }

        self.get_active_budget_for_user(user_id)
            .await?
            .ok_or_else(|| {
                StoreError::Unexpected(
                    "active user budget missing after successful upsert".to_string(),
                )
            })
    }

    async fn deactivate_active_budget_for_user(
        &self,
        user_id: Uuid,
        updated_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let updated = sqlx::query(
            r#"
            UPDATE user_budgets
            SET is_active = 0,
                updated_at = $1
            WHERE user_id = $2
              AND is_active = 1
            "#,
        )
        .bind(updated_at.unix_timestamp())
        .bind(user_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?
        .rows_affected();

        Ok(updated > 0)
    }

    async fn upsert_active_budget_for_team(
        &self,
        team_id: Uuid,
        cadence: BudgetCadence,
        amount_usd: Money4,
        hard_limit: bool,
        timezone: &str,
        updated_at: OffsetDateTime,
    ) -> Result<TeamBudgetRecord, StoreError> {
        let updated = sqlx::query(
            r#"
            UPDATE team_budgets
            SET cadence = $1,
                amount_10000 = $2,
                hard_limit = $3,
                timezone = $4,
                updated_at = $5
            WHERE team_id = $6
              AND is_active = 1
            "#,
        )
        .bind(cadence.as_str())
        .bind(amount_usd.as_scaled_i64())
        .bind(if hard_limit { 1_i64 } else { 0_i64 })
        .bind(timezone)
        .bind(updated_at.unix_timestamp())
        .bind(team_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?
        .rows_affected();

        if updated == 0 {
            sqlx::query(
                r#"
                INSERT INTO team_budgets (
                    team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone,
                    is_active, created_at, updated_at
                ) VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $8)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(team_id.to_string())
            .bind(cadence.as_str())
            .bind(amount_usd.as_scaled_i64())
            .bind(if hard_limit { 1_i64 } else { 0_i64 })
            .bind(timezone)
            .bind(updated_at.unix_timestamp())
            .bind(updated_at.unix_timestamp())
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?;
        }

        self.get_active_budget_for_team(team_id)
            .await?
            .ok_or_else(|| {
                StoreError::Unexpected(
                    "active team budget missing after successful upsert".to_string(),
                )
            })
    }

    async fn deactivate_active_budget_for_team(
        &self,
        team_id: Uuid,
        updated_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let updated = sqlx::query(
            r#"
            UPDATE team_budgets
            SET is_active = 0,
                updated_at = $1
            WHERE team_id = $2
              AND is_active = 1
            "#,
        )
        .bind(updated_at.unix_timestamp())
        .bind(team_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?
        .rows_affected();
        Ok(updated > 0)
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
        sum_usage_cost_for_owner_in_window(&self.pool, "user_id", user_id, window_start, window_end)
            .await
    }

    async fn sum_usage_cost_for_team_in_window(
        &self,
        team_id: Uuid,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError> {
        sum_usage_cost_for_owner_in_window(&self.pool, "team_id", team_id, window_start, window_end)
            .await
    }

    async fn list_usage_daily_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<ApiKeyOwnerKind>,
    ) -> Result<Vec<SpendDailyAggregateRecord>, StoreError> {
        let query = match owner_kind {
            Some(ApiKeyOwnerKind::User) => {
                r#"
                SELECT
                    (occurred_at / 86400) * 86400 AS day_start,
                    COALESCE(SUM(CASE WHEN pricing_status IN ('priced', 'legacy_estimated')
                        THEN computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                    SUM(CASE WHEN pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                        AS priced_request_count,
                    SUM(CASE WHEN pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                        AS unpriced_request_count,
                    SUM(CASE WHEN pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                        AS usage_missing_request_count
                FROM usage_cost_events
                WHERE occurred_at >= $1
                  AND occurred_at < $2
                  AND user_id IS NOT NULL
                GROUP BY day_start
                ORDER BY day_start ASC
                "#
            }
            Some(ApiKeyOwnerKind::Team) => {
                r#"
                SELECT
                    (occurred_at / 86400) * 86400 AS day_start,
                    COALESCE(SUM(CASE WHEN pricing_status IN ('priced', 'legacy_estimated')
                        THEN computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                    SUM(CASE WHEN pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                        AS priced_request_count,
                    SUM(CASE WHEN pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                        AS unpriced_request_count,
                    SUM(CASE WHEN pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                        AS usage_missing_request_count
                FROM usage_cost_events
                WHERE occurred_at >= $1
                  AND occurred_at < $2
                  AND team_id IS NOT NULL
                GROUP BY day_start
                ORDER BY day_start ASC
                "#
            }
            None => {
                r#"
                SELECT
                    (occurred_at / 86400) * 86400 AS day_start,
                    COALESCE(SUM(CASE WHEN pricing_status IN ('priced', 'legacy_estimated')
                        THEN computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                    SUM(CASE WHEN pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                        AS priced_request_count,
                    SUM(CASE WHEN pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                        AS unpriced_request_count,
                    SUM(CASE WHEN pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                        AS usage_missing_request_count
                FROM usage_cost_events
                WHERE occurred_at >= $1
                  AND occurred_at < $2
                GROUP BY day_start
                ORDER BY day_start ASC
                "#
            }
        };

        let rows = sqlx::query(query)
            .bind(window_start.unix_timestamp())
            .bind(window_end.unix_timestamp())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;

        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let day_start: i64 = row.try_get(0).map_err(to_query_error)?;
            output.push(SpendDailyAggregateRecord {
                day_start: unix_to_datetime(day_start)?,
                priced_cost_usd: Money4::from_scaled(
                    row.try_get::<i64, _>(1).map_err(to_query_error)?,
                ),
                priced_request_count: row.try_get(2).map_err(to_query_error)?,
                unpriced_request_count: row.try_get(3).map_err(to_query_error)?,
                usage_missing_request_count: row.try_get(4).map_err(to_query_error)?,
            });
        }
        Ok(output)
    }

    async fn list_usage_owner_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<ApiKeyOwnerKind>,
    ) -> Result<Vec<SpendOwnerAggregateRecord>, StoreError> {
        let query = match owner_kind {
            Some(ApiKeyOwnerKind::User) => {
                r#"
                SELECT
                    'user' AS owner_kind,
                    u.user_id AS owner_id,
                    users.name AS owner_name,
                    COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                        THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                        AS usage_missing_request_count
                FROM usage_cost_events u
                INNER JOIN users ON users.user_id = u.user_id
                WHERE u.occurred_at >= $1
                  AND u.occurred_at < $2
                  AND u.user_id IS NOT NULL
                GROUP BY u.user_id, users.name
                ORDER BY priced_cost_10000 DESC, owner_name ASC
                "#
            }
            Some(ApiKeyOwnerKind::Team) => {
                r#"
                SELECT
                    'team' AS owner_kind,
                    u.team_id AS owner_id,
                    teams.team_name AS owner_name,
                    COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                        THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                        AS usage_missing_request_count
                FROM usage_cost_events u
                INNER JOIN teams ON teams.team_id = u.team_id
                WHERE u.occurred_at >= $1
                  AND u.occurred_at < $2
                  AND u.team_id IS NOT NULL
                GROUP BY u.team_id, teams.team_name
                ORDER BY priced_cost_10000 DESC, owner_name ASC
                "#
            }
            None => {
                r#"
                SELECT * FROM (
                    SELECT
                        'user' AS owner_kind,
                        u.user_id AS owner_id,
                        users.name AS owner_name,
                        COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                            THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                        SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                            AS priced_request_count,
                        SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                            AS unpriced_request_count,
                        SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                            AS usage_missing_request_count
                    FROM usage_cost_events u
                    INNER JOIN users ON users.user_id = u.user_id
                    WHERE u.occurred_at >= $1
                      AND u.occurred_at < $2
                      AND u.user_id IS NOT NULL
                    GROUP BY u.user_id, users.name
                    UNION ALL
                    SELECT
                        'team' AS owner_kind,
                        u.team_id AS owner_id,
                        teams.team_name AS owner_name,
                        COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                            THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                        SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                            AS priced_request_count,
                        SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                            AS unpriced_request_count,
                        SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                            AS usage_missing_request_count
                    FROM usage_cost_events u
                    INNER JOIN teams ON teams.team_id = u.team_id
                    WHERE u.occurred_at >= $1
                      AND u.occurred_at < $2
                      AND u.team_id IS NOT NULL
                    GROUP BY u.team_id, teams.team_name
                ) owner_rollup
                ORDER BY priced_cost_10000 DESC, owner_name ASC
                "#
            }
        };

        let rows = sqlx::query(query)
            .bind(window_start.unix_timestamp())
            .bind(window_end.unix_timestamp())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;

        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let owner_kind: String = row.try_get(0).map_err(to_query_error)?;
            let owner_id: String = row.try_get(1).map_err(to_query_error)?;
            output.push(SpendOwnerAggregateRecord {
                owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
                    StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
                })?,
                owner_id: parse_uuid(&owner_id)?,
                owner_name: row.try_get(2).map_err(to_query_error)?,
                priced_cost_usd: Money4::from_scaled(
                    row.try_get::<i64, _>(3).map_err(to_query_error)?,
                ),
                priced_request_count: row.try_get(4).map_err(to_query_error)?,
                unpriced_request_count: row.try_get(5).map_err(to_query_error)?,
                usage_missing_request_count: row.try_get(6).map_err(to_query_error)?,
            });
        }
        Ok(output)
    }

    async fn list_usage_model_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<ApiKeyOwnerKind>,
    ) -> Result<Vec<SpendModelAggregateRecord>, StoreError> {
        let query = match owner_kind {
            Some(ApiKeyOwnerKind::User) => {
                r#"
                SELECT
                    COALESCE(g.model_key, u.upstream_model) AS model_key,
                    COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                        THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                        AS usage_missing_request_count
                FROM usage_cost_events u
                LEFT JOIN gateway_models g ON g.id = u.model_id
                WHERE u.occurred_at >= $1
                  AND u.occurred_at < $2
                  AND u.user_id IS NOT NULL
                GROUP BY COALESCE(g.model_key, u.upstream_model)
                ORDER BY priced_cost_10000 DESC, model_key ASC
                "#
            }
            Some(ApiKeyOwnerKind::Team) => {
                r#"
                SELECT
                    COALESCE(g.model_key, u.upstream_model) AS model_key,
                    COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                        THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                        AS usage_missing_request_count
                FROM usage_cost_events u
                LEFT JOIN gateway_models g ON g.id = u.model_id
                WHERE u.occurred_at >= $1
                  AND u.occurred_at < $2
                  AND u.team_id IS NOT NULL
                GROUP BY COALESCE(g.model_key, u.upstream_model)
                ORDER BY priced_cost_10000 DESC, model_key ASC
                "#
            }
            None => {
                r#"
                SELECT
                    COALESCE(g.model_key, u.upstream_model) AS model_key,
                    COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                        THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                        AS usage_missing_request_count
                FROM usage_cost_events u
                LEFT JOIN gateway_models g ON g.id = u.model_id
                WHERE u.occurred_at >= $1
                  AND u.occurred_at < $2
                GROUP BY COALESCE(g.model_key, u.upstream_model)
                ORDER BY priced_cost_10000 DESC, model_key ASC
                "#
            }
        };

        let rows = sqlx::query(query)
            .bind(window_start.unix_timestamp())
            .bind(window_end.unix_timestamp())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?;

        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(SpendModelAggregateRecord {
                model_key: row.try_get(0).map_err(to_query_error)?,
                priced_cost_usd: Money4::from_scaled(
                    row.try_get::<i64, _>(1).map_err(to_query_error)?,
                ),
                priced_request_count: row.try_get(2).map_err(to_query_error)?,
                unpriced_request_count: row.try_get(3).map_err(to_query_error)?,
                usage_missing_request_count: row.try_get(4).map_err(to_query_error)?,
            });
        }
        Ok(output)
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

async fn sum_usage_cost_for_owner_in_window(
    pool: &PgPool,
    owner_column: &str,
    owner_id: Uuid,
    window_start: OffsetDateTime,
    window_end: OffsetDateTime,
) -> Result<Money4, StoreError> {
    let query = format!(
        r#"
        SELECT COALESCE(SUM(computed_cost_10000), 0)::BIGINT
        FROM usage_cost_events
        WHERE {owner_column} = $1
          AND pricing_status IN ('priced', 'legacy_estimated')
          AND occurred_at >= $2
          AND occurred_at < $3
        "#
    );

    let row = sqlx::query(query.as_str())
        .bind(owner_id.to_string())
        .bind(window_start.unix_timestamp())
        .bind(window_end.unix_timestamp())
        .fetch_one(pool)
        .await
        .map_err(to_query_error)?;

    Ok(Money4::from_scaled(
        row.try_get::<i64, _>(0).map_err(to_query_error)?,
    ))
}
