use std::{fs, path::Path};

use anyhow::Context;
use gateway_service::{
    AdminModelStatus as ServiceAdminModelStatus, ModelIconKey as ServiceModelIconKey,
    ProviderIconKey as ServiceProviderIconKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use utoipa::{
    IntoParams, Modify, OpenApi, ToSchema,
    openapi::{
        Components,
        security::{ApiKey, ApiKeyValue, SecurityScheme},
    },
};

pub const ADMIN_OPENAPI_PATH: &str = "crates/gateway/openapi/admin-api.json";
const ADMIN_OPENAPI_DOCUMENT_VERSION: &str = "0.0.0";

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
pub struct OpenAiErrorBodyView {
    pub message: String,
    #[schema(rename = "type")]
    pub error_type: String,
    pub code: Option<String>,
    pub param: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OpenAiErrorEnvelopeView {
    pub error: OpenAiErrorBodyView,
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
    pub request_logging_enabled: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AdminModelStatusView {
    Healthy,
    Degraded,
}

impl From<ServiceAdminModelStatus> for AdminModelStatusView {
    fn from(value: ServiceAdminModelStatus) -> Self {
        match value {
            ServiceAdminModelStatus::Healthy => Self::Healthy,
            ServiceAdminModelStatus::Degraded => Self::Degraded,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProviderIconKeyView {
    Anthropic,
    AWS,
    OpenAI,
    OpenRouter,
    VertexAI,
}

impl From<ServiceProviderIconKey> for ProviderIconKeyView {
    fn from(value: ServiceProviderIconKey) -> Self {
        match value {
            ServiceProviderIconKey::Anthropic => Self::Anthropic,
            ServiceProviderIconKey::AWS => Self::AWS,
            ServiceProviderIconKey::OpenAI => Self::OpenAI,
            ServiceProviderIconKey::OpenRouter => Self::OpenRouter,
            ServiceProviderIconKey::VertexAI => Self::VertexAI,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ModelIconKeyView {
    Anthropic,
    Claude,
    Gemini,
    OpenAI,
    OpenRouter,
    VertexAI,
}

impl From<ServiceModelIconKey> for ModelIconKeyView {
    fn from(value: ServiceModelIconKey) -> Self {
        match value {
            ServiceModelIconKey::Anthropic => Self::Anthropic,
            ServiceModelIconKey::Claude => Self::Claude,
            ServiceModelIconKey::Gemini => Self::Gemini,
            ServiceModelIconKey::OpenAI => Self::OpenAI,
            ServiceModelIconKey::OpenRouter => Self::OpenRouter,
            ServiceModelIconKey::VertexAI => Self::VertexAI,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminModelView {
    pub id: String,
    pub resolved_model_key: String,
    pub alias_of: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub status: AdminModelStatusView,
    pub provider_key: Option<String>,
    pub provider_label: Option<String>,
    pub provider_icon_key: Option<ProviderIconKeyView>,
    pub upstream_model: Option<String>,
    pub model_icon_key: Option<ModelIconKeyView>,
    pub input_cost_per_million_tokens_usd_10000: Option<i64>,
    pub output_cost_per_million_tokens_usd_10000: Option<i64>,
    pub cache_read_cost_per_million_tokens_usd_10000: Option<i64>,
    pub context_window_tokens: Option<i64>,
    pub input_window_tokens: Option<i64>,
    pub output_window_tokens: Option<i64>,
    pub supports_streaming: Option<bool>,
    pub supports_vision: Option<bool>,
    pub supports_tool_calling: Option<bool>,
    pub supports_structured_output: Option<bool>,
    pub supports_attachments: Option<bool>,
    pub client_configurations: Vec<AdminModelClientConfigView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminModelClientConfigView {
    pub key: String,
    pub label: String,
    pub filename: String,
    pub content: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Deserialize, Default, IntoParams)]
pub struct AdminModelListQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminModelPageView {
    pub items: Vec<AdminModelView>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
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
pub struct LeaderboardQuery {
    pub range: Option<String>,
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
pub struct LeaderboardChartUserView {
    pub rank: u32,
    pub user_id: String,
    pub user_name: String,
    pub total_spend_usd_10000: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LeaderboardSeriesValueView {
    pub user_id: String,
    pub spend_usd_10000: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LeaderboardSeriesPointView {
    pub bucket_start: String,
    pub values: Vec<LeaderboardSeriesValueView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LeaderboardLeaderView {
    pub rank: u32,
    pub user_id: String,
    pub user_name: String,
    pub total_spend_usd_10000: i64,
    pub most_used_model: Option<String>,
    pub total_requests: i64,
    pub tool_cardinality_averages: RequestToolCardinalityAveragesView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LeaderboardView {
    pub range: String,
    pub bucket_hours: u8,
    pub window_start: String,
    pub window_end: String,
    pub chart_users: Vec<LeaderboardChartUserView>,
    pub series: Vec<LeaderboardSeriesPointView>,
    pub leaders: Vec<LeaderboardLeaderView>,
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
    pub model_icon_key: Option<ModelIconKeyView>,
    pub provider_key: String,
    pub provider_icon_key: Option<ProviderIconKeyView>,
    pub status_code: Option<i64>,
    pub latency_ms: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub error_code: Option<String>,
    pub has_payload: bool,
    pub request_payload_truncated: bool,
    pub response_payload_truncated: bool,
    pub payload_policy: RequestLogPayloadPolicyView,
    pub request_tags: RequestTagsView,
    pub tool_cardinality: RequestToolCardinalityView,
    #[schema(additional_properties = true)]
    pub metadata: Map<String, Value>,
    pub occurred_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestToolCardinalityView {
    pub referenced_mcp_server_count: Option<i64>,
    pub exposed_tool_count: Option<i64>,
    pub invoked_tool_count: Option<i64>,
    pub filtered_tool_count: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestToolCardinalityAveragesView {
    pub referenced_mcp_server_count: Option<f64>,
    pub exposed_tool_count: Option<f64>,
    pub invoked_tool_count: Option<f64>,
    pub filtered_tool_count: Option<f64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestLogPayloadPolicyView {
    pub capture_mode: RequestLogPayloadCaptureModeView,
    #[schema(minimum = 1)]
    pub request_max_bytes: u64,
    #[schema(minimum = 1)]
    pub response_max_bytes: u64,
    #[schema(minimum = 1)]
    pub stream_max_events: u64,
    pub version: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestLogPayloadCaptureModeView {
    Disabled,
    SummaryOnly,
    RedactedPayloads,
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
    pub attempts: Vec<RequestAttemptView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestAttemptView {
    pub request_attempt_id: String,
    pub request_log_id: String,
    pub request_id: String,
    pub attempt_number: i64,
    pub route_id: String,
    pub provider_key: String,
    pub upstream_model: String,
    pub status: String,
    pub status_code: Option<i64>,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub error_detail_truncated: bool,
    pub retryable: bool,
    pub terminal: bool,
    pub produced_final_response: bool,
    pub stream: bool,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub latency_ms: Option<i64>,
    #[schema(additional_properties = true)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestLogPayloadView {
    pub request_json: Value,
    pub response_json: Value,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::http::api_keys::list_api_keys,
        crate::http::api_keys::create_api_key,
        crate::http::api_keys::update_api_key,
        crate::http::api_keys::revoke_api_key,
        crate::http::identity::list_identity_users,
        crate::http::identity::list_identity_teams,
        crate::http::models::list_models,
        crate::http::identity::create_identity_team,
        crate::http::identity::update_identity_team,
        crate::http::identity::add_identity_team_members,
        crate::http::identity::remove_identity_team_member,
        crate::http::identity::transfer_identity_team_member,
        crate::http::identity::get_auth_session,
        crate::http::identity::login_with_password,
        crate::http::identity::logout_current_session,
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
        crate::http::observability::get_usage_leaderboard,
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
    let mut openapi = AdminApiDoc::openapi();
    openapi.info.version = ADMIN_OPENAPI_DOCUMENT_VERSION.to_string();
    openapi
}

pub fn write_admin_openapi(path: &Path) -> anyhow::Result<()> {
    let mut document = serde_json::to_string_pretty(&admin_openapi())
        .context("failed serializing admin OpenAPI document")?;
    document.push('\n');

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed creating OpenAPI output directory `{}`",
                parent.display()
            )
        })?;
    }

    fs::write(path, document).with_context(|| {
        format!(
            "failed writing admin OpenAPI document to `{}`",
            path.display()
        )
    })?;
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
    value
        .format(&Rfc3339)
        .unwrap_or_else(|_| value.unix_timestamp().to_string())
}

#[cfg(test)]
mod tests {
    use super::admin_openapi;

    #[test]
    fn openapi_document_version_is_release_agnostic() {
        let openapi = admin_openapi();

        assert_eq!(openapi.info.version, "0.0.0");
    }

    #[test]
    fn openapi_document_includes_live_admin_paths_and_envelopes() {
        let openapi = admin_openapi();
        let paths = openapi.paths.paths;
        let components = openapi.components.expect("components");

        assert!(paths.contains_key("/api/v1/admin/identity/users"));
        assert!(paths.contains_key("/api/v1/admin/models"));
        assert!(paths.contains_key("/api/v1/admin/spend/report"));
        assert!(paths.contains_key("/api/v1/admin/observability/leaderboard"));
        assert!(paths.contains_key("/api/v1/admin/observability/request-logs/{request_log_id}"));
        assert!(paths.contains_key("/api/v1/auth/session"));
        assert!(paths.contains_key("/api/v1/auth/logout"));

        assert!(
            components
                .schemas
                .contains_key("Envelope_AdminIdentityPayload")
        );
        assert!(components.schemas.contains_key("Envelope_SpendReportView"));
        assert!(components.schemas.contains_key("Envelope_LeaderboardView"));
        assert!(
            components
                .schemas
                .contains_key("Envelope_RequestLogDetailView")
        );
    }
}
