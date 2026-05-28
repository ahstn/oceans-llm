use std::{collections::BTreeSet, sync::Arc};

use async_trait::async_trait;
use gateway_core::{
    ApiKeyOwnerKind, AuthenticatedApiKey, BudgetAlertChannel, BudgetAlertDeliveryRecord,
    BudgetAlertDeliveryStatus, BudgetAlertDispatchTask, BudgetAlertRecord, BudgetAlertRepository,
    BudgetCadence, BudgetRecord, BudgetRepository, BudgetScope, GatewayError, IdentityRepository,
    MembershipRole, Money4, UsageLedgerRecord, UserStatus, budget_window_utc,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{info, warn};
use uuid::Uuid;

use crate::budget_scopes::applicable_budget_scopes;

pub const BUDGET_ALERT_THRESHOLD_BPS: i32 = 2_000;

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
    spent_before: Money4,
    spent_after: Money4,
    immediate_on_existing_threshold: bool,
}

impl<R> BudgetAlertService<R>
where
    R: BudgetRepository + BudgetAlertRepository + IdentityRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>, sender: Arc<dyn BudgetAlertSender>) -> Self {
        Self { repo, sender }
    }

    pub async fn evaluate_after_usage(
        &self,
        api_key: &AuthenticatedApiKey,
        ledger: &UsageLedgerRecord,
    ) -> Result<(), GatewayError> {
        if !ledger.pricing_status.counts_toward_spend() {
            return Ok(());
        }

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
            let (window_start, window_end) =
                usage_window_bounds(budget.settings.cadence, ledger.occurred_at)?;
            let spent_after = self
                .repo
                .sum_usage_cost_for_budget_scope_in_window(&scope, window_start, window_end)
                .await?;
            let spent_before = spent_after
                .checked_sub(ledger.computed_cost_usd)
                .ok_or_else(|| {
                    GatewayError::Internal(
                        "budget threshold spend subtraction overflow".to_string(),
                    )
                })?;
            let owner = self.resolve_budget_owner(&budget).await?;
            self.create_alert_if_needed(AlertEvaluation {
                owner,
                budget_id: budget.budget_id,
                cadence: budget.settings.cadence,
                budget_amount: budget.settings.amount_usd,
                occurred_at: ledger.occurred_at,
                spent_before,
                spent_after,
                immediate_on_existing_threshold: false,
            })
            .await?;
        }
        Ok(())
    }

    pub async fn evaluate_after_budget_upsert(
        &self,
        budget: &BudgetRecord,
        current_spend: Money4,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        let owner = self.resolve_budget_owner(budget).await?;
        self.create_alert_if_needed(AlertEvaluation {
            owner,
            budget_id: budget.budget_id,
            cadence: budget.settings.cadence,
            budget_amount: budget.settings.amount_usd,
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

        let crossed_threshold = is_at_or_below_threshold(remaining_after, evaluation.budget_amount)
            && !is_at_or_below_threshold(remaining_before, evaluation.budget_amount);
        let already_below_threshold = evaluation.immediate_on_existing_threshold
            && is_at_or_below_threshold(remaining_after, evaluation.budget_amount);

        if !crossed_threshold && !already_below_threshold {
            return Ok(());
        }

        let window = budget_window_utc(evaluation.cadence, evaluation.occurred_at)
            .map_err(GatewayError::Internal)?;
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
            spend_before_usd: evaluation.spent_before,
            spend_after_usd: evaluation.spent_after,
            remaining_budget_usd: remaining_after,
            created_at,
            updated_at: created_at,
        };

        let deliveries = build_delivery_records(&alert, &evaluation.owner.recipients, created_at);
        let inserted = self
            .repo
            .create_budget_alert_with_deliveries(&alert, &deliveries)
            .await?;
        if !inserted {
            return Ok(());
        }

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

fn remaining_budget_floor_zero(budget_amount: Money4, spend: Money4) -> Money4 {
    match budget_amount.checked_sub(spend) {
        Some(remaining) if !remaining.is_negative() => remaining,
        Some(_) | None => Money4::ZERO,
    }
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

fn is_at_or_below_threshold(remaining_budget: Money4, total_budget: Money4) -> bool {
    i128::from(remaining_budget.as_scaled_i64()) * 10_000
        <= i128::from(total_budget.as_scaled_i64()) * i128::from(BUDGET_ALERT_THRESHOLD_BPS)
}

fn usage_window_bounds(
    cadence: BudgetCadence,
    occurred_at: OffsetDateTime,
) -> Result<(OffsetDateTime, OffsetDateTime), GatewayError> {
    let window = budget_window_utc(cadence, occurred_at).map_err(GatewayError::Internal)?;
    Ok((window.period_start, window.observed_end))
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

#[cfg(test)]
mod tests {
    use gateway_core::{BudgetModelSelector, BudgetScope};
    use uuid::Uuid;

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
