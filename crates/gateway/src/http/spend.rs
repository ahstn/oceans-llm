use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    ApiKeyOwnerKind, BudgetAlertChannel, BudgetAlertDeliveryStatus, BudgetAlertHistoryQuery,
    BudgetAlertRepository, BudgetCadence, BudgetRepository, GatewayError, IdentityRepository,
    MembershipRole, Money4, UserStatus, budget_window_utc,
};
use gateway_store::GatewayStore;
use time::{Duration, OffsetDateTime, UtcOffset};
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{
        BudgetAlertHistoryItemView, BudgetAlertHistoryRequestQuery, BudgetAlertHistoryView,
        BudgetSettingsView, DeactivateBudgetResultView, Envelope, SpendBudgetTeamView,
        SpendBudgetUserView, SpendBudgetsView, SpendDailyPointView, SpendModelBreakdownView,
        SpendOwnerBreakdownView, SpendReportQuery, SpendReportView, SpendTotalsView,
        UpsertBudgetRequest, UpsertBudgetResultView, envelope, format_timestamp,
    },
    error::AppError,
    state::AppState,
};

#[utoipa::path(
    get,
    path = "/api/v1/admin/spend/report",
    params(SpendReportQuery),
    responses((status = 200, body = Envelope<SpendReportView>)),
    security(("session_cookie" = []))
)]
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/spend/budgets",
    responses((status = 200, body = Envelope<SpendBudgetsView>)),
    security(("session_cookie" = []))
)]
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
        let user_email = user.user.email.clone();
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
            email: user_email.clone(),
            team_id: user.team_id.map(|value| value.to_string()),
            team_name: user.team_name,
            budget: budget.map(user_budget_to_view),
            current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
            alert_email_ready: true,
            alert_recipient_summary: user_email,
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
        let team_recipients =
            active_team_budget_recipients(state.store.as_ref(), team.team_id).await?;

        team_views.push(SpendBudgetTeamView {
            team_id: team.team_id.to_string(),
            team_name: team.team_name,
            team_key: team.team_key,
            budget: budget.map(team_budget_to_view),
            current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
            alert_email_ready: !team_recipients.is_empty(),
            alert_recipient_summary: if team_recipients.is_empty() {
                "No active team owners/admins with email addresses".to_string()
            } else {
                team_recipients.join(", ")
            },
        });
    }

    Ok(Json(envelope(SpendBudgetsView {
        users: user_views,
        teams: team_views,
    })))
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/spend/budgets/users/{user_id}",
    request_body = UpsertBudgetRequest,
    params(("user_id" = String, Path, description = "User identifier")),
    responses((status = 200, body = Envelope<UpsertBudgetResultView>)),
    security(("session_cookie" = []))
)]
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
    state
        .service
        .evaluate_budget_alert_after_user_budget_upsert(&budget, current_window_spend, now)
        .await?;

    Ok(Json(envelope(UpsertBudgetResultView {
        owner_kind: "user".to_string(),
        owner_id: user_id.to_string(),
        budget: user_budget_to_view(budget),
        current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
    })))
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/spend/budgets/users/{user_id}",
    params(("user_id" = String, Path, description = "User identifier")),
    responses((status = 200, body = Envelope<DeactivateBudgetResultView>)),
    security(("session_cookie" = []))
)]
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

#[utoipa::path(
    put,
    path = "/api/v1/admin/spend/budgets/teams/{team_id}",
    request_body = UpsertBudgetRequest,
    params(("team_id" = String, Path, description = "Team identifier")),
    responses((status = 200, body = Envelope<UpsertBudgetResultView>)),
    security(("session_cookie" = []))
)]
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
    state
        .service
        .evaluate_budget_alert_after_team_budget_upsert(&budget, current_window_spend, now)
        .await?;

    Ok(Json(envelope(UpsertBudgetResultView {
        owner_kind: "team".to_string(),
        owner_id: team_id.to_string(),
        budget: team_budget_to_view(budget),
        current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
    })))
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/spend/budgets/teams/{team_id}",
    params(("team_id" = String, Path, description = "Team identifier")),
    responses((status = 200, body = Envelope<DeactivateBudgetResultView>)),
    security(("session_cookie" = []))
)]
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/spend/budget-alerts",
    params(BudgetAlertHistoryRequestQuery),
    responses((status = 200, body = Envelope<BudgetAlertHistoryView>)),
    security(("session_cookie" = []))
)]
pub async fn list_budget_alert_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<BudgetAlertHistoryRequestQuery>,
) -> Result<Json<Envelope<BudgetAlertHistoryView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let history = state
        .store
        .list_budget_alert_history(&BudgetAlertHistoryQuery {
            page: query.page.unwrap_or(1),
            page_size: query.page_size.unwrap_or(25),
            owner_kind: parse_owner_kind(query.owner_kind.as_deref())?,
            channel: parse_budget_alert_channel(query.channel.as_deref())?,
            delivery_status: parse_budget_alert_status(query.status.as_deref())?,
        })
        .await?;

    Ok(Json(envelope(BudgetAlertHistoryView {
        items: history
            .items
            .into_iter()
            .map(|item| BudgetAlertHistoryItemView {
                budget_alert_id: item.budget_alert_id.to_string(),
                owner_kind: item.owner_kind.as_str().to_string(),
                owner_id: item.owner_id.to_string(),
                owner_name: item.owner_name,
                channel: item.channel.as_str().to_string(),
                delivery_status: item.delivery_status.as_str().to_string(),
                recipient_summary: item.recipient_summary,
                threshold_bps: item.threshold_bps,
                cadence: item.cadence.as_str().to_string(),
                window_start: format_timestamp(item.window_start),
                window_end: format_timestamp(item.window_end),
                spend_before_usd_10000: item.spend_before_usd.as_scaled_i64(),
                spend_after_usd_10000: item.spend_after_usd.as_scaled_i64(),
                remaining_budget_usd_10000: item.remaining_budget_usd.as_scaled_i64(),
                created_at: format_timestamp(item.created_at),
                last_attempted_at: item.last_attempted_at.map(format_timestamp),
                sent_at: item.sent_at.map(format_timestamp),
                failure_reason: item.failure_reason,
            })
            .collect(),
        page: history.page,
        page_size: history.page_size,
        total: history.total,
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
    let window = budget_window_utc(cadence, occurred_at)
        .map_err(|error| AppError(GatewayError::Internal(error)))?;
    Ok((window.period_start, window.observed_end))
}

async fn active_team_budget_recipients(
    store: &gateway_store::AnyStore,
    team_id: Uuid,
) -> Result<Vec<String>, AppError> {
    let memberships = GatewayStore::list_team_memberships(store, team_id).await?;
    let mut recipients = Vec::new();

    for membership in memberships {
        if !matches!(
            membership.role,
            MembershipRole::Owner | MembershipRole::Admin
        ) {
            continue;
        }
        let Some(user) = store.get_user_by_id(membership.user_id).await? else {
            continue;
        };
        if user.status != UserStatus::Active {
            continue;
        }
        recipients.push(user.email);
    }

    recipients.sort();
    recipients.dedup();
    Ok(recipients)
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

fn parse_budget_alert_channel(value: Option<&str>) -> Result<Option<BudgetAlertChannel>, AppError> {
    match value {
        None | Some("all") => Ok(None),
        Some(raw) => BudgetAlertChannel::from_db(raw).map(Some).ok_or_else(|| {
            AppError(GatewayError::InvalidRequest(format!(
                "invalid budget alert channel `{raw}`"
            )))
        }),
    }
}

fn parse_budget_alert_status(
    value: Option<&str>,
) -> Result<Option<BudgetAlertDeliveryStatus>, AppError> {
    match value {
        None | Some("all") => Ok(None),
        Some(raw) => BudgetAlertDeliveryStatus::from_db(raw)
            .map(Some)
            .ok_or_else(|| {
                AppError(GatewayError::InvalidRequest(format!(
                    "invalid budget alert status `{raw}`"
                )))
            }),
    }
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
