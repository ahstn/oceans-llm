use super::*;
use gateway_core::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct CountingRepository {
    reads: AtomicUsize,
    writes: AtomicUsize,
    scopes_written: AtomicUsize,
    lists: AtomicUsize,
    deactivations: AtomicUsize,
}

#[async_trait::async_trait]
impl BudgetRepository for CountingRepository {
    async fn get_budget_states_by_scope_keys(
        &self,
        _scope_keys: &[String],
    ) -> Result<Vec<BudgetRecord>, StoreError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(Vec::new())
    }
    async fn upsert_active_budgets_with_source_guard(
        &self,
        upserts: &[gateway_core::BudgetUpsert<'_>],
        _updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.scopes_written
            .fetch_add(upserts.len(), Ordering::Relaxed);
        Ok(())
    }
    async fn deactivate_budgets_by_source(
        &self,
        _budgets: &[&BudgetRecord],
        _updated_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        self.deactivations.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    async fn sum_usage_cost_by_budget_scope(
        &self,
        _windows: &[gateway_core::BudgetScopeWindow<'_>],
    ) -> Result<std::collections::HashMap<String, Money4>, StoreError> {
        panic!("unexpected per-scope repository call")
    }
    async fn get_active_budget_by_scope(
        &self,
        _scope: &BudgetScope,
    ) -> Result<Option<BudgetRecord>, StoreError> {
        panic!("unexpected per-scope repository call")
    }
    async fn get_latest_budget_by_scope(
        &self,
        _scope: &BudgetScope,
    ) -> Result<Option<BudgetRecord>, StoreError> {
        panic!("unexpected per-scope repository call")
    }
    async fn list_active_budgets(
        &self,
        _scope_kind: Option<BudgetScopeKind>,
    ) -> Result<Vec<BudgetRecord>, StoreError> {
        self.lists.fetch_add(1, Ordering::Relaxed);
        Ok(Vec::new())
    }
    async fn upsert_active_budget(
        &self,
        _scope: &BudgetScope,
        _settings: &BudgetSettings,
        _updated_at: OffsetDateTime,
    ) -> Result<BudgetRecord, StoreError> {
        panic!("unexpected per-scope repository call")
    }
    async fn upsert_active_budget_with_source(
        &self,
        _scope: &BudgetScope,
        _settings: &BudgetSettings,
        _source: &BudgetSource,
        _updated_at: OffsetDateTime,
    ) -> Result<BudgetRecord, StoreError> {
        panic!("unexpected per-scope repository call")
    }
    async fn upsert_active_budget_with_source_guard(
        &self,
        _scope: &BudgetScope,
        _settings: &BudgetSettings,
        _source: &BudgetSource,
        _expected_current_source: Option<&BudgetSource>,
        _updated_at: OffsetDateTime,
    ) -> Result<Option<BudgetRecord>, StoreError> {
        panic!("unexpected per-scope repository call")
    }
    async fn deactivate_active_budget(
        &self,
        _scope: &BudgetScope,
        _updated_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        panic!("unexpected per-scope repository call")
    }
    async fn deactivate_active_budget_by_source(
        &self,
        _scope: &BudgetScope,
        _source: &BudgetSource,
        _updated_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        panic!("unexpected per-scope repository call")
    }
    async fn get_usage_ledger_by_request_and_scope(
        &self,
        _request_id: &str,
        _ownership_scope_key: &str,
    ) -> Result<Option<UsageLedgerRecord>, StoreError> {
        panic!("unexpected per-scope repository call")
    }
    async fn sum_usage_cost_for_budget_scope_in_window(
        &self,
        _scope: &BudgetScope,
        _window_start: OffsetDateTime,
        _window_end: OffsetDateTime,
    ) -> Result<Money4, StoreError> {
        panic!("unexpected per-scope repository call")
    }
    async fn insert_usage_ledger_if_absent(
        &self,
        _event: &UsageLedgerRecord,
    ) -> Result<bool, StoreError> {
        panic!("unexpected per-scope repository call")
    }
}

#[tokio::test]
async fn reconciliation_call_count_depends_on_model_defaults_not_user_count() {
    let budget = SeedBudget {
        cadence: BudgetCadence::Daily,
        amount_usd: Money4::from_scaled(100),
        hard_limit: true,
        timezone: "UTC".into(),
    };
    let defaults = SeedHumanBudgetDefaults {
        default_user_budget: Some(budget.clone()),
        model_defaults: (0..3)
            .map(|index| SeedUserModelBudgetDefault {
                model_key: format!("model-{index}"),
                model_id: Uuid::new_v4(),
                budget: budget.clone(),
            })
            .collect(),
    };
    for user_count in [1, 2_000] {
        let repository = CountingRepository::default();
        let users = (0..user_count).map(|_| Uuid::new_v4()).collect();
        apply_defaults(&repository, &defaults, &users, OffsetDateTime::now_utc())
            .await
            .expect("defaults");
        deactivate_stale_defaults(&repository, &defaults, OffsetDateTime::now_utc())
            .await
            .expect("stale removal");
        assert_eq!(repository.reads.load(Ordering::Relaxed), 4);
        assert_eq!(repository.writes.load(Ordering::Relaxed), 4);
        assert_eq!(
            repository.scopes_written.load(Ordering::Relaxed),
            4 * user_count
        );
        assert_eq!(repository.lists.load(Ordering::Relaxed), 1);
        assert_eq!(repository.deactivations.load(Ordering::Relaxed), 1);
    }
}
