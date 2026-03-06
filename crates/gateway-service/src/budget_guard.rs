use std::sync::Arc;

use gateway_core::{
    ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, BudgetCadence, BudgetRepository, GatewayError,
    Money4, UsageCostEventRecord,
};
use time::{Duration, OffsetDateTime, UtcOffset};
use uuid::Uuid;

#[derive(Clone)]
pub struct BudgetGuard<R> {
    repo: Arc<R>,
}

impl<R> BudgetGuard<R>
where
    R: BudgetRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn enforce_and_record_usage(
        &self,
        api_key: &AuthenticatedApiKey,
        request_id: &str,
        model_id: Option<Uuid>,
        estimated_cost_usd: Money4,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        if estimated_cost_usd.is_negative() {
            return Err(GatewayError::InvalidRequest(
                "estimated_cost_usd must be >= 0".to_string(),
            ));
        }

        if api_key.owner_kind == ApiKeyOwnerKind::User {
            let user_id = api_key.owner_user_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
            if let Some(budget) = self.repo.get_active_budget_for_user(user_id).await? {
                let (window_start, window_end) =
                    budget_window_bounds_utc(budget.cadence, occurred_at)?;
                let spent = self
                    .repo
                    .sum_usage_cost_for_user_in_window(user_id, window_start, window_end)
                    .await?;
                let projected = spent.checked_add(estimated_cost_usd).ok_or_else(|| {
                    GatewayError::Internal("budget projection overflow".to_string())
                })?;
                if budget.hard_limit && projected > budget.amount_usd {
                    return Err(GatewayError::BudgetExceeded {
                        user_id: user_id.to_string(),
                        projected_cost_usd: projected,
                        limit_usd: budget.amount_usd,
                    });
                }
            }
        }

        self.repo
            .insert_usage_cost_event(&UsageCostEventRecord {
                usage_event_id: Uuid::new_v4(),
                request_id: request_id.to_string(),
                api_key_id: api_key.id,
                user_id: api_key.owner_user_id,
                team_id: api_key.owner_team_id,
                model_id,
                estimated_cost_usd,
                occurred_at,
            })
            .await?;

        Ok(())
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyOwnerKind, AuthenticatedApiKey, BudgetCadence, BudgetRepository, Money4, StoreError,
        UsageCostEventRecord, UserBudgetRecord,
    };
    use time::{Date, Month, OffsetDateTime};
    use uuid::Uuid;

    use super::{BudgetGuard, budget_window_bounds_utc};

    #[derive(Clone, Default)]
    struct InMemoryBudgetRepo {
        active_budget: Option<UserBudgetRecord>,
        current_spend: Money4,
        inserted_events: Arc<Mutex<Vec<UsageCostEventRecord>>>,
    }

    #[async_trait]
    impl BudgetRepository for InMemoryBudgetRepo {
        async fn get_active_budget_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Option<UserBudgetRecord>, StoreError> {
            Ok(self.active_budget.clone())
        }

        async fn sum_usage_cost_for_user_in_window(
            &self,
            _user_id: Uuid,
            _window_start: OffsetDateTime,
            _window_end: OffsetDateTime,
        ) -> Result<Money4, StoreError> {
            Ok(self.current_spend)
        }

        async fn insert_usage_cost_event(
            &self,
            event: &UsageCostEventRecord,
        ) -> Result<(), StoreError> {
            self.inserted_events
                .lock()
                .expect("events lock")
                .push(event.clone());
            Ok(())
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
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });

        let guard = BudgetGuard::new(repo.clone());
        let error = guard
            .enforce_and_record_usage(
                &user_auth(user_id),
                "req_1",
                None,
                Money4::from_scaled(10_000),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect_err("budget should block request");

        assert_eq!(error.error_code(), "budget_exceeded");
        assert_eq!(repo.inserted_events.lock().expect("events lock").len(), 0);
    }

    #[tokio::test]
    async fn team_owned_keys_bypass_user_budget_check() {
        let team_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: None,
            current_spend: Money4::ZERO,
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });

        let guard = BudgetGuard::new(repo.clone());
        guard
            .enforce_and_record_usage(
                &team_auth(team_id),
                "req_2",
                None,
                Money4::from_scaled(125_000),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect("team-owned keys should not be blocked by user budget policy");

        assert_eq!(repo.inserted_events.lock().expect("events lock").len(), 1);
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
