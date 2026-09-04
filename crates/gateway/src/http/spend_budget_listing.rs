//! Budget page loading uses six store calls, independent of directory size.
use std::collections::HashMap;

use gateway_core::{
    BudgetContact, BudgetRecord, BudgetScope, BudgetScopeWindow, IdentityUserRecord, Money4,
    ServiceAccountRecord, TeamRecord,
};
use gateway_store::GatewayStore;
use time::OffsetDateTime;
use uuid::Uuid;

use super::admin_contract::{
    SpendBudgetServiceAccountView, SpendBudgetUserModelView, SpendBudgetUserView, SpendBudgetsView,
};
use super::error::AppError;
use super::spend::{budget_source_to_view, budget_to_settings_view, budget_window_bounds_utc};

pub(super) async fn load_budget_listing<S: GatewayStore + ?Sized>(
    store: &S,
    now: OffsetDateTime,
) -> Result<SpendBudgetsView, AppError> {
    let (users, accounts, teams, budgets, contacts) = tokio::try_join!(
        store.list_identity_users(),
        store.list_active_service_accounts(),
        store.list_teams(),
        store.list_active_budgets(None),
        store.list_budget_contacts(),
    )?;
    let windows = budgets
        .iter()
        .map(|budget| {
            let (window_start, window_end) = budget_window_bounds_utc(budget.settings.cadence, now);
            BudgetScopeWindow {
                scope: &budget.scope,
                window_start,
                window_end,
            }
        })
        .collect::<Vec<_>>();
    let spend = store.sum_usage_cost_by_budget_scope(&windows).await?;
    let index = BudgetListingIndex::new(&budgets, &contacts, &spend);
    let team_map = teams
        .iter()
        .map(|team| (team.team_id, team))
        .collect::<HashMap<_, _>>();
    Ok(SpendBudgetsView {
        users: users
            .into_iter()
            .map(|user| index.user_view(user))
            .collect(),
        service_accounts: accounts
            .into_iter()
            .map(|account| {
                let team = team_map.get(&account.team_id).ok_or_else(|| {
                    AppError(gateway_core::GatewayError::InvalidRequest(format!(
                        "service account `{}` references missing team",
                        account.service_account_id
                    )))
                })?;
                Ok(index.service_account_view(account, team))
            })
            .collect::<Result<_, AppError>>()?,
        user_model_budgets: budgets
            .iter()
            .filter_map(|budget| index.model_view(budget))
            .collect(),
    })
}

/// Borrow records once; each view then uses only in-memory lookups.
struct BudgetListingIndex<'a> {
    budgets: HashMap<&'a str, &'a BudgetRecord>,
    emails: HashMap<Uuid, &'a str>,
    recipients: HashMap<Uuid, Vec<&'a str>>,
    spend: &'a HashMap<String, Money4>,
}

impl<'a> BudgetListingIndex<'a> {
    fn new(
        budgets: &'a [BudgetRecord],
        contacts: &'a [BudgetContact],
        spend: &'a HashMap<String, Money4>,
    ) -> Self {
        let mut recipients: HashMap<Uuid, Vec<&str>> = HashMap::new();
        for contact in contacts {
            if let Some(team_id) = contact.alert_team_id {
                recipients.entry(team_id).or_default().push(&contact.email);
            }
        }
        for emails in recipients.values_mut() {
            emails.sort_unstable();
            emails.dedup();
        }
        Self {
            budgets: budgets
                .iter()
                .map(|budget| (budget.scope_key.as_str(), budget))
                .collect(),
            emails: contacts
                .iter()
                .map(|contact| (contact.user_id, contact.email.as_str()))
                .collect(),
            recipients,
            spend,
        }
    }

    fn spend(&self, scope_key: &str) -> i64 {
        self.spend
            .get(scope_key)
            .copied()
            .unwrap_or(Money4::ZERO)
            .as_scaled_i64()
    }

    fn user_view(&self, user: IdentityUserRecord) -> SpendBudgetUserView {
        let key = BudgetScope::User {
            user_id: user.user.user_id,
        }
        .scope_key();
        let budget = self.budgets.get(key.as_str());
        SpendBudgetUserView {
            user_id: user.user.user_id.to_string(),
            name: user.user.name,
            email: user.user.email.clone(),
            team_id: user.team_id.map(|id| id.to_string()),
            team_name: user.team_name,
            budget: budget.map(|budget| budget_to_settings_view(budget)),
            budget_source: budget.map(|budget| budget_source_to_view(&budget.source)),
            current_window_spend_usd_10000: self.spend(&key),
            alert_email_ready: true,
            alert_recipient_summary: user.user.email,
        }
    }

    fn service_account_view(
        &self,
        account: ServiceAccountRecord,
        team: &TeamRecord,
    ) -> SpendBudgetServiceAccountView {
        let key = BudgetScope::ServiceAccount {
            service_account_id: account.service_account_id,
        }
        .scope_key();
        let budget = self.budgets.get(key.as_str());
        let recipients = self
            .recipients
            .get(&account.team_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        SpendBudgetServiceAccountView {
            service_account_id: account.service_account_id.to_string(),
            service_account_name: account.service_account_name,
            service_account_key: account.service_account_key,
            team_id: team.team_id.to_string(),
            team_name: team.team_name.clone(),
            team_key: team.team_key.clone(),
            budget: budget.map(|budget| budget_to_settings_view(budget)),
            budget_source: budget.map(|budget| budget_source_to_view(&budget.source)),
            current_window_spend_usd_10000: self.spend(&key),
            alert_email_ready: !recipients.is_empty(),
            alert_recipient_summary: if recipients.is_empty() {
                "No active team owners/admins with email addresses".to_string()
            } else {
                recipients.join(", ")
            },
        }
    }

    fn model_view(&self, budget: &BudgetRecord) -> Option<SpendBudgetUserModelView> {
        let BudgetScope::UserModel { user_id, selector } = &budget.scope else {
            return None;
        };
        let email = self.emails.get(user_id);
        Some(SpendBudgetUserModelView {
            budget_id: budget.budget_id.to_string(),
            scope_key: budget.scope_key.clone(),
            user_id: user_id.to_string(),
            model_id: selector.model_id().map(|id| id.to_string()),
            upstream_model: selector.upstream_model().map(ToOwned::to_owned),
            budget: budget_to_settings_view(budget),
            budget_source: budget_source_to_view(&budget.source),
            current_window_spend_usd_10000: self.spend(&budget.scope_key),
            alert_email_ready: email.is_some(),
            alert_recipient_summary: email
                .copied()
                .unwrap_or("Budget user no longer exists")
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::{
        AuthMode, BudgetCadence, BudgetModelSelector, BudgetRepository, BudgetSettings, GlobalRole,
        MembershipRole, UserStatus,
    };
    use gateway_store::{LibsqlStore, run_migrations};

    #[tokio::test]
    async fn budget_listing_preserves_contacts_sources_and_unbudgeted_users() -> anyhow::Result<()>
    {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("listing.db");
        run_migrations(&path).await?;
        let store = LibsqlStore::new_local(path.to_str().expect("path")).await?;
        let now = OffsetDateTime::now_utc();
        let team = store.create_team("team", "Team").await?;
        let account = store
            .create_service_account(team.team_id, "worker", "Worker", now)
            .await?;
        for (email, role, status) in [
            (
                "z-owner@example.com",
                MembershipRole::Owner,
                UserStatus::Active,
            ),
            (
                "a-admin@example.com",
                MembershipRole::Admin,
                UserStatus::Active,
            ),
            (
                "member@example.com",
                MembershipRole::Member,
                UserStatus::Active,
            ),
            (
                "disabled@example.com",
                MembershipRole::Owner,
                UserStatus::Disabled,
            ),
        ] {
            let user = store
                .create_identity_user(
                    email,
                    email,
                    email,
                    GlobalRole::User,
                    AuthMode::Password,
                    status,
                )
                .await?;
            store
                .assign_team_membership(user.user_id, team.team_id, role)
                .await?;
        }
        let bootstrap = store
            .upsert_bootstrap_admin_user("Admin", "admin@local", true)
            .await?;
        let settings = BudgetSettings {
            cadence: BudgetCadence::Monthly,
            amount_usd: Money4::from_scaled(2500),
            hard_limit: true,
            timezone: "UTC".into(),
        };
        let scope = BudgetScope::UserModel {
            user_id: bootstrap.user_id,
            selector: BudgetModelSelector::UpstreamModel {
                upstream_model: "expensive-model".into(),
            },
        };
        store.upsert_active_budget(&scope, &settings, now).await?;
        store
            .upsert_active_budget(
                &BudgetScope::ServiceAccount {
                    service_account_id: account.service_account_id,
                },
                &settings,
                now,
            )
            .await?;
        let listing = load_budget_listing(&store, now)
            .await
            .map_err(|error| error.0)?;
        assert_eq!(listing.users.len(), 4);
        assert!(
            listing
                .users
                .iter()
                .all(|user| user.budget.is_none() && user.current_window_spend_usd_10000 == 0)
        );
        assert_eq!(
            listing.service_accounts[0].alert_recipient_summary,
            "a-admin@example.com, z-owner@example.com"
        );
        assert!(listing.service_accounts[0].alert_email_ready);
        assert_eq!(
            listing.service_accounts[0]
                .budget_source
                .as_ref()
                .expect("source")
                .kind,
            "manual"
        );
        assert_eq!(listing.user_model_budgets.len(), 1);
        assert!(listing.user_model_budgets[0].alert_email_ready);
        assert_eq!(
            listing.user_model_budgets[0].alert_recipient_summary,
            "admin@local"
        );
        assert_eq!(
            listing.user_model_budgets[0].upstream_model.as_deref(),
            Some("expensive-model")
        );
        Ok(())
    }
}
