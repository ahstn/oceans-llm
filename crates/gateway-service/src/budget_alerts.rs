use std::{collections::BTreeSet, sync::Arc};

use async_trait::async_trait;
use gateway_core::{
    ApiKeyOwnerKind, AuthenticatedApiKey, BudgetAlertChannel, BudgetAlertDeliveryRecord,
    BudgetAlertDeliveryStatus, BudgetAlertDispatchTask, BudgetAlertRecord, BudgetAlertRepository,
    BudgetCadence, BudgetRepository, GatewayError, IdentityRepository, MembershipRole, Money4,
    TeamBudgetRecord, UsageLedgerRecord, UserBudgetRecord, UserStatus, budget_window_utc,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{info, warn};
use uuid::Uuid;

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

        match api_key.owner_kind {
            ApiKeyOwnerKind::User => {
                let user_id = api_key.owner_user_id.ok_or_else(|| {
                    GatewayError::Internal("user-owned key missing user_id".to_string())
                })?;
                let Some(budget) = self.repo.get_active_budget_for_user(user_id).await? else {
                    return Ok(());
                };
                let (window_start, window_end) =
                    usage_window_bounds(budget.cadence, ledger.occurred_at)?;
                let spent_after = self
                    .repo
                    .sum_usage_cost_for_user_in_window(user_id, window_start, window_end)
                    .await?;
                let spent_before = spent_after
                    .checked_sub(ledger.computed_cost_usd)
                    .ok_or_else(|| {
                        GatewayError::Internal(
                            "budget threshold spend subtraction overflow".to_string(),
                        )
                    })?;
                let owner = self.resolve_user_owner(user_id).await?;
                self.create_alert_if_needed(AlertEvaluation {
                    owner,
                    budget_id: budget.user_budget_id,
                    cadence: budget.cadence,
                    budget_amount: budget.amount_usd,
                    occurred_at: ledger.occurred_at,
                    spent_before,
                    spent_after,
                    immediate_on_existing_threshold: false,
                })
                .await
            }
            ApiKeyOwnerKind::Team => {
                let team_id = api_key.owner_team_id.ok_or_else(|| {
                    GatewayError::Internal("team-owned key missing team_id".to_string())
                })?;
                let Some(budget) = self.repo.get_active_budget_for_team(team_id).await? else {
                    return Ok(());
                };
                let (window_start, window_end) =
                    usage_window_bounds(budget.cadence, ledger.occurred_at)?;
                let spent_after = self
                    .repo
                    .sum_usage_cost_for_team_in_window(team_id, window_start, window_end)
                    .await?;
                let spent_before = spent_after
                    .checked_sub(ledger.computed_cost_usd)
                    .ok_or_else(|| {
                        GatewayError::Internal(
                            "budget threshold spend subtraction overflow".to_string(),
                        )
                    })?;
                let owner = self.resolve_team_owner(team_id).await?;
                self.create_alert_if_needed(AlertEvaluation {
                    owner,
                    budget_id: budget.team_budget_id,
                    cadence: budget.cadence,
                    budget_amount: budget.amount_usd,
                    occurred_at: ledger.occurred_at,
                    spent_before,
                    spent_after,
                    immediate_on_existing_threshold: false,
                })
                .await
            }
        }
    }

    pub async fn evaluate_after_user_budget_upsert(
        &self,
        budget: &UserBudgetRecord,
        current_spend: Money4,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        let owner = self.resolve_user_owner(budget.user_id).await?;
        self.create_alert_if_needed(AlertEvaluation {
            owner,
            budget_id: budget.user_budget_id,
            cadence: budget.cadence,
            budget_amount: budget.amount_usd,
            occurred_at,
            spent_before: current_spend,
            spent_after: current_spend,
            immediate_on_existing_threshold: true,
        })
        .await
    }

    pub async fn evaluate_after_team_budget_upsert(
        &self,
        budget: &TeamBudgetRecord,
        current_spend: Money4,
        occurred_at: OffsetDateTime,
    ) -> Result<(), GatewayError> {
        let owner = self.resolve_team_owner(budget.team_id).await?;
        self.create_alert_if_needed(AlertEvaluation {
            owner,
            budget_id: budget.team_budget_id,
            cadence: budget.cadence,
            budget_amount: budget.amount_usd,
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

    async fn resolve_team_owner(&self, team_id: Uuid) -> Result<AlertOwnerContext, GatewayError> {
        let team = self.repo.get_team_by_id(team_id).await?.ok_or_else(|| {
            GatewayError::Internal(format!("budget alert team `{team_id}` missing"))
        })?;
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

        Ok(AlertOwnerContext {
            owner_kind: ApiKeyOwnerKind::Team,
            owner_id: team.team_id,
            owner_name: team.team_name,
            ownership_scope_key: format!("team:{team_id}:actor:none"),
            recipients: recipients.into_iter().collect(),
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
    use std::sync::{Arc, Mutex};

    use gateway_core::{
        AuthMode, BudgetAlertDispatchTask, BudgetAlertHistoryPage, BudgetAlertHistoryQuery,
        BudgetAlertRepository, GlobalRole, IdentityRepository, ModelAccessMode, StoreError,
        TeamMembershipRecord, TeamRecord, UserBudgetRecord, UserRecord,
    };
    use time::OffsetDateTime;

    use super::*;

    #[derive(Clone, Default)]
    struct InMemoryRepo {
        user: Option<UserRecord>,
        team: Option<TeamRecord>,
        team_memberships: Vec<TeamMembershipRecord>,
        team_users: Vec<UserRecord>,
        active_team_budget: Option<TeamBudgetRecord>,
        team_spend: Money4,
        alerts: Arc<Mutex<Vec<BudgetAlertRecord>>>,
        deliveries: Arc<Mutex<Vec<BudgetAlertDeliveryRecord>>>,
    }

    #[async_trait]
    impl IdentityRepository for InMemoryRepo {
        async fn get_user_by_id(&self, user_id: Uuid) -> Result<Option<UserRecord>, StoreError> {
            if self
                .user
                .as_ref()
                .is_some_and(|user| user.user_id == user_id)
            {
                return Ok(self.user.clone());
            }
            Ok(self
                .team_users
                .iter()
                .find(|user| user.user_id == user_id)
                .cloned())
        }

        async fn get_team_by_id(&self, team_id: Uuid) -> Result<Option<TeamRecord>, StoreError> {
            Ok(self.team.clone().filter(|team| team.team_id == team_id))
        }

        async fn get_team_membership_for_user(
            &self,
            user_id: Uuid,
        ) -> Result<Option<TeamMembershipRecord>, StoreError> {
            Ok(self
                .team_memberships
                .iter()
                .find(|membership| membership.user_id == user_id)
                .cloned())
        }

        async fn list_allowed_model_keys_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }

        async fn list_allowed_model_keys_for_team(
            &self,
            _team_id: Uuid,
        ) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }

        async fn list_team_memberships(
            &self,
            team_id: Uuid,
        ) -> Result<Vec<TeamMembershipRecord>, StoreError> {
            Ok(self
                .team_memberships
                .iter()
                .filter(|membership| membership.team_id == team_id)
                .cloned()
                .collect())
        }
    }

    #[async_trait]
    impl BudgetRepository for InMemoryRepo {
        async fn get_active_budget_for_user(
            &self,
            _user_id: Uuid,
        ) -> Result<Option<UserBudgetRecord>, StoreError> {
            Ok(None)
        }

        async fn get_active_budget_for_team(
            &self,
            _team_id: Uuid,
        ) -> Result<Option<TeamBudgetRecord>, StoreError> {
            Ok(self.active_team_budget.clone())
        }

        async fn get_usage_ledger_by_request_and_scope(
            &self,
            _request_id: &str,
            _ownership_scope_key: &str,
        ) -> Result<Option<UsageLedgerRecord>, StoreError> {
            Ok(None)
        }

        async fn sum_usage_cost_for_user_in_window(
            &self,
            _user_id: Uuid,
            _window_start: OffsetDateTime,
            _window_end: OffsetDateTime,
        ) -> Result<Money4, StoreError> {
            Ok(Money4::ZERO)
        }

        async fn sum_usage_cost_for_team_in_window(
            &self,
            _team_id: Uuid,
            _window_start: OffsetDateTime,
            _window_end: OffsetDateTime,
        ) -> Result<Money4, StoreError> {
            Ok(self.team_spend)
        }

        async fn insert_usage_ledger_if_absent(
            &self,
            _event: &UsageLedgerRecord,
        ) -> Result<bool, StoreError> {
            Ok(true)
        }
    }

    #[async_trait]
    impl BudgetAlertRepository for InMemoryRepo {
        async fn create_budget_alert_with_deliveries(
            &self,
            alert: &BudgetAlertRecord,
            deliveries: &[BudgetAlertDeliveryRecord],
        ) -> Result<bool, StoreError> {
            let mut alerts = self.alerts.lock().expect("alerts lock");
            if alerts.iter().any(|existing| {
                existing.ownership_scope_key == alert.ownership_scope_key
                    && existing.budget_id == alert.budget_id
                    && existing.threshold_bps == alert.threshold_bps
                    && existing.window_start == alert.window_start
            }) {
                return Ok(false);
            }
            alerts.push(alert.clone());
            self.deliveries
                .lock()
                .expect("deliveries lock")
                .extend(deliveries.iter().cloned());
            Ok(true)
        }

        async fn list_budget_alert_history(
            &self,
            _query: &BudgetAlertHistoryQuery,
        ) -> Result<BudgetAlertHistoryPage, StoreError> {
            Ok(BudgetAlertHistoryPage {
                items: Vec::new(),
                page: 1,
                page_size: 25,
                total: 0,
            })
        }

        async fn claim_pending_budget_alert_delivery_tasks(
            &self,
            limit: u32,
            claimed_at: OffsetDateTime,
        ) -> Result<Vec<BudgetAlertDispatchTask>, StoreError> {
            let alerts = self.alerts.lock().expect("alerts lock").clone();
            let mut deliveries = self.deliveries.lock().expect("deliveries lock");
            let mut tasks = Vec::new();

            for delivery in deliveries.iter_mut() {
                if delivery.delivery_status != BudgetAlertDeliveryStatus::Pending
                    || delivery.last_attempted_at.is_some()
                {
                    continue;
                }
                if tasks.len() >= limit as usize {
                    break;
                }
                delivery.last_attempted_at = Some(claimed_at);
                let alert = alerts
                    .iter()
                    .find(|alert| alert.budget_alert_id == delivery.budget_alert_id)
                    .expect("matching alert")
                    .clone();
                tasks.push(BudgetAlertDispatchTask {
                    alert,
                    delivery: delivery.clone(),
                });
            }

            Ok(tasks)
        }

        async fn mark_budget_alert_delivery_sent(
            &self,
            delivery_id: Uuid,
            provider_message_id: Option<&str>,
            sent_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            let mut deliveries = self.deliveries.lock().expect("deliveries lock");
            let delivery = deliveries
                .iter_mut()
                .find(|delivery| delivery.budget_alert_delivery_id == delivery_id)
                .expect("delivery");
            delivery.delivery_status = BudgetAlertDeliveryStatus::Sent;
            delivery.provider_message_id = provider_message_id.map(ToString::to_string);
            delivery.sent_at = Some(sent_at);
            delivery.updated_at = sent_at;
            Ok(())
        }

        async fn mark_budget_alert_delivery_failed(
            &self,
            delivery_id: Uuid,
            failure_reason: &str,
            failed_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            let mut deliveries = self.deliveries.lock().expect("deliveries lock");
            let delivery = deliveries
                .iter_mut()
                .find(|delivery| delivery.budget_alert_delivery_id == delivery_id)
                .expect("delivery");
            delivery.delivery_status = BudgetAlertDeliveryStatus::Failed;
            delivery.failure_reason = Some(failure_reason.to_string());
            delivery.updated_at = failed_at;
            Ok(())
        }
    }

    fn build_user(user_id: Uuid, name: &str, email: &str, status: UserStatus) -> UserRecord {
        UserRecord {
            user_id,
            name: name.to_string(),
            email: email.to_string(),
            email_normalized: email.to_string(),
            global_role: GlobalRole::User,
            auth_mode: AuthMode::Password,
            status,
            must_change_password: false,
            request_logging_enabled: true,
            model_access_mode: ModelAccessMode::All,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        }
    }

    #[tokio::test]
    async fn monthly_budget_upsert_creates_one_alert_even_when_repeated() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryRepo {
            user: Some(build_user(
                user_id,
                "Jane User",
                "jane@example.com",
                UserStatus::Active,
            )),
            ..InMemoryRepo::default()
        });
        let service = BudgetAlertService::new(repo.clone(), Arc::new(SinkBudgetAlertSender));
        let now = OffsetDateTime::now_utc();
        let budget = UserBudgetRecord {
            user_budget_id: Uuid::new_v4(),
            user_id,
            cadence: BudgetCadence::Monthly,
            amount_usd: Money4::from_scaled(1_000_000),
            hard_limit: true,
            timezone: "UTC".to_string(),
            is_active: true,
            created_at: now,
            updated_at: now,
        };

        service
            .evaluate_after_user_budget_upsert(&budget, Money4::from_scaled(850_000), now)
            .await
            .expect("first alert");
        service
            .evaluate_after_user_budget_upsert(&budget, Money4::from_scaled(850_000), now)
            .await
            .expect("duplicate suppressed");

        let alerts = repo.alerts.lock().expect("alerts lock");
        let deliveries = repo.deliveries.lock().expect("deliveries lock");
        assert_eq!(alerts.len(), 1);
        assert_eq!(deliveries.len(), 1);
        assert_eq!(
            deliveries[0].delivery_status,
            BudgetAlertDeliveryStatus::Pending
        );
    }

    #[tokio::test]
    async fn budget_reconfiguration_can_emit_a_new_alert_in_the_same_window() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryRepo {
            user: Some(build_user(
                user_id,
                "Jane User",
                "jane@example.com",
                UserStatus::Active,
            )),
            ..InMemoryRepo::default()
        });
        let service = BudgetAlertService::new(repo.clone(), Arc::new(SinkBudgetAlertSender));
        let now = OffsetDateTime::now_utc();
        let original_budget = UserBudgetRecord {
            user_budget_id: Uuid::new_v4(),
            user_id,
            cadence: BudgetCadence::Monthly,
            amount_usd: Money4::from_scaled(1_000_000),
            hard_limit: true,
            timezone: "UTC".to_string(),
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        let reconfigured_budget = UserBudgetRecord {
            user_budget_id: Uuid::new_v4(),
            user_id,
            cadence: BudgetCadence::Monthly,
            amount_usd: Money4::from_scaled(900_000),
            hard_limit: true,
            timezone: "UTC".to_string(),
            is_active: true,
            created_at: now,
            updated_at: now,
        };

        service
            .evaluate_after_user_budget_upsert(&original_budget, Money4::from_scaled(850_000), now)
            .await
            .expect("first budget alert");
        service
            .evaluate_after_user_budget_upsert(
                &reconfigured_budget,
                Money4::from_scaled(850_000),
                now + time::Duration::minutes(1),
            )
            .await
            .expect("reconfigured budget alert");

        let alerts = repo.alerts.lock().expect("alerts lock");
        assert_eq!(alerts.len(), 2);
        assert_ne!(alerts[0].budget_id, alerts[1].budget_id);
    }

    #[tokio::test]
    async fn team_usage_crossing_threshold_creates_alert_and_dispatch_sends_to_admin_recipients() {
        let team_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let admin_id = Uuid::new_v4();
        let member_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();
        let repo = Arc::new(InMemoryRepo {
            team: Some(TeamRecord {
                team_id,
                team_key: "platform".to_string(),
                team_name: "Platform".to_string(),
                status: "active".to_string(),
                model_access_mode: ModelAccessMode::All,
                created_at: now,
                updated_at: now,
            }),
            team_memberships: vec![
                TeamMembershipRecord {
                    team_id,
                    user_id: owner_id,
                    role: MembershipRole::Owner,
                    created_at: now,
                    updated_at: now,
                },
                TeamMembershipRecord {
                    team_id,
                    user_id: admin_id,
                    role: MembershipRole::Admin,
                    created_at: now,
                    updated_at: now,
                },
                TeamMembershipRecord {
                    team_id,
                    user_id: member_id,
                    role: MembershipRole::Member,
                    created_at: now,
                    updated_at: now,
                },
            ],
            team_users: vec![
                build_user(owner_id, "Owner", "owner@example.com", UserStatus::Active),
                build_user(admin_id, "Admin", "admin@example.com", UserStatus::Active),
                build_user(
                    member_id,
                    "Member",
                    "member@example.com",
                    UserStatus::Active,
                ),
            ],
            active_team_budget: Some(TeamBudgetRecord {
                team_budget_id: Uuid::new_v4(),
                team_id,
                cadence: BudgetCadence::Weekly,
                amount_usd: Money4::from_scaled(1_000_000),
                hard_limit: false,
                timezone: "UTC".to_string(),
                is_active: true,
                created_at: now,
                updated_at: now,
            }),
            team_spend: Money4::from_scaled(850_000),
            ..InMemoryRepo::default()
        });
        let service = BudgetAlertService::new(repo.clone(), Arc::new(SinkBudgetAlertSender));
        let api_key = AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "gwk_test".to_string(),
            name: "test".to_string(),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_user_id: None,
            owner_team_id: Some(team_id),
        };
        let ledger = UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: "req-1".to_string(),
            ownership_scope_key: format!("team:{team_id}:actor:none"),
            api_key_id: api_key.id,
            user_id: None,
            team_id: Some(team_id),
            actor_user_id: None,
            model_id: None,
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-5".to_string(),
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            total_tokens: Some(150),
            provider_usage: serde_json::json!({}),
            pricing_status: gateway_core::UsagePricingStatus::Priced,
            unpriced_reason: None,
            pricing_row_id: None,
            pricing_provider_id: Some("openai".to_string()),
            pricing_model_id: Some("gpt-5".to_string()),
            pricing_source: None,
            pricing_source_etag: None,
            pricing_source_fetched_at: None,
            pricing_last_updated: None,
            input_cost_per_million_tokens: None,
            output_cost_per_million_tokens: None,
            computed_cost_usd: Money4::from_scaled(100_000),
            occurred_at: now,
        };

        service
            .evaluate_after_usage(&api_key, &ledger)
            .await
            .expect("alert created");
        let sent = service
            .dispatch_pending_deliveries(10)
            .await
            .expect("deliveries sent");

        let alerts = repo.alerts.lock().expect("alerts lock");
        let deliveries = repo.deliveries.lock().expect("deliveries lock");
        assert_eq!(alerts.len(), 1);
        assert_eq!(deliveries.len(), 2);
        assert_eq!(sent, 2);
        assert!(
            deliveries
                .iter()
                .all(|delivery| delivery.delivery_status == BudgetAlertDeliveryStatus::Sent)
        );
    }

    #[tokio::test]
    async fn overspent_budget_upsert_still_creates_alert() {
        let user_id = Uuid::new_v4();
        let repo = Arc::new(InMemoryRepo {
            user: Some(build_user(
                user_id,
                "Jane User",
                "jane@example.com",
                UserStatus::Active,
            )),
            ..InMemoryRepo::default()
        });
        let service = BudgetAlertService::new(repo.clone(), Arc::new(SinkBudgetAlertSender));
        let now = OffsetDateTime::now_utc();
        let budget = UserBudgetRecord {
            user_budget_id: Uuid::new_v4(),
            user_id,
            cadence: BudgetCadence::Monthly,
            amount_usd: Money4::from_scaled(1_000_000),
            hard_limit: true,
            timezone: "UTC".to_string(),
            is_active: true,
            created_at: now,
            updated_at: now,
        };

        service
            .evaluate_after_user_budget_upsert(&budget, Money4::from_scaled(1_250_000), now)
            .await
            .expect("overspent alert should not error");

        let alerts = repo.alerts.lock().expect("alerts lock");
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].remaining_budget_usd, Money4::ZERO);
    }

    #[tokio::test]
    async fn threshold_crossing_to_overspent_still_creates_alert() {
        let team_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();
        let repo = Arc::new(InMemoryRepo {
            team: Some(TeamRecord {
                team_id,
                team_key: "platform".to_string(),
                team_name: "Platform".to_string(),
                status: "active".to_string(),
                model_access_mode: ModelAccessMode::All,
                created_at: now,
                updated_at: now,
            }),
            team_memberships: vec![TeamMembershipRecord {
                team_id,
                user_id: owner_id,
                role: MembershipRole::Owner,
                created_at: now,
                updated_at: now,
            }],
            team_users: vec![build_user(
                owner_id,
                "Owner",
                "owner@example.com",
                UserStatus::Active,
            )],
            active_team_budget: Some(TeamBudgetRecord {
                team_budget_id: Uuid::new_v4(),
                team_id,
                cadence: BudgetCadence::Weekly,
                amount_usd: Money4::from_scaled(1_000_000),
                hard_limit: false,
                timezone: "UTC".to_string(),
                is_active: true,
                created_at: now,
                updated_at: now,
            }),
            team_spend: Money4::from_scaled(1_100_000),
            ..InMemoryRepo::default()
        });
        let service = BudgetAlertService::new(repo.clone(), Arc::new(SinkBudgetAlertSender));
        let api_key = AuthenticatedApiKey {
            id: Uuid::new_v4(),
            public_id: "gwk_test".to_string(),
            name: "test".to_string(),
            owner_kind: ApiKeyOwnerKind::Team,
            owner_user_id: None,
            owner_team_id: Some(team_id),
        };
        let ledger = UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: "req-overspent".to_string(),
            ownership_scope_key: format!("team:{team_id}:actor:none"),
            api_key_id: api_key.id,
            user_id: None,
            team_id: Some(team_id),
            actor_user_id: None,
            model_id: None,
            provider_key: "openai-prod".to_string(),
            upstream_model: "gpt-5".to_string(),
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            total_tokens: Some(150),
            provider_usage: serde_json::json!({}),
            pricing_status: gateway_core::UsagePricingStatus::Priced,
            unpriced_reason: None,
            pricing_row_id: None,
            pricing_provider_id: Some("openai".to_string()),
            pricing_model_id: Some("gpt-5".to_string()),
            pricing_source: None,
            pricing_source_etag: None,
            pricing_source_fetched_at: None,
            pricing_last_updated: None,
            input_cost_per_million_tokens: None,
            output_cost_per_million_tokens: None,
            computed_cost_usd: Money4::from_scaled(400_000),
            occurred_at: now,
        };

        service
            .evaluate_after_usage(&api_key, &ledger)
            .await
            .expect("overspent threshold crossing should still alert");

        let alerts = repo.alerts.lock().expect("alerts lock");
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].remaining_budget_usd, Money4::ZERO);
    }
}
