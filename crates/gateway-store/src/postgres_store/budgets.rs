use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};

#[async_trait]
impl BudgetRepository for PostgresStore {
    async fn get_active_budget_by_scope(
        &self,
        scope: &BudgetScope,
    ) -> Result<Option<BudgetRecord>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT budget_id, scope_kind, scope_key, user_id, service_account_id, model_id,
                   upstream_model, cadence, amount_10000, hard_limit, timezone, is_active,
                   created_at, updated_at
            FROM budgets
            WHERE scope_key = $1
              AND is_active = 1
            LIMIT 1
            "#,
        )
        .bind(scope.scope_key())
        .fetch_optional(&self.pool)
        .await
        .map_err(to_query_error)?;

        row.as_ref().map(decode_budget_record).transpose()
    }

    async fn list_active_budgets(
        &self,
        scope_kind: Option<BudgetScopeKind>,
    ) -> Result<Vec<BudgetRecord>, StoreError> {
        let rows = if let Some(scope_kind) = scope_kind {
            sqlx::query(
                r#"
                SELECT budget_id, scope_kind, scope_key, user_id, service_account_id, model_id,
                       upstream_model, cadence, amount_10000, hard_limit, timezone, is_active,
                       created_at, updated_at
                FROM budgets
                WHERE scope_kind = $1
                  AND is_active = 1
                ORDER BY updated_at DESC, scope_key ASC
                "#,
            )
            .bind(scope_kind.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?
        } else {
            sqlx::query(
                r#"
                SELECT budget_id, scope_kind, scope_key, user_id, service_account_id, model_id,
                       upstream_model, cadence, amount_10000, hard_limit, timezone, is_active,
                       created_at, updated_at
                FROM budgets
                WHERE is_active = 1
                ORDER BY updated_at DESC, scope_key ASC
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(to_query_error)?
        };
        rows.iter().map(decode_budget_record).collect()
    }

    async fn upsert_active_budget(
        &self,
        scope: &BudgetScope,
        settings: &BudgetSettings,
        updated_at: OffsetDateTime,
    ) -> Result<BudgetRecord, StoreError> {
        sqlx::query(
            r#"
            INSERT INTO budgets (
                budget_id, scope_kind, scope_key, user_id, service_account_id, model_id,
                upstream_model, cadence, amount_10000, hard_limit, timezone, is_active,
                created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 1, $12, $13)
            ON CONFLICT (scope_key) WHERE is_active = 1
            DO UPDATE SET
                cadence = excluded.cadence,
                amount_10000 = excluded.amount_10000,
                hard_limit = excluded.hard_limit,
                timezone = excluded.timezone,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(scope.kind().as_str())
        .bind(scope.scope_key())
        .bind(scope.user_id().map(|id| id.to_string()))
        .bind(scope.service_account_id().map(|id| id.to_string()))
        .bind(scope.model_id().map(|id| id.to_string()))
        .bind(scope.upstream_model().map(ToOwned::to_owned))
        .bind(settings.cadence.as_str())
        .bind(settings.amount_usd.as_scaled_i64())
        .bind(if settings.hard_limit { 1_i64 } else { 0_i64 })
        .bind(&settings.timezone)
        .bind(updated_at.unix_timestamp())
        .bind(updated_at.unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(to_query_error)?;

        self.get_active_budget_by_scope(scope)
            .await?
            .ok_or_else(|| {
                StoreError::Unexpected("active budget missing after successful upsert".to_string())
            })
    }

    async fn deactivate_active_budget(
        &self,
        scope: &BudgetScope,
        updated_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        let updated = sqlx::query(
            r#"
            UPDATE budgets
            SET is_active = 0,
                updated_at = $1
            WHERE scope_key = $2
              AND is_active = 1
            "#,
        )
        .bind(updated_at.unix_timestamp())
        .bind(scope.scope_key())
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
                team_id, service_account_id, actor_user_id, model_id, provider_key, upstream_model,
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

    async fn sum_usage_cost_for_budget_scope_in_window(
        &self,
        scope: &BudgetScope,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError> {
        sum_usage_cost_for_budget_scope(&self.pool, scope, window_start, window_end).await
    }

    async fn count_active_api_keys_for_service_account(
        &self,
        service_account_id: Uuid,
    ) -> Result<u64, StoreError> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM api_keys
            WHERE owner_kind = 'service_account'
              AND owner_service_account_id = $1
              AND status = 'active'
              AND revoked_at IS NULL
            "#,
        )
        .bind(service_account_id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(to_query_error)?;
        u64::try_from(count).map_err(|error| StoreError::Serialization(error.to_string()))
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
            Some(ApiKeyOwnerKind::ServiceAccount) => {
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
                  AND service_account_id IS NOT NULL
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
            Some(ApiKeyOwnerKind::ServiceAccount) => {
                r#"
                SELECT
                    'service_account' AS owner_kind,
                    u.service_account_id AS owner_id,
                    service_accounts.service_account_name AS owner_name,
                    COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                        THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                        AS usage_missing_request_count
                FROM usage_cost_events u
                INNER JOIN service_accounts ON service_accounts.service_account_id = u.service_account_id
                WHERE u.occurred_at >= $1
                  AND u.occurred_at < $2
                  AND u.service_account_id IS NOT NULL
                GROUP BY u.service_account_id, service_accounts.service_account_name
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
                        'service_account' AS owner_kind,
                        u.service_account_id AS owner_id,
                        service_accounts.service_account_name AS owner_name,
                        COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                            THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000,
                        SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)::BIGINT
                            AS priced_request_count,
                        SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)::BIGINT
                            AS unpriced_request_count,
                        SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)::BIGINT
                            AS usage_missing_request_count
                    FROM usage_cost_events u
                    INNER JOIN service_accounts ON service_accounts.service_account_id = u.service_account_id
                    WHERE u.occurred_at >= $1
                      AND u.occurred_at < $2
                      AND u.service_account_id IS NOT NULL
                    GROUP BY u.service_account_id, service_accounts.service_account_name
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
            Some(ApiKeyOwnerKind::ServiceAccount) => {
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
                  AND u.service_account_id IS NOT NULL
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

    async fn list_focus_export_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<ApiKeyOwnerKind>,
        owner_user_id: Option<Uuid>,
    ) -> Result<Vec<FocusExportAggregateRecord>, StoreError> {
        let owner_kind_filter = owner_kind.map(|kind| kind.as_str().to_string());
        let owner_user_filter = owner_user_id.map(|id| id.to_string());
        let rows = sqlx::query(
            r#"
            SELECT * FROM (
                SELECT
                    ((u.occurred_at / 86400) * 86400)::BIGINT AS day_start,
                    'user' AS owner_kind,
                    u.user_id::TEXT AS owner_id,
                    users.name AS owner_name,
                    users.tags_json AS owner_tags_json,
                    u.model_id::TEXT AS model_id,
                    COALESCE(g.model_key, u.upstream_model) AS model_key,
                    u.provider_key,
                    u.upstream_model,
                    u.pricing_status,
                    u.pricing_row_id::TEXT AS pricing_row_id,
                    COALESCE(SUM(u.prompt_tokens), 0)::BIGINT AS prompt_tokens,
                    COALESCE(SUM(u.completion_tokens), 0)::BIGINT AS completion_tokens,
                    COALESCE(SUM(u.total_tokens), 0)::BIGINT AS total_tokens,
                    COUNT(*)::BIGINT AS request_count,
                    COALESCE(SUM(u.computed_cost_10000), 0)::BIGINT AS computed_cost_10000
                FROM usage_cost_events u
                INNER JOIN users ON users.user_id = u.user_id
                LEFT JOIN gateway_models g ON g.id = u.model_id
                WHERE u.occurred_at >= $1
                  AND u.occurred_at < $2
                  AND u.user_id IS NOT NULL
                  AND u.ownership_scope_key LIKE 'user:%'
                  AND u.pricing_status IN ('priced', 'legacy_estimated')
                GROUP BY day_start, u.user_id, users.name, users.tags_json, u.model_id, model_key,
                         u.provider_key, u.upstream_model, u.pricing_status, u.pricing_row_id
                UNION ALL
                SELECT
                    ((u.occurred_at / 86400) * 86400)::BIGINT AS day_start,
                    'service_account' AS owner_kind,
                    u.service_account_id::TEXT AS owner_id,
                    service_accounts.service_account_name AS owner_name,
                    teams.tags_json AS owner_tags_json,
                    u.model_id::TEXT AS model_id,
                    COALESCE(g.model_key, u.upstream_model) AS model_key,
                    u.provider_key,
                    u.upstream_model,
                    u.pricing_status,
                    u.pricing_row_id::TEXT AS pricing_row_id,
                    COALESCE(SUM(u.prompt_tokens), 0)::BIGINT AS prompt_tokens,
                    COALESCE(SUM(u.completion_tokens), 0)::BIGINT AS completion_tokens,
                    COALESCE(SUM(u.total_tokens), 0)::BIGINT AS total_tokens,
                    COUNT(*)::BIGINT AS request_count,
                    COALESCE(SUM(u.computed_cost_10000), 0)::BIGINT AS computed_cost_10000
                FROM usage_cost_events u
                INNER JOIN service_accounts ON service_accounts.service_account_id = u.service_account_id
                INNER JOIN teams ON teams.team_id = service_accounts.team_id
                LEFT JOIN gateway_models g ON g.id = u.model_id
                WHERE u.occurred_at >= $1
                  AND u.occurred_at < $2
                  AND u.service_account_id IS NOT NULL
                  AND u.ownership_scope_key LIKE 'service_account:%'
                  AND u.pricing_status IN ('priced', 'legacy_estimated')
                GROUP BY day_start, u.service_account_id, service_accounts.service_account_name,
                         teams.tags_json, u.model_id, model_key, u.provider_key, u.upstream_model,
                         u.pricing_status, u.pricing_row_id
            ) focus_rows
            WHERE ($3 IS NULL OR owner_kind = $3)
              AND ($4 IS NULL OR (owner_kind = 'user' AND owner_id = $4))
            ORDER BY day_start ASC, owner_kind ASC, owner_name ASC, owner_id ASC,
                     provider_key ASC, upstream_model ASC, pricing_status ASC,
                     model_key ASC, COALESCE(model_id, '') ASC,
                     COALESCE(pricing_row_id, '') ASC
            "#,
        )
        .bind(window_start.unix_timestamp())
        .bind(window_end.unix_timestamp())
        .bind(owner_kind_filter)
        .bind(owner_user_filter)
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let owner_kind: String = row.try_get(1).map_err(to_query_error)?;
            let owner_id: String = row.try_get(2).map_err(to_query_error)?;
            let owner_tags_json: String = row.try_get(4).map_err(to_query_error)?;
            let model_id: Option<String> = row.try_get(5).map_err(to_query_error)?;
            let pricing_status: String = row.try_get(9).map_err(to_query_error)?;
            let pricing_row_id: Option<String> = row.try_get(10).map_err(to_query_error)?;
            output.push(FocusExportAggregateRecord {
                day_start: unix_to_datetime(row.try_get::<i64, _>(0).map_err(to_query_error)?)?,
                owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
                    StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
                })?,
                owner_id: parse_uuid(&owner_id)?,
                owner_name: row.try_get(3).map_err(to_query_error)?,
                owner_tags: serde_json::from_str(&owner_tags_json).map_err(|error| {
                    StoreError::Serialization(format!("invalid owner tags json: {error}"))
                })?,
                model_id: model_id.as_deref().map(parse_uuid).transpose()?,
                model_key: row.try_get(6).map_err(to_query_error)?,
                provider_key: row.try_get(7).map_err(to_query_error)?,
                upstream_model: row.try_get(8).map_err(to_query_error)?,
                pricing_status: UsagePricingStatus::from_db(&pricing_status).ok_or_else(|| {
                    StoreError::Serialization(format!("unknown pricing status `{pricing_status}`"))
                })?,
                pricing_row_id: pricing_row_id.as_deref().map(parse_uuid).transpose()?,
                prompt_tokens: row.try_get(11).map_err(to_query_error)?,
                completion_tokens: row.try_get(12).map_err(to_query_error)?,
                total_tokens: row.try_get(13).map_err(to_query_error)?,
                request_count: row.try_get(14).map_err(to_query_error)?,
                computed_cost_usd: Money4::from_scaled(row.try_get(15).map_err(to_query_error)?),
            });
        }
        Ok(output)
    }

    async fn get_focus_export_diagnostics(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        owner_kind: Option<ApiKeyOwnerKind>,
        owner_user_id: Option<Uuid>,
    ) -> Result<FocusExportDiagnosticsRecord, StoreError> {
        let owner_kind_filter = owner_kind.map(|kind| kind.as_str().to_string());
        let owner_user_filter = owner_user_id.map(|id| id.to_string());
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN pricing_status = 'unpriced' THEN 1 ELSE 0 END), 0)::BIGINT,
                COALESCE(SUM(CASE WHEN pricing_status = 'usage_missing' THEN 1 ELSE 0 END), 0)::BIGINT
            FROM (
                SELECT 'user' AS owner_kind, user_id::TEXT AS owner_id, pricing_status
                FROM usage_cost_events
                WHERE occurred_at >= $1 AND occurred_at < $2 AND user_id IS NOT NULL
                  AND ownership_scope_key LIKE 'user:%'
                UNION ALL
                SELECT 'service_account' AS owner_kind, service_account_id::TEXT AS owner_id, pricing_status
                FROM usage_cost_events
                WHERE occurred_at >= $1 AND occurred_at < $2 AND service_account_id IS NOT NULL
                  AND ownership_scope_key LIKE 'service_account:%'
            ) focus_diagnostics
            WHERE ($3 IS NULL OR owner_kind = $3)
              AND ($4 IS NULL OR (owner_kind = 'user' AND owner_id = $4))
            "#,
        )
        .bind(window_start.unix_timestamp())
        .bind(window_end.unix_timestamp())
        .bind(owner_kind_filter)
        .bind(owner_user_filter)
        .fetch_one(&self.pool)
        .await
        .map_err(to_query_error)?;

        Ok(FocusExportDiagnosticsRecord {
            unpriced_request_count: row.try_get(0).map_err(to_query_error)?,
            usage_missing_request_count: row.try_get(1).map_err(to_query_error)?,
        })
    }

    async fn list_usage_user_leaderboard(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<UsageLeaderboardUserRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            WITH user_totals AS (
                SELECT
                    u.user_id AS user_id,
                    users.name AS user_name,
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
            ),
            model_totals AS (
                SELECT
                    u.user_id AS user_id,
                    COALESCE(g.model_key, u.upstream_model) AS model_key,
                    COUNT(*)::BIGINT AS request_count,
                    COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                        THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000
                FROM usage_cost_events u
                LEFT JOIN gateway_models g ON g.id = u.model_id
                WHERE u.occurred_at >= $1
                  AND u.occurred_at < $2
                  AND u.user_id IS NOT NULL
                GROUP BY u.user_id, COALESCE(g.model_key, u.upstream_model)
            ),
            ranked_models AS (
                SELECT
                    user_id,
                    model_key,
                    ROW_NUMBER() OVER (
                        PARTITION BY user_id
                        ORDER BY request_count DESC, priced_cost_10000 DESC, model_key ASC
                    ) AS model_rank
                FROM model_totals
            ),
            tool_totals AS (
                SELECT
                    user_id,
                    AVG(referenced_mcp_server_count)::DOUBLE PRECISION AS avg_referenced_mcp_server_count,
                    AVG(exposed_tool_count)::DOUBLE PRECISION AS avg_exposed_tool_count,
                    AVG(invoked_tool_count)::DOUBLE PRECISION AS avg_invoked_tool_count,
                    AVG(filtered_tool_count)::DOUBLE PRECISION AS avg_filtered_tool_count
                FROM request_logs
                WHERE occurred_at >= $1
                  AND occurred_at < $2
                  AND user_id IS NOT NULL
                GROUP BY user_id
            )
            SELECT
                user_totals.user_id,
                user_totals.user_name,
                user_totals.priced_cost_10000,
                (
                    user_totals.priced_request_count
                    + user_totals.unpriced_request_count
                    + user_totals.usage_missing_request_count
                )::BIGINT AS total_request_count,
                ranked_models.model_key,
                tool_totals.avg_referenced_mcp_server_count,
                tool_totals.avg_exposed_tool_count,
                tool_totals.avg_invoked_tool_count,
                tool_totals.avg_filtered_tool_count
            FROM user_totals
            LEFT JOIN ranked_models
                ON ranked_models.user_id = user_totals.user_id
               AND ranked_models.model_rank = 1
            LEFT JOIN tool_totals
                ON tool_totals.user_id = user_totals.user_id
            ORDER BY
                user_totals.priced_cost_10000 DESC,
                total_request_count DESC,
                user_totals.user_name ASC,
                user_totals.user_id ASC
            LIMIT $3
            "#,
        )
        .bind(window_start.unix_timestamp())
        .bind(window_end.unix_timestamp())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(UsageLeaderboardUserRecord {
                user_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
                user_name: row.try_get(1).map_err(to_query_error)?,
                priced_cost_usd: Money4::from_scaled(
                    row.try_get::<i64, _>(2).map_err(to_query_error)?,
                ),
                total_request_count: row.try_get(3).map_err(to_query_error)?,
                top_model_key: row.try_get(4).map_err(to_query_error)?,
                tool_cardinality_averages: gateway_core::RequestToolCardinalityAverages {
                    referenced_mcp_server_count: row.try_get(5).map_err(to_query_error)?,
                    exposed_tool_count: row.try_get(6).map_err(to_query_error)?,
                    invoked_tool_count: row.try_get(7).map_err(to_query_error)?,
                    filtered_tool_count: row.try_get(8).map_err(to_query_error)?,
                },
            });
        }

        Ok(output)
    }

    async fn list_usage_user_bucket_aggregates(
        &self,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
        bucket_hours: u8,
        user_ids: &[Uuid],
    ) -> Result<Vec<UsageLeaderboardBucketRecord>, StoreError> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }

        let bucket_seconds = i64::from(bucket_hours) * 60 * 60;
        let user_ids_json =
            serde_json::to_string(&user_ids.iter().map(Uuid::to_string).collect::<Vec<_>>())
                .map_err(|error| StoreError::Serialization(error.to_string()))?;

        let rows = sqlx::query(
            r#"
            WITH selected_users AS (
                SELECT jsonb_array_elements_text($4::jsonb) AS user_id
            )
            SELECT
                u.user_id,
                (u.occurred_at / $3) * $3 AS bucket_start,
                COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                    THEN u.computed_cost_10000 ELSE 0 END), 0)::BIGINT AS priced_cost_10000
            FROM usage_cost_events u
            INNER JOIN selected_users ON selected_users.user_id = u.user_id
            WHERE u.occurred_at >= $1
              AND u.occurred_at < $2
            GROUP BY u.user_id, bucket_start
            ORDER BY bucket_start ASC, u.user_id ASC
            "#,
        )
        .bind(window_start.unix_timestamp())
        .bind(window_end.unix_timestamp())
        .bind(bucket_seconds)
        .bind(user_ids_json)
        .fetch_all(&self.pool)
        .await
        .map_err(to_query_error)?;

        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(UsageLeaderboardBucketRecord {
                user_id: parse_uuid(&row.try_get::<String, _>(0).map_err(to_query_error)?)?,
                bucket_start: unix_to_datetime(row.try_get::<i64, _>(1).map_err(to_query_error)?)?,
                priced_cost_usd: Money4::from_scaled(
                    row.try_get::<i64, _>(2).map_err(to_query_error)?,
                ),
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
                team_id, service_account_id, actor_user_id, model_id, provider_key, upstream_model,
                prompt_tokens, completion_tokens, total_tokens, provider_usage_json,
                pricing_status, unpriced_reason, pricing_row_id, pricing_provider_id,
                pricing_model_id, pricing_source, pricing_source_etag,
                pricing_source_fetched_at, pricing_last_updated,
                input_cost_per_million_tokens_10000,
                output_cost_per_million_tokens_10000, computed_cost_10000, occurred_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
                $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28
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
        .bind(event.service_account_id.map(|value| value.to_string()))
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

    async fn delete_usage_ledger_events_by_request_ids(
        &self,
        request_ids: &[String],
    ) -> Result<u64, StoreError> {
        if request_ids.is_empty() {
            return Ok(0);
        }

        let deleted = sqlx::query("DELETE FROM usage_cost_events WHERE request_id = ANY($1)")
            .bind(request_ids)
            .execute(&self.pool)
            .await
            .map_err(to_query_error)?
            .rows_affected();

        Ok(deleted)
    }
}

async fn sum_usage_cost_for_budget_scope(
    pool: &PgPool,
    scope: &BudgetScope,
    window_start: OffsetDateTime,
    window_end: OffsetDateTime,
) -> Result<Money4, StoreError> {
    let (predicate, owner_id, extra_value) = match scope {
        BudgetScope::User { user_id } => ("user_id = $1", user_id.to_string(), None),
        BudgetScope::ServiceAccount { service_account_id } => (
            "service_account_id = $1",
            service_account_id.to_string(),
            None,
        ),
        BudgetScope::UserModel {
            user_id,
            selector: BudgetModelSelector::Model { model_id },
        } => (
            "user_id = $1 AND model_id = $4",
            user_id.to_string(),
            Some(model_id.to_string()),
        ),
        BudgetScope::UserModel {
            user_id,
            selector: BudgetModelSelector::UpstreamModel { upstream_model },
        } => (
            "user_id = $1 AND model_id IS NULL AND TRIM(upstream_model) = $4",
            user_id.to_string(),
            Some(upstream_model.trim().to_string()),
        ),
    };

    let query = format!(
        "SELECT COALESCE(SUM(computed_cost_10000), 0)::BIGINT
         FROM usage_cost_events
         WHERE {predicate}
           AND pricing_status IN ('priced', 'legacy_estimated')
           AND occurred_at >= $2
           AND occurred_at < $3"
    );

    let mut query = sqlx::query(query.as_str())
        .bind(owner_id)
        .bind(window_start.unix_timestamp())
        .bind(window_end.unix_timestamp());
    if let Some(extra_value) = extra_value {
        query = query.bind(extra_value);
    }
    let row = query.fetch_one(pool).await.map_err(to_query_error)?;

    Ok(Money4::from_scaled(
        row.try_get::<i64, _>(0).map_err(to_query_error)?,
    ))
}
