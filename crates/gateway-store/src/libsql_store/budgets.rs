use super::*;
use crate::shared::{parse_uuid, unix_to_datetime};

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

    async fn get_active_budget_for_team(
        &self,
        team_id: Uuid,
    ) -> Result<Option<TeamBudgetRecord>, StoreError> {
        let mut rows = self
            .connection
            .query(
                r#"
                SELECT team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone,
                       is_active, created_at, updated_at
                FROM team_budgets
                WHERE team_id = ?1 AND is_active = 1
                LIMIT 1
                "#,
                [team_id.to_string()],
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

        decode_team_budget_record(&row).map(Some)
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
        let updated = self
            .connection
            .execute(
                r#"
                UPDATE user_budgets
                SET cadence = ?1,
                    amount_10000 = ?2,
                    hard_limit = ?3,
                    timezone = ?4,
                    updated_at = ?5
                WHERE user_id = ?6
                  AND is_active = 1
                "#,
                libsql::params![
                    cadence.as_str(),
                    amount_usd.as_scaled_i64(),
                    if hard_limit { 1 } else { 0 },
                    timezone,
                    updated_at.unix_timestamp(),
                    user_id.to_string()
                ],
            )
            .await
            .map_err(to_query_error)?;

        if updated == 0 {
            self.connection
                .execute(
                    r#"
                    INSERT INTO user_budgets (
                        user_budget_id, user_id, cadence, amount_10000, hard_limit, timezone,
                        is_active, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8)
                    "#,
                    libsql::params![
                        Uuid::new_v4().to_string(),
                        user_id.to_string(),
                        cadence.as_str(),
                        amount_usd.as_scaled_i64(),
                        if hard_limit { 1 } else { 0 },
                        timezone,
                        updated_at.unix_timestamp(),
                        updated_at.unix_timestamp(),
                    ],
                )
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
        let updated = self
            .connection
            .execute(
                r#"
                UPDATE user_budgets
                SET is_active = 0,
                    updated_at = ?1
                WHERE user_id = ?2
                  AND is_active = 1
                "#,
                libsql::params![updated_at.unix_timestamp(), user_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
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
        let updated = self
            .connection
            .execute(
                r#"
                UPDATE team_budgets
                SET cadence = ?1,
                    amount_10000 = ?2,
                    hard_limit = ?3,
                    timezone = ?4,
                    updated_at = ?5
                WHERE team_id = ?6
                  AND is_active = 1
                "#,
                libsql::params![
                    cadence.as_str(),
                    amount_usd.as_scaled_i64(),
                    if hard_limit { 1 } else { 0 },
                    timezone,
                    updated_at.unix_timestamp(),
                    team_id.to_string()
                ],
            )
            .await
            .map_err(to_query_error)?;

        if updated == 0 {
            self.connection
                .execute(
                    r#"
                    INSERT INTO team_budgets (
                        team_budget_id, team_id, cadence, amount_10000, hard_limit, timezone,
                        is_active, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8)
                    "#,
                    libsql::params![
                        Uuid::new_v4().to_string(),
                        team_id.to_string(),
                        cadence.as_str(),
                        amount_usd.as_scaled_i64(),
                        if hard_limit { 1 } else { 0 },
                        timezone,
                        updated_at.unix_timestamp(),
                        updated_at.unix_timestamp(),
                    ],
                )
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
        let updated = self
            .connection
            .execute(
                r#"
                UPDATE team_budgets
                SET is_active = 0,
                    updated_at = ?1
                WHERE team_id = ?2
                  AND is_active = 1
                "#,
                libsql::params![updated_at.unix_timestamp(), team_id.to_string()],
            )
            .await
            .map_err(to_query_error)?;
        Ok(updated > 0)
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
        sum_usage_cost_for_owner_in_window(
            &self.connection,
            "user_id",
            user_id,
            window_start,
            window_end,
        )
        .await
    }

    async fn sum_usage_cost_for_team_in_window(
        &self,
        team_id: Uuid,
        window_start: OffsetDateTime,
        window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError> {
        sum_usage_cost_for_owner_in_window(
            &self.connection,
            "team_id",
            team_id,
            window_start,
            window_end,
        )
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
                        THEN computed_cost_10000 ELSE 0 END), 0) AS priced_cost_10000,
                    SUM(CASE WHEN pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)
                        AS priced_request_count,
                    SUM(CASE WHEN pricing_status = 'unpriced' THEN 1 ELSE 0 END)
                        AS unpriced_request_count,
                    SUM(CASE WHEN pricing_status = 'usage_missing' THEN 1 ELSE 0 END)
                        AS usage_missing_request_count
                FROM usage_cost_events
                WHERE occurred_at >= ?1
                  AND occurred_at < ?2
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
                        THEN computed_cost_10000 ELSE 0 END), 0) AS priced_cost_10000,
                    SUM(CASE WHEN pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)
                        AS priced_request_count,
                    SUM(CASE WHEN pricing_status = 'unpriced' THEN 1 ELSE 0 END)
                        AS unpriced_request_count,
                    SUM(CASE WHEN pricing_status = 'usage_missing' THEN 1 ELSE 0 END)
                        AS usage_missing_request_count
                FROM usage_cost_events
                WHERE occurred_at >= ?1
                  AND occurred_at < ?2
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
                        THEN computed_cost_10000 ELSE 0 END), 0) AS priced_cost_10000,
                    SUM(CASE WHEN pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)
                        AS priced_request_count,
                    SUM(CASE WHEN pricing_status = 'unpriced' THEN 1 ELSE 0 END)
                        AS unpriced_request_count,
                    SUM(CASE WHEN pricing_status = 'usage_missing' THEN 1 ELSE 0 END)
                        AS usage_missing_request_count
                FROM usage_cost_events
                WHERE occurred_at >= ?1
                  AND occurred_at < ?2
                GROUP BY day_start
                ORDER BY day_start ASC
                "#
            }
        };

        let mut rows = self
            .connection
            .query(
                query,
                libsql::params![window_start.unix_timestamp(), window_end.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;

        let mut output = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            let day_start: i64 = row.get(0).map_err(to_query_error)?;
            let priced_cost_10000: i64 = row.get(1).map_err(to_query_error)?;
            let priced_request_count: i64 = row.get(2).map_err(to_query_error)?;
            let unpriced_request_count: i64 = row.get(3).map_err(to_query_error)?;
            let usage_missing_request_count: i64 = row.get(4).map_err(to_query_error)?;
            output.push(SpendDailyAggregateRecord {
                day_start: unix_to_datetime(day_start)?,
                priced_cost_usd: Money4::from_scaled(priced_cost_10000),
                priced_request_count,
                unpriced_request_count,
                usage_missing_request_count,
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
                        THEN u.computed_cost_10000 ELSE 0 END), 0) AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)
                        AS usage_missing_request_count
                FROM usage_cost_events u
                INNER JOIN users ON users.user_id = u.user_id
                WHERE u.occurred_at >= ?1
                  AND u.occurred_at < ?2
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
                        THEN u.computed_cost_10000 ELSE 0 END), 0) AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)
                        AS usage_missing_request_count
                FROM usage_cost_events u
                INNER JOIN teams ON teams.team_id = u.team_id
                WHERE u.occurred_at >= ?1
                  AND u.occurred_at < ?2
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
                            THEN u.computed_cost_10000 ELSE 0 END), 0) AS priced_cost_10000,
                        SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)
                            AS priced_request_count,
                        SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)
                            AS unpriced_request_count,
                        SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)
                            AS usage_missing_request_count
                    FROM usage_cost_events u
                    INNER JOIN users ON users.user_id = u.user_id
                    WHERE u.occurred_at >= ?1
                      AND u.occurred_at < ?2
                      AND u.user_id IS NOT NULL
                    GROUP BY u.user_id, users.name
                    UNION ALL
                    SELECT
                        'team' AS owner_kind,
                        u.team_id AS owner_id,
                        teams.team_name AS owner_name,
                        COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                            THEN u.computed_cost_10000 ELSE 0 END), 0) AS priced_cost_10000,
                        SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)
                            AS priced_request_count,
                        SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)
                            AS unpriced_request_count,
                        SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)
                            AS usage_missing_request_count
                    FROM usage_cost_events u
                    INNER JOIN teams ON teams.team_id = u.team_id
                    WHERE u.occurred_at >= ?1
                      AND u.occurred_at < ?2
                      AND u.team_id IS NOT NULL
                    GROUP BY u.team_id, teams.team_name
                )
                ORDER BY priced_cost_10000 DESC, owner_name ASC
                "#
            }
        };

        let mut rows = self
            .connection
            .query(
                query,
                libsql::params![window_start.unix_timestamp(), window_end.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;

        let mut output = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            let owner_kind: String = row.get(0).map_err(to_query_error)?;
            let owner_id: String = row.get(1).map_err(to_query_error)?;
            let priced_cost_10000: i64 = row.get(3).map_err(to_query_error)?;
            let priced_request_count: i64 = row.get(4).map_err(to_query_error)?;
            let unpriced_request_count: i64 = row.get(5).map_err(to_query_error)?;
            let usage_missing_request_count: i64 = row.get(6).map_err(to_query_error)?;
            output.push(SpendOwnerAggregateRecord {
                owner_kind: ApiKeyOwnerKind::from_db(&owner_kind).ok_or_else(|| {
                    StoreError::Serialization(format!("unknown owner kind `{owner_kind}`"))
                })?,
                owner_id: parse_uuid(&owner_id)?,
                owner_name: row.get(2).map_err(to_query_error)?,
                priced_cost_usd: Money4::from_scaled(priced_cost_10000),
                priced_request_count,
                unpriced_request_count,
                usage_missing_request_count,
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
                        THEN u.computed_cost_10000 ELSE 0 END), 0) AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)
                        AS usage_missing_request_count
                FROM usage_cost_events u
                LEFT JOIN gateway_models g ON g.id = u.model_id
                WHERE u.occurred_at >= ?1
                  AND u.occurred_at < ?2
                  AND u.user_id IS NOT NULL
                GROUP BY model_key
                ORDER BY priced_cost_10000 DESC, model_key ASC
                "#
            }
            Some(ApiKeyOwnerKind::Team) => {
                r#"
                SELECT
                    COALESCE(g.model_key, u.upstream_model) AS model_key,
                    COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                        THEN u.computed_cost_10000 ELSE 0 END), 0) AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)
                        AS usage_missing_request_count
                FROM usage_cost_events u
                LEFT JOIN gateway_models g ON g.id = u.model_id
                WHERE u.occurred_at >= ?1
                  AND u.occurred_at < ?2
                  AND u.team_id IS NOT NULL
                GROUP BY model_key
                ORDER BY priced_cost_10000 DESC, model_key ASC
                "#
            }
            None => {
                r#"
                SELECT
                    COALESCE(g.model_key, u.upstream_model) AS model_key,
                    COALESCE(SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated')
                        THEN u.computed_cost_10000 ELSE 0 END), 0) AS priced_cost_10000,
                    SUM(CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') THEN 1 ELSE 0 END)
                        AS priced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'unpriced' THEN 1 ELSE 0 END)
                        AS unpriced_request_count,
                    SUM(CASE WHEN u.pricing_status = 'usage_missing' THEN 1 ELSE 0 END)
                        AS usage_missing_request_count
                FROM usage_cost_events u
                LEFT JOIN gateway_models g ON g.id = u.model_id
                WHERE u.occurred_at >= ?1
                  AND u.occurred_at < ?2
                GROUP BY model_key
                ORDER BY priced_cost_10000 DESC, model_key ASC
                "#
            }
        };

        let mut rows = self
            .connection
            .query(
                query,
                libsql::params![window_start.unix_timestamp(), window_end.unix_timestamp()],
            )
            .await
            .map_err(to_query_error)?;

        let mut output = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            let priced_cost_10000: i64 = row.get(1).map_err(to_query_error)?;
            let priced_request_count: i64 = row.get(2).map_err(to_query_error)?;
            let unpriced_request_count: i64 = row.get(3).map_err(to_query_error)?;
            let usage_missing_request_count: i64 = row.get(4).map_err(to_query_error)?;
            output.push(SpendModelAggregateRecord {
                model_key: row.get(0).map_err(to_query_error)?,
                priced_cost_usd: Money4::from_scaled(priced_cost_10000),
                priced_request_count,
                unpriced_request_count,
                usage_missing_request_count,
            });
        }
        Ok(output)
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

async fn sum_usage_cost_for_owner_in_window(
    connection: &libsql::Connection,
    owner_column: &str,
    owner_id: Uuid,
    window_start: OffsetDateTime,
    window_end: OffsetDateTime,
) -> Result<Money4, StoreError> {
    let query = format!(
        r#"
        SELECT COALESCE(SUM(computed_cost_10000), 0)
        FROM usage_cost_events
        WHERE {owner_column} = ?1
          AND pricing_status IN ('priced', 'legacy_estimated')
          AND occurred_at >= ?2
          AND occurred_at < ?3
        "#
    );

    let mut rows = connection
        .query(
            query.as_str(),
            libsql::params![
                owner_id.to_string(),
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
