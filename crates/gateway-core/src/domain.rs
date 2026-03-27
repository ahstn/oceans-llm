use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use time::{Date, Duration, Month, OffsetDateTime, UtcOffset};
use uuid::Uuid;

pub const SYSTEM_LEGACY_TEAM_ID: &str = "00000000-0000-0000-0000-000000000001";
pub const SYSTEM_LEGACY_TEAM_KEY: &str = "system-legacy";
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
    Team,
}

impl ApiKeyOwnerKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Team => "team",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "team" => Some(Self::Team),
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
    pub status: String,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamRecord {
    pub team_id: Uuid,
    pub team_key: String,
    pub team_name: String,
    pub status: String,
    pub model_access_mode: ModelAccessMode,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
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
    pub issuer_url: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserBudgetRecord {
    pub user_budget_id: Uuid,
    pub user_id: Uuid,
    pub cadence: BudgetCadence,
    pub amount_usd: Money4,
    pub hard_limit: bool,
    pub timezone: String,
    pub is_active: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamBudgetRecord {
    pub team_budget_id: Uuid,
    pub team_id: Uuid,
    pub cadence: BudgetCadence,
    pub amount_usd: Money4,
    pub hard_limit: bool,
    pub timezone: String,
    pub is_active: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
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
pub struct UsageLedgerRecord {
    pub usage_event_id: Uuid,
    pub request_id: String,
    pub ownership_scope_key: String,
    pub api_key_id: Uuid,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub metadata: Map<String, Value>,
    pub occurred_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogPayloadRecord {
    pub request_log_id: Uuid,
    pub request_json: Value,
    pub response_json: Value,
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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCapabilities {
    #[serde(default = "default_true")]
    pub chat_completions: bool,
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
        Self::with_dimensions(true, true, true, true, true, true, true)
    }

    #[must_use]
    pub const fn all_enabled() -> Self {
        Self::with_dimensions(true, true, true, true, true, true, true)
    }

    #[must_use]
    pub const fn intersect(self, other: Self) -> Self {
        Self::with_dimensions(
            self.chat_completions && other.chat_completions,
            self.stream && other.stream,
            self.embeddings && other.embeddings,
            self.tools && other.tools,
            self.vision && other.vision,
            self.json_schema && other.json_schema,
            self.developer_role && other.developer_role,
        )
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
    #[serde(default)]
    pub allowed_models: Vec<String>,
}
