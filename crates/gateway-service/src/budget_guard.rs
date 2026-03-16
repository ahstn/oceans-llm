use std::sync::Arc;

use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, BudgetCadence, BudgetRepository, GatewayError,
    UsageLedgerRecord,
};
use time::{Duration, OffsetDateTime, UtcOffset};

#[derive(Clone)]
pub struct BudgetGuard<R> {
    repo: Arc<R>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetGuardDisposition {
    Inserted,
    Duplicate,
}

impl<R> BudgetGuard<R>
where
    R: BudgetRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn enforce_pre_provider_budget(
        &self,
        api_key: &AuthenticatedApiKey,
        request_id: &str,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        let ownership_scope_key = ownership_scope_key(api_key)?;
        if self
            .repo
            .get_usage_ledger_by_request_and_scope(request_id, &ownership_scope_key)
            .await?
            .is_some()
        {
            return Ok(());
        }

        match api_key.owner_kind {
            ApiKeyOwnerKind::User => {
                let user_id = api_key.owner_user_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
                if let Some(budget) = self.repo.get_active_budget_for_user(user_id).await? {
                    let (window_start, window_end) =
                        budget_window_bounds_utc(budget.cadence, occurred_at)?;
                    let spent = self
                        .repo
                        .sum_usage_cost_for_user_in_window(user_id, window_start, window_end)
                        .await?;
                    if budget.hard_limit && spent >= budget.amount_usd {
                        return Err(GatewayError::BudgetExceeded {
                            ownership_scope: format!("user:{user_id}"),
                            projected_cost_usd: spent,
                            limit_usd: budget.amount_usd,
                        });
                    }
                }
            }
            ApiKeyOwnerKind::Team => {
                let team_id = api_key.owner_team_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
                if let Some(budget) = self.repo.get_active_budget_for_team(team_id).await? {
                    let (window_start, window_end) =
                        budget_window_bounds_utc(budget.cadence, occurred_at)?;
                    let spent = self
                        .repo
                        .sum_usage_cost_for_team_in_window(team_id, window_start, window_end)
                        .await?;
                    if budget.hard_limit && spent >= budget.amount_usd {
                        return Err(GatewayError::BudgetExceeded {
                            ownership_scope: format!("team:{team_id}:actor:none"),
                            projected_cost_usd: spent,
                            limit_usd: budget.amount_usd,
                        });
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn enforce_and_record_usage(
        &self,
        api_key: &AuthenticatedApiKey,
        ledger: &UsageLedgerRecord,
    ) -> Result<BudgetGuardDisposition, GatewayError> {
        if ledger.computed_cost_usd.is_negative() {
            return Err(GatewayError::InvalidRequest(
                "computed_cost_usd must be >= 0".to_string(),
            ));
        }

        if self
            .repo
            .get_usage_ledger_by_request_and_scope(&ledger.request_id, &ledger.ownership_scope_key)
            .await?
            .is_some()
        {
            return Ok(BudgetGuardDisposition::Duplicate);
        }

        if ledger.pricing_status.counts_toward_spend() {
            match api_key.owner_kind {
                ApiKeyOwnerKind::User => {
                    let user_id = api_key.owner_user_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
                    if let Some(budget) = self.repo.get_active_budget_for_user(user_id).await? {
                        let (window_start, window_end) =
                            budget_window_bounds_utc(budget.cadence, ledger.occurred_at)?;
                        let spent = self
                            .repo
                            .sum_usage_cost_for_user_in_window(user_id, window_start, window_end)
                            .await?;
                        let projected =
                            spent.checked_add(ledger.computed_cost_usd).ok_or_else(|| {
                                GatewayError::Internal("budget projection overflow".to_string())
                            })?;
                        if budget.hard_limit && projected > budget.amount_usd {
                            return Err(GatewayError::BudgetExceeded {
                                ownership_scope: format!("user:{user_id}"),
                                projected_cost_usd: projected,
                                limit_usd: budget.amount_usd,
                            });
                        }
                    }
                }
                ApiKeyOwnerKind::Team => {
                    let team_id = api_key.owner_team_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
                    if let Some(budget) = self.repo.get_active_budget_for_team(team_id).await? {
                        let (window_start, window_end) =
                            budget_window_bounds_utc(budget.cadence, ledger.occurred_at)?;
                        let spent = self
                            .repo
                            .sum_usage_cost_for_team_in_window(team_id, window_start, window_end)
                            .await?;
                        let projected =
                            spent.checked_add(ledger.computed_cost_usd).ok_or_else(|| {
                                GatewayError::Internal("budget projection overflow".to_string())
                            })?;
                        if budget.hard_limit && projected > budget.amount_usd {
                            return Err(GatewayError::BudgetExceeded {
                                ownership_scope: format!("team:{team_id}:actor:none"),
                                projected_cost_usd: projected,
                                limit_usd: budget.amount_usd,
                            });
                        }
                    }
                }
            }
        }

        if self.repo.insert_usage_ledger_if_absent(ledger).await? {
            Ok(BudgetGuardDisposition::Inserted)
        } else {
            Ok(BudgetGuardDisposition::Duplicate)
        }
    }
}

fn budget_window_bounds_utc(
    cadence: BudgetCadence,
    occurred_at: OffsetDateTime,
) -> Result<(OffsetDateTime, OffsetDateTime), GatewayError> {
    let now_utc = occurred_at.to_offset(UtcOffset::UTC);
    let day_start = now_utc
        .date()
        .with_hms(0, 0, 0)
        .map_err(|error| GatewayError::Internal(format!("invalid day start: {error}")))?
        .assume_offset(UtcOffset::UTC);
    let end = now_utc + Duration::seconds(1);

    // Budget windows are fixed to UTC:
    // - Daily: starts at 00:00:00 UTC.
    // - Weekly: starts at Monday 00:00:00 UTC.
    //   Sunday 23:59:59 UTC remains in the prior week.
    let start = match cadence {
        BudgetCadence::Daily => day_start,
        BudgetCadence::Weekly => {
            let days_from_monday = i64::from(now_utc.weekday().number_days_from_monday());
            day_start - Duration::days(days_from_monday)
        }
    };

    Ok((start, end))
}

fn ownership_scope_key(api_key: &AuthenticatedApiKey) -> Result<String, AuthError> {
    match api_key.owner_kind {
        ApiKeyOwnerKind::User => {
            let user_id = api_key.owner_user_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
            Ok(format!("user:{user_id}"))
        }
        ApiKeyOwnerKind::Team => {
            let team_id = api_key.owner_team_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
            Ok(format!("team:{team_id}:actor:none"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyOwnerKind, AuthenticatedApiKey, BudgetCadence, BudgetRepository, Money4, StoreError,
        TeamBudgetRecord, UsageLedgerRecord, UsagePricingStatus, UserBudgetRecord,
    };
    use serde_json::json;
    use time::{Date, Month, OffsetDateTime};
    use uuid::Uuid;

    use super::{BudgetGuard, BudgetGuardDisposition, budget_window_bounds_utc};

    #[derive(Clone, Default)]
    struct InMemoryBudgetRepo {
        active_budget: Option<UserBudgetRecord>,
        current_spend: Money4,
        active_team_budget: Option<TeamBudgetRecord>,
        current_team_spend: Money4,
        inserted_events: Arc<Mutex<Vec<UsageLedgerRecord>>>,
    }

    #[async_trait]
    impl BudgetRepository for InMemoryBudgetRepo {
        async fn get_active_budget_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Option<UserBudgetRecord>, StoreError> {
            Ok(self.active_budget.clone())
        }

        async fn get_active_budget_for_team(
            &self,
            _team_id: Uuid,
        ) -> Result<Option<TeamBudgetRecord>, StoreError> {
            Ok(self.active_team_budget.clone())
        }

        async fn get_usage_ledger_by_request_and_scope(
            &self,
            request_id: &str,
            ownership_scope_key: &str,
        ) -> Result<Option<UsageLedgerRecord>, StoreError> {
            Ok(self
                .inserted_events
                .lock()
                .expect("events lock")
                .iter()
                .find(|event| {
                    event.request_id == request_id
                        && event.ownership_scope_key == ownership_scope_key
                })
                .cloned())
        }

        async fn sum_usage_cost_for_user_in_window(
            &self,
            _user_id: Uuid,
            _window_start: OffsetDateTime,
            _window_end: OffsetDateTime,
        ) -> Result<Money4, StoreError> {
            Ok(self.current_spend)
        }

        async fn sum_usage_cost_for_team_in_window(
            &self,
            _team_id: Uuid,
            _window_start: OffsetDateTime,
            _window_end: OffsetDateTime,
        ) -> Result<Money4, StoreError> {
            Ok(self.current_team_spend)
        }

        async fn insert_usage_ledger_if_absent(
            &self,
            event: &UsageLedgerRecord,
        ) -> Result<bool, StoreError> {
            let mut events = self.inserted_events.lock().expect("events lock");
            if events.iter().any(|existing| {
                existing.request_id == event.request_id
                    && existing.ownership_scope_key == event.ownership_scope_key
            }) {
                return Ok(false);
            }

            events.push(event.clone());
            Ok(true)
        }
    }

    fn user_auth(user_id: Uuid) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user_id),
            owner_team_id: None,
        }
    }

    fn team_auth(team_id: Uuid) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "dev123".to_string(),
            name: "dev".to_string(),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_user_id: None,
            owner_team_id: Some(team_id),
        }
    }

    fn sample_usage_ledger(
        api_key: &AuthenticatedApiKey,
        request_id: &str,
        pricing_status: UsagePricingStatus,
        computed_cost_usd: Money4,
        occurred_at: OffsetDateTime,
    ) -> UsageLedgerRecord {
        let ownership_scope_key = match api_key.owner_kind {
            ApiKeyOwnerKind::User => {
                format!(
                    "user:{}",
                    api_key.owner_user_id.expect("user owner").simple()
                )
            }
            ApiKeyOwnerKind::Team => format!(
                "team:{}:actor:none",
                api_key.owner_team_id.expect("team owner").simple()
            ),
        };

        UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: request_id.to_string(),
            ownership_scope_key,
            api_key_id: api_key.id,
            user_id: api_key.owner_user_id,
            team_id: api_key.owner_team_id,
            actor_user_id: None,
            model_id: None,
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            total_tokens: Some(150),
            provider_usage: json!({"prompt_tokens": 100, "completion_tokens": 50, "total_tokens": 150}),
            pricing_status,
            unpriced_reason: None,
            pricing_row_id: None,
            pricing_provider_id: Some("openai".to_string()),
            pricing_model_id: Some("gpt-4o-mini".to_string()),
            pricing_source: Some("models_dev_api".to_string()),
            pricing_source_etag: None,
            pricing_source_fetched_at: Some(occurred_at),
            pricing_last_updated: Some("2026-01-01".to_string()),
            input_cost_per_million_tokens: Some(Money4::from_scaled(50_000)),
            output_cost_per_million_tokens: Some(Money4::from_scaled(200_000)),
            computed_cost_usd,
            occurred_at,
        }
    }

    #[tokio::test]
    async fn blocks_when_hard_limit_would_be_exceeded() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: Some(UserBudgetRecord {
                user_budget_id: Uuid::new_v4(),
                user_id,
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
                is_active: true,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            }),
            current_spend: Money4::from_scaled(95_000),
            active_team_budget: None,
            current_team_spend: Money4::ZERO,
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });

        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);
        let ledger = sample_usage_ledger(
            &auth,
            "req_1",
            UsagePricingStatus::Priced,
            Money4::from_scaled(10_000),
            OffsetDateTime::now_utc(),
        );
        let error = guard
            .enforce_and_record_usage(&auth, &ledger)
            .await
            .expect_err("budget should block request");

        assert_eq!(error.error_code(), "budget_exceeded");
        assert_eq!(repo.inserted_events.lock().expect("events lock").len(), 0);
    }

    #[tokio::test]
    async fn pre_provider_blocks_user_when_hard_limit_is_already_reached() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: Some(UserBudgetRecord {
                user_budget_id: Uuid::new_v4(),
                user_id,
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(100_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
                is_active: true,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            }),
            current_spend: Money4::from_scaled(100_000),
            active_team_budget: None,
            current_team_spend: Money4::ZERO,
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });

        let guard = BudgetGuard::new(repo);
        let auth = user_auth(user_id);
        let error = guard
            .enforce_pre_provider_budget(&auth, "req_pre_user", OffsetDateTime::now_utc())
            .await
            .expect_err("pre-provider guard should block at the hard limit");

        assert_eq!(error.error_code(), "budget_exceeded");
    }

    #[tokio::test]
    async fn team_owned_keys_bypass_user_budget_check() {
        let team_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: None,
            current_spend: Money4::ZERO,
            active_team_budget: None,
            current_team_spend: Money4::ZERO,
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });

        let guard = BudgetGuard::new(repo.clone());
        let auth = team_auth(team_id);
        let outcome = guard
            .enforce_and_record_usage(
                &auth,
                &sample_usage_ledger(
                    &auth,
                    "req_2",
                    UsagePricingStatus::Priced,
                    Money4::from_scaled(125_000),
                    OffsetDateTime::now_utc(),
                ),
            )
            .await
            .expect("team-owned keys should not be blocked by user budget policy");

        assert_eq!(outcome, BudgetGuardDisposition::Inserted);
        assert_eq!(repo.inserted_events.lock().expect("events lock").len(), 1);
    }

    #[tokio::test]
    async fn team_hard_limit_blocks_when_spend_would_be_exceeded() {
        let team_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: None,
            current_spend: Money4::ZERO,
            active_team_budget: Some(TeamBudgetRecord {
                team_budget_id: Uuid::new_v4(),
                team_id,
                cadence: BudgetCadence::Weekly,
                amount_usd: Money4::from_scaled(80_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
                is_active: true,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            }),
            current_team_spend: Money4::from_scaled(79_500),
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });

        let guard = BudgetGuard::new(repo.clone());
        let auth = team_auth(team_id);
        let error = guard
            .enforce_and_record_usage(
                &auth,
                &sample_usage_ledger(
                    &auth,
                    "req_team_hard_limit",
                    UsagePricingStatus::Priced,
                    Money4::from_scaled(1_000),
                    OffsetDateTime::now_utc(),
                ),
            )
            .await
            .expect_err("team hard budget should block request");

        assert_eq!(error.error_code(), "budget_exceeded");
        assert_eq!(repo.inserted_events.lock().expect("events lock").len(), 0);
    }

    #[tokio::test]
    async fn pre_provider_blocks_team_when_hard_limit_is_already_reached() {
        let team_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: None,
            current_spend: Money4::ZERO,
            active_team_budget: Some(TeamBudgetRecord {
                team_budget_id: Uuid::new_v4(),
                team_id,
                cadence: BudgetCadence::Weekly,
                amount_usd: Money4::from_scaled(80_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
                is_active: true,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            }),
            current_team_spend: Money4::from_scaled(80_000),
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });

        let guard = BudgetGuard::new(repo);
        let auth = team_auth(team_id);
        let error = guard
            .enforce_pre_provider_budget(&auth, "req_pre_team", OffsetDateTime::now_utc())
            .await
            .expect_err("pre-provider guard should block at the hard limit");

        assert_eq!(error.error_code(), "budget_exceeded");
    }

    #[tokio::test]
    async fn team_unpriced_requests_do_not_trigger_budget_blocking() {
        let team_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: None,
            current_spend: Money4::ZERO,
            active_team_budget: Some(TeamBudgetRecord {
                team_budget_id: Uuid::new_v4(),
                team_id,
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(1),
                hard_limit: true,
                timezone: "UTC".to_string(),
                is_active: true,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            }),
            current_team_spend: Money4::from_scaled(100_000),
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });

        let guard = BudgetGuard::new(repo.clone());
        let auth = team_auth(team_id);
        let outcome = guard
            .enforce_and_record_usage(
                &auth,
                &sample_usage_ledger(
                    &auth,
                    "req_team_unpriced",
                    UsagePricingStatus::Unpriced,
                    Money4::ZERO,
                    OffsetDateTime::now_utc(),
                ),
            )
            .await
            .expect("unpriced request should bypass hard-limit blocking");

        assert_eq!(outcome, BudgetGuardDisposition::Inserted);
        assert_eq!(repo.inserted_events.lock().expect("events lock").len(), 1);
    }

    #[tokio::test]
    async fn duplicate_request_is_a_no_op() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: None,
            current_spend: Money4::ZERO,
            active_team_budget: None,
            current_team_spend: Money4::ZERO,
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });
        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);
        let ledger = sample_usage_ledger(
            &auth,
            "req_dup",
            UsagePricingStatus::Priced,
            Money4::from_scaled(1_000),
            OffsetDateTime::now_utc(),
        );

        let first = guard
            .enforce_and_record_usage(&auth, &ledger)
            .await
            .expect("first insert");
        let second = guard
            .enforce_and_record_usage(&auth, &ledger)
            .await
            .expect("duplicate insert should succeed");

        assert_eq!(first, BudgetGuardDisposition::Inserted);
        assert_eq!(second, BudgetGuardDisposition::Duplicate);
        assert_eq!(repo.inserted_events.lock().expect("events lock").len(), 1);
    }

    #[tokio::test]
    async fn pre_provider_budget_check_allows_duplicate_replays() {
        let user_id = Uuid::new_v4();
        let auth = user_auth(user_id);
        let mut existing = sample_usage_ledger(
            &auth,
            "req_dup_preflight",
            UsagePricingStatus::Priced,
            Money4::from_scaled(1_000),
            OffsetDateTime::now_utc(),
        );
        existing.ownership_scope_key = format!("user:{user_id}");
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: Some(UserBudgetRecord {
                user_budget_id: Uuid::new_v4(),
                user_id,
                cadence: BudgetCadence::Daily,
                amount_usd: Money4::from_scaled(1),
                hard_limit: true,
                timezone: "UTC".to_string(),
                is_active: true,
                created_at: OffsetDateTime::now_utc(),
                updated_at: OffsetDateTime::now_utc(),
            }),
            current_spend: Money4::from_scaled(200_000),
            active_team_budget: None,
            current_team_spend: Money4::ZERO,
            inserted_events: Arc::new(Mutex::new(vec![existing])),
        });

        let guard = BudgetGuard::new(repo);
        guard
            .enforce_pre_provider_budget(&auth, "req_dup_preflight", OffsetDateTime::now_utc())
            .await
            .expect("duplicate replay should bypass pre-provider hard-limit blocking");
    }

    #[test]
    fn weekly_budget_window_includes_sunday_in_prior_week() {
        let sunday = date_time(2025, Month::March, 2, 23, 59, 59);
        let (start, _) =
            budget_window_bounds_utc(BudgetCadence::Weekly, sunday).expect("window bounds");
        assert_eq!(start, date_time(2025, Month::February, 24, 0, 0, 0));
    }

    #[test]
    fn weekly_budget_window_starts_new_week_at_monday_midnight_utc() {
        let monday = date_time(2025, Month::March, 3, 0, 0, 0);
        let (start, _) =
            budget_window_bounds_utc(BudgetCadence::Weekly, monday).expect("window bounds");
        assert_eq!(start, date_time(2025, Month::March, 3, 0, 0, 0));
    }

    fn date_time(
        year: i32,
        month: Month,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> OffsetDateTime {
        Date::from_calendar_date(year, month, day)
            .expect("valid date")
            .with_hms(hour, minute, second)
            .expect("valid time")
            .assume_utc()
    }
}
