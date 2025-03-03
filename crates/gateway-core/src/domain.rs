use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

pub const SYSTEM_LEGACY_TEAM_ID: &str = "00000000-0000-0000-0000-000000000001";
pub const SYSTEM_LEGACY_TEAM_KEY: &str = "system-legacy";

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
}

impl BudgetCadence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            _ => None,
        }
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
    pub status: String,
    pub request_logging_enabled: bool,
    pub model_access_mode: ModelAccessMode,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
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
pub struct UserBudgetRecord {
    pub user_budget_id: Uuid,
    pub user_id: Uuid,
    pub cadence: BudgetCadence,
    pub amount_usd: f64,
    pub hard_limit: bool,
    pub timezone: String,
    pub is_active: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageCostEventRecord {
    pub usage_event_id: Uuid,
    pub request_id: String,
    pub api_key_id: Uuid,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub model_id: Option<Uuid>,
    pub estimated_cost_usd: f64,
    pub occurred_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestLogRecord {
    pub request_log_id: Uuid,
    pub request_id: String,
    pub api_key_id: Uuid,
    pub user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub model_key: String,
    pub provider_key: String,
    pub status_code: Option<i64>,
    pub latency_ms: Option<i64>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub error_code: Option<String>,
    pub metadata: Map<String, Value>,
    pub occurred_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayModel {
    pub id: Uuid,
    pub model_key: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedModel {
    pub model_key: String,
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
