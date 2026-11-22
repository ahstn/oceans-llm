use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use gateway_core::{
    ApiKeyOwnerKind, BudgetAlertChannel, BudgetAlertDeliveryRecord, BudgetAlertDeliveryStatus,
    BudgetAlertDispatchTask, BudgetAlertRecord, BudgetAlertRepository, BudgetCadence, BudgetRecord,
    BudgetRepository, BudgetScope, GatewayError, IdentityRepository, MembershipRole, Money4,
    Money9, UsageLedgerRecord, UserStatus, budget_window_utc,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{budget_guard::BudgetSpend, budget_scopes::effective_budget_limit};

pub const BUDGET_ALERT_THRESHOLD_BPS: i32 = 2_000;

/// Entries in [`AlertedWindows`] above which ended windows are dropped.
const ALERTED_WINDOWS_PRUNE_AT: usize = 4_096;

#[derive(Debug, Clone)]
pub struct BudgetAlertEmail {
    pub recipient: String,
    pub subject: String,
    pub text_body: String,
}

#[derive(Debug, Clone, Default)]
pub struct BudgetAlertSendResult {
    pub provider_message_id: Option<String>,
}

#[async_trait]
pub trait BudgetAlertSender: Send + Sync {
    async fn send(&self, email: &BudgetAlertEmail) -> anyhow::Result<BudgetAlertSendResult>;
}

#[derive(Debug, Default)]
pub struct SinkBudgetAlertSender;

#[async_trait]
impl BudgetAlertSender for SinkBudgetAlertSender {
    async fn send(&self, email: &BudgetAlertEmail) -> anyhow::Result<BudgetAlertSendResult> {
        info!(
            recipient = %email.recipient,
            subject = %email.subject,
            "budget alert email sent via sink transport"
        );
        Ok(BudgetAlertSendResult {
            provider_message_id: Some(format!("sink:{}", Uuid::new_v4())),
        })
    }
}

#[derive(Clone)]
pub struct BudgetAlertService<R> {
    repo: Arc<R>,
    sender: Arc<dyn BudgetAlertSender>,
    alerted_windows: Arc<AlertedWindows>,
}

/// Budget windows (budget, window start) for which this process already asked the
/// store to create the threshold alert, with each window's end for pruning.
///
/// Spend comes from a cached total, and another replica's charges reach it only on
/// a reload. A reload can therefore show a window already past the threshold
/// without any charge seen here crossing it. The first charge this process sees
/// past the threshold in a window still asks the store, which keeps one alert per
/// window; later charges in that window skip the write.
#[derive(Default)]
struct AlertedWindows(Mutex<HashMap<(Uuid, OffsetDateTime), OffsetDateTime>>);

impl AlertedWindows {
    fn contains(&self, key: &(Uuid, OffsetDateTime)) -> bool {
        self.0
            .lock()
            .expect("alerted windows lock poisoned")
            .contains_key(key)
    }

    fn insert(&self, key: (Uuid, OffsetDateTime), window_end: OffsetDateTime) {
        let mut windows = self.0.lock().expect("alerted windows lock poisoned");
        if windows.len() >= ALERTED_WINDOWS_PRUNE_AT {
            let now = OffsetDateTime::now_utc();
            windows.retain(|_, end| *end > now);
        }
        windows.insert(key, window_end);
    }
}

#[derive(Debug, Clone)]
struct AlertOwnerContext {
    owner_kind: ApiKeyOwnerKind,
    owner_id: Uuid,
    owner_name: String,
    ownership_scope_key: String,
    recipients: Vec<String>,
}

#[derive(Debug, Clone)]
struct AlertEvaluation {
    owner: AlertOwnerContext,
    budget_id: Uuid,
    cadence: BudgetCadence,
    budget_amount: Money4,
    occurred_at: OffsetDateTime,
    spent_before: Money9,
    spent_after: Money9,
    immediate_on_existing_threshold: bool,
}

impl<R> BudgetAlertService<R>
where
    R: BudgetRepository + BudgetAlertRepository + IdentityRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>, sender: Arc<dyn BudgetAlertSender>) -> Self {
        Self {
            repo,
            sender,
            alerted_windows: Arc::default(),
        }
    }

    /// Raises alerts for the budgets a recorded charge crossed into the threshold, from the
    /// window spend [`crate::budget_guard::BudgetGuard::spend_after_usage`] already read.
    pub async fn evaluate_after_usage(
        &self,
        ledger: &UsageLedgerRecord,
        budgets: &[BudgetSpend],
    ) -> Result<(), GatewayError> {
        for BudgetSpend {
            budget,
            spent_usd: spent_after,
        } in budgets
        {
            let spent_before = spent_after
                .checked_sub(ledger.computed_cost_usd)
                .ok_or_else(|| {
                    GatewayError::Internal(
                        "budget threshold spend subtraction overflow".to_string(),
                    )
                })?;
            let owner = self.resolve_budget_owner(budget).await?;
            let budget_amount = if alert_possible_against_base(budget, *spent_after) {
                effective_budget_limit(self.repo.as_ref(), budget, ledger.occurred_at).await?
            } else {
                budget.settings.amount_usd
            };
            self.create_alert_if_needed(AlertEvaluation {
                owner,
                budget_id: budget.budget_id,
                cadence: budget.settings.cadence,
                budget_amount,
                occurred_at: ledger.occurred_at,
                spent_before,
                spent_after: *spent_after,
                immediate_on_existing_threshold: false,
            })
            .await?;
        }
        Ok(())
    }

    pub async fn evaluate_after_budget_upsert(
        &self,
        budget: &BudgetRecord,
        current_spend: Money9,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        let owner = self.resolve_budget_owner(budget).await?;
        let budget_amount = effective_budget_limit(self.repo.as_ref(), budget, occurred_at).await?;
        self.create_alert_if_needed(AlertEvaluation {
            owner,
            budget_id: budget.budget_id,
            cadence: budget.settings.cadence,
            budget_amount,
            occurred_at,
            spent_before: current_spend,
            spent_after: current_spend,
            immediate_on_existing_threshold: true,
        })
        .await
    }

    pub async fn dispatch_pending_deliveries(&self, limit: u32) -> Result<usize, GatewayError> {
        let claimed_at = OffsetDateTime::now_utc();
        let tasks = self
            .repo
            .claim_pending_budget_alert_delivery_tasks(limit, claimed_at)
            .await?;
        let mut sent_count = 0_usize;

        for task in tasks {
            let recipient = match task.delivery.recipient.clone() {
                Some(recipient) if !recipient.trim().is_empty() => recipient,
                _ => {
                    self.repo
                        .mark_budget_alert_delivery_failed(
                            task.delivery.budget_alert_delivery_id,
                            "missing recipient email",
                            claimed_at,
                        )
                        .await?;
                    continue;
                }
            };

            let email = BudgetAlertEmail {
                recipient,
                subject: format!(
                    "Budget alert: {} is below 20% remaining",
                    task.alert.owner_name
                ),
                text_body: render_budget_alert_email(&task)?,
            };

            match self.sender.send(&email).await {
                Ok(result) => {
                    self.repo
                        .mark_budget_alert_delivery_sent(
                            task.delivery.budget_alert_delivery_id,
                            result.provider_message_id.as_deref(),
                            OffsetDateTime::now_utc(),
                        )
                        .await?;
                    sent_count += 1;
                }
                Err(error) => {
                    warn!(
                        delivery_id = %task.delivery.budget_alert_delivery_id,
                        error = %error,
                        "budget alert email delivery failed"
                    );
                    self.repo
                        .mark_budget_alert_delivery_failed(
                            task.delivery.budget_alert_delivery_id,
                            &error.to_string(),
                            OffsetDateTime::now_utc(),
                        )
                        .await?;
                }
            }
        }

        Ok(sent_count)
    }

    async fn create_alert_if_needed(
        &self,
        evaluation: AlertEvaluation,
    ) -> Result<(), GatewayError> {
        let remaining_before =
            remaining_budget_floor_zero(evaluation.budget_amount, evaluation.spent_before);
        let remaining_after =
            remaining_budget_floor_zero(evaluation.budget_amount, evaluation.spent_after);
        if !is_at_or_below_threshold(remaining_after, evaluation.budget_amount) {
            return Ok(());
        }
        let crossed_threshold =
            !is_at_or_below_threshold(remaining_before, evaluation.budget_amount);
        let window = budget_window_utc(evaluation.cadence, evaluation.occurred_at);
        let window_key = (evaluation.budget_id, window.period_start);
        if !crossed_threshold
            && !evaluation.immediate_on_existing_threshold
            && self.alerted_windows.contains(&window_key)
        {
            return Ok(());
        }

        let created_at = OffsetDateTime::now_utc();
        let alert = BudgetAlertRecord {
            budget_alert_id: Uuid::new_v4(),
            ownership_scope_key: evaluation.owner.ownership_scope_key,
            owner_kind: evaluation.owner.owner_kind,
            owner_id: evaluation.owner.owner_id,
            owner_name: evaluation.owner.owner_name,
            budget_id: evaluation.budget_id,
            cadence: evaluation.cadence,
            threshold_bps: BUDGET_ALERT_THRESHOLD_BPS,
            window_start: window.period_start,
            window_end: window.period_end,
            spend_before_usd: evaluation.spent_before.round_half_up_to_money4(),
            spend_after_usd: evaluation.spent_after.round_half_up_to_money4(),
            remaining_budget_usd: evaluation
                .budget_amount
                .checked_sub(evaluation.spent_after.round_half_up_to_money4())
                .filter(|remaining| !remaining.is_negative())
                .unwrap_or(Money4::ZERO),
            created_at,
            updated_at: created_at,
        };

        let deliveries = build_delivery_records(&alert, &evaluation.owner.recipients, created_at);
        // The store keeps one alert per budget window, so `false` means it exists.
        self.repo
            .create_budget_alert_with_deliveries(&alert, &deliveries)
            .await?;
        self.alerted_windows.insert(window_key, window.period_end);
        Ok(())
    }

    async fn resolve_budget_owner(
        &self,
        budget: &BudgetRecord,
    ) -> Result<AlertOwnerContext, GatewayError> {
        let mut owner = match budget.scope {
            BudgetScope::User { user_id } | BudgetScope::UserModel { user_id, .. } => {
                self.resolve_user_owner(user_id).await?
            }
            BudgetScope::ServiceAccount { service_account_id } => {
                self.resolve_service_account_owner(service_account_id)
                    .await?
            }
        };
        owner.ownership_scope_key = budget.scope_key.clone();
        Ok(owner)
    }

    async fn resolve_user_owner(&self, user_id: Uuid) -> Result<AlertOwnerContext, GatewayError> {
        let user = self.repo.get_user_by_id(user_id).await?.ok_or_else(|| {
            GatewayError::Internal(format!("budget alert user `{user_id}` missing"))
        })?;

        Ok(AlertOwnerContext {
            owner_kind: ApiKeyOwnerKind::User,
            owner_id: user.user_id,
            owner_name: user.name,
            ownership_scope_key: format!("user:{user_id}"),
            recipients: vec![user.email],
        })
    }

    async fn team_alert_recipients(&self, team_id: Uuid) -> Result<Vec<String>, GatewayError> {
        let memberships = self.repo.list_team_memberships(team_id).await?;
        let mut recipients = BTreeSet::new();

        for membership in memberships {
            if !matches!(
                membership.role,
                MembershipRole::Owner | MembershipRole::Admin
            ) {
                continue;
            }
            let Some(user) = self.repo.get_user_by_id(membership.user_id).await? else {
                continue;
            };
            if user.status != UserStatus::Active {
                continue;
            }
            recipients.insert(user.email);
        }

        Ok(recipients.into_iter().collect())
    }

    async fn resolve_service_account_owner(
        &self,
        service_account_id: Uuid,
    ) -> Result<AlertOwnerContext, GatewayError> {
        let service_account = self
            .repo
            .get_service_account_by_id(service_account_id)
            .await?
            .ok_or_else(|| {
                GatewayError::Internal(format!(
                    "budget alert service account `{service_account_id}` missing"
                ))
            })?;
        Ok(AlertOwnerContext {
            owner_kind: ApiKeyOwnerKind::ServiceAccount,
            owner_id: service_account.service_account_id,
            owner_name: service_account.service_account_name,
            ownership_scope_key: format!("service_account:{service_account_id}"),
            recipients: self.team_alert_recipients(service_account.team_id).await?,
        })
    }
}

fn remaining_budget_floor_zero(budget_amount: Money4, spend: Money9) -> i128 {
    (i128::from(budget_amount.as_scaled_i64()) * i128::from(Money9::SCALE / Money4::SCALE)
        - i128::from(spend.as_nanos()))
    .max(0)
}

fn build_delivery_records(
    alert: &BudgetAlertRecord,
    recipients: &[String],
    queued_at: OffsetDateTime,
) -> Vec<BudgetAlertDeliveryRecord> {
    if recipients.is_empty() {
        return vec![BudgetAlertDeliveryRecord {
            budget_alert_delivery_id: Uuid::new_v4(),
            budget_alert_id: alert.budget_alert_id,
            channel: BudgetAlertChannel::Email,
            delivery_status: BudgetAlertDeliveryStatus::Failed,
            recipient: None,
            provider_message_id: None,
            failure_reason: Some("no eligible email recipients".to_string()),
            queued_at,
            last_attempted_at: Some(queued_at),
            sent_at: None,
            updated_at: queued_at,
        }];
    }

    recipients
        .iter()
        .map(|recipient| BudgetAlertDeliveryRecord {
            budget_alert_delivery_id: Uuid::new_v4(),
            budget_alert_id: alert.budget_alert_id,
            channel: BudgetAlertChannel::Email,
            delivery_status: BudgetAlertDeliveryStatus::Pending,
            recipient: Some(recipient.clone()),
            provider_message_id: None,
            failure_reason: None,
            queued_at,
            last_attempted_at: None,
            sent_at: None,
            updated_at: queued_at,
        })
        .collect()
}

fn is_at_or_below_threshold(remaining_budget: i128, total_budget: Money4) -> bool {
    remaining_budget * i128::from(Money4::SCALE)
        <= i128::from(total_budget.as_scaled_i64())
            * i128::from(Money9::SCALE / Money4::SCALE)
            * i128::from(BUDGET_ALERT_THRESHOLD_BPS)
}

fn render_budget_alert_email(task: &BudgetAlertDispatchTask) -> Result<String, GatewayError> {
    let window_start = task.alert.window_start.format(&Rfc3339).map_err(|error| {
        GatewayError::Internal(format!("failed formatting alert start: {error}"))
    })?;
    let window_end =
        task.alert.window_end.format(&Rfc3339).map_err(|error| {
            GatewayError::Internal(format!("failed formatting alert end: {error}"))
        })?;

    Ok(format!(
        "Budget threshold alert\n\nOwner: {owner_name}\nScope: {owner_kind}:{owner_id}\nCadence: {cadence}\nThreshold: 20% remaining\nWindow: {window_start} to {window_end}\nSpend before threshold: ${spend_before}\nSpend after threshold: ${spend_after}\nRemaining budget: ${remaining}\n",
        owner_name = task.alert.owner_name,
        owner_kind = task.alert.owner_kind.as_str(),
        owner_id = task.alert.owner_id,
        cadence = task.alert.cadence.as_str(),
        spend_before = task.alert.spend_before_usd,
        spend_after = task.alert.spend_after_usd,
        remaining = task.alert.remaining_budget_usd,
    ))
}

/// Whether `spent` reaches the alert threshold against the base budget. An increase only
/// raises the limit, so when this is false no alert can fire and the increase lookup is skipped.
fn alert_possible_against_base(budget: &BudgetRecord, spent: Money9) -> bool {
    let base = budget.settings.amount_usd;
    is_at_or_below_threshold(remaining_budget_floor_zero(base, spent), base)
}

#[cfg(test)]
mod tests {
    use gateway_core::{
        BudgetCadence, BudgetModelSelector, BudgetRecord, BudgetScope, BudgetSettings,
        BudgetSource, Money4, Money9,
    };
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::alert_possible_against_base;

    #[test]
    fn alert_is_only_possible_once_spend_reaches_the_base_threshold() {
        let scope = BudgetScope::User {
            user_id: Uuid::new_v4(),
        };
        let budget = BudgetRecord {
            budget_id: Uuid::new_v4(),
            scope_key: scope.scope_key(),
            scope,
            settings: BudgetSettings {
                cadence: BudgetCadence::Monthly,
                amount_usd: Money4::from_scaled(1_000_000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            source: BudgetSource::manual(),
            is_active: true,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        assert!(!alert_possible_against_base(
            &budget,
            Money9::from_nanos(79_000_000_000)
        ));
        assert!(!alert_possible_against_base(
            &budget,
            Money9::from_nanos(79_999_999_999)
        ));
        assert!(alert_possible_against_base(
            &budget,
            Money9::from_nanos(80_000_000_000)
        ));
        assert!(alert_possible_against_base(
            &budget,
            Money9::from_nanos(120_000_000_000)
        ));
    }

    #[test]
    fn user_model_scope_keys_are_canonical() {
        let user_id = Uuid::new_v4();
        let model_id = Uuid::new_v4();
        assert_eq!(
            BudgetScope::UserModel {
                user_id,
                selector: BudgetModelSelector::Model { model_id },
            }
            .scope_key(),
            format!("budget:v1:user:{user_id}:model:{model_id}")
        );
    }
}
