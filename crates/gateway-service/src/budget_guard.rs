use std::sync::Arc;

use gateway_core::{
    AuthenticatedApiKey, BudgetCadence, BudgetRecord, BudgetRepository, BudgetScope, GatewayError,
    UsageLedgerRecord, budget_window_utc,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::budget_scopes::{applicable_budget_scopes, usage_ownership_scope_key};

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

    pub async fn enforce_pre_provider_budget(
        &self,
        api_key: &AuthenticatedApiKey,
        request_id: &str,
        model_id: Option<Uuid>,
        upstream_model: Option<&str>,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        let ownership_scope_key = usage_ownership_scope_key(api_key)?;
        if self
            .repo
            .get_usage_ledger_by_request_and_scope(request_id, &ownership_scope_key)
            .await?
            .is_some()
        {
            return Err(duplicate_request_error(request_id));
        }

        for scope in applicable_budget_scopes(api_key, model_id, upstream_model)? {
            let Some(budget) = self.repo.get_active_budget_by_scope(&scope).await? else {
                continue;
            };
            self.reject_if_pre_provider_exceeded(&scope, &budget, occurred_at)
                .await?;
        }

        Ok(())
    }

    pub async fn enforce_and_record_usage(
        &self,
        api_key: &AuthenticatedApiKey,
        ledger: &UsageLedgerRecord,
    ) -> Result<(), GatewayError> {
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
            return Err(duplicate_request_error(&ledger.request_id));
        }

        if ledger.pricing_status.counts_toward_spend() {
            for scope in applicable_budget_scopes(
                api_key,
                ledger.model_id,
                if ledger.model_id.is_none() {
                    Some(ledger.upstream_model.as_str())
                } else {
                    None
                },
            )? {
                let Some(budget) = self.repo.get_active_budget_by_scope(&scope).await? else {
                    continue;
                };
                self.reject_if_projected_exceeded(
                    &scope,
                    &budget,
                    ledger.occurred_at,
                    ledger.computed_cost_usd,
                )
                .await?;
            }
        }

        if !self.repo.insert_usage_ledger_if_absent(ledger).await? {
            return Err(duplicate_request_error(&ledger.request_id));
        }
        Ok(())
    }

    async fn reject_if_pre_provider_exceeded(
        &self,
        scope: &BudgetScope,
        budget: &BudgetRecord,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        let (window_start, window_end) =
            budget_window_bounds_utc(budget.settings.cadence, occurred_at)?;
        let spent = self
            .repo
            .sum_usage_cost_for_budget_scope_in_window(scope, window_start, window_end)
            .await?;
        if budget.settings.hard_limit && spent >= budget.settings.amount_usd {
            return Err(GatewayError::BudgetExceeded {
                ownership_scope: budget.scope_key.clone(),
                projected_cost_usd: spent,
                limit_usd: budget.settings.amount_usd,
            });
        }
        Ok(())
    }

    async fn reject_if_projected_exceeded(
        &self,
        scope: &BudgetScope,
        budget: &BudgetRecord,
        occurred_at: OffsetDateTime,
        cost_usd: gateway_core::Money4,
    ) -> Result<(), GatewayError> {
        let (window_start, window_end) =
            budget_window_bounds_utc(budget.settings.cadence, occurred_at)?;
        let spent = self
            .repo
            .sum_usage_cost_for_budget_scope_in_window(scope, window_start, window_end)
            .await?;
        let projected = spent
            .checked_add(cost_usd)
            .ok_or_else(|| GatewayError::Internal("budget projection overflow".to_string()))?;
        if budget.settings.hard_limit && projected > budget.settings.amount_usd {
            return Err(GatewayError::BudgetExceeded {
                ownership_scope: budget.scope_key.clone(),
                projected_cost_usd: projected,
                limit_usd: budget.settings.amount_usd,
            });
        }
        Ok(())
    }
}

fn duplicate_request_error(request_id: &str) -> GatewayError {
    GatewayError::InvalidRequest(format!(
        "request_id `{request_id}` has already been recorded for this owner"
    ))
}

fn budget_window_bounds_utc(
    cadence: BudgetCadence,
    occurred_at: OffsetDateTime,
) -> Result<(OffsetDateTime, OffsetDateTime), GatewayError> {
    let window = budget_window_utc(cadence, occurred_at).map_err(GatewayError::Internal)?;
    Ok((window.period_start, window.observed_end))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthenticatedApiKey, BudgetCadence, BudgetRecord,
        BudgetRepository, BudgetScope, BudgetSettings, Money4, StoreError, UsageLedgerRecord,
        UsagePricingStatus,
    };
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::BudgetGuard;

    #[derive(Clone, Default)]
    struct InMemoryBudgetRepo {
        active_budget: Option<BudgetRecord>,
        current_spend: Money4,
        inserted_events: Arc<Mutex<Vec<UsageLedgerRecord>>>,
    }

    #[async_trait]
    impl BudgetRepository for InMemoryBudgetRepo {
        async fn get_active_budget_by_scope(
            &self,
            _scope: &BudgetScope,
        ) -> Result<Option<BudgetRecord>, StoreError> {
            Ok(self.active_budget.clone())
        }

        async fn upsert_active_budget(
            &self,
            _scope: &BudgetScope,
            _settings: &BudgetSettings,
            _updated_at: OffsetDateTime,
        ) -> Result<BudgetRecord, StoreError> {
            self.active_budget
                .clone()
                .ok_or_else(|| StoreError::NotFound("budget missing".to_string()))
        }

        async fn deactivate_active_budget(
            &self,
            _scope: &BudgetScope,
            _updated_at: OffsetDateTime,
        ) -> Result<bool, StoreError> {
            Ok(false)
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

        async fn sum_usage_cost_for_budget_scope_in_window(
            &self,
            _scope: &BudgetScope,
            _window_start: OffsetDateTime,
            _window_end: OffsetDateTime,
        ) -> Result<Money4, StoreError> {
            Ok(self.current_spend)
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
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(user_id),
            owner_team_id: None,
            owner_service_account_id: None,
        }
    }

    fn budget(scope: BudgetScope, amount_usd: Money4, hard_limit: bool) -> BudgetRecord {
        let now = OffsetDateTime::now_utc();
        BudgetRecord {
            budget_id: Uuid::new_v4(),
            scope_key: scope.scope_key(),
            scope,
            settings: BudgetSettings {
                cadence: BudgetCadence::Daily,
                amount_usd,
                hard_limit,
                timezone: "UTC".to_string(),
            },
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }

    fn sample_usage_ledger(
        api_key: &AuthenticatedApiKey,
        request_id: &str,
        pricing_status: UsagePricingStatus,
        computed_cost_usd: Money4,
    ) -> UsageLedgerRecord {
        let occurred_at = OffsetDateTime::now_utc();
        UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: request_id.to_string(),
            ownership_scope_key: format!("user:{}", api_key.owner_user_id.expect("user owner")),
            api_key_id: api_key.id,
            user_id: api_key.owner_user_id,
            team_id: None,
            service_account_id: None,
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
        let scope = BudgetScope::User { user_id };
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: Some(budget(scope, Money4::from_scaled(100_000), true)),
            current_spend: Money4::from_scaled(95_000),
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });
        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);
        let ledger = sample_usage_ledger(
            &auth,
            "req_1",
            UsagePricingStatus::Priced,
            Money4::from_scaled(10_000),
        );
        let error = guard
            .enforce_and_record_usage(&auth, &ledger)
            .await
            .expect_err("budget should block request");

        assert_eq!(error.error_code(), "budget_exceeded");
        assert_eq!(repo.inserted_events.lock().expect("events lock").len(), 0);
    }

    #[tokio::test]
    async fn soft_budget_never_blocks() {
        let user_id = Uuid::new_v4();
        let scope = BudgetScope::User { user_id };
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: Some(budget(scope, Money4::from_scaled(1), false)),
            current_spend: Money4::from_scaled(100_000),
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });
        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);
        let ledger = sample_usage_ledger(
            &auth,
            "req_2",
            UsagePricingStatus::Priced,
            Money4::from_scaled(10_000),
        );

        guard
            .enforce_and_record_usage(&auth, &ledger)
            .await
            .expect("soft budget should not block");

        assert_eq!(repo.inserted_events.lock().expect("events lock").len(), 1);
    }

    #[tokio::test]
    async fn duplicate_request_id_rejected_before_provider_execution() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: Some(budget(
                BudgetScope::User { user_id },
                Money4::from_scaled(100_000),
                true,
            )),
            current_spend: Money4::ZERO,
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });
        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);
        let ledger = sample_usage_ledger(
            &auth,
            "req_duplicate",
            UsagePricingStatus::Priced,
            Money4::from_scaled(10_000),
        );
        repo.inserted_events
            .lock()
            .expect("events lock")
            .push(ledger);

        let error = guard
            .enforce_pre_provider_budget(
                &auth,
                "req_duplicate",
                None,
                Some("gpt-4o-mini"),
                OffsetDateTime::now_utc(),
            )
            .await
            .expect_err("duplicate request id should be rejected");

        assert_eq!(error.error_code(), "invalid_request");
    }

    #[tokio::test]
    async fn duplicate_request_id_rejected_when_recording_usage() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo {
            active_budget: None,
            current_spend: Money4::ZERO,
            inserted_events: Arc::new(Mutex::new(Vec::new())),
        });
        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);
        let ledger = sample_usage_ledger(
            &auth,
            "req_duplicate_record",
            UsagePricingStatus::Priced,
            Money4::from_scaled(10_000),
        );
        repo.inserted_events
            .lock()
            .expect("events lock")
            .push(ledger.clone());

        let error = guard
            .enforce_and_record_usage(&auth, &ledger)
            .await
            .expect_err("duplicate request id should be rejected");

        assert_eq!(error.error_code(), "invalid_request");
        assert_eq!(repo.inserted_events.lock().expect("events lock").len(), 1);
    }
}
