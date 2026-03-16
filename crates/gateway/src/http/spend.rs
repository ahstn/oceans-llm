use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    ApiKeyOwnerKind, BudgetCadence, BudgetRepository, GatewayError, IdentityRepository, Money4,
};
use gateway_store::GatewayStore;
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::http::{admin_auth::require_platform_admin, error::AppError, state::AppState};

#[derive(Debug, Serialize)]
pub(crate) struct Envelope<T> {
    data: T,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResponseMeta {
    generated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SpendReportQuery {
    pub days: Option<u16>,
    pub owner_kind: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SpendTotalsView {
    pub priced_cost_usd_10000: i64,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Serialize)]
pub struct SpendDailyPointView {
    pub day_start: String,
    pub priced_cost_usd_10000: i64,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Serialize)]
pub struct SpendOwnerBreakdownView {
    pub owner_kind: String,
    pub owner_id: String,
    pub owner_name: String,
    pub priced_cost_usd_10000: i64,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Serialize)]
pub struct SpendModelBreakdownView {
    pub model_key: String,
    pub priced_cost_usd_10000: i64,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Serialize)]
pub struct SpendReportView {
    pub window_days: u16,
    pub owner_kind: String,
    pub window_start: String,
    pub window_end: String,
    pub totals: SpendTotalsView,
    pub daily: Vec<SpendDailyPointView>,
    pub owners: Vec<SpendOwnerBreakdownView>,
    pub models: Vec<SpendModelBreakdownView>,
}

#[derive(Debug, Serialize)]
pub struct BudgetSettingsView {
    pub cadence: String,
    pub amount_usd: String,
    pub amount_usd_10000: i64,
    pub hard_limit: bool,
    pub timezone: String,
}

#[derive(Debug, Serialize)]
pub struct SpendBudgetUserView {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub team_id: Option<String>,
    pub team_name: Option<String>,
    pub budget: Option<BudgetSettingsView>,
    pub current_window_spend_usd_10000: i64,
}

#[derive(Debug, Serialize)]
pub struct SpendBudgetTeamView {
    pub team_id: String,
    pub team_name: String,
    pub team_key: String,
    pub budget: Option<BudgetSettingsView>,
    pub current_window_spend_usd_10000: i64,
}

#[derive(Debug, Serialize)]
pub struct SpendBudgetsView {
    pub users: Vec<SpendBudgetUserView>,
    pub teams: Vec<SpendBudgetTeamView>,
}

#[derive(Debug, Serialize)]
pub struct UpsertBudgetResultView {
    pub owner_kind: String,
    pub owner_id: String,
    pub budget: BudgetSettingsView,
    pub current_window_spend_usd_10000: i64,
}

#[derive(Debug, Serialize)]
pub struct DeactivateBudgetResultView {
    pub owner_kind: String,
    pub owner_id: String,
    pub deactivated: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpsertBudgetRequest {
    pub cadence: String,
    pub amount_usd: String,
    pub hard_limit: bool,
    pub timezone: Option<String>,
}

pub async fn get_spend_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SpendReportQuery>,
) -> Result<Json<Envelope<SpendReportView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let window_days = parse_window_days(query.days)?;
    let owner_kind = parse_owner_kind(query.owner_kind.as_deref())?;
    let (window_start, window_end) = report_window_bounds_utc(window_days)?;

    let daily_rows = state
        .store
        .list_usage_daily_aggregates(window_start, window_end, owner_kind)
        .await?;
    let owner_rows = state
        .store
        .list_usage_owner_aggregates(window_start, window_end, owner_kind)
        .await?;
    let model_rows = state
        .store
        .list_usage_model_aggregates(window_start, window_end, owner_kind)
        .await?;

    let mut daily_map = std::collections::BTreeMap::new();
    for row in daily_rows {
        daily_map.insert(row.day_start.unix_timestamp(), row);
    }

    let mut totals = SpendTotalsView {
        priced_cost_usd_10000: 0,
        priced_request_count: 0,
        unpriced_request_count: 0,
        usage_missing_request_count: 0,
    };

    let mut daily = Vec::with_capacity(window_days as usize);
    for day_offset in 0..window_days {
        let day_start = window_start + Duration::days(i64::from(day_offset));
        if let Some(row) = daily_map.remove(&day_start.unix_timestamp()) {
            let priced_cost = row.priced_cost_usd.as_scaled_i64();
            totals.priced_cost_usd_10000 += priced_cost;
            totals.priced_request_count += row.priced_request_count;
            totals.unpriced_request_count += row.unpriced_request_count;
            totals.usage_missing_request_count += row.usage_missing_request_count;
            daily.push(SpendDailyPointView {
                day_start: format_timestamp(row.day_start),
                priced_cost_usd_10000: priced_cost,
                priced_request_count: row.priced_request_count,
                unpriced_request_count: row.unpriced_request_count,
                usage_missing_request_count: row.usage_missing_request_count,
            });
        } else {
            daily.push(SpendDailyPointView {
                day_start: format_timestamp(day_start),
                priced_cost_usd_10000: 0,
                priced_request_count: 0,
                unpriced_request_count: 0,
                usage_missing_request_count: 0,
            });
        }
    }

    let owners = owner_rows
        .into_iter()
        .map(|row| SpendOwnerBreakdownView {
            owner_kind: row.owner_kind.as_str().to_string(),
            owner_id: row.owner_id.to_string(),
            owner_name: row.owner_name,
            priced_cost_usd_10000: row.priced_cost_usd.as_scaled_i64(),
            priced_request_count: row.priced_request_count,
            unpriced_request_count: row.unpriced_request_count,
            usage_missing_request_count: row.usage_missing_request_count,
        })
        .collect();

    let models = model_rows
        .into_iter()
        .map(|row| SpendModelBreakdownView {
            model_key: row.model_key,
            priced_cost_usd_10000: row.priced_cost_usd.as_scaled_i64(),
            priced_request_count: row.priced_request_count,
            unpriced_request_count: row.unpriced_request_count,
            usage_missing_request_count: row.usage_missing_request_count,
        })
        .collect();

    Ok(Json(envelope(SpendReportView {
        window_days,
        owner_kind: owner_kind
            .map(ApiKeyOwnerKind::as_str)
            .unwrap_or("all")
            .to_string(),
        window_start: format_timestamp(window_start),
        window_end: format_timestamp(window_end),
        totals,
        daily,
        owners,
        models,
    })))
}

pub async fn list_spend_budgets(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<SpendBudgetsView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let now = OffsetDateTime::now_utc();

    let users = state.store.list_identity_users().await?;
    let teams = state.store.list_teams().await?;

    let mut user_views = Vec::with_capacity(users.len());
    for user in users {
        let budget = state
            .store
            .get_active_budget_for_user(user.user.user_id)
            .await?;
        let current_window_spend = if let Some(ref active_budget) = budget {
            let (window_start, window_end) = budget_window_bounds_utc(active_budget.cadence, now)?;
            state
                .store
                .sum_usage_cost_for_user_in_window(user.user.user_id, window_start, window_end)
                .await?
        } else {
            Money4::ZERO
        };

        user_views.push(SpendBudgetUserView {
            user_id: user.user.user_id.to_string(),
            name: user.user.name,
            email: user.user.email,
            team_id: user.team_id.map(|value| value.to_string()),
            team_name: user.team_name,
            budget: budget.map(user_budget_to_view),
            current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
        });
    }

    let mut team_views = Vec::with_capacity(teams.len());
    for team in teams {
        let budget = state.store.get_active_budget_for_team(team.team_id).await?;
        let current_window_spend = if let Some(ref active_budget) = budget {
            let (window_start, window_end) = budget_window_bounds_utc(active_budget.cadence, now)?;
            state
                .store
                .sum_usage_cost_for_team_in_window(team.team_id, window_start, window_end)
                .await?
        } else {
            Money4::ZERO
        };

        team_views.push(SpendBudgetTeamView {
            team_id: team.team_id.to_string(),
            team_name: team.team_name,
            team_key: team.team_key,
            budget: budget.map(team_budget_to_view),
            current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
        });
    }

    Ok(Json(envelope(SpendBudgetsView {
        users: user_views,
        teams: team_views,
    })))
}

pub async fn upsert_user_budget(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(request): Json<UpsertBudgetRequest>,
) -> Result<Json<Envelope<UpsertBudgetResultView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let user_id = parse_uuid(&user_id)?;
    state
        .store
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("user not found".to_string())))?;

    let cadence = parse_budget_cadence(&request.cadence)?;
    let amount_usd = parse_budget_amount(&request.amount_usd)?;
    let timezone = parse_timezone(request.timezone.as_deref())?;
    let now = OffsetDateTime::now_utc();

    let budget = state
        .store
        .upsert_active_budget_for_user(
            user_id,
            cadence,
            amount_usd,
            request.hard_limit,
            &timezone,
            now,
        )
        .await?;
    let (window_start, window_end) = budget_window_bounds_utc(budget.cadence, now)?;
    let current_window_spend = state
        .store
        .sum_usage_cost_for_user_in_window(user_id, window_start, window_end)
        .await?;

    Ok(Json(envelope(UpsertBudgetResultView {
        owner_kind: "user".to_string(),
        owner_id: user_id.to_string(),
        budget: user_budget_to_view(budget),
        current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
    })))
}

pub async fn deactivate_user_budget(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<Envelope<DeactivateBudgetResultView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let user_id = parse_uuid(&user_id)?;
    state
        .store
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("user not found".to_string())))?;

    let deactivated = state
        .store
        .deactivate_active_budget_for_user(user_id, OffsetDateTime::now_utc())
        .await?;
    Ok(Json(envelope(DeactivateBudgetResultView {
        owner_kind: "user".to_string(),
        owner_id: user_id.to_string(),
        deactivated,
    })))
}

pub async fn upsert_team_budget(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(request): Json<UpsertBudgetRequest>,
) -> Result<Json<Envelope<UpsertBudgetResultView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let team_id = parse_uuid(&team_id)?;
    state
        .store
        .get_team_by_id(team_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("team not found".to_string())))?;

    let cadence = parse_budget_cadence(&request.cadence)?;
    let amount_usd = parse_budget_amount(&request.amount_usd)?;
    let timezone = parse_timezone(request.timezone.as_deref())?;
    let now = OffsetDateTime::now_utc();

    let budget = state
        .store
        .upsert_active_budget_for_team(
            team_id,
            cadence,
            amount_usd,
            request.hard_limit,
            &timezone,
            now,
        )
        .await?;
    let (window_start, window_end) = budget_window_bounds_utc(budget.cadence, now)?;
    let current_window_spend = state
        .store
        .sum_usage_cost_for_team_in_window(team_id, window_start, window_end)
        .await?;

    Ok(Json(envelope(UpsertBudgetResultView {
        owner_kind: "team".to_string(),
        owner_id: team_id.to_string(),
        budget: team_budget_to_view(budget),
        current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
    })))
}

pub async fn deactivate_team_budget(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<Envelope<DeactivateBudgetResultView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let team_id = parse_uuid(&team_id)?;
    state
        .store
        .get_team_by_id(team_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("team not found".to_string())))?;

    let deactivated = state
        .store
        .deactivate_active_budget_for_team(team_id, OffsetDateTime::now_utc())
        .await?;
    Ok(Json(envelope(DeactivateBudgetResultView {
        owner_kind: "team".to_string(),
        owner_id: team_id.to_string(),
        deactivated,
    })))
}

fn user_budget_to_view(record: gateway_core::UserBudgetRecord) -> BudgetSettingsView {
    BudgetSettingsView {
        cadence: record.cadence.as_str().to_string(),
        amount_usd: record.amount_usd.to_string(),
        amount_usd_10000: record.amount_usd.as_scaled_i64(),
        hard_limit: record.hard_limit,
        timezone: record.timezone,
    }
}

fn team_budget_to_view(record: gateway_core::TeamBudgetRecord) -> BudgetSettingsView {
    BudgetSettingsView {
        cadence: record.cadence.as_str().to_string(),
        amount_usd: record.amount_usd.to_string(),
        amount_usd_10000: record.amount_usd.as_scaled_i64(),
        hard_limit: record.hard_limit,
        timezone: record.timezone,
    }
}

fn budget_window_bounds_utc(
    cadence: BudgetCadence,
    occurred_at: OffsetDateTime,
) -> Result<(OffsetDateTime, OffsetDateTime), AppError> {
    let now_utc = occurred_at.to_offset(UtcOffset::UTC);
    let day_start = now_utc
        .date()
        .with_hms(0, 0, 0)
        .map_err(|error| {
            AppError(GatewayError::Internal(format!(
                "invalid day start: {error}"
            )))
        })?
        .assume_offset(UtcOffset::UTC);
    let end = now_utc + Duration::seconds(1);

    let start = match cadence {
        BudgetCadence::Daily => day_start,
        BudgetCadence::Weekly => {
            let days_from_monday = i64::from(now_utc.weekday().number_days_from_monday());
            day_start - Duration::days(days_from_monday)
        }
    };

    Ok((start, end))
}

fn report_window_bounds_utc(
    window_days: u16,
) -> Result<(OffsetDateTime, OffsetDateTime), AppError> {
    let now_utc = OffsetDateTime::now_utc().to_offset(UtcOffset::UTC);
    let window_end = now_utc
        .date()
        .with_hms(0, 0, 0)
        .map_err(|error| {
            AppError(GatewayError::Internal(format!(
                "invalid day start: {error}"
            )))
        })?
        .assume_offset(UtcOffset::UTC)
        + Duration::days(1);
    let window_start = window_end - Duration::days(i64::from(window_days));
    Ok((window_start, window_end))
}

fn parse_window_days(days: Option<u16>) -> Result<u16, AppError> {
    let days = days.unwrap_or(7);
    if days == 7 || days == 30 {
        Ok(days)
    } else {
        Err(AppError(GatewayError::InvalidRequest(
            "days must be either 7 or 30".to_string(),
        )))
    }
}

fn parse_owner_kind(value: Option<&str>) -> Result<Option<ApiKeyOwnerKind>, AppError> {
    match value.unwrap_or("all") {
        "all" => Ok(None),
        "user" => Ok(Some(ApiKeyOwnerKind::User)),
        "team" => Ok(Some(ApiKeyOwnerKind::Team)),
        other => Err(AppError(GatewayError::InvalidRequest(format!(
            "invalid owner_kind `{other}`"
        )))),
    }
}

fn parse_budget_cadence(value: &str) -> Result<BudgetCadence, AppError> {
    BudgetCadence::from_db(value).ok_or_else(|| {
        AppError(GatewayError::InvalidRequest(format!(
            "invalid budget cadence `{value}`"
        )))
    })
}

fn parse_budget_amount(value: &str) -> Result<Money4, AppError> {
    let amount = Money4::from_decimal_str(value)
        .map_err(|error| AppError(GatewayError::InvalidRequest(error)))?;
    if amount.is_negative() {
        return Err(AppError(GatewayError::InvalidRequest(
            "amount_usd must be non-negative".to_string(),
        )));
    }
    Ok(amount)
}

fn parse_timezone(raw: Option<&str>) -> Result<String, AppError> {
    let timezone = raw.unwrap_or("UTC").trim();
    if timezone.is_empty() {
        return Err(AppError(GatewayError::InvalidRequest(
            "timezone cannot be empty".to_string(),
        )));
    }
    Ok(timezone.to_string())
}

fn parse_uuid(raw: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|error| {
        AppError(GatewayError::InvalidRequest(format!(
            "invalid uuid `{raw}`: {error}"
        )))
    })
}

fn envelope<T>(data: T) -> Envelope<T> {
    Envelope {
        data,
        meta: ResponseMeta {
            generated_at: format_timestamp(OffsetDateTime::now_utc()),
        },
    }
}

fn format_timestamp(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .unwrap_or_else(|_| value.unix_timestamp().to_string())
}
