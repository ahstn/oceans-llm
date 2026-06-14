use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use time::{Date, Duration, Month, OffsetDateTime, UtcOffset};
use uuid::Uuid;

pub const SYSTEM_BOOTSTRAP_ADMIN_USER_ID: &str = "00000000-0000-0000-0000-000000000002";
pub const SYSTEM_BOOTSTRAP_ADMIN_EMAIL: &str = "admin@local";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct Money4 {
    amount_10000: i64,
}

impl Money4 {
    pub const SCALE: i64 = 10_000;
    pub const ZERO: Self = Self { amount_10000: 0 };

    #[must_use]
    pub const fn from_scaled(amount_10000: i64) -> Self {
        Self { amount_10000 }
    }

    #[must_use]
    pub const fn as_scaled_i64(self) -> i64 {
        self.amount_10000
    }

    #[must_use]
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.amount_10000
            .checked_add(other.amount_10000)
            .map(Self::from_scaled)
    }

    #[must_use]
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.amount_10000
            .checked_sub(other.amount_10000)
            .map(Self::from_scaled)
    }

    pub fn from_decimal_str(value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("money value cannot be empty".to_string());
        }

        let (negative, digits) = if let Some(stripped) = trimmed.strip_prefix('-') {
            (true, stripped)
        } else {
            (false, trimmed)
        };

        let mut parts = digits.split('.');
        let integer_part = parts
            .next()
            .ok_or_else(|| "money value is missing integer part".to_string())?;
        let fraction_part = parts.next().unwrap_or_default();

        if parts.next().is_some() {
            return Err(format!(
                "money value `{value}` has too many decimal separators"
            ));
        }
        if integer_part.is_empty() || !integer_part.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(format!("money value `{value}` has an invalid integer part"));
        }
        if fraction_part.len() > 4 || !fraction_part.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(format!(
                "money value `{value}` has an invalid fractional part"
            ));
        }

        let integer = integer_part
            .parse::<i64>()
            .map_err(|error| format!("failed parsing integer money value `{value}`: {error}"))?;
        let mut scaled = integer
            .checked_mul(Self::SCALE)
            .ok_or_else(|| format!("money value `{value}` overflowed"))?;

        if !fraction_part.is_empty() {
            let fraction = fraction_part.parse::<i64>().map_err(|error| {
                format!("failed parsing fractional money value `{value}`: {error}")
            })?;
            let scale = 10_i64.pow((4 - fraction_part.len()) as u32);
            scaled = scaled
                .checked_add(
                    fraction
                        .checked_mul(scale)
                        .ok_or_else(|| format!("money value `{value}` overflowed"))?,
                )
                .ok_or_else(|| format!("money value `{value}` overflowed"))?;
        }

        if negative {
            scaled = scaled
                .checked_neg()
                .ok_or_else(|| format!("money value `{value}` overflowed"))?;
        }

        Ok(Self::from_scaled(scaled))
    }

    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.amount_10000 < 0
    }

    #[must_use]
    pub fn format_4dp(self) -> String {
        let is_negative = self.amount_10000 < 0;
        let absolute = self.amount_10000.unsigned_abs();
        let integer = absolute / (Self::SCALE as u64);
        let fraction = absolute % (Self::SCALE as u64);
        if is_negative {
            format!("-{integer}.{fraction:04}")
        } else {
            format!("{integer}.{fraction:04}")
        }
    }
}

impl std::fmt::Display for Money4 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.format_4dp())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    Password,
    Oidc,
    Oauth,
}

impl AuthMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Oidc => "oidc",
            Self::Oauth => "oauth",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "password" => Some(Self::Password),
            "oidc" => Some(Self::Oidc),
            "oauth" => Some(Self::Oauth),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalRole {
    PlatformAdmin,
    User,
}

impl GlobalRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlatformAdmin => "platform_admin",
            Self::User => "user",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "platform_admin" => Some(Self::PlatformAdmin),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    Active,
    Invited,
    Disabled,
}

impl UserStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Invited => "invited",
            Self::Disabled => "disabled",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "invited" => Some(Self::Invited),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipRole {
    Owner,
    Admin,
    Member,
}

impl MembershipRole {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAccessMode {
    All,
    Restricted,
}

impl ModelAccessMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Restricted => "restricted",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "restricted" => Some(Self::Restricted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetCadence {
    Daily,
    Weekly,
    Monthly,
}

impl BudgetCadence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            "monthly" => Some(Self::Monthly),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetWindow {
    pub period_start: OffsetDateTime,
    pub period_end: OffsetDateTime,
    pub observed_end: OffsetDateTime,
}

pub fn budget_window_utc(
    cadence: BudgetCadence,
    occurred_at: OffsetDateTime,
) -> Result<BudgetWindow, String> {
    let now_utc = occurred_at.to_offset(UtcOffset::UTC);
    let date = now_utc.date();
    let day_start = date
        .with_hms(0, 0, 0)
        .map_err(|error| format!("invalid day start: {error}"))?
        .assume_offset(UtcOffset::UTC);

    let (period_start, period_end) = match cadence {
        BudgetCadence::Daily => (day_start, day_start + Duration::days(1)),
        BudgetCadence::Weekly => {
            let days_from_monday = i64::from(now_utc.weekday().number_days_from_monday());
            let start = day_start - Duration::days(days_from_monday);
            (start, start + Duration::days(7))
        }
        BudgetCadence::Monthly => {
            let start_date = Date::from_calendar_date(date.year(), date.month(), 1)
                .map_err(|error| format!("invalid month start: {error}"))?;
            let start = start_date
                .with_hms(0, 0, 0)
                .map_err(|error| format!("invalid month start time: {error}"))?
                .assume_offset(UtcOffset::UTC);
            let (next_year, next_month) = next_calendar_month(date.year(), date.month());
            let end = Date::from_calendar_date(next_year, next_month, 1)
                .map_err(|error| format!("invalid next month start: {error}"))?
                .with_hms(0, 0, 0)
                .map_err(|error| format!("invalid next month start time: {error}"))?
                .assume_offset(UtcOffset::UTC);
            (start, end)
        }
    };
    let observed_end = std::cmp::min(now_utc + Duration::seconds(1), period_end);

    Ok(BudgetWindow {
        period_start,
        period_end,
        observed_end,
    })
}

const fn next_calendar_month(year: i32, month: Month) -> (i32, Month) {
    match month {
        Month::January => (year, Month::February),
        Month::February => (year, Month::March),
        Month::March => (year, Month::April),
        Month::April => (year, Month::May),
        Month::May => (year, Month::June),
        Month::June => (year, Month::July),
        Month::July => (year, Month::August),
        Month::August => (year, Month::September),
        Month::September => (year, Month::October),
        Month::October => (year, Month::November),
        Month::November => (year, Month::December),
        Month::December => (year + 1, Month::January),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyOwnerKind {
    User,
    ServiceAccount,
}

impl ApiKeyOwnerKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ServiceAccount => "service_account",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "service_account" => Some(Self::ServiceAccount),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAccountStatus {
    Active,
    Disabled,
}

impl ServiceAccountStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyStatus {
    Active,
    Revoked,
}

impl ApiKeyStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "revoked" => Some(Self::Revoked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub id: Uuid,
    pub public_id: String,
    pub secret_hash: String,
    pub name: String,
    pub status: ApiKeyStatus,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewApiKeyRecord {
    pub name: String,
    pub public_id: String,
    pub secret_hash: String,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamRecord {
    pub team_id: Uuid,
    pub team_key: String,
    pub team_name: String,
    pub status: String,
    pub model_access_mode: ModelAccessMode,
    #[serde(default)]
    pub tags: Vec<RequestTag>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccountRecord {
    pub service_account_id: Uuid,
    pub team_id: Uuid,
    pub service_account_key: String,
    pub service_account_name: String,
    pub status: ServiceAccountStatus,
    pub model_access_mode: ModelAccessMode,
    pub metadata: Value,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub disabled_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecord {
    pub user_id: Uuid,
    pub name: String,
    pub email: String,
    pub email_normalized: String,
    pub global_role: GlobalRole,
    pub auth_mode: AuthMode,
    pub status: UserStatus,
    pub must_change_password: bool,
    pub request_logging_enabled: bool,
    pub model_access_mode: ModelAccessMode,
    #[serde(default)]
    pub tags: Vec<RequestTag>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPasswordAuthRecord {
    pub user_id: Uuid,
    pub password_hash: String,
    pub password_updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityUserRecord {
    pub user: UserRecord,
    pub team_id: Option<Uuid>,
    pub team_name: Option<String>,
    pub membership_role: Option<MembershipRole>,
    pub oidc_provider_id: Option<String>,
    pub oidc_provider_key: Option<String>,
    pub oauth_provider_id: Option<String>,
    pub oauth_provider_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMembershipRecord {
    pub team_id: Uuid,
    pub user_id: Uuid,
    pub role: MembershipRole,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcProviderRecord {
    pub oidc_provider_id: String,
    pub provider_key: String,
    pub provider_type: String,
    pub label: String,
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret_ref: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    pub jit: OidcJitPolicy,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OauthProviderRecord {
    pub oauth_provider_id: String,
    pub provider_key: String,
    pub provider_type: String,
    pub label: String,
    pub client_id: String,
    pub client_secret_ref: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    pub jit: OauthJitPolicy,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcJitPolicy {
    pub enabled: bool,
    pub global_role: GlobalRole,
    pub membership: Option<OidcJitMembership>,
    pub request_logging_enabled: bool,
}

impl Default for OidcJitPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            global_role: GlobalRole::User,
            membership: None,
            request_logging_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcJitMembership {
    pub team_key: String,
    pub role: MembershipRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OauthJitPolicy {
    pub enabled: bool,
    pub global_role: GlobalRole,
    pub membership: Option<OauthJitMembership>,
    pub request_logging_enabled: bool,
}

impl Default for OauthJitPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            global_role: GlobalRole::User,
            membership: None,
            request_logging_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OauthJitMembership {
    pub team_key: String,
    pub role: MembershipRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcLoginStateRecord {
    pub state_hash: String,
    pub oidc_provider_id: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub redirect_to: String,
    pub login_hint: Option<String>,
    pub expires_at: OffsetDateTime,
    pub consumed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OauthLoginStateRecord {
    pub state_hash: String,
    pub oauth_provider_id: String,
    pub pkce_verifier: String,
    pub redirect_to: String,
    pub login_hint: Option<String>,
    pub expires_at: OffsetDateTime,
    pub consumed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserOidcAuthRecord {
    pub user_id: Uuid,
    pub oidc_provider_id: String,
    pub subject: String,
    pub email_claim: Option<String>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserOauthAuthRecord {
    pub user_id: Uuid,
    pub oauth_provider_id: String,
    pub subject: String,
    pub email_claim: Option<String>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordInvitationRecord {
    pub invitation_id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: OffsetDateTime,
    pub consumed_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSessionRecord {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub last_seen_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BudgetAlertChannel {
    Email,
}

impl BudgetAlertChannel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "email" => Some(Self::Email),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BudgetAlertDeliveryStatus {
    Pending,
    Sent,
    Failed,
}

impl BudgetAlertDeliveryStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "sent" => Some(Self::Sent),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertRecord {
    pub budget_alert_id: Uuid,
    pub ownership_scope_key: String,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_id: Uuid,
    pub owner_name: String,
    pub budget_id: Uuid,
    pub cadence: BudgetCadence,
    pub threshold_bps: i32,
    pub window_start: OffsetDateTime,
    pub window_end: OffsetDateTime,
    pub spend_before_usd: Money4,
    pub spend_after_usd: Money4,
    pub remaining_budget_usd: Money4,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertDeliveryRecord {
    pub budget_alert_delivery_id: Uuid,
    pub budget_alert_id: Uuid,
    pub channel: BudgetAlertChannel,
    pub delivery_status: BudgetAlertDeliveryStatus,
    pub recipient: Option<String>,
    pub provider_message_id: Option<String>,
    pub failure_reason: Option<String>,
    pub queued_at: OffsetDateTime,
    pub last_attempted_at: Option<OffsetDateTime>,
    pub sent_at: Option<OffsetDateTime>,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertDispatchTask {
    pub alert: BudgetAlertRecord,
    pub delivery: BudgetAlertDeliveryRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertHistoryQuery {
    pub page: u32,
    pub page_size: u32,
    pub owner_kind: Option<ApiKeyOwnerKind>,
    pub channel: Option<BudgetAlertChannel>,
    pub delivery_status: Option<BudgetAlertDeliveryStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertHistoryRecord {
    pub budget_alert_id: Uuid,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_id: Uuid,
    pub owner_name: String,
    pub channel: BudgetAlertChannel,
    pub delivery_status: BudgetAlertDeliveryStatus,
    pub recipient_summary: String,
    pub threshold_bps: i32,
    pub cadence: BudgetCadence,
    pub window_start: OffsetDateTime,
    pub window_end: OffsetDateTime,
    pub spend_before_usd: Money4,
    pub spend_after_usd: Money4,
    pub remaining_budget_usd: Money4,
    pub created_at: OffsetDateTime,
    pub last_attempted_at: Option<OffsetDateTime>,
    pub sent_at: Option<OffsetDateTime>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetAlertHistoryPage {
    pub items: Vec<BudgetAlertHistoryRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendDailyAggregateRecord {
    pub day_start: OffsetDateTime,
    pub priced_cost_usd: Money4,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendOwnerAggregateRecord {
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_id: Uuid,
    pub owner_name: String,
    pub priced_cost_usd: Money4,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendModelAggregateRecord {
    pub model_key: String,
    pub priced_cost_usd: Money4,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusExportAggregateRecord {
    pub day_start: OffsetDateTime,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_id: Uuid,
    pub owner_name: String,
    pub owner_tags: Vec<RequestTag>,
    pub model_id: Option<Uuid>,
    pub model_key: String,
    pub provider_key: String,
    pub upstream_model: String,
    pub pricing_status: UsagePricingStatus,
    pub pricing_row_id: Option<Uuid>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub request_count: i64,
    pub computed_cost_usd: Money4,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FocusExportDiagnosticsRecord {
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLeaderboardUserRecord {
    pub user_id: Uuid,
    pub user_name: String,
    pub priced_cost_usd: Money4,
    pub total_request_count: i64,
    pub top_model_key: Option<String>,
    pub tool_cardinality_averages: RequestToolCardinalityAverages,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLeaderboardBucketRecord {
    pub user_id: Uuid,
    pub bucket_start: OffsetDateTime,
    pub priced_cost_usd: Money4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessUsageLeaderRecord {
    pub agent_harness_key: String,
    pub agent_harness_label: String,
    pub request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessUsageBucketRecord {
    pub agent_harness_key: String,
    pub bucket_start: OffsetDateTime,
    pub request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLedgerRecord {
    pub usage_event_id: Uuid,
    pub request_id: String,
    pub ownership_scope_key: String,
    pub api_key_id: Uuid,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub model_id: Option<Uuid>,
    pub provider_key: String,
    pub upstream_model: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub provider_usage: Value,
    pub pricing_status: UsagePricingStatus,
    pub unpriced_reason: Option<String>,
    pub pricing_row_id: Option<Uuid>,
    pub pricing_provider_id: Option<String>,
    pub pricing_model_id: Option<String>,
    pub pricing_source: Option<String>,
    pub pricing_source_etag: Option<String>,
    pub pricing_source_fetched_at: Option<OffsetDateTime>,
    pub pricing_last_updated: Option<String>,
    pub input_cost_per_million_tokens: Option<Money4>,
    pub output_cost_per_million_tokens: Option<Money4>,
    pub computed_cost_usd: Money4,
    pub occurred_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsagePricingStatus {
    Priced,
    Unpriced,
    UsageMissing,
    LegacyEstimated,
}

impl UsagePricingStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Priced => "priced",
            Self::Unpriced => "unpriced",
            Self::UsageMissing => "usage_missing",
            Self::LegacyEstimated => "legacy_estimated",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "priced" => Some(Self::Priced),
            "unpriced" => Some(Self::Unpriced),
            "usage_missing" => Some(Self::UsageMissing),
            "legacy_estimated" => Some(Self::LegacyEstimated),
            _ => None,
        }
    }

    #[must_use]
    pub const fn counts_toward_spend(self) -> bool {
        matches!(self, Self::Priced | Self::LegacyEstimated)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestTag {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RequestTags {
    pub service: Option<String>,
    pub component: Option<String>,
    pub env: Option<String>,
    #[serde(default)]
    pub bespoke: Vec<RequestTag>,
}

impl RequestTags {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.service.is_none()
            && self.component.is_none()
            && self.env.is_none()
            && self.bespoke.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogRecord {
    pub request_log_id: Uuid,
    pub request_id: String,
    pub api_key_id: Uuid,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
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
    pub request_tags: RequestTags,
    pub tool_cardinality: RequestToolCardinality,
    pub user_agent_raw: Option<String>,
    pub agent_harness_key: String,
    pub agent_harness_label: String,
    pub metadata: Map<String, Value>,
    pub occurred_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RequestToolCardinality {
    pub referenced_mcp_server_count: Option<i64>,
    pub exposed_tool_count: Option<i64>,
    pub invoked_tool_count: Option<i64>,
    pub filtered_tool_count: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
pub struct RequestToolCardinalityAverages {
    pub referenced_mcp_server_count: Option<f64>,
    pub exposed_tool_count: Option<f64>,
    pub invoked_tool_count: Option<f64>,
    pub filtered_tool_count: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogPayloadRecord {
    pub request_log_id: Uuid,
    pub request_json: Value,
    pub response_json: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestAttemptStatus {
    Success,
    ProviderError,
    StreamStartError,
    StreamError,
}

impl RequestAttemptStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::ProviderError => "provider_error",
            Self::StreamStartError => "stream_start_error",
            Self::StreamError => "stream_error",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "provider_error" => Some(Self::ProviderError),
            "stream_start_error" => Some(Self::StreamStartError),
            "stream_error" => Some(Self::StreamError),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestAttemptRecord {
    pub request_attempt_id: Uuid,
    pub request_log_id: Uuid,
    pub request_id: String,
    pub attempt_number: i64,
    pub route_id: Uuid,
    pub provider_key: String,
    pub upstream_model: String,
    pub status: RequestAttemptStatus,
    pub status_code: Option<i64>,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
    pub error_detail_truncated: bool,
    pub retryable: bool,
    pub terminal: bool,
    pub produced_final_response: bool,
    pub stream: bool,
    pub started_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub latency_ms: Option<i64>,
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RequestLogQuery {
    pub page: u32,
    pub page_size: u32,
    pub request_id: Option<String>,
    pub model_key: Option<String>,
    pub provider_key: Option<String>,
    pub status_code: Option<i64>,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub service_account_id: Option<Uuid>,
    pub service: Option<String>,
    pub component: Option<String>,
    pub env: Option<String>,
    pub tag_key: Option<String>,
    pub tag_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogPage {
    pub items: Vec<RequestLogRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogDetail {
    pub log: RequestLogRecord,
    pub payload: Option<RequestLogPayloadRecord>,
    pub attempts: Vec<RequestAttemptRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestLogRetentionWindow {
    #[serde(rename = "1d")]
    OneDay,
    #[serde(rename = "3d")]
    ThreeDays,
    #[serde(rename = "7d")]
    SevenDays,
}

impl RequestLogRetentionWindow {
    #[must_use]
    pub const fn days(self) -> i64 {
        match self {
            Self::OneDay => 1,
            Self::ThreeDays => 3,
            Self::SevenDays => 7,
        }
    }

    #[must_use]
    pub fn cutoff_at(self, now: OffsetDateTime) -> OffsetDateTime {
        now - time::Duration::days(self.days())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogPurgeResult {
    pub cutoff: OffsetDateTime,
    pub dry_run: bool,
    pub matched_count: u64,
    pub deleted_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolInvocationStatus {
    Success,
    Unauthorized,
    PolicyDenied,
    UpstreamError,
    GatewayError,
    Timeout,
    InvalidRequest,
}

impl McpToolInvocationStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Unauthorized => "unauthorized",
            Self::PolicyDenied => "policy_denied",
            Self::UpstreamError => "upstream_error",
            Self::GatewayError => "gateway_error",
            Self::Timeout => "timeout",
            Self::InvalidRequest => "invalid_request",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "unauthorized" => Some(Self::Unauthorized),
            "policy_denied" => Some(Self::PolicyDenied),
            "upstream_error" => Some(Self::UpstreamError),
            "gateway_error" => Some(Self::GatewayError),
            "timeout" => Some(Self::Timeout),
            "invalid_request" => Some(Self::InvalidRequest),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolPolicyResult {
    Allowed,
    Denied,
    NotEvaluated,
}

impl McpToolPolicyResult {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::NotEvaluated => "not_evaluated",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "allowed" => Some(Self::Allowed),
            "denied" => Some(Self::Denied),
            "not_evaluated" => Some(Self::NotEvaluated),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalMcpTransport {
    StreamableHttp,
}

impl ExternalMcpTransport {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StreamableHttp => "streamable_http",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "streamable_http" => Some(Self::StreamableHttp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalMcpServerStatus {
    Active,
    Disabled,
}

impl ExternalMcpServerStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalMcpAuthMode {
    None,
    GatewayStaticHeader,
    GatewayBearerToken,
    UserPassthrough,
    OauthObo,
}

impl ExternalMcpAuthMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::GatewayStaticHeader => "gateway_static_header",
            Self::GatewayBearerToken => "gateway_bearer_token",
            Self::UserPassthrough => "user_passthrough",
            Self::OauthObo => "oauth_obo",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "gateway_static_header" => Some(Self::GatewayStaticHeader),
            "gateway_bearer_token" => Some(Self::GatewayBearerToken),
            "user_passthrough" => Some(Self::UserPassthrough),
            "oauth_obo" => Some(Self::OauthObo),
            _ => None,
        }
    }

    #[must_use]
    pub const fn supports_gateway_discovery(self) -> bool {
        matches!(
            self,
            Self::None | Self::GatewayStaticHeader | Self::GatewayBearerToken
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalMcpDiscoveryStatus {
    Success,
    Failed,
    AuthRequired,
    Disabled,
}

impl ExternalMcpDiscoveryStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::AuthRequired => "auth_required",
            Self::Disabled => "disabled",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "failed" => Some(Self::Failed),
            "auth_required" => Some(Self::AuthRequired),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalMcpServerRecord {
    pub mcp_server_id: Uuid,
    pub server_key: String,
    pub display_name: String,
    pub description: Option<String>,
    pub transport: ExternalMcpTransport,
    pub server_url: String,
    pub auth_mode: ExternalMcpAuthMode,
    pub auth_config: Map<String, Value>,
    pub timeout_ms: i64,
    pub status: ExternalMcpServerStatus,
    pub last_discovery_status: Option<ExternalMcpDiscoveryStatus>,
    pub last_discovery_at: Option<OffsetDateTime>,
    pub last_successful_discovery_at: Option<OffsetDateTime>,
    pub last_error_summary: Option<String>,
    pub last_tool_count: Option<i64>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub disabled_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewExternalMcpServerRecord {
    pub server_key: String,
    pub display_name: String,
    pub description: Option<String>,
    pub transport: ExternalMcpTransport,
    pub server_url: String,
    pub auth_mode: ExternalMcpAuthMode,
    pub auth_config: Map<String, Value>,
    pub timeout_ms: i64,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateExternalMcpServerRecord {
    pub mcp_server_id: Uuid,
    pub display_name: String,
    pub description: Option<String>,
    pub server_url: String,
    pub auth_mode: ExternalMcpAuthMode,
    pub auth_config: Map<String, Value>,
    pub timeout_ms: i64,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalMcpToolRecord {
    pub mcp_tool_id: Uuid,
    pub mcp_server_id: Uuid,
    pub upstream_name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub schema_hash: String,
    pub schema_version: i64,
    pub is_active: bool,
    pub first_discovered_at: OffsetDateTime,
    pub last_discovered_at: OffsetDateTime,
    pub deactivated_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCatalogToolRecord {
    pub server: ExternalMcpServerRecord,
    pub tool: ExternalMcpToolRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpCatalogAccessResolution {
    pub allowed_tools: Vec<McpCatalogToolRecord>,
    pub referenced_server_count: i64,
    pub exposed_tool_count: i64,
    pub filtered_tool_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpUpstreamCredentialOwnerScopeKind {
    User,
    Team,
    ServiceAccount,
}

impl McpUpstreamCredentialOwnerScopeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Team => "team",
            Self::ServiceAccount => "service_account",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "team" => Some(Self::Team),
            "service_account" => Some(Self::ServiceAccount),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpUpstreamCredentialMaterialKind {
    StaticHeader,
    BearerToken,
    OauthTokens,
}

impl McpUpstreamCredentialMaterialKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StaticHeader => "static_header",
            Self::BearerToken => "bearer_token",
            Self::OauthTokens => "oauth_tokens",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "static_header" => Some(Self::StaticHeader),
            "bearer_token" => Some(Self::BearerToken),
            "oauth_tokens" => Some(Self::OauthTokens),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpUpstreamSecretStorageKind {
    EncryptedBlob,
    SecretRef,
}

impl McpUpstreamSecretStorageKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EncryptedBlob => "encrypted_blob",
            Self::SecretRef => "secret_ref",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "encrypted_blob" => Some(Self::EncryptedBlob),
            "secret_ref" => Some(Self::SecretRef),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpUpstreamCredentialBindingRecord {
    pub credential_binding_id: Uuid,
    pub mcp_server_id: Uuid,
    pub owner_scope_kind: McpUpstreamCredentialOwnerScopeKind,
    pub owner_scope_key: String,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
    pub material_kind: McpUpstreamCredentialMaterialKind,
    pub header_name: Option<String>,
    pub storage_kind: McpUpstreamSecretStorageKind,
    pub secret_ciphertext: Option<String>,
    pub secret_nonce: Option<String>,
    pub secret_key_id: Option<String>,
    pub secret_ref: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    pub metadata: Map<String, Value>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertMcpUpstreamCredentialBindingRecord {
    pub credential_binding_id: Option<Uuid>,
    pub mcp_server_id: Uuid,
    pub owner_scope_kind: McpUpstreamCredentialOwnerScopeKind,
    pub owner_scope_key: String,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
    pub material_kind: McpUpstreamCredentialMaterialKind,
    pub header_name: Option<String>,
    pub storage_kind: McpUpstreamSecretStorageKind,
    pub secret_ciphertext: Option<String>,
    pub secret_nonce: Option<String>,
    pub secret_key_id: Option<String>,
    pub secret_ref: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    pub metadata: Map<String, Value>,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertExternalMcpToolRecord {
    pub mcp_server_id: Uuid,
    pub upstream_name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub input_schema: Value,
    pub schema_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalMcpDiscoveryRunRecord {
    pub discovery_run_id: Uuid,
    pub mcp_server_id: Uuid,
    pub status: ExternalMcpDiscoveryStatus,
    pub started_at: OffsetDateTime,
    pub finished_at: OffsetDateTime,
    pub discovered_tool_count: i64,
    pub active_tool_count: i64,
    pub schema_set_hash: Option<String>,
    pub error_summary: Option<String>,
    pub details: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolsetStatus {
    Active,
    Disabled,
}

impl McpToolsetStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolGrantSubjectKind {
    ApiKey,
    User,
    Team,
    ServiceAccount,
}

impl McpToolGrantSubjectKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::User => "user",
            Self::Team => "team",
            Self::ServiceAccount => "service_account",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "api_key" => Some(Self::ApiKey),
            "user" => Some(Self::User),
            "team" => Some(Self::Team),
            "service_account" => Some(Self::ServiceAccount),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolGrantTargetKind {
    Tool,
    Toolset,
}

impl McpToolGrantTargetKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Toolset => "toolset",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "tool" => Some(Self::Tool),
            "toolset" => Some(Self::Toolset),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpGrantSubject {
    pub subject_kind: McpToolGrantSubjectKind,
    pub subject_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolsetRecord {
    pub toolset_id: Uuid,
    pub toolset_key: String,
    pub display_name: String,
    pub description: Option<String>,
    pub status: McpToolsetStatus,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub disabled_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMcpToolsetRecord {
    pub toolset_key: String,
    pub display_name: String,
    pub description: Option<String>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMcpToolsetRecord {
    pub toolset_id: Uuid,
    pub display_name: String,
    pub description: Option<String>,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolsetToolRecord {
    pub toolset_id: Uuid,
    pub mcp_tool_id: Uuid,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolGrantRecord {
    pub grant_id: Uuid,
    pub subject_kind: McpToolGrantSubjectKind,
    pub subject_id: Uuid,
    pub target_kind: McpToolGrantTargetKind,
    pub target_id: Uuid,
    pub is_active: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertMcpToolGrantRecord {
    pub subject_kind: McpToolGrantSubjectKind,
    pub subject_id: Uuid,
    pub target_kind: McpToolGrantTargetKind,
    pub target_id: Uuid,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpAccessResolution {
    pub allowed_tools: Vec<ExternalMcpToolRecord>,
    pub referenced_server_count: i64,
    pub exposed_tool_count: i64,
    pub filtered_tool_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAggregateSessionRecord {
    pub session_id: Uuid,
    pub token_hash: String,
    pub api_key_id: Uuid,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
    pub protocol_version: String,
    pub initialized: bool,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMcpAggregateSessionRecord {
    pub session_id: Uuid,
    pub token_hash: String,
    pub api_key_id: Uuid,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
    pub protocol_version: String,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTokenEstimateSource {
    LocalTokenizer,
    ConservativeFallback,
}

impl McpTokenEstimateSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalTokenizer => "local_tokenizer",
            Self::ConservativeFallback => "conservative_fallback",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "local_tokenizer" => Some(Self::LocalTokenizer),
            "conservative_fallback" => Some(Self::ConservativeFallback),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTokenEstimateConfidence {
    High,
    Low,
}

impl McpTokenEstimateConfidence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Low => "low",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "high" => Some(Self::High),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolTokenEstimateRecord {
    pub cache_key: String,
    pub provider_family: String,
    pub model_or_encoding: String,
    pub mcp_server_id: Uuid,
    pub mcp_tool_id: Uuid,
    pub tool_name: String,
    pub schema_hash: String,
    pub description_hash: String,
    pub protocol_version: String,
    pub serializer_version: String,
    pub estimated_tokens: i64,
    pub estimator_source: McpTokenEstimateSource,
    pub confidence: McpTokenEstimateConfidence,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMcpTokenOverheadRecord {
    pub request_id: String,
    pub request_log_id: Option<Uuid>,
    pub model_key: Option<String>,
    pub provider_family: String,
    pub model_or_encoding: String,
    pub exposed_tool_count: i64,
    pub estimated_definition_tokens: i64,
    pub estimated_result_tokens: Option<i64>,
    pub estimator_source: McpTokenEstimateSource,
    pub confidence: McpTokenEstimateConfidence,
    pub cache_hit_count: i64,
    pub cache_miss_count: i64,
    pub context_window_tokens: Option<i64>,
    pub context_window_percent_bps: Option<i64>,
    pub metadata: Map<String, Value>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInvocationRecord {
    pub mcp_tool_invocation_id: Uuid,
    pub request_log_id: Option<Uuid>,
    pub request_id: String,
    pub api_key_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub owner_kind: ApiKeyOwnerKind,
    pub server_id: Option<Uuid>,
    pub server_display_key: String,
    pub server_display_name: String,
    pub tool_id: Option<Uuid>,
    pub tool_display_key: String,
    pub tool_display_name: String,
    pub status: McpToolInvocationStatus,
    pub policy_result: McpToolPolicyResult,
    pub latency_ms: Option<i64>,
    pub error_code: Option<String>,
    pub has_payload: bool,
    pub arguments_payload_truncated: bool,
    pub result_payload_truncated: bool,
    pub arguments_payload_redacted: bool,
    pub result_payload_redacted: bool,
    pub metadata: Map<String, Value>,
    pub occurred_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInvocationPayloadRecord {
    pub mcp_tool_invocation_id: Uuid,
    pub arguments_json: Value,
    pub result_json: Value,
}

pub const MAX_MCP_TOOL_INVOCATION_PAGE_SIZE: u32 = 500;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpToolInvocationQuery {
    pub page: u32,
    pub page_size: u32,
    pub request_id: Option<String>,
    pub server_display_key: Option<String>,
    pub server_display_name: Option<String>,
    pub tool_display_key: Option<String>,
    pub tool_display_name: Option<String>,
    pub api_key_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub status: Option<McpToolInvocationStatus>,
    pub policy_result: Option<McpToolPolicyResult>,
    pub occurred_at_start: Option<OffsetDateTime>,
    pub occurred_at_end: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInvocationPage {
    pub items: Vec<McpToolInvocationRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInvocationDetail {
    pub invocation: McpToolInvocationRecord,
    pub payload: Option<McpToolInvocationPayloadRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PricingCatalogCacheRecord {
    pub catalog_key: String,
    pub source: String,
    pub etag: Option<String>,
    pub fetched_at: OffsetDateTime,
    pub snapshot_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PricingLimits {
    pub context: Option<i64>,
    pub input: Option<i64>,
    pub output: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PricingModalities {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PricingProvenance {
    pub source: String,
    pub etag: Option<String>,
    pub fetched_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedModelPricing {
    pub model_pricing_id: Uuid,
    pub pricing_provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub input_cost_per_million_tokens: Option<Money4>,
    pub output_cost_per_million_tokens: Option<Money4>,
    pub cache_read_cost_per_million_tokens: Option<Money4>,
    pub cache_write_cost_per_million_tokens: Option<Money4>,
    pub input_audio_cost_per_million_tokens: Option<Money4>,
    pub output_audio_cost_per_million_tokens: Option<Money4>,
    pub release_date: String,
    pub last_updated: String,
    pub effective_start_at: OffsetDateTime,
    pub effective_end_at: Option<OffsetDateTime>,
    pub limits: PricingLimits,
    pub modalities: PricingModalities,
    pub provenance: PricingProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelPricingRecord {
    pub model_pricing_id: Uuid,
    pub pricing_provider_id: String,
    pub pricing_model_id: String,
    pub display_name: String,
    pub input_cost_per_million_tokens: Option<Money4>,
    pub output_cost_per_million_tokens: Option<Money4>,
    pub cache_read_cost_per_million_tokens: Option<Money4>,
    pub cache_write_cost_per_million_tokens: Option<Money4>,
    pub input_audio_cost_per_million_tokens: Option<Money4>,
    pub output_audio_cost_per_million_tokens: Option<Money4>,
    pub release_date: String,
    pub last_updated: String,
    pub effective_start_at: OffsetDateTime,
    pub effective_end_at: Option<OffsetDateTime>,
    pub limits: PricingLimits,
    pub modalities: PricingModalities,
    pub provenance: PricingProvenance,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum PricingUnpricedReason {
    ProviderPricingSourceMissing,
    UnsupportedPricingProviderId(String),
    UnsupportedVertexPublisher(String),
    UnsupportedVertexLocation(String),
    UnsupportedBillingModifier(String),
    ModelNotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PricingResolution {
    Exact { pricing: Box<ResolvedModelPricing> },
    Unpriced { reason: PricingUnpricedReason },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayModel {
    pub id: Uuid,
    pub model_key: String,
    #[serde(default)]
    pub alias_target_model_key: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub rank: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRoute {
    pub id: Uuid,
    pub model_id: Uuid,
    pub provider_key: String,
    pub upstream_model: String,
    pub priority: i32,
    pub weight: f64,
    pub enabled: bool,
    #[serde(default)]
    pub extra_headers: Map<String, Value>,
    #[serde(default)]
    pub extra_body: Map<String, Value>,
    #[serde(default = "ProviderCapabilities::all_enabled")]
    pub capabilities: ProviderCapabilities,
    #[serde(default)]
    pub compatibility: RouteCompatibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConnection {
    pub provider_key: String,
    pub provider_type: String,
    pub config: Value,
    pub secrets: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequestContext {
    pub request_id: String,
    pub model_key: String,
    pub provider_key: String,
    pub upstream_model: String,
    #[serde(default)]
    pub extra_headers: Map<String, Value>,
    #[serde(default)]
    pub extra_body: Map<String, Value>,
    #[serde(default)]
    pub request_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub compatibility: RouteCompatibility,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCapabilities {
    #[serde(default = "default_true")]
    pub chat_completions: bool,
    #[serde(default = "default_true")]
    pub responses: bool,
    #[serde(default = "default_true")]
    pub stream: bool,
    #[serde(default = "default_true")]
    pub embeddings: bool,
    #[serde(default = "default_true")]
    pub tools: bool,
    #[serde(default = "default_true")]
    pub vision: bool,
    #[serde(default = "default_true")]
    pub json_schema: bool,
    #[serde(default = "default_true")]
    pub developer_role: bool,
}

impl ProviderCapabilities {
    #[must_use]
    pub const fn new(chat_completions: bool, stream: bool, embeddings: bool) -> Self {
        Self::with_dimensions(
            chat_completions,
            stream,
            embeddings,
            false,
            false,
            false,
            false,
        )
    }

    #[must_use]
    pub const fn with_dimensions(
        chat_completions: bool,
        stream: bool,
        embeddings: bool,
        tools: bool,
        vision: bool,
        json_schema: bool,
        developer_role: bool,
    ) -> Self {
        Self {
            chat_completions,
            responses: false,
            stream,
            embeddings,
            tools,
            vision,
            json_schema,
            developer_role,
        }
    }

    #[must_use]
    pub const fn chat_only_streaming() -> Self {
        Self::with_dimensions(true, true, false, false, true, false, true)
    }

    #[must_use]
    pub const fn openai_compat_baseline() -> Self {
        Self {
            chat_completions: true,
            responses: true,
            stream: true,
            embeddings: true,
            tools: true,
            vision: true,
            json_schema: true,
            developer_role: true,
        }
    }

    #[must_use]
    pub const fn all_enabled() -> Self {
        Self {
            chat_completions: true,
            responses: true,
            stream: true,
            embeddings: true,
            tools: true,
            vision: true,
            json_schema: true,
            developer_role: true,
        }
    }

    #[must_use]
    pub const fn intersect(self, other: Self) -> Self {
        Self {
            chat_completions: self.chat_completions && other.chat_completions,
            responses: self.responses && other.responses,
            stream: self.stream && other.stream,
            embeddings: self.embeddings && other.embeddings,
            tools: self.tools && other.tools,
            vision: self.vision && other.vision,
            json_schema: self.json_schema && other.json_schema,
            developer_role: self.developer_role && other.developer_role,
        }
    }
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self::all_enabled()
    }
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RouteCompatibility {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_compat: Option<OpenAiCompatRouteCompatibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openrouter: Option<OpenRouterRouteCompatibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aws_bedrock: Option<AwsBedrockRouteCompatibility>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenRouterRouteCompatibility {
    pub provider: OpenRouterProviderRouting,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct OpenRouterProviderRouting {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zdr: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_max_latency: Option<OpenRouterPercentilePreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_price: Option<OpenRouterMaxPrice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum OpenRouterPercentilePreference {
    Number(f64),
    Percentiles(OpenRouterPercentileCutoffs),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct OpenRouterPercentileCutoffs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p50: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p75: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p90: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p99: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct OpenRouterMaxPrice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AwsBedrockRouteCompatibility {
    pub api_style: AwsBedrockApiStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_base_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AwsBedrockApiStyle {
    RuntimeConverse,
    RuntimeAnthropicInvoke,
    RuntimeOpenaiChat,
    MantleOpenaiResponses,
    MantleOpenaiChat,
    MantleAnthropicMessages,
}

impl AwsBedrockApiStyle {
    #[must_use]
    pub const fn is_runtime(self) -> bool {
        matches!(
            self,
            Self::RuntimeConverse | Self::RuntimeAnthropicInvoke | Self::RuntimeOpenaiChat
        )
    }

    #[must_use]
    pub const fn is_mantle(self) -> bool {
        matches!(
            self,
            Self::MantleOpenaiResponses | Self::MantleOpenaiChat | Self::MantleAnthropicMessages
        )
    }

    #[must_use]
    pub const fn is_openai_shaped(self) -> bool {
        matches!(
            self,
            Self::RuntimeOpenaiChat | Self::MantleOpenaiResponses | Self::MantleOpenaiChat
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAiCompatRouteCompatibility {
    #[serde(default = "default_true")]
    pub supports_store: bool,
    #[serde(default)]
    pub max_tokens_field: OpenAiCompatMaxTokensField,
    #[serde(default)]
    pub developer_role: OpenAiCompatDeveloperRole,
    #[serde(default)]
    pub reasoning_effort: OpenAiCompatReasoningEffort,
    #[serde(default)]
    pub supports_stream_usage: bool,
}

impl Default for OpenAiCompatRouteCompatibility {
    fn default() -> Self {
        Self {
            supports_store: true,
            max_tokens_field: OpenAiCompatMaxTokensField::default(),
            developer_role: OpenAiCompatDeveloperRole::default(),
            reasoning_effort: OpenAiCompatReasoningEffort::default(),
            supports_stream_usage: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiCompatMaxTokensField {
    #[default]
    MaxCompletionTokens,
    MaxTokens,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiCompatDeveloperRole {
    #[default]
    Developer,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiCompatReasoningEffort {
    #[default]
    Passthrough,
    Omit,
    ReasoningObject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedProvider {
    pub provider_key: String,
    pub provider_type: String,
    pub config: Value,
    pub secrets: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedModelRoute {
    pub provider_key: String,
    pub upstream_model: String,
    pub priority: i32,
    pub weight: f64,
    pub enabled: bool,
    #[serde(default)]
    pub extra_headers: Map<String, Value>,
    #[serde(default)]
    pub extra_body: Map<String, Value>,
    #[serde(default = "ProviderCapabilities::all_enabled")]
    pub capabilities: ProviderCapabilities,
    #[serde(default)]
    pub compatibility: RouteCompatibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedModel {
    pub model_key: String,
    #[serde(default)]
    pub alias_target_model_key: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub rank: i32,
    #[serde(default)]
    pub routes: Vec<SeedModelRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedApiKey {
    pub name: String,
    pub public_id: String,
    pub secret_hash: String,
    pub service_account_key: String,
    pub service_account_name: String,
    pub service_account_team_key: String,
    pub service_account_budget: SeedBudget,
    #[serde(default)]
    pub allowed_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedOidcProvider {
    pub provider_key: String,
    pub provider_type: String,
    pub label: String,
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret_ref: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    pub jit: OidcJitPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedOauthProvider {
    pub provider_key: String,
    pub provider_type: String,
    pub label: String,
    pub client_id: String,
    pub client_secret_ref: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    pub jit: OauthJitPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedBudget {
    pub cadence: BudgetCadence,
    pub amount_usd: Money4,
    pub hard_limit: bool,
    pub timezone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedTeam {
    pub team_key: String,
    pub team_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedUserMembership {
    pub team_key: String,
    pub role: MembershipRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedUser {
    pub name: String,
    pub email: String,
    pub email_normalized: String,
    pub global_role: GlobalRole,
    pub auth_mode: AuthMode,
    pub request_logging_enabled: bool,
    #[serde(default)]
    pub oidc_provider_key: Option<String>,
    #[serde(default)]
    pub oauth_provider_key: Option<String>,
    #[serde(default)]
    pub membership: Option<SeedUserMembership>,
    #[serde(default)]
    pub budget: Option<SeedBudget>,
}
