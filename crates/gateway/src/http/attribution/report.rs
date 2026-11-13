//! `GET /api/v1/admin/attribution/report`: commits, merged PRs and the gateway requests behind
//! them. Platform admins see everyone; other users see the rows they own or authored.

use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use gateway_core::{
    ATTRIBUTION_MAX_REPORT_ROWS, AttributionOwner, AttributionReportQuery, GatewayError,
    GlobalRole, UserRecord,
};
use gateway_service::attribution::{
    AttributionChangeActivity, AttributionModelActivity, AttributionPullRequestActivity,
    AttributionReport, AttributionReportCoverage, AttributionReportTotals, AttributionUserActivity,
    RequestTally, build_attribution_report,
};
use gateway_store::{AnyStore, GatewayStore};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime, Time, UtcOffset};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::AttributionOwnerKind;
use crate::http::{
    admin_auth::require_authenticated_session,
    admin_contract::{Envelope, envelope, format_timestamp},
    error::AppError,
    state::AppState,
};

const REPORT_WINDOW_DAYS: [u16; 3] = [7, 30, 90];
/// Shown when a user or service account was removed while the report was built.
const UNKNOWN_NAME: &str = "Unknown";

#[derive(Debug, Deserialize, IntoParams)]
pub struct AttributionReportRequestQuery {
    /// 7, 30 or 90 whole UTC days ending today. Defaults to 7.
    pub days: Option<u16>,
    /// Platform admins only: the rows one user owns or authored. Other users always get
    /// their own.
    pub user_id: Option<String>,
    /// One normalised repository, such as `github.com/acme/widgets`.
    pub repository: Option<String>,
}

/// Request counts and priced cost, split by pricing outcome as in the spend report.
#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionRequestTallyView {
    pub request_count: i64,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
    pub priced_cost_usd_10000: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionTotalsView {
    pub commit_count: i64,
    pub pull_request_count: i64,
    pub ai_lines: i64,
    pub human_lines: i64,
    /// Distinct AI sessions named by the window's commit notes.
    pub ai_session_count: i64,
    /// Those sessions that matched gateway requests.
    pub joined_session_count: i64,
    /// Harness sessions reported by hook events in the window.
    pub agent_session_count: i64,
    /// Requests allocated to the window's commits.
    #[serde(flatten)]
    pub requests: AttributionRequestTallyView,
    /// Requests after their session's last commit.
    pub uncommitted_request_count: i64,
    pub uncommitted_cost_usd_10000: i64,
    /// Window commits whose author is not an Oceans user.
    pub unmatched_author_commit_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionCoverageView {
    /// Every request the report counts, allocated or uncommitted.
    #[serde(flatten)]
    pub requests: AttributionRequestTallyView,
    /// The 1000-row cap cut commits inside the window, or, when the window has merged pull
    /// requests, older commits their totals may read.
    pub commits_truncated: bool,
    pub pull_requests_truncated: bool,
    /// Some requests were left out because a lookup or the report hit its cap.
    pub request_facts_truncated: bool,
    /// The 1000-row cap cut hook-reported agent sessions, so session counts and uncommitted
    /// requests are low.
    pub agent_sessions_truncated: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionUserRefView {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionOwnerRefView {
    pub kind: AttributionOwnerKind,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionUserActivityView {
    pub user: AttributionUserRefView,
    pub commit_count: i64,
    pub merged_pull_request_count: i64,
    pub ai_lines: i64,
    pub human_lines: i64,
    pub ai_session_count: i64,
    pub agent_session_count: i64,
    #[serde(flatten)]
    pub requests: AttributionRequestTallyView,
    /// Gateway model keys, most requests first.
    pub models: Vec<String>,
    pub last_activity_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionModelView {
    pub model_key: String,
    /// Requests allocated to commits.
    #[serde(flatten)]
    pub requests: AttributionRequestTallyView,
    /// Window commits with at least one request on this model.
    pub commit_count: i64,
    pub uncommitted_request_count: i64,
    pub uncommitted_cost_usd_10000: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionPullRequestView {
    pub repository: String,
    pub number: i64,
    pub title: String,
    pub url: Option<String>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub merged_at: String,
    pub author: Option<AttributionUserRefView>,
    pub commit_count: i64,
    /// Listed commits with a stored note; the merge commit when none of them has one.
    pub attributed_commit_count: i64,
    pub ai_lines: i64,
    pub human_lines: i64,
    #[serde(flatten)]
    pub requests: AttributionRequestTallyView,
    pub models: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionChangeView {
    pub attribution_commit_id: String,
    pub repository: String,
    pub commit_sha: String,
    pub branch: Option<String>,
    pub is_default_branch: bool,
    /// When the first note for this commit was observed.
    pub committed_at: String,
    pub author: Option<AttributionUserRefView>,
    pub owner: AttributionOwnerRefView,
    /// The most recently merged window PR listing this commit.
    pub pull_request_number: Option<i64>,
    pub ai_lines: i64,
    pub human_lines: i64,
    /// AI sessions named by the note.
    pub session_count: i64,
    pub joined_session_count: i64,
    pub unjoined_session_count: i64,
    #[serde(flatten)]
    pub requests: AttributionRequestTallyView,
    /// Gateway model keys of the allocated requests.
    pub models: Vec<String>,
    /// Models the note names.
    pub note_models: Vec<String>,
    /// git-ai agent tools the note names, such as `claude` or `pi`.
    pub agent_tools: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionReportView {
    pub window_days: u16,
    pub window_start: String,
    pub window_end: String,
    /// The user the rows are limited to, if any.
    pub user_id: Option<String>,
    pub repository: Option<String>,
    pub totals: AttributionTotalsView,
    pub coverage: AttributionCoverageView,
    pub users: Vec<AttributionUserActivityView>,
    pub models: Vec<AttributionModelView>,
    pub pull_requests: Vec<AttributionPullRequestView>,
    pub changes: Vec<AttributionChangeView>,
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/attribution/report",
    tag = "crate::http::attribution",
    params(AttributionReportRequestQuery),
    responses(
        (status = 200, body = Envelope<AttributionReportView>),
        (status = 400, description = "`days` is not 7, 30 or 90, or `user_id` is not a UUID"),
        (status = 401, description = "No signed-in session")
    ),
    security(("session_cookie" = []))
)]
pub async fn get_attribution_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AttributionReportRequestQuery>,
) -> Result<Json<Envelope<AttributionReportView>>, AppError> {
    let current_user = require_authenticated_session(&state, &headers).await?;
    let window_days = parse_window_days(query.days)?;
    let user_id = report_user(&current_user, query.user_id.as_deref())?;
    let repository = query.repository.filter(|repository| !repository.is_empty());
    let (window_start, window_end) = report_window(window_days, OffsetDateTime::now_utc());
    let report_query = AttributionReportQuery {
        from: window_start,
        to: window_end,
        user_id,
        repository: repository.clone(),
        limit: ATTRIBUTION_MAX_REPORT_ROWS,
    };
    let report = build_attribution_report(state.store.as_ref(), &report_query).await?;
    let names = Names::load(&state.store, &report).await?;
    let AttributionReport {
        totals,
        coverage,
        users,
        models,
        pull_requests,
        changes,
    } = report;
    Ok(Json(envelope(AttributionReportView {
        window_days,
        window_start: format_timestamp(window_start),
        window_end: format_timestamp(window_end),
        user_id: user_id.map(|id| id.to_string()),
        repository,
        totals: totals.into(),
        coverage: coverage.into(),
        users: users
            .into_iter()
            .map(|user| names.user_activity(user))
            .collect(),
        models: models.into_iter().map(Into::into).collect(),
        pull_requests: pull_requests
            .into_iter()
            .map(|pull_request| names.pull_request(pull_request))
            .collect(),
        changes: changes
            .into_iter()
            .map(|change| names.change(change))
            .collect(),
    })))
}

fn parse_window_days(days: Option<u16>) -> Result<u16, AppError> {
    let days = days.unwrap_or(7);
    if REPORT_WINDOW_DAYS.contains(&days) {
        Ok(days)
    } else {
        Err(AppError(GatewayError::InvalidRequest(
            "days must be 7, 30 or 90".to_string(),
        )))
    }
}

/// Whole UTC days ending at the next midnight, as the spend report counts them.
fn report_window(window_days: u16, now: OffsetDateTime) -> (OffsetDateTime, OffsetDateTime) {
    let window_end = now.to_offset(UtcOffset::UTC).replace_time(Time::MIDNIGHT) + Duration::days(1);
    (
        window_end - Duration::days(i64::from(window_days)),
        window_end,
    )
}

fn report_user(
    current_user: &UserRecord,
    requested: Option<&str>,
) -> Result<Option<Uuid>, AppError> {
    if current_user.global_role != GlobalRole::PlatformAdmin {
        return Ok(Some(current_user.user_id));
    }
    requested
        .filter(|value| !value.is_empty())
        .map(|value| {
            Uuid::parse_str(value).map_err(|_| {
                AppError(GatewayError::InvalidRequest(
                    "user_id must be a UUID".to_string(),
                ))
            })
        })
        .transpose()
}

/// Display names of the users and service accounts a report mentions.
#[derive(Default)]
struct Names {
    users: HashMap<Uuid, String>,
    service_accounts: HashMap<Uuid, String>,
}

impl Names {
    /// Two directory reads rather than one per id: a report can name up to 1,000 users.
    async fn load(store: &AnyStore, report: &AttributionReport) -> Result<Self, AppError> {
        let mut user_ids: HashSet<Uuid> = report.users.iter().map(|user| user.user_id).collect();
        let mut service_account_ids = HashSet::new();
        for activity in &report.pull_requests {
            user_ids.extend(activity.pull_request.author_user_id);
        }
        for activity in &report.changes {
            user_ids.extend(activity.commit.author_user_id);
            user_ids.extend(activity.commit.owner.owner_user_id);
            service_account_ids.extend(activity.commit.owner.owner_service_account_id);
        }
        let mut names = Self::default();
        if !user_ids.is_empty() {
            names.users = store
                .list_identity_users()
                .await?
                .into_iter()
                .filter(|identity| user_ids.contains(&identity.user.user_id))
                .map(|identity| (identity.user.user_id, identity.user.name))
                .collect();
        }
        if !service_account_ids.is_empty() {
            names.service_accounts = GatewayStore::list_service_accounts(store)
                .await?
                .into_iter()
                .filter(|account| service_account_ids.contains(&account.service_account_id))
                .map(|account| (account.service_account_id, account.service_account_name))
                .collect();
        }
        Ok(names)
    }

    fn user(&self, user_id: Uuid) -> AttributionUserRefView {
        AttributionUserRefView {
            id: user_id.to_string(),
            name: name_or_unknown(self.users.get(&user_id)),
        }
    }

    fn owner(&self, owner: &AttributionOwner) -> AttributionOwnerRefView {
        match (owner.owner_user_id, owner.owner_service_account_id) {
            (None, Some(service_account_id)) => AttributionOwnerRefView {
                kind: AttributionOwnerKind::ServiceAccount,
                id: service_account_id.to_string(),
                name: name_or_unknown(self.service_accounts.get(&service_account_id)),
            },
            (user_id, _) => AttributionOwnerRefView {
                kind: AttributionOwnerKind::User,
                id: user_id.map(|id| id.to_string()).unwrap_or_default(),
                name: name_or_unknown(user_id.and_then(|id| self.users.get(&id))),
            },
        }
    }

    fn user_activity(&self, activity: AttributionUserActivity) -> AttributionUserActivityView {
        AttributionUserActivityView {
            user: self.user(activity.user_id),
            commit_count: activity.commit_count,
            merged_pull_request_count: activity.merged_pull_request_count,
            ai_lines: activity.ai_lines,
            human_lines: activity.human_lines,
            ai_session_count: activity.ai_session_count,
            agent_session_count: activity.agent_session_count,
            requests: activity.requests.into(),
            models: activity.models,
            last_activity_at: activity.last_activity_at.map(format_timestamp),
        }
    }

    fn pull_request(&self, activity: AttributionPullRequestActivity) -> AttributionPullRequestView {
        let pull_request = activity.pull_request;
        AttributionPullRequestView {
            author: pull_request.author_user_id.map(|id| self.user(id)),
            commit_count: count(pull_request.commits.len()),
            merged_at: format_timestamp(pull_request.merged_at),
            repository: pull_request.repository,
            number: pull_request.number,
            title: pull_request.title,
            url: pull_request.url,
            base_ref: pull_request.base_ref,
            head_ref: pull_request.head_ref,
            attributed_commit_count: activity.attributed_commit_count,
            ai_lines: activity.ai_lines,
            human_lines: activity.human_lines,
            requests: activity.requests.into(),
            models: activity.models,
        }
    }

    fn change(&self, activity: AttributionChangeActivity) -> AttributionChangeView {
        let commit = activity.commit;
        let session_count = count(commit.sessions.len());
        AttributionChangeView {
            attribution_commit_id: commit.attribution_commit_id.to_string(),
            author: commit.author_user_id.map(|id| self.user(id)),
            owner: self.owner(&commit.owner),
            committed_at: format_timestamp(commit.first_observed_at),
            repository: commit.repository,
            commit_sha: commit.commit_sha,
            branch: commit.branch,
            is_default_branch: commit.is_default_branch,
            pull_request_number: activity.pull_request_number,
            ai_lines: commit.ai_lines,
            human_lines: commit.human_lines,
            session_count,
            joined_session_count: activity.joined_session_count,
            unjoined_session_count: (session_count - activity.joined_session_count).max(0),
            requests: activity.requests.into(),
            models: activity.models,
            note_models: activity.note_models,
            agent_tools: activity.agent_tools,
        }
    }
}

fn name_or_unknown(name: Option<&String>) -> String {
    name.cloned().unwrap_or_else(|| UNKNOWN_NAME.to_string())
}

fn count(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

impl From<RequestTally> for AttributionRequestTallyView {
    fn from(tally: RequestTally) -> Self {
        Self {
            request_count: tally.request_count,
            priced_request_count: tally.priced_request_count,
            unpriced_request_count: tally.unpriced_request_count,
            usage_missing_request_count: tally.usage_missing_request_count,
            priced_cost_usd_10000: tally.priced_cost_10000,
        }
    }
}

impl From<AttributionReportTotals> for AttributionTotalsView {
    fn from(totals: AttributionReportTotals) -> Self {
        Self {
            commit_count: totals.commit_count,
            pull_request_count: totals.pull_request_count,
            ai_lines: totals.ai_lines,
            human_lines: totals.human_lines,
            ai_session_count: totals.ai_session_count,
            joined_session_count: totals.joined_session_count,
            agent_session_count: totals.agent_session_count,
            requests: totals.requests.into(),
            uncommitted_request_count: totals.uncommitted.request_count,
            uncommitted_cost_usd_10000: totals.uncommitted.priced_cost_10000,
            unmatched_author_commit_count: totals.unmatched_author_commit_count,
        }
    }
}

impl From<AttributionReportCoverage> for AttributionCoverageView {
    fn from(coverage: AttributionReportCoverage) -> Self {
        Self {
            requests: coverage.requests.into(),
            commits_truncated: coverage.commits_truncated,
            pull_requests_truncated: coverage.pull_requests_truncated,
            request_facts_truncated: coverage.request_facts_truncated,
            agent_sessions_truncated: coverage.agent_sessions_truncated,
        }
    }
}

impl From<AttributionModelActivity> for AttributionModelView {
    fn from(model: AttributionModelActivity) -> Self {
        Self {
            model_key: model.model_key,
            requests: model.requests.into(),
            commit_count: model.commit_count,
            uncommitted_request_count: model.uncommitted.request_count,
            uncommitted_cost_usd_10000: model.uncommitted.priced_cost_10000,
        }
    }
}

#[cfg(test)]
mod window_tests {
    use time::{Date, Month};

    use super::*;

    fn day(day: u8) -> Date {
        Date::from_calendar_date(2026, Month::November, day).unwrap()
    }

    #[test]
    fn windows_are_whole_utc_days_ending_at_the_next_midnight() {
        let now = day(12)
            .with_hms(8, 30, 0)
            .unwrap()
            .assume_offset(UtcOffset::from_hms(2, 0, 0).unwrap());
        let (start, end) = report_window(7, now);
        assert_eq!(end, day(13).midnight().assume_utc());
        assert_eq!(start, day(6).midnight().assume_utc());
        assert!(parse_window_days(Some(90)).is_ok());
        assert!(parse_window_days(Some(14)).is_err());
        assert_eq!(parse_window_days(None).ok(), Some(7));
    }
}
