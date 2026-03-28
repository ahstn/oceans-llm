use std::{fs, path::Path};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use utoipa::{
    IntoParams, Modify, OpenApi, ToSchema,
    openapi::{
        Components,
        security::{ApiKey, ApiKeyValue, SecurityScheme},
    },
};

pub const ADMIN_OPENAPI_PATH: &str = "crates/gateway/openapi/admin-api.json";

#[derive(Debug, Serialize, ToSchema)]
#[schema(bound = "T: ToSchema")]
pub struct Envelope<T> {
    pub data: T,
    pub meta: ResponseMeta,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ResponseMeta {
    pub generated_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminIdentityPayload {
    pub users: Vec<AdminIdentityUserView>,
    pub teams: Vec<AdminTeamView>,
    pub oidc_providers: Vec<AdminOidcProviderView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminIdentityUserView {
    pub id: String,
    pub name: String,
    pub email: String,
    pub auth_mode: String,
    pub global_role: String,
    pub team_id: Option<String>,
    pub team_name: Option<String>,
    pub team_role: Option<String>,
    pub status: String,
    pub onboarding: Option<AdminOnboardingActionView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminTeamView {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminTeamsPayload {
    pub teams: Vec<AdminTeamManagementView>,
    pub users: Vec<AdminTeamAssignableUserView>,
    pub oidc_providers: Vec<AdminOidcProviderView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminTeamManagementView {
    pub id: String,
    pub name: String,
    pub key: String,
    pub status: String,
    pub member_count: usize,
    pub admins: Vec<AdminTeamAdminView>,
    pub members: Vec<AdminTeamMemberView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminTeamAdminView {
    pub id: String,
    pub name: String,
    pub email: String,
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminTeamMemberView {
    pub id: String,
    pub name: String,
    pub email: String,
    pub status: String,
    pub role: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminTeamAssignableUserView {
    pub id: String,
    pub name: String,
    pub email: String,
    pub status: String,
    pub team_id: Option<String>,
    pub team_name: Option<String>,
    pub team_role: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminOidcProviderView {
    pub id: String,
    pub key: String,
    pub label: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthSessionUserView {
    pub id: String,
    pub name: String,
    pub email: String,
    pub global_role: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthSessionView {
    pub user: AuthSessionUserView,
    pub must_change_password: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdminOnboardingActionView {
    PasswordInvite {
        invite_url: Option<String>,
        expires_at: Option<String>,
        can_resend: bool,
    },
    OidcSignIn {
        sign_in_url: String,
        provider_key: String,
        provider_label: String,
    },
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateUserRequest {
    pub name: String,
    pub email: String,
    pub auth_mode: String,
    pub global_role: String,
    pub team_id: Option<String>,
    pub team_role: Option<String>,
    pub oidc_provider_key: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateTeamRequest {
    pub name: String,
    pub admin_user_ids: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateTeamRequest {
    pub name: String,
    pub admin_user_ids: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddTeamMembersRequest {
    pub user_ids: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateUserRequest {
    pub global_role: String,
    pub auth_mode: Option<String>,
    pub team_id: Option<String>,
    pub team_role: Option<String>,
    pub oidc_provider_key: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TransferTeamMemberRequest {
    pub destination_team_id: String,
    pub destination_role: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreateUserResponse {
    PasswordInvite {
        user: AdminIdentityUserView,
        invite_url: String,
        expires_at: String,
    },
    OidcSignIn {
        user: AdminIdentityUserView,
        sign_in_url: String,
        provider_label: String,
    },
}

#[derive(Debug, Serialize, ToSchema)]
pub struct IdentityActionStatus {
    pub status: &'static str,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PasswordInviteResponse {
    pub user_id: String,
    pub invite_url: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct InvitationView {
    pub state: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CompleteInvitationRequest {
    pub password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PasswordLoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CompleteInvitationResponse {
    pub status: &'static str,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct OidcStartQuery {
    pub provider_key: String,
    pub login_hint: String,
    pub redirect_to: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct OidcCallbackQuery {
    pub provider_key: String,
    pub email: String,
    pub subject: Option<String>,
    pub redirect_to: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct SpendReportQuery {
    pub days: Option<u16>,
    pub owner_kind: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct BudgetAlertHistoryRequestQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub owner_kind: Option<String>,
    pub channel: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SpendTotalsView {
    pub priced_cost_usd_10000: i64,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SpendDailyPointView {
    pub day_start: String,
    pub priced_cost_usd_10000: i64,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SpendOwnerBreakdownView {
    pub owner_kind: String,
    pub owner_id: String,
    pub owner_name: String,
    pub priced_cost_usd_10000: i64,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SpendModelBreakdownView {
    pub model_key: String,
    pub priced_cost_usd_10000: i64,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct BudgetSettingsView {
    pub cadence: String,
    pub amount_usd: String,
    pub amount_usd_10000: i64,
    pub hard_limit: bool,
    pub timezone: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SpendBudgetUserView {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub team_id: Option<String>,
    pub team_name: Option<String>,
    pub budget: Option<BudgetSettingsView>,
    pub current_window_spend_usd_10000: i64,
    pub alert_email_ready: bool,
    pub alert_recipient_summary: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SpendBudgetTeamView {
    pub team_id: String,
    pub team_name: String,
    pub team_key: String,
    pub budget: Option<BudgetSettingsView>,
    pub current_window_spend_usd_10000: i64,
    pub alert_email_ready: bool,
    pub alert_recipient_summary: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SpendBudgetsView {
    pub users: Vec<SpendBudgetUserView>,
    pub teams: Vec<SpendBudgetTeamView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BudgetAlertHistoryItemView {
    pub budget_alert_id: String,
    pub owner_kind: String,
    pub owner_id: String,
    pub owner_name: String,
    pub channel: String,
    pub delivery_status: String,
    pub recipient_summary: String,
    pub threshold_bps: i32,
    pub cadence: String,
    pub window_start: String,
    pub window_end: String,
    pub spend_before_usd_10000: i64,
    pub spend_after_usd_10000: i64,
    pub remaining_budget_usd_10000: i64,
    pub created_at: String,
    pub last_attempted_at: Option<String>,
    pub sent_at: Option<String>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BudgetAlertHistoryView {
    pub items: Vec<BudgetAlertHistoryItemView>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UpsertBudgetResultView {
    pub owner_kind: String,
    pub owner_id: String,
    pub budget: BudgetSettingsView,
    pub current_window_spend_usd_10000: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeactivateBudgetResultView {
    pub owner_kind: String,
    pub owner_id: String,
    pub deactivated: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpsertBudgetRequest {
    pub cadence: String,
    pub amount_usd: String,
    pub hard_limit: bool,
    pub timezone: Option<String>,
}

#[derive(Debug, Deserialize, Default, IntoParams)]
pub struct RequestLogListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub request_id: Option<String>,
    pub model_key: Option<String>,
    pub provider_key: Option<String>,
    pub status_code: Option<i64>,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub service: Option<String>,
    pub component: Option<String>,
    pub env: Option<String>,
    pub tag_key: Option<String>,
    pub tag_value: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestLogPageView {
    pub items: Vec<RequestLogSummaryView>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestLogSummaryView {
    pub request_log_id: String,
    pub request_id: String,
    pub api_key_id: String,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub model_key: String,
    pub resolved_model_key: String,
    pub provider_key: String,
    pub status_code: Option<i64>,
    pub latency_ms: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub error_code: Option<String>,
    pub has_payload: bool,
    pub request_payload_truncated: bool,
    pub response_payload_truncated: bool,
    pub request_tags: RequestTagsView,
    pub metadata: Value,
    pub occurred_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestTagsView {
    pub service: Option<String>,
    pub component: Option<String>,
    pub env: Option<String>,
    pub bespoke: Vec<RequestTagView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestTagView {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestLogDetailView {
    pub log: RequestLogSummaryView,
    pub payload: Option<RequestLogPayloadView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestLogPayloadView {
    pub request_json: Value,
    pub response_json: Value,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::http::identity::list_identity_users,
        crate::http::identity::list_identity_teams,
        crate::http::identity::create_identity_team,
        crate::http::identity::update_identity_team,
        crate::http::identity::add_identity_team_members,
        crate::http::identity::remove_identity_team_member,
        crate::http::identity::transfer_identity_team_member,
        crate::http::identity::get_auth_session,
        crate::http::identity::login_with_password,
        crate::http::identity::change_password,
        crate::http::identity::create_identity_user,
        crate::http::identity::update_identity_user,
        crate::http::identity::deactivate_identity_user,
        crate::http::identity::reactivate_identity_user,
        crate::http::identity::reset_identity_user_onboarding,
        crate::http::identity::regenerate_password_invite,
        crate::http::identity::validate_password_invitation,
        crate::http::identity::complete_password_invitation,
        crate::http::identity::oidc_start,
        crate::http::identity::oidc_callback,
        crate::http::spend::get_spend_report,
        crate::http::spend::list_spend_budgets,
        crate::http::spend::list_budget_alert_history,
        crate::http::spend::upsert_user_budget,
        crate::http::spend::deactivate_user_budget,
        crate::http::spend::upsert_team_budget,
        crate::http::spend::deactivate_team_budget,
        crate::http::observability::list_request_logs,
        crate::http::observability::get_request_log_detail
    ),
    modifiers(&AdminApiSecurity)
)]
pub struct AdminApiDoc;

struct AdminApiSecurity;

impl Modify for AdminApiSecurity {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Components::new);
        components.add_security_scheme(
            "session_cookie",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("ogw_session"))),
        );
    }
}

pub fn admin_openapi() -> utoipa::openapi::OpenApi {
    AdminApiDoc::openapi()
}

pub fn write_admin_openapi(path: &Path) -> anyhow::Result<()> {
    let document = serde_json::to_string_pretty(&admin_openapi())
        .context("failed serializing admin OpenAPI document")?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating OpenAPI output directory `{}`", parent.display()))?;
    }

    fs::write(path, document)
        .with_context(|| format!("failed writing admin OpenAPI document to `{}`", path.display()))?;
    Ok(())
}

pub fn envelope<T>(data: T) -> Envelope<T> {
    Envelope {
        data,
        meta: ResponseMeta {
            generated_at: format_timestamp(OffsetDateTime::now_utc()),
        },
    }
}

pub fn format_timestamp(value: OffsetDateTime) -> String {
    value.format(&Rfc3339)
        .unwrap_or_else(|_| value.unix_timestamp().to_string())
}

#[cfg(test)]
mod tests {
    use super::admin_openapi;

    #[test]
    fn openapi_document_includes_live_admin_paths_and_envelopes() {
        let openapi = admin_openapi();
        let paths = openapi.paths.paths;
        let components = openapi.components.expect("components");

        assert!(paths.contains_key("/api/v1/admin/identity/users"));
        assert!(paths.contains_key("/api/v1/admin/spend/report"));
        assert!(paths.contains_key("/api/v1/admin/observability/request-logs/{request_log_id}"));
        assert!(paths.contains_key("/api/v1/auth/session"));

        assert!(components.schemas.contains_key("Envelope_AdminIdentityPayload"));
        assert!(components.schemas.contains_key("Envelope_SpendReportView"));
        assert!(components.schemas.contains_key("Envelope_RequestLogDetailView"));
    }
}
