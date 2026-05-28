use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use gateway_core::{
    ApiKeyOwnerKind, BudgetAlertChannel, BudgetAlertDeliveryStatus, BudgetAlertHistoryQuery,
    BudgetAlertRepository, BudgetCadence, BudgetModelSelector, BudgetRecord, BudgetRepository,
    BudgetScope, BudgetScopeKind, BudgetSettings, GatewayError, IdentityRepository, MembershipRole,
    ModelRepository, Money4, UserStatus, budget_window_utc,
};
use gateway_store::GatewayStore;
use time::{Date, Duration, Month, OffsetDateTime, UtcOffset};
use uuid::Uuid;

use crate::http::{
    admin_auth::{require_authenticated_session, require_platform_admin},
    admin_contract::{
        BudgetAlertHistoryItemView, BudgetAlertHistoryRequestQuery, BudgetAlertHistoryView,
        BudgetScopeRequest, BudgetScopeView, BudgetServiceAccountScopeKind,
        BudgetServiceAccountScopeView, BudgetSettingsView, BudgetUserModelByModelScopeView,
        BudgetUserModelByUpstreamModelScopeView, BudgetUserModelScopeKind, BudgetUserScopeKind,
        BudgetUserScopeView, DeactivateBudgetRequest, DeactivateBudgetResultView, Envelope,
        FocusExportQuery, FocusSelfExportQuery, SpendBudgetServiceAccountView,
        SpendBudgetUserModelView, SpendBudgetUserView, SpendBudgetsView, SpendDailyPointView,
        SpendModelBreakdownView, SpendOwnerBreakdownView, SpendReportQuery, SpendReportView,
        SpendTotalsView, UpsertBudgetRequest, UpsertBudgetResultView, envelope, format_timestamp,
    },
    error::AppError,
    focus_export::{FocusCsvExport, build_focus_csv_export},
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
    path = "/api/v1/admin/spend/focus.csv",
    params(FocusExportQuery),
    responses((status = 200, content_type = "text/csv", body = String)),
    security(("session_cookie" = []))
)]
pub async fn get_admin_focus_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FocusExportQuery>,
) -> Result<Response, AppError> {
    require_platform_admin(&state, &headers).await?;
    let owner_kind = parse_owner_kind(query.owner_kind.as_deref())?;
    let (window_start, window_end) = focus_export_window_bounds_utc(&query)?;

    let rows = state
        .store
        .list_focus_export_aggregates(window_start, window_end, owner_kind, None)
        .await?;
    let diagnostics = state
        .store
        .get_focus_export_diagnostics(window_start, window_end, owner_kind, None)
        .await?;

    Ok(focus_csv_response(build_focus_csv_export(
        &rows,
        diagnostics,
        window_start,
        window_end,
    )))
}

#[utoipa::path(
    get,
    path = "/api/v1/me/spend/focus.csv",
    params(FocusSelfExportQuery),
    responses((status = 200, content_type = "text/csv", body = String)),
    security(("session_cookie" = []))
)]
pub async fn get_my_focus_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FocusSelfExportQuery>,
) -> Result<Response, AppError> {
    let current_user = require_authenticated_session(&state, &headers).await?;
    if current_user.status != UserStatus::Active {
        return Err(AppError(GatewayError::InvalidRequest(
            "only active users can export spend".to_string(),
        )));
    }
    let (window_start, window_end) = focus_self_export_window_bounds_utc(&query)?;

    let rows = state
        .store
        .list_focus_export_aggregates(
            window_start,
            window_end,
            Some(ApiKeyOwnerKind::User),
            Some(current_user.user_id),
        )
        .await?;
    let diagnostics = state
        .store
        .get_focus_export_diagnostics(
            window_start,
            window_end,
            Some(ApiKeyOwnerKind::User),
            Some(current_user.user_id),
        )
        .await?;

    Ok(focus_csv_response(build_focus_csv_export(
        &rows,
        diagnostics,
        window_start,
        window_end,
    )))
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
    let service_accounts = state.store.list_active_service_accounts().await?;

    let mut user_views = Vec::with_capacity(users.len());
    for user in users {
        let user_email = user.user.email.clone();
        let scope = BudgetScope::User {
            user_id: user.user.user_id,
        };
        let budget = state.store.get_active_budget_by_scope(&scope).await?;
        let current_window_spend = if let Some(ref active_budget) = budget {
            let (window_start, window_end) =
                budget_window_bounds_utc(active_budget.settings.cadence, now)?;
            state
                .store
                .sum_usage_cost_for_budget_scope_in_window(&scope, window_start, window_end)
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
            budget: budget.as_ref().map(budget_to_settings_view),
            current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
            alert_email_ready: true,
            alert_recipient_summary: user_email,
        });
    }

    let teams = state.store.list_teams().await?;
    let team_map = teams
        .iter()
        .map(|team| (team.team_id, team))
        .collect::<std::collections::HashMap<_, _>>();
    let mut service_account_views = Vec::with_capacity(service_accounts.len());
    for account in service_accounts {
        let scope = BudgetScope::ServiceAccount {
            service_account_id: account.service_account_id,
        };
        let budget = state.store.get_active_budget_by_scope(&scope).await?;
        let current_window_spend = if let Some(ref active_budget) = budget {
            let (window_start, window_end) =
                budget_window_bounds_utc(active_budget.settings.cadence, now)?;
            state
                .store
                .sum_usage_cost_for_budget_scope_in_window(&scope, window_start, window_end)
                .await?
        } else {
            Money4::ZERO
        };
        let recipients =
            active_team_budget_recipients(state.store.as_ref(), account.team_id).await?;
        let team = team_map.get(&account.team_id).ok_or_else(|| {
            AppError(GatewayError::InvalidRequest(format!(
                "service account `{}` references missing team",
                account.service_account_id
            )))
        })?;
        service_account_views.push(SpendBudgetServiceAccountView {
            service_account_id: account.service_account_id.to_string(),
            service_account_name: account.service_account_name,
            service_account_key: account.service_account_key,
            team_id: team.team_id.to_string(),
            team_name: team.team_name.clone(),
            team_key: team.team_key.clone(),
            budget: budget.as_ref().map(budget_to_settings_view),
            current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
            alert_email_ready: !recipients.is_empty(),
            alert_recipient_summary: if recipients.is_empty() {
                "No active team owners/admins with email addresses".to_string()
            } else {
                recipients.join(", ")
            },
        });
    }

    let user_model_budgets = active_user_model_budget_views(&state, now).await?;

    Ok(Json(envelope(SpendBudgetsView {
        users: user_views,
        service_accounts: service_account_views,
        user_model_budgets,
    })))
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/spend/budgets",
    request_body = UpsertBudgetRequest,
    responses((status = 200, body = Envelope<UpsertBudgetResultView>)),
    security(("session_cookie" = []))
)]
pub async fn upsert_budget(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UpsertBudgetRequest>,
) -> Result<Json<Envelope<UpsertBudgetResultView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let scope = parse_budget_scope(&request.scope)?;
    validate_budget_scope_exists(&state, &scope).await?;
    let cadence = parse_budget_cadence(&request.cadence)?;
    let amount_usd = parse_budget_amount(&request.amount_usd)?;
    let timezone = parse_timezone(request.timezone.as_deref())?;
    let settings = BudgetSettings {
        cadence,
        amount_usd,
        hard_limit: request.hard_limit,
        timezone,
    };
    let now = OffsetDateTime::now_utc();

    let budget = state
        .store
        .upsert_active_budget(&scope, &settings, now)
        .await?;
    let current_window_spend = current_window_spend(&state, &budget, now).await?;
    state
        .service
        .evaluate_budget_alert_after_budget_upsert(&budget, current_window_spend, now)
        .await?;

    Ok(Json(envelope(UpsertBudgetResultView {
        budget_id: budget.budget_id.to_string(),
        scope: scope_to_view(&budget.scope),
        scope_key: budget.scope_key.clone(),
        budget: budget_to_settings_view(&budget),
        current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/spend/budgets/deactivate",
    request_body = DeactivateBudgetRequest,
    responses((status = 200, body = Envelope<DeactivateBudgetResultView>)),
    security(("session_cookie" = []))
)]
pub async fn deactivate_budget(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<DeactivateBudgetRequest>,
) -> Result<Json<Envelope<DeactivateBudgetResultView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let scope = parse_budget_scope(&request.scope)?;
    if let BudgetScope::ServiceAccount { service_account_id } = scope
        && state
            .store
            .count_active_api_keys_for_service_account(service_account_id)
            .await?
            > 0
    {
        return Err(AppError(GatewayError::InvalidRequest(
            "cannot deactivate a service account budget while active service account API keys exist"
                .to_string(),
        )));
    }
    let scope_key = scope.scope_key();

    let deactivated = state
        .store
        .deactivate_active_budget(&scope, OffsetDateTime::now_utc())
        .await?;
    Ok(Json(envelope(DeactivateBudgetResultView {
        scope: scope_to_view(&scope),
        scope_key,
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

fn budget_to_settings_view(record: &BudgetRecord) -> BudgetSettingsView {
    BudgetSettingsView {
        cadence: record.settings.cadence.as_str().to_string(),
        amount_usd: record.settings.amount_usd.to_string(),
        amount_usd_10000: record.settings.amount_usd.as_scaled_i64(),
        hard_limit: record.settings.hard_limit,
        timezone: record.settings.timezone.clone(),
    }
}

async fn active_user_model_budget_views(
    state: &AppState,
    now: OffsetDateTime,
) -> Result<Vec<SpendBudgetUserModelView>, AppError> {
    let budgets = state
        .store
        .list_active_budgets(Some(BudgetScopeKind::UserModel))
        .await?;
    let mut views = Vec::with_capacity(budgets.len());
    for budget in budgets {
        let (user_id, model_id, upstream_model) = match &budget.scope {
            BudgetScope::UserModel { user_id, selector } => (
                *user_id,
                selector.model_id().map(|value| value.to_string()),
                selector.upstream_model().map(ToOwned::to_owned),
            ),
            BudgetScope::User { .. } | BudgetScope::ServiceAccount { .. } => continue,
        };
        let (window_start, window_end) = budget_window_bounds_utc(budget.settings.cadence, now)?;
        let current_window_spend = state
            .store
            .sum_usage_cost_for_budget_scope_in_window(&budget.scope, window_start, window_end)
            .await?;
        let user = state.store.get_user_by_id(user_id).await?;
        views.push(SpendBudgetUserModelView {
            budget_id: budget.budget_id.to_string(),
            scope_key: budget.scope_key.clone(),
            user_id: user_id.to_string(),
            model_id,
            upstream_model,
            budget: budget_to_settings_view(&budget),
            current_window_spend_usd_10000: current_window_spend.as_scaled_i64(),
            alert_email_ready: user.is_some(),
            alert_recipient_summary: user
                .map(|user| user.email)
                .unwrap_or_else(|| "Budget user no longer exists".to_string()),
        });
    }
    Ok(views)
}

fn parse_budget_scope(request: &BudgetScopeRequest) -> Result<BudgetScope, AppError> {
    match request {
        BudgetScopeRequest::User(request) => Ok(BudgetScope::User {
            user_id: parse_uuid(&request.user_id)?,
        }),
        BudgetScopeRequest::ServiceAccount(request) => Ok(BudgetScope::ServiceAccount {
            service_account_id: parse_uuid(&request.service_account_id)?,
        }),
        BudgetScopeRequest::UserModelByModel(request) => Ok(BudgetScope::UserModel {
            user_id: parse_uuid(&request.user_id)?,
            selector: BudgetModelSelector::Model {
                model_id: parse_uuid(&request.model_id)?,
            },
        }),
        BudgetScopeRequest::UserModelByUpstreamModel(request) => {
            let upstream_model = request.upstream_model.trim();
            if upstream_model.is_empty() {
                return Err(AppError(GatewayError::InvalidRequest(
                    "user_model upstream_model cannot be empty".to_string(),
                )));
            }
            Ok(BudgetScope::UserModel {
                user_id: parse_uuid(&request.user_id)?,
                selector: BudgetModelSelector::UpstreamModel {
                    upstream_model: upstream_model.to_string(),
                },
            })
        }
    }
}

async fn validate_budget_scope_exists(
    state: &AppState,
    scope: &BudgetScope,
) -> Result<(), AppError> {
    match scope {
        BudgetScope::User { user_id } => {
            state.store.get_user_by_id(*user_id).await?.ok_or_else(|| {
                AppError(GatewayError::InvalidRequest("user not found".to_string()))
            })?;
        }
        BudgetScope::UserModel { user_id, selector } => {
            state.store.get_user_by_id(*user_id).await?.ok_or_else(|| {
                AppError(GatewayError::InvalidRequest("user not found".to_string()))
            })?;
            if let BudgetModelSelector::Model { model_id } = selector {
                let model_exists = state
                    .store
                    .list_models()
                    .await?
                    .into_iter()
                    .any(|model| model.id == *model_id);
                if !model_exists {
                    return Err(AppError(GatewayError::InvalidRequest(
                        "model not found".to_string(),
                    )));
                }
            }
        }
        BudgetScope::ServiceAccount { service_account_id } => {
            state
                .store
                .get_service_account_by_id(*service_account_id)
                .await?
                .ok_or_else(|| {
                    AppError(GatewayError::InvalidRequest(
                        "service account not found".to_string(),
                    ))
                })?;
        }
    }
    Ok(())
}

async fn current_window_spend(
    state: &AppState,
    budget: &BudgetRecord,
    now: OffsetDateTime,
) -> Result<Money4, AppError> {
    let (window_start, window_end) = budget_window_bounds_utc(budget.settings.cadence, now)?;
    Ok(state
        .store
        .sum_usage_cost_for_budget_scope_in_window(&budget.scope, window_start, window_end)
        .await?)
}

fn scope_to_view(scope: &BudgetScope) -> BudgetScopeView {
    match scope {
        BudgetScope::User { user_id } => BudgetScopeView::User(BudgetUserScopeView {
            kind: BudgetUserScopeKind::User,
            user_id: user_id.to_string(),
        }),
        BudgetScope::ServiceAccount { service_account_id } => {
            BudgetScopeView::ServiceAccount(BudgetServiceAccountScopeView {
                kind: BudgetServiceAccountScopeKind::ServiceAccount,
                service_account_id: service_account_id.to_string(),
            })
        }
        BudgetScope::UserModel {
            user_id,
            selector: BudgetModelSelector::Model { model_id },
        } => BudgetScopeView::UserModelByModel(BudgetUserModelByModelScopeView {
            kind: BudgetUserModelScopeKind::UserModel,
            user_id: user_id.to_string(),
            model_id: model_id.to_string(),
        }),
        BudgetScope::UserModel {
            user_id,
            selector: BudgetModelSelector::UpstreamModel { upstream_model },
        } => BudgetScopeView::UserModelByUpstreamModel(BudgetUserModelByUpstreamModelScopeView {
            kind: BudgetUserModelScopeKind::UserModel,
            user_id: user_id.to_string(),
            upstream_model: upstream_model.trim().to_string(),
        }),
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
        "service_account" => Ok(Some(ApiKeyOwnerKind::ServiceAccount)),
        other => Err(AppError(GatewayError::InvalidRequest(format!(
            "invalid owner_kind `{other}`"
        )))),
    }
}

fn focus_export_window_bounds_utc(
    query: &FocusExportQuery,
) -> Result<(OffsetDateTime, OffsetDateTime), AppError> {
    focus_window_bounds_from_parts(
        query.start.as_deref(),
        query.end.as_deref(),
        query.day.as_deref(),
        query.granularity.as_deref(),
    )
}

fn focus_self_export_window_bounds_utc(
    query: &FocusSelfExportQuery,
) -> Result<(OffsetDateTime, OffsetDateTime), AppError> {
    focus_window_bounds_from_parts(
        query.start.as_deref(),
        query.end.as_deref(),
        query.day.as_deref(),
        query.granularity.as_deref(),
    )
}

fn focus_default_window_bounds_utc() -> Result<(OffsetDateTime, OffsetDateTime), AppError> {
    let now_utc = OffsetDateTime::now_utc().to_offset(UtcOffset::UTC);
    let window_end = date_start_utc(now_utc.date())?;
    let window_start = window_end - Duration::days(30);
    Ok((window_start, window_end))
}

fn focus_window_bounds_from_parts(
    start: Option<&str>,
    end: Option<&str>,
    day: Option<&str>,
    granularity: Option<&str>,
) -> Result<(OffsetDateTime, OffsetDateTime), AppError> {
    if !matches!(granularity, None | Some("daily")) {
        return Err(AppError(GatewayError::InvalidRequest(
            "granularity must be daily".to_string(),
        )));
    }
    if day.is_some() && (start.is_some() || end.is_some()) {
        return Err(AppError(GatewayError::InvalidRequest(
            "day is mutually exclusive with start and end".to_string(),
        )));
    }

    let (start_date, end_date) = if let Some(day) = day {
        let day = parse_utc_date(day, "day")?;
        (day, day)
    } else if start.is_some() || end.is_some() {
        let start = start
            .ok_or_else(|| {
                AppError(GatewayError::InvalidRequest(
                    "start is required when end is supplied".to_string(),
                ))
            })
            .and_then(|value| parse_utc_date(value, "start"))?;
        let end = end
            .ok_or_else(|| {
                AppError(GatewayError::InvalidRequest(
                    "end is required when start is supplied".to_string(),
                ))
            })
            .and_then(|value| parse_utc_date(value, "end"))?;
        (start, end)
    } else {
        return focus_default_window_bounds_utc();
    };

    if end_date < start_date {
        return Err(AppError(GatewayError::InvalidRequest(
            "end must be on or after start".to_string(),
        )));
    }
    let days = (end_date - start_date).whole_days() + 1;
    if days > 90 {
        return Err(AppError(GatewayError::InvalidRequest(
            "FOCUS exports are limited to 90 days".to_string(),
        )));
    }

    Ok((
        date_start_utc(start_date)?,
        date_start_utc(end_date)? + Duration::days(1),
    ))
}

fn parse_utc_date(value: &str, field: &str) -> Result<Date, AppError> {
    if value.len() != 10 {
        return invalid_date(field);
    }
    let mut parts = value.split('-');
    let year = parts.next().and_then(|part| part.parse::<i32>().ok());
    let month = parts.next().and_then(|part| part.parse::<u8>().ok());
    let day = parts.next().and_then(|part| part.parse::<u8>().ok());
    if parts.next().is_some() {
        return invalid_date(field);
    }
    let (Some(year), Some(month), Some(day)) = (year, month, day) else {
        return invalid_date(field);
    };
    let month = Month::try_from(month).map_err(|_| {
        AppError(GatewayError::InvalidRequest(format!(
            "{field} must be a valid YYYY-MM-DD date"
        )))
    })?;
    Date::from_calendar_date(year, month, day).map_err(|_| {
        AppError(GatewayError::InvalidRequest(format!(
            "{field} must be a valid YYYY-MM-DD date"
        )))
    })
}

fn invalid_date<T>(field: &str) -> Result<T, AppError> {
    Err(AppError(GatewayError::InvalidRequest(format!(
        "{field} must be a valid YYYY-MM-DD date"
    ))))
}

fn date_start_utc(date: Date) -> Result<OffsetDateTime, AppError> {
    date.with_hms(0, 0, 0)
        .map_err(|error| {
            AppError(GatewayError::Internal(format!(
                "invalid day start: {error}"
            )))
        })
        .map(|value| value.assume_offset(UtcOffset::UTC))
}

fn focus_csv_response(export: FocusCsvExport) -> Response {
    let content_disposition = format!("attachment; filename=\"{}\"", export.filename);
    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, content_disposition),
            (
                header::HeaderName::from_static("x-focus-excluded-unpriced-requests"),
                export.diagnostics.unpriced_request_count.to_string(),
            ),
            (
                header::HeaderName::from_static("x-focus-excluded-usage-missing-requests"),
                export.diagnostics.usage_missing_request_count.to_string(),
            ),
        ],
        export.body,
    )
        .into_response()
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
