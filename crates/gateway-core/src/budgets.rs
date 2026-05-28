use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::domain::{BudgetCadence, Money4};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetScopeKind {
    User,
    ServiceAccount,
    UserModel,
}

impl BudgetScopeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ServiceAccount => "service_account",
            Self::UserModel => "user_model",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "service_account" => Some(Self::ServiceAccount),
            "user_model" => Some(Self::UserModel),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BudgetModelSelector {
    Model { model_id: Uuid },
    UpstreamModel { upstream_model: String },
}

impl BudgetModelSelector {
    #[must_use]
    pub fn model_id(&self) -> Option<Uuid> {
        match self {
            Self::Model { model_id } => Some(*model_id),
            Self::UpstreamModel { .. } => None,
        }
    }

    #[must_use]
    pub fn upstream_model(&self) -> Option<&str> {
        match self {
            Self::Model { .. } => None,
            Self::UpstreamModel { upstream_model } => Some(upstream_model.trim()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BudgetScope {
    User {
        user_id: Uuid,
    },
    ServiceAccount {
        service_account_id: Uuid,
    },
    UserModel {
        user_id: Uuid,
        selector: BudgetModelSelector,
    },
}

impl BudgetScope {
    #[must_use]
    pub const fn kind(&self) -> BudgetScopeKind {
        match self {
            Self::User { .. } => BudgetScopeKind::User,
            Self::ServiceAccount { .. } => BudgetScopeKind::ServiceAccount,
            Self::UserModel { .. } => BudgetScopeKind::UserModel,
        }
    }

    #[must_use]
    pub fn scope_key(&self) -> String {
        match self {
            Self::User { user_id } => format!("budget:v1:user:{user_id}"),
            Self::ServiceAccount { service_account_id } => {
                format!("budget:v1:service_account:{service_account_id}")
            }
            Self::UserModel { user_id, selector } => match selector {
                BudgetModelSelector::Model { model_id } => {
                    format!("budget:v1:user:{user_id}:model:{model_id}")
                }
                BudgetModelSelector::UpstreamModel { upstream_model } => {
                    format!(
                        "budget:v1:user:{user_id}:upstream_model:{}",
                        upstream_model.trim()
                    )
                }
            },
        }
    }

    #[must_use]
    pub fn user_id(&self) -> Option<Uuid> {
        match self {
            Self::User { user_id } | Self::UserModel { user_id, .. } => Some(*user_id),
            Self::ServiceAccount { .. } => None,
        }
    }

    #[must_use]
    pub fn service_account_id(&self) -> Option<Uuid> {
        match self {
            Self::ServiceAccount { service_account_id } => Some(*service_account_id),
            Self::User { .. } | Self::UserModel { .. } => None,
        }
    }

    #[must_use]
    pub fn model_id(&self) -> Option<Uuid> {
        match self {
            Self::UserModel { selector, .. } => selector.model_id(),
            Self::User { .. } | Self::ServiceAccount { .. } => None,
        }
    }

    #[must_use]
    pub fn upstream_model(&self) -> Option<&str> {
        match self {
            Self::UserModel { selector, .. } => selector.upstream_model(),
            Self::User { .. } | Self::ServiceAccount { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetSettings {
    pub cadence: BudgetCadence,
    pub amount_usd: Money4,
    pub hard_limit: bool,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetRecord {
    pub budget_id: Uuid,
    pub scope: BudgetScope,
    pub scope_key: String,
    pub settings: BudgetSettings,
    pub is_active: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}
