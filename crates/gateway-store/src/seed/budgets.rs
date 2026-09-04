//! Config budget ownership and reconciliation, independent of identity seeding.
use crate::GatewayStore;
use gateway_core::{
    BudgetModelSelector, BudgetRepository, BudgetScope, BudgetSettings, BudgetSource,
    BudgetSourceKind, BudgetUpsert, SYSTEM_BOOTSTRAP_ADMIN_USER_ID, SeedHumanBudgetDefaults,
    StoreError,
};
use std::collections::{BTreeSet, HashMap};
use time::OffsetDateTime;
use uuid::Uuid;

pub(crate) async fn reconcile_human_budget_defaults<S: GatewayStore + ?Sized>(
    store: &S,
    defaults: &SeedHumanBudgetDefaults,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    let mut user_ids = store
        .list_identity_users()
        .await?
        .into_iter()
        .map(|identity| identity.user.user_id)
        .collect::<BTreeSet<_>>();
    let bootstrap_id = Uuid::parse_str(SYSTEM_BOOTSTRAP_ADMIN_USER_ID)
        .map_err(|error| StoreError::Unexpected(error.to_string()))?;
    if store.get_user_by_id(bootstrap_id).await?.is_some() {
        user_ids.insert(bootstrap_id);
    }
    apply_defaults(store, defaults, &user_ids, now).await?;
    deactivate_stale_defaults(store, defaults, now).await
}

pub(crate) async fn apply_human_budget_defaults_for_user<S: BudgetRepository + ?Sized>(
    store: &S,
    defaults: &SeedHumanBudgetDefaults,
    user_id: Uuid,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    apply_defaults(store, defaults, &BTreeSet::from([user_id]), now).await
}

async fn apply_defaults<S: BudgetRepository + ?Sized>(
    store: &S,
    defaults: &SeedHumanBudgetDefaults,
    user_ids: &BTreeSet<Uuid>,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    if let Some(default) = &defaults.default_user_budget {
        let scopes = user_ids
            .iter()
            .map(|&user_id| BudgetScope::User { user_id })
            .collect();
        apply_default_scopes(
            store,
            scopes,
            &budget_settings(default),
            &BudgetSource::config_user_default(),
            now,
        )
        .await?;
    }
    for default in &defaults.model_defaults {
        let scopes = user_ids
            .iter()
            .map(|&user_id| BudgetScope::UserModel {
                user_id,
                selector: BudgetModelSelector::Model {
                    model_id: default.model_id,
                },
            })
            .collect();
        apply_default_scopes(
            store,
            scopes,
            &budget_settings(&default.budget),
            &BudgetSource::config_user_model_default(&default.model_key),
            now,
        )
        .await?;
    }
    Ok(())
}

async fn apply_default_scopes<S: BudgetRepository + ?Sized>(
    store: &S,
    scopes: Vec<BudgetScope>,
    settings: &BudgetSettings,
    source: &BudgetSource,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    let keys = scopes
        .iter()
        .map(BudgetScope::scope_key)
        .collect::<Vec<_>>();
    let states = store.get_budget_states_by_scope_keys(&keys).await?;
    let active = states
        .iter()
        .filter(|budget| budget.is_active)
        .map(|budget| (budget.scope_key.as_str(), budget))
        .collect::<HashMap<_, _>>();
    // The repository returns only the latest inactive row (when it is latest
    // overall), plus the active row. Preserve explicit admin deactivation.
    let deactivated = states
        .iter()
        .filter(|budget| !budget.is_active && budget.source.is_manual_deactivation())
        .map(|budget| budget.scope_key.as_str())
        .collect::<BTreeSet<_>>();
    let writes = scopes
        .into_iter()
        .filter_map(|scope| {
            let key = scope.scope_key();
            let existing = active.get(key.as_str());
            if deactivated.contains(key.as_str())
                || existing.is_some_and(|budget| !budget.source.matches(source))
            {
                return None;
            }
            Some(BudgetUpsert {
                scope,
                settings,
                source,
                expected_current_source: existing.map(|budget| &budget.source),
            })
        })
        .collect::<Vec<_>>();
    store
        .upsert_active_budgets_with_source_guard(&writes, now)
        .await
}

async fn deactivate_stale_defaults<S: BudgetRepository + ?Sized>(
    store: &S,
    defaults: &SeedHumanBudgetDefaults,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    let sources = defaults
        .model_defaults
        .iter()
        .map(|default| BudgetSource::config_user_model_default(&default.model_key))
        .collect::<Vec<_>>();
    let active_keys = sources
        .iter()
        .filter_map(|source| source.key.as_deref())
        .collect::<BTreeSet<_>>();
    let budgets = store.list_active_budgets(None).await?;
    let stale = budgets
        .iter()
        .filter(|budget| match budget.source.kind {
            BudgetSourceKind::ConfigUserDefault => defaults.default_user_budget.is_none(),
            BudgetSourceKind::ConfigUserModelDefault => budget
                .source
                .key
                .as_deref()
                .is_none_or(|key| !active_keys.contains(key)),
            BudgetSourceKind::Manual
            | BudgetSourceKind::ConfigUserOverride
            | BudgetSourceKind::ConfigServiceAccount => false,
        })
        .collect::<Vec<_>>();
    store.deactivate_budgets_by_source(&stale, now).await
}

/// Apply a declarative budget (`users[*].budget`, `service_accounts[*].budget`)
/// unless an admin has taken ownership of the active row.
///
/// A `manual` active row wins over config; any other active source is replaced
/// through the source guard so a concurrent admin edit cannot be clobbered. A
/// manually deactivated scope is re-activated because the declarative value is
/// still the operator's stated intent.
pub(crate) async fn upsert_unless_manually_overridden<S>(
    store: &S,
    scope: &BudgetScope,
    settings: &BudgetSettings,
    source: &BudgetSource,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let existing = store.get_active_budget_by_scope(scope).await?;
    if existing
        .as_ref()
        .is_some_and(|budget| budget.source.kind == BudgetSourceKind::Manual)
    {
        return Ok(());
    }
    let expected_current_source = existing.as_ref().map(|budget| &budget.source);
    store
        .upsert_active_budget_with_source_guard(
            scope,
            settings,
            source,
            expected_current_source,
            now,
        )
        .await?;
    Ok(())
}

pub(super) fn budget_settings(budget: &gateway_core::SeedBudget) -> BudgetSettings {
    BudgetSettings {
        cadence: budget.cadence,
        amount_usd: budget.amount_usd,
        hard_limit: budget.hard_limit,
        timezone: budget.timezone.clone(),
    }
}

#[cfg(test)]
mod tests;
