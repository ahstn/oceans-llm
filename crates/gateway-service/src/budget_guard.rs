use std::sync::Arc;

use gateway_core::{
    AuthenticatedApiKey, BudgetCadence, BudgetRecord, BudgetRepository, BudgetScope, GatewayError,
    Money4, UsageLedgerRecord, budget_window_utc,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::budget_scopes::{applicable_budget_scopes, usage_ownership_scope_key};

/// A hard budget whose window was pushed past its cap by spend that had already
/// been incurred upstream. The ledger row is still recorded; the pre-provider
/// check blocks the caller's next chargeable request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetOverrun {
    pub scope_key: String,
    pub spent_usd: Money4,
    pub limit_usd: Money4,
}

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

    #[tracing::instrument(name = "gateway.budget.precheck", skip_all)]
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

    /// Record spend that the provider has already incurred.
    ///
    /// Unlike the pre-provider check this never rejects on budget: the money is
    /// spent, so dropping the row would only hide it from the ledger and let the
    /// window stay under its cap. Every hard budget that the charge pushes over
    /// its cap is returned so callers can log it; the next request is blocked by
    /// `enforce_pre_provider_budget`.
    ///
    /// Returns `DuplicateUsageRecord` when the `(request_id, owner)` pair is
    /// already in the ledger.
    #[tracing::instrument(
        name = "gateway.usage.ledger",
        skip_all,
        fields(gateway.provider.key = %ledger.provider_key)
    )]
    pub async fn record_incurred_usage(
        &self,
        api_key: &AuthenticatedApiKey,
        ledger: &UsageLedgerRecord,
    ) -> Result<Vec<BudgetOverrun>, GatewayError> {
        if ledger.computed_cost_usd.is_negative() {
            return Err(GatewayError::InvalidRequest(
                "computed_cost_usd must be >= 0".to_string(),
            ));
        }

        let mut overruns = Vec::new();
        if ledger.pricing_status.counts_toward_spend() {
            for scope in applicable_budget_scopes(
                api_key,
                ledger.model_id,
                Some(ledger.upstream_model.as_str()),
            )? {
                let Some(budget) = self.repo.get_active_budget_by_scope(&scope).await? else {
                    continue;
                };
                if let Some(overrun) = self
                    .projected_overrun(
                        &scope,
                        &budget,
                        ledger.occurred_at,
                        ledger.computed_cost_usd,
                    )
                    .await?
                {
                    overruns.push(overrun);
                }
            }
        }

        if !self.repo.insert_usage_ledger_if_absent(ledger).await? {
            return Err(duplicate_request_error(&ledger.request_id));
        }
        Ok(overruns)
    }

    async fn reject_if_pre_provider_exceeded(
        &self,
        scope: &BudgetScope,
        budget: &BudgetRecord,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        if !budget.settings.hard_limit {
            return Ok(());
        }
        let (window_start, window_end) =
            budget_window_bounds_utc(budget.settings.cadence, occurred_at);
        let spent = self
            .repo
            .sum_usage_cost_for_budget_scope_in_window(scope, window_start, window_end)
            .await?;
        if spent >= budget.settings.amount_usd {
            return Err(GatewayError::BudgetExceeded {
                ownership_scope: budget.scope_key.clone(),
                projected_cost_usd: spent,
                limit_usd: budget.settings.amount_usd,
            });
        }
        Ok(())
    }

    async fn projected_overrun(
        &self,
        scope: &BudgetScope,
        budget: &BudgetRecord,
        occurred_at: OffsetDateTime,
        cost_usd: Money4,
    ) -> Result<Option<BudgetOverrun>, GatewayError> {
        if !budget.settings.hard_limit {
            return Ok(None);
        }
        let (window_start, window_end) =
            budget_window_bounds_utc(budget.settings.cadence, occurred_at);
        let spent = self
            .repo
            .sum_usage_cost_for_budget_scope_in_window(scope, window_start, window_end)
            .await?;
        let projected = spent.saturating_add(cost_usd);
        if projected > budget.settings.amount_usd {
            return Ok(Some(BudgetOverrun {
                scope_key: budget.scope_key.clone(),
                spent_usd: projected,
                limit_usd: budget.settings.amount_usd,
            }));
        }
        Ok(None)
    }
}

fn duplicate_request_error(request_id: &str) -> GatewayError {
    GatewayError::DuplicateUsageRecord {
        request_id: request_id.to_string(),
    }
}

fn budget_window_bounds_utc(
    cadence: BudgetCadence,
    occurred_at: OffsetDateTime,
) -> (OffsetDateTime, OffsetDateTime) {
    let window = budget_window_utc(cadence, occurred_at);
    (window.period_start, window.observed_end)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use gateway_core::{
        ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthenticatedApiKey, BudgetCadence,
        BudgetModelSelector, BudgetRecord, BudgetRepository, BudgetScope, BudgetSettings,
        BudgetSource, GatewayError, Money4, StoreError, UsageLedgerRecord, UsagePricingStatus,
    };
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::BudgetGuard;

    /// Scope-aware fake: budgets and prior spend are keyed by `scope_key`, and
    /// recorded ledger rows count toward every scope they match, so the tests
    /// can tell a user-model cap apart from the user cap.
    #[derive(Default)]
    struct InMemoryBudgetRepo {
        budgets: HashMap<String, BudgetRecord>,
        prior_spend: HashMap<String, Money4>,
        inserted_events: Mutex<Vec<UsageLedgerRecord>>,
    }

    impl InMemoryBudgetRepo {
        fn with_budget(mut self, budget: BudgetRecord, prior_spend: Money4) -> Self {
            self.prior_spend
                .insert(budget.scope_key.clone(), prior_spend);
            self.budgets.insert(budget.scope_key.clone(), budget);
            self
        }

        fn inserted(&self) -> Vec<UsageLedgerRecord> {
            self.inserted_events.lock().expect("events lock").clone()
        }
    }

    fn event_matches_scope(event: &UsageLedgerRecord, scope: &BudgetScope) -> bool {
        match scope {
            BudgetScope::User { user_id } => event.user_id == Some(*user_id),
            BudgetScope::ServiceAccount { service_account_id } => {
                event.service_account_id == Some(*service_account_id)
            }
            BudgetScope::UserModel { user_id, selector } => {
                event.user_id == Some(*user_id)
                    && match selector {
                        BudgetModelSelector::Model { model_id } => {
                            event.model_id == Some(*model_id)
                        }
                        BudgetModelSelector::UpstreamModel { upstream_model } => {
                            event.model_id.is_none()
                                && event.upstream_model.trim() == upstream_model.trim()
                        }
                    }
            }
        }
    }

    #[async_trait]
    impl BudgetRepository for InMemoryBudgetRepo {
        async fn get_budget_states_by_scope_keys(
            &self,
            _scope_keys: &[String],
        ) -> Result<Vec<BudgetRecord>, StoreError> {
            unreachable!("batch operations are not used by this fixture")
        }

        async fn upsert_active_budgets_with_source_guard(
            &self,
            _upserts: &[gateway_core::BudgetUpsert<'_>],
            _updated_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            unreachable!("batch operations are not used by this fixture")
        }

        async fn deactivate_budgets_by_source(
            &self,
            _budgets: &[&BudgetRecord],
            _updated_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            unreachable!("batch operations are not used by this fixture")
        }

        async fn sum_usage_cost_by_budget_scope(
            &self,
            _windows: &[gateway_core::BudgetScopeWindow<'_>],
        ) -> Result<std::collections::HashMap<String, Money4>, StoreError> {
            unreachable!("batch operations are not used by this fixture")
        }

        async fn get_active_budget_by_scope(
            &self,
            scope: &BudgetScope,
        ) -> Result<Option<BudgetRecord>, StoreError> {
            Ok(self.budgets.get(&scope.scope_key()).cloned())
        }

        async fn get_latest_budget_by_scope(
            &self,
            scope: &BudgetScope,
        ) -> Result<Option<BudgetRecord>, StoreError> {
            self.get_active_budget_by_scope(scope).await
        }

        async fn upsert_active_budget(
            &self,
            _scope: &BudgetScope,
            _settings: &BudgetSettings,
            _updated_at: OffsetDateTime,
        ) -> Result<BudgetRecord, StoreError> {
            unimplemented!("not exercised by guard tests")
        }

        async fn upsert_active_budget_with_source(
            &self,
            _scope: &BudgetScope,
            _settings: &BudgetSettings,
            _source: &BudgetSource,
            _updated_at: OffsetDateTime,
        ) -> Result<BudgetRecord, StoreError> {
            unimplemented!("not exercised by guard tests")
        }

        async fn upsert_active_budget_with_source_guard(
            &self,
            _scope: &BudgetScope,
            _settings: &BudgetSettings,
            _source: &BudgetSource,
            _expected_current_source: Option<&BudgetSource>,
            _updated_at: OffsetDateTime,
        ) -> Result<Option<BudgetRecord>, StoreError> {
            unimplemented!("not exercised by guard tests")
        }

        async fn deactivate_active_budget(
            &self,
            _scope: &BudgetScope,
            _updated_at: OffsetDateTime,
        ) -> Result<bool, StoreError> {
            unimplemented!("not exercised by guard tests")
        }

        async fn deactivate_active_budget_by_source(
            &self,
            _scope: &BudgetScope,
            _source: &BudgetSource,
            _updated_at: OffsetDateTime,
        ) -> Result<bool, StoreError> {
            unimplemented!("not exercised by guard tests")
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
            scope: &BudgetScope,
            _window_start: OffsetDateTime,
            _window_end: OffsetDateTime,
        ) -> Result<Money4, StoreError> {
            let prior = self
                .prior_spend
                .get(&scope.scope_key())
                .copied()
                .unwrap_or(Money4::ZERO);
            let recorded = self
                .inserted_events
                .lock()
                .expect("events lock")
                .iter()
                .filter(|event| {
                    event.pricing_status.counts_toward_spend() && event_matches_scope(event, scope)
                })
                .fold(Money4::ZERO, |sum, event| {
                    sum.saturating_add(event.computed_cost_usd)
                });
            Ok(prior.saturating_add(recorded))
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

    fn usd(dollars: i64) -> Money4 {
        Money4::from_scaled(dollars * 10_000)
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

    fn service_account_auth(service_account_id: Uuid) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "sa123".to_string(),
            name: "ci".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind: ApiKeyOwnerKind::ServiceAccount,
            owner_user_id: None,
            owner_team_id: Some(Uuid::new_v4()),
            owner_service_account_id: Some(service_account_id),
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
            source: BudgetSource::manual(),
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }

    fn user_model_scope(user_id: Uuid, model_id: Uuid) -> BudgetScope {
        BudgetScope::UserModel {
            user_id,
            selector: BudgetModelSelector::Model { model_id },
        }
    }

    fn ledger(
        api_key: &AuthenticatedApiKey,
        request_id: &str,
        model_id: Option<Uuid>,
        pricing_status: UsagePricingStatus,
        computed_cost_usd: Money4,
    ) -> UsageLedgerRecord {
        let occurred_at = OffsetDateTime::now_utc();
        let ownership_scope_key = match api_key.owner_kind {
            ApiKeyOwnerKind::User => format!("user:{}", api_key.owner_user_id.expect("user")),
            ApiKeyOwnerKind::ServiceAccount => format!(
                "service_account:{}",
                api_key.owner_service_account_id.expect("service account")
            ),
        };
        UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: request_id.to_string(),
            ownership_scope_key,
            api_key_id: api_key.id,
            user_id: api_key.owner_user_id,
            team_id: api_key.owner_team_id,
            service_account_id: api_key.owner_service_account_id,
            actor_user_id: None,
            model_id,
            model_route_id: None,
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-4o-mini".to_string(),
            prompt_tokens: Some(100),
            uncached_input_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
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
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            computed_cost_usd,
            occurred_at,
        }
    }

    async fn precheck(
        guard: &BudgetGuard<InMemoryBudgetRepo>,
        auth: &AuthenticatedApiKey,
        request_id: &str,
        model_id: Uuid,
    ) -> Result<(), GatewayError> {
        guard
            .enforce_pre_provider_budget(
                auth,
                request_id,
                Some(model_id),
                Some("gpt-4o-mini"),
                OffsetDateTime::now_utc(),
            )
            .await
    }

    fn assert_budget_exceeded(error: &GatewayError, expected_scope_key: &str) {
        match error {
            GatewayError::BudgetExceeded {
                ownership_scope, ..
            } => assert_eq!(ownership_scope, expected_scope_key),
            other => panic!("expected budget_exceeded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn model_budget_blocks_before_user_budget_is_exhausted() {
        // User cap $80 with $50 spent; model cap $40 already reached.
        let user_id = Uuid::new_v4();
        let model_id = Uuid::new_v4();
        let model_scope = user_model_scope(user_id, model_id);
        let repo = Arc::new(
            InMemoryBudgetRepo::default()
                .with_budget(
                    budget(BudgetScope::User { user_id }, usd(80), true),
                    usd(50),
                )
                .with_budget(budget(model_scope.clone(), usd(40), true), usd(40)),
        );
        let guard = BudgetGuard::new(repo);
        let auth = user_auth(user_id);

        let error = precheck(&guard, &auth, "req_model_cap", model_id)
            .await
            .expect_err("model cap should block");
        assert_budget_exceeded(&error, &model_scope.scope_key());

        // A different model has no model-specific cap and the user cap has headroom.
        precheck(&guard, &auth, "req_other_model", Uuid::new_v4())
            .await
            .expect("other model passes on user headroom");
    }

    #[tokio::test]
    async fn user_budget_blocks_even_when_model_budget_has_headroom() {
        let user_id = Uuid::new_v4();
        let model_id = Uuid::new_v4();
        let user_scope = BudgetScope::User { user_id };
        let repo = Arc::new(
            InMemoryBudgetRepo::default()
                .with_budget(budget(user_scope.clone(), usd(80), true), usd(80))
                .with_budget(
                    budget(user_model_scope(user_id, model_id), usd(40), true),
                    usd(1),
                ),
        );
        let guard = BudgetGuard::new(repo);
        let auth = user_auth(user_id);

        let error = precheck(&guard, &auth, "req_user_cap", model_id)
            .await
            .expect_err("user cap should block");
        assert_budget_exceeded(&error, &user_scope.scope_key());
    }

    #[tokio::test]
    async fn soft_model_budget_does_not_block_but_hard_user_budget_does() {
        let user_id = Uuid::new_v4();
        let model_id = Uuid::new_v4();
        let repo = Arc::new(
            InMemoryBudgetRepo::default()
                .with_budget(
                    budget(user_model_scope(user_id, model_id), usd(5), false),
                    usd(99),
                )
                .with_budget(
                    budget(BudgetScope::User { user_id }, usd(80), true),
                    usd(10),
                ),
        );
        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);

        precheck(&guard, &auth, "req_soft", model_id)
            .await
            .expect("soft model cap must not block");

        let overruns = guard
            .record_incurred_usage(
                &auth,
                &ledger(
                    &auth,
                    "req_soft",
                    Some(model_id),
                    UsagePricingStatus::Priced,
                    usd(1),
                ),
            )
            .await
            .expect("recording succeeds");
        assert!(overruns.is_empty(), "soft budgets never report overruns");
        assert_eq!(repo.inserted().len(), 1);
    }

    #[tokio::test]
    async fn incurred_overrun_is_recorded_and_blocks_the_next_request() {
        // $79 spent of an $80 cap: the precheck passes, the $5 charge lands
        // anyway, and the following request is rejected.
        let user_id = Uuid::new_v4();
        let model_id = Uuid::new_v4();
        let user_scope = BudgetScope::User { user_id };
        let repo = Arc::new(
            InMemoryBudgetRepo::default()
                .with_budget(budget(user_scope.clone(), usd(80), true), usd(79)),
        );
        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);

        precheck(&guard, &auth, "req_1", model_id)
            .await
            .expect("under the cap before the provider runs");

        let overruns = guard
            .record_incurred_usage(
                &auth,
                &ledger(
                    &auth,
                    "req_1",
                    Some(model_id),
                    UsagePricingStatus::Priced,
                    usd(5),
                ),
            )
            .await
            .expect("incurred spend is always recorded");
        assert_eq!(overruns.len(), 1);
        assert_eq!(overruns[0].scope_key, user_scope.scope_key());
        assert_eq!(overruns[0].spent_usd, usd(84));
        assert_eq!(overruns[0].limit_usd, usd(80));
        assert_eq!(repo.inserted().len(), 1, "ledger row must not be dropped");

        let error = precheck(&guard, &auth, "req_2", model_id)
            .await
            .expect_err("window is now exhausted");
        assert_budget_exceeded(&error, &user_scope.scope_key());
    }

    #[tokio::test]
    async fn model_spend_counts_toward_both_model_and_user_budgets() {
        let user_id = Uuid::new_v4();
        let model_id = Uuid::new_v4();
        let model_scope = user_model_scope(user_id, model_id);
        let repo = Arc::new(
            InMemoryBudgetRepo::default()
                .with_budget(budget(BudgetScope::User { user_id }, usd(80), true), usd(0))
                .with_budget(budget(model_scope.clone(), usd(40), true), usd(38)),
        );
        let guard = BudgetGuard::new(repo);
        let auth = user_auth(user_id);

        let overruns = guard
            .record_incurred_usage(
                &auth,
                &ledger(
                    &auth,
                    "req_1",
                    Some(model_id),
                    UsagePricingStatus::Priced,
                    usd(3),
                ),
            )
            .await
            .expect("recorded");
        assert_eq!(overruns.len(), 1, "only the model cap is exceeded");
        assert_eq!(overruns[0].scope_key, model_scope.scope_key());

        let error = precheck(&guard, &auth, "req_2", model_id)
            .await
            .expect_err("model window exhausted");
        assert_budget_exceeded(&error, &model_scope.scope_key());
        precheck(&guard, &auth, "req_3", Uuid::new_v4())
            .await
            .expect("user cap still has headroom for other models");
    }

    #[tokio::test]
    async fn unpriced_usage_is_recorded_without_touching_budgets() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(
            InMemoryBudgetRepo::default()
                .with_budget(budget(BudgetScope::User { user_id }, usd(1), true), usd(1)),
        );
        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);

        let overruns = guard
            .record_incurred_usage(
                &auth,
                &ledger(
                    &auth,
                    "req_unpriced",
                    None,
                    UsagePricingStatus::Unpriced,
                    usd(0),
                ),
            )
            .await
            .expect("unpriced rows are still stored for reporting");
        assert!(overruns.is_empty());
        assert_eq!(repo.inserted().len(), 1);
    }

    #[tokio::test]
    async fn service_account_traffic_only_checks_the_service_account_budget() {
        let service_account_id = Uuid::new_v4();
        let sa_scope = BudgetScope::ServiceAccount { service_account_id };
        let repo = Arc::new(
            InMemoryBudgetRepo::default()
                .with_budget(budget(sa_scope.clone(), usd(25), true), usd(25)),
        );
        let guard = BudgetGuard::new(repo);
        let auth = service_account_auth(service_account_id);

        let error = precheck(&guard, &auth, "req_sa", Uuid::new_v4())
            .await
            .expect_err("service account cap reached");
        assert_budget_exceeded(&error, &sa_scope.scope_key());
    }

    #[tokio::test]
    async fn duplicate_request_id_rejected_before_provider_execution() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo::default());
        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);
        repo.inserted_events
            .lock()
            .expect("events lock")
            .push(ledger(
                &auth,
                "req_duplicate",
                None,
                UsagePricingStatus::Priced,
                usd(1),
            ));

        let error = precheck(&guard, &auth, "req_duplicate", Uuid::new_v4())
            .await
            .expect_err("duplicate request id should be rejected");

        assert!(matches!(error, GatewayError::DuplicateUsageRecord { .. }));
        assert_eq!(error.error_code(), "invalid_request");
    }

    #[tokio::test]
    async fn duplicate_request_id_rejected_when_recording_usage() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryBudgetRepo::default());
        let guard = BudgetGuard::new(repo.clone());
        let auth = user_auth(user_id);
        let row = ledger(
            &auth,
            "req_dup_record",
            None,
            UsagePricingStatus::Priced,
            usd(1),
        );
        repo.inserted_events
            .lock()
            .expect("events lock")
            .push(row.clone());

        let error = guard
            .record_incurred_usage(&auth, &row)
            .await
            .expect_err("duplicate request id should be rejected");

        assert!(matches!(error, GatewayError::DuplicateUsageRecord { .. }));
        assert_eq!(repo.inserted().len(), 1);
    }
}
