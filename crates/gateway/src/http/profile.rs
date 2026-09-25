//! Self-scoped profile summary: the signed-in user's budget and a year of usage.
use std::collections::BTreeMap;

use axum::{Json, extract::State, http::HeaderMap};
use gateway_core::{
    ApiKeyOwnerKind, BudgetRepository, BudgetScope, GatewayError, HarnessUsageDailyRecord,
    RequestLogRepository, SpendDailyAggregateRecord, SpendModelTokenDailyRecord, UserStatus,
    budget_window_utc,
};
use serde::Serialize;
use time::{Duration, OffsetDateTime};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::{
    admin_auth::require_authenticated_session,
    admin_contract::{BudgetSettingsView, BudgetSourceView, Envelope, envelope, format_timestamp},
    error::AppError,
    spend::{budget_source_to_view, budget_to_settings_view},
    state::AppState,
};

/// Days of history returned for the heatmap and trend charts.
pub const PROFILE_HISTORY_DAYS: i64 = 365;

#[derive(Debug, Serialize, ToSchema)]
pub struct MyProfileView {
    pub window_start: String,
    pub window_end: String,
    pub budget: Option<MyProfileBudgetView>,
    pub days: Vec<MyProfileDayView>,
    pub model_days: Vec<MyProfileModelDayView>,
    pub harness_days: Vec<MyProfileHarnessDayView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MyProfileBudgetView {
    pub settings: BudgetSettingsView,
    pub source: BudgetSourceView,
    /// Current budget period; spend resets at `period_end`.
    pub period_start: String,
    pub period_end: String,
    pub spent_usd_10000: i64,
}

/// Usage for one UTC day. `day` is an ISO date (`YYYY-MM-DD`).
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct MyProfileDayView {
    pub day: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Input with a provider cache split that missed the cache; with the two cache buckets this
    /// forms the cache hit-rate denominator.
    pub uncached_input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd_10000: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MyProfileModelDayView {
    pub day: String,
    pub model_key: String,
    pub request_count: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MyProfileHarnessDayView {
    pub day: String,
    pub harness_key: String,
    pub harness_label: String,
    pub request_count: i64,
    pub total_tokens: i64,
}

#[utoipa::path(
    get,
    path = "/api/v1/me/profile",
    responses((status = 200, body = Envelope<MyProfileView>)),
    security(("session_cookie" = []))
)]
pub async fn get_my_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<MyProfileView>>, AppError> {
    let current_user = require_authenticated_session(&state, &headers).await?;
    if current_user.status != UserStatus::Active {
        return Err(AppError(GatewayError::InvalidRequest(
            "only active users can view their profile".to_string(),
        )));
    }
    let user_id = current_user.user_id;
    let now = OffsetDateTime::now_utc();
    let (window_start, window_end) = profile_window_bounds_utc(now);
    let owner = Some(ApiKeyOwnerKind::User);

    let store = state.store.as_ref();
    let (daily, model_daily, harness_daily, budget) = tokio::try_join!(
        store.list_usage_daily_aggregates(window_start, window_end, owner, Some(user_id)),
        store.list_usage_model_token_daily_aggregates(
            window_start,
            window_end,
            owner,
            Some(user_id)
        ),
        store.list_user_harness_daily_usage(window_start, window_end, user_id),
        load_budget(&state, user_id, now),
    )?;

    Ok(Json(envelope(MyProfileView {
        window_start: format_timestamp(window_start),
        window_end: format_timestamp(window_end),
        budget,
        days: build_day_views(&daily, &model_daily),
        model_days: model_daily.iter().map(model_day_view).collect(),
        harness_days: harness_daily.iter().map(harness_day_view).collect(),
    })))
}

async fn load_budget(
    state: &AppState,
    user_id: Uuid,
    now: OffsetDateTime,
) -> Result<Option<MyProfileBudgetView>, gateway_core::StoreError> {
    let scope = BudgetScope::User { user_id };
    let Some(budget) = state.store.get_active_budget_by_scope(&scope).await? else {
        return Ok(None);
    };
    let window = budget_window_utc(budget.settings.cadence, now);
    let spent = state
        .store
        .sum_usage_cost_for_budget_scope_in_window(&scope, window.period_start, window.observed_end)
        .await?;
    Ok(Some(MyProfileBudgetView {
        settings: budget_to_settings_view(&budget),
        source: budget_source_to_view(&budget.source),
        period_start: format_timestamp(window.period_start),
        period_end: format_timestamp(window.period_end),
        spent_usd_10000: spent.as_scaled_i64(),
    }))
}

/// History ends at the start of tomorrow (UTC) so today is always included.
fn profile_window_bounds_utc(now: OffsetDateTime) -> (OffsetDateTime, OffsetDateTime) {
    let today = now
        .to_offset(time::UtcOffset::UTC)
        .date()
        .midnight()
        .assume_utc();
    let window_end = today + Duration::days(1);
    (
        window_end - Duration::days(PROFILE_HISTORY_DAYS),
        window_end,
    )
}

/// Merge cost rows with per-model token rows into one row per active day.
fn build_day_views(
    daily: &[SpendDailyAggregateRecord],
    model_daily: &[SpendModelTokenDailyRecord],
) -> Vec<MyProfileDayView> {
    let mut days: BTreeMap<OffsetDateTime, MyProfileDayView> = BTreeMap::new();
    for row in daily {
        let day = days.entry(row.day_start).or_default();
        day.cost_usd_10000 += row.priced_cost_usd.as_scaled_i64();
    }
    for row in model_daily {
        let day = days.entry(row.day_start).or_default();
        let tokens = &row.tokens;
        day.request_count += tokens.request_count;
        day.input_tokens += tokens.input_tokens;
        day.output_tokens += tokens.output_tokens;
        day.uncached_input_tokens += tokens.uncached_input_tokens;
        day.cache_read_tokens += tokens.cache_read_tokens;
        day.cache_write_tokens += tokens.cache_write_tokens;
        day.total_tokens += tokens.input_tokens + tokens.output_tokens;
    }
    days.into_iter()
        .map(|(day_start, view)| MyProfileDayView {
            day: day_start.date().to_string(),
            ..view
        })
        .collect()
}

fn model_day_view(row: &SpendModelTokenDailyRecord) -> MyProfileModelDayView {
    MyProfileModelDayView {
        day: row.day_start.date().to_string(),
        model_key: row.model_key.clone(),
        request_count: row.tokens.request_count,
        total_tokens: row.tokens.input_tokens + row.tokens.output_tokens,
    }
}

fn harness_day_view(row: &HarnessUsageDailyRecord) -> MyProfileHarnessDayView {
    MyProfileHarnessDayView {
        day: row.day_start.date().to_string(),
        harness_key: row.agent_harness_key.clone(),
        harness_label: row.agent_harness_label.clone(),
        request_count: row.request_count,
        total_tokens: row.total_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::{Money4, TokenUsageBuckets};
    use time::macros::datetime;

    fn tokens(request_count: i64, input: i64, output: i64, cache_read: i64) -> TokenUsageBuckets {
        TokenUsageBuckets {
            request_count,
            input_tokens: input,
            output_tokens: output,
            uncached_input_tokens: input - cache_read,
            cache_read_tokens: cache_read,
            cache_write_tokens: 0,
        }
    }

    #[test]
    fn window_covers_a_year_including_today() {
        let (start, end) = profile_window_bounds_utc(datetime!(2026-09-25 13:45 UTC));
        assert_eq!(end, datetime!(2026-09-26 00:00 UTC));
        assert_eq!(end - start, Duration::days(PROFILE_HISTORY_DAYS));
    }

    #[test]
    fn day_views_merge_cost_and_model_tokens() {
        let day = datetime!(2026-09-24 00:00 UTC);
        let other_day = datetime!(2026-09-25 00:00 UTC);
        let daily = vec![SpendDailyAggregateRecord {
            day_start: day,
            priced_cost_usd: Money4::from_scaled(12_500),
            priced_request_count: 3,
            unpriced_request_count: 0,
            usage_missing_request_count: 0,
            uncached_input_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
        }];
        let model_daily = vec![
            SpendModelTokenDailyRecord {
                day_start: day,
                model_key: "fast".to_string(),
                tokens: tokens(2, 100, 50, 40),
            },
            SpendModelTokenDailyRecord {
                day_start: day,
                model_key: "smart".to_string(),
                tokens: tokens(1, 10, 5, 0),
            },
            SpendModelTokenDailyRecord {
                day_start: other_day,
                model_key: "fast".to_string(),
                tokens: tokens(1, 1, 1, 0),
            },
        ];

        let views = build_day_views(&daily, &model_daily);

        assert_eq!(views.len(), 2);
        assert_eq!(views[0].day, "2026-09-24");
        assert_eq!(views[0].request_count, 3);
        assert_eq!(views[0].total_tokens, 165);
        assert_eq!(views[0].cache_read_tokens, 40);
        assert_eq!(views[0].cost_usd_10000, 12_500);
        assert_eq!(views[1].day, "2026-09-25");
        assert_eq!(views[1].cost_usd_10000, 0);
    }
}
