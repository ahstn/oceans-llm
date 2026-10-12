//! Temporary budget increases for users near their limit.
//!
//! Platform admins can raise any user's budget; team owners/admins can raise
//! budgets for members of their own team. The increase is only available once
//! spend reaches `INCREASE_THRESHOLD_PERCENT` of the current effective limit,
//! and is capped at `MAX_INCREASE_MULTIPLIER` times the base budget.
use axum::{Json, extract::State, http::HeaderMap};
use gateway_core::{
    AuthError, BudgetIncreaseRecord, BudgetRecord, BudgetRepository, BudgetScope, GatewayError,
    IdentityRepository, Money4,
};
use gateway_service::budget_scopes::limit_with_increase;
use time::{Date, Duration, Month, OffsetDateTime, Time};
use uuid::Uuid;

use super::{
    admin_auth::{AdminDataScope, require_spend_controls_scope},
    admin_contract::{
        BudgetIncreasePeriod, BudgetIncreaseView, CreateBudgetIncreaseRequest,
        CreateBudgetIncreaseResultView, Envelope, envelope, format_timestamp,
    },
    error::AppError,
    spend::{budget_to_settings_view, current_window_spend, parse_budget_amount, parse_uuid},
    state::AppState,
};

/// Spend must reach this share of the effective limit before an increase is allowed.
pub(super) const INCREASE_THRESHOLD_PERCENT: i128 = 85;
/// An increase may add at most this multiple of the base budget.
pub(super) const MAX_INCREASE_MULTIPLIER: i64 = 3;

#[utoipa::path(
    post,
    path = "/api/v1/admin/spend/budgets/increases",
    request_body = CreateBudgetIncreaseRequest,
    responses((status = 200, body = Envelope<CreateBudgetIncreaseResultView>)),
    security(("session_cookie" = []))
)]
pub async fn create_budget_increase(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateBudgetIncreaseRequest>,
) -> Result<Json<Envelope<CreateBudgetIncreaseResultView>>, AppError> {
    let (actor, scope) = require_spend_controls_scope(&state, &headers).await?;
    let user_id = parse_uuid(&request.user_id)?;
    if let AdminDataScope::Team(team_id) = scope {
        let membership = state.store.get_team_membership_for_user(user_id).await?;
        if membership.map(|membership| membership.team_id) != Some(team_id) {
            return Err(AppError(GatewayError::Auth(
                AuthError::InsufficientPrivileges,
            )));
        }
    }
    let budget = state
        .store
        .get_active_budget_by_scope(&BudgetScope::User { user_id })
        .await?
        .ok_or_else(|| invalid("user has no active budget to increase"))?;
    let amount_usd = parse_budget_amount(&request.amount_usd)?;
    validate_increase_amount(&budget, amount_usd)?;

    let now = OffsetDateTime::now_utc();
    let spent = current_window_spend(&state, &budget, now).await?;
    let active = state
        .store
        .get_active_budget_increase(budget.budget_id, now)
        .await?;
    let limit = limit_with_increase(&budget, active.as_ref());
    if !within_increase_threshold(spent, limit) {
        return Err(invalid(format!(
            "budget increases are available once spend reaches {INCREASE_THRESHOLD_PERCENT}% of the limit"
        )));
    }

    let increase = BudgetIncreaseRecord {
        budget_increase_id: Uuid::new_v4(),
        budget_id: budget.budget_id,
        scope_key: budget.scope_key.clone(),
        amount_usd,
        starts_at: now,
        expires_at: increase_expires_at(request.period, now),
        created_by_user_id: Some(actor.user_id),
        created_at: now,
    };
    // The store rejects the create if another increase landed since `active` was read,
    // which would have changed the limit this threshold check used.
    let expected_active = active.map(|active| active.budget_increase_id);
    state
        .store
        .create_budget_increase(&increase, expected_active)
        .await?;

    Ok(Json(envelope(CreateBudgetIncreaseResultView {
        user_id: user_id.to_string(),
        budget_id: budget.budget_id.to_string(),
        budget: budget_to_settings_view(&budget),
        effective_amount_usd_10000: budget
            .settings
            .amount_usd
            .saturating_add(amount_usd)
            .as_scaled_i64(),
        current_window_spend_usd_10000: spent.as_scaled_i64(),
        increase: budget_increase_to_view(&increase),
    })))
}

pub(super) fn budget_increase_to_view(increase: &BudgetIncreaseRecord) -> BudgetIncreaseView {
    BudgetIncreaseView {
        budget_increase_id: increase.budget_increase_id.to_string(),
        amount_usd: increase.amount_usd.to_string(),
        amount_usd_10000: increase.amount_usd.as_scaled_i64(),
        starts_at: format_timestamp(increase.starts_at),
        expires_at: format_timestamp(increase.expires_at),
    }
}

fn validate_increase_amount(budget: &BudgetRecord, amount_usd: Money4) -> Result<(), AppError> {
    let base = budget.settings.amount_usd.as_scaled_i64();
    let cap = Money4::from_scaled(base.saturating_mul(MAX_INCREASE_MULTIPLIER));
    if amount_usd > cap {
        return Err(invalid(format!(
            "amount_usd cannot exceed {MAX_INCREASE_MULTIPLIER}x the base budget ({cap})"
        )));
    }
    Ok(())
}

/// Spend at or above the threshold share of `limit`. Over-limit spend qualifies.
fn within_increase_threshold(spent: Money4, limit: Money4) -> bool {
    i128::from(spent.as_scaled_i64()) * 100
        >= i128::from(limit.as_scaled_i64()) * INCREASE_THRESHOLD_PERCENT
}

fn increase_expires_at(period: BudgetIncreasePeriod, now: OffsetDateTime) -> OffsetDateTime {
    match period {
        BudgetIncreasePeriod::SevenDays => now + Duration::days(7),
        BudgetIncreasePeriod::EndOfMonth => {
            let now = now.to_offset(time::UtcOffset::UTC);
            let (year, month) = match now.month() {
                Month::December => (now.year() + 1, Month::January),
                month => (now.year(), month.next()),
            };
            Date::from_calendar_date(year, month, 1)
                .expect("first day of month is valid")
                .with_time(Time::MIDNIGHT)
                .assume_utc()
        }
    }
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError(GatewayError::InvalidRequest(message.into()))
}

#[cfg(test)]
mod handler_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::{BudgetCadence, BudgetSettings, BudgetSource};
    use time::macros::datetime;

    fn budget(amount_10000: i64) -> BudgetRecord {
        let user_id = Uuid::new_v4();
        let scope = BudgetScope::User { user_id };
        BudgetRecord {
            budget_id: Uuid::new_v4(),
            scope_key: scope.scope_key(),
            scope,
            settings: BudgetSettings {
                cadence: BudgetCadence::Monthly,
                amount_usd: Money4::from_scaled(amount_10000),
                hard_limit: true,
                timezone: "UTC".to_string(),
            },
            source: BudgetSource::manual(),
            is_active: true,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn increase_is_capped_at_three_times_the_base_budget() {
        let base = budget(1_500_000);
        assert!(validate_increase_amount(&base, Money4::from_scaled(4_500_000)).is_ok());
        assert!(validate_increase_amount(&base, Money4::from_scaled(4_500_001)).is_err());
    }

    #[test]
    fn threshold_starts_at_eighty_five_percent_of_the_limit() {
        let limit = Money4::from_scaled(1_000_000);
        assert!(!within_increase_threshold(
            Money4::from_scaled(849_999),
            limit
        ));
        assert!(within_increase_threshold(
            Money4::from_scaled(850_000),
            limit
        ));
        assert!(within_increase_threshold(
            Money4::from_scaled(1_200_000),
            limit
        ));
    }

    #[test]
    fn end_of_month_expires_at_the_next_month_boundary() {
        assert_eq!(
            increase_expires_at(
                BudgetIncreasePeriod::EndOfMonth,
                datetime!(2026-10-12 15:30 UTC)
            ),
            datetime!(2026-11-01 0:00 UTC)
        );
        assert_eq!(
            increase_expires_at(
                BudgetIncreasePeriod::EndOfMonth,
                datetime!(2026-12-31 23:59 UTC)
            ),
            datetime!(2027-01-01 0:00 UTC)
        );
        assert_eq!(
            increase_expires_at(
                BudgetIncreasePeriod::SevenDays,
                datetime!(2026-10-12 15:30 UTC)
            ),
            datetime!(2026-10-19 15:30 UTC)
        );
    }
}
