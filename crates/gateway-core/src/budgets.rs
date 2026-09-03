use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeStruct};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetSourceKind {
    Manual,
    ConfigUserOverride,
    ConfigUserDefault,
    ConfigUserModelDefault,
    ConfigServiceAccount,
}

impl BudgetSourceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::ConfigUserOverride => "config_user_override",
            Self::ConfigUserDefault => "config_user_default",
            Self::ConfigUserModelDefault => "config_user_model_default",
            Self::ConfigServiceAccount => "config_service_account",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "config_user_override" => Some(Self::ConfigUserOverride),
            "config_user_default" => Some(Self::ConfigUserDefault),
            "config_user_model_default" => Some(Self::ConfigUserModelDefault),
            "config_service_account" => Some(Self::ConfigServiceAccount),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetSource {
    pub kind: BudgetSourceKind,
    #[serde(default)]
    pub key: Option<String>,
}

impl BudgetSource {
    #[must_use]
    pub fn manual() -> Self {
        Self {
            kind: BudgetSourceKind::Manual,
            key: None,
        }
    }

    #[must_use]
    pub fn manual_deactivated() -> Self {
        Self {
            kind: BudgetSourceKind::Manual,
            key: Some("deactivated".to_string()),
        }
    }

    #[must_use]
    pub fn is_manual_deactivation(&self) -> bool {
        self.kind == BudgetSourceKind::Manual && self.key.as_deref() == Some("deactivated")
    }

    #[must_use]
    pub fn config_user_override(email_normalized: impl Into<String>) -> Self {
        Self {
            kind: BudgetSourceKind::ConfigUserOverride,
            key: Some(email_normalized.into()),
        }
    }

    #[must_use]
    pub fn config_user_default() -> Self {
        Self {
            kind: BudgetSourceKind::ConfigUserDefault,
            key: Some("budgets.users.default".to_string()),
        }
    }

    #[must_use]
    pub fn config_user_model_default(model_key: impl Into<String>) -> Self {
        Self {
            kind: BudgetSourceKind::ConfigUserModelDefault,
            key: Some(format!("budgets.users.model_defaults:{}", model_key.into())),
        }
    }

    #[must_use]
    pub fn config_service_account(service_account_key: impl Into<String>) -> Self {
        Self {
            kind: BudgetSourceKind::ConfigServiceAccount,
            key: Some(service_account_key.into()),
        }
    }

    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        self.kind == other.kind && self.key.as_deref() == other.key.as_deref()
    }
}

#[derive(Debug, Clone)]
pub enum BudgetModelSelector {
    Model { model_id: Uuid },
    UpstreamModel { upstream_model: String },
}

impl PartialEq for BudgetModelSelector {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Model { model_id: left }, Self::Model { model_id: right }) => left == right,
            (
                Self::UpstreamModel {
                    upstream_model: left,
                },
                Self::UpstreamModel {
                    upstream_model: right,
                },
            ) => left.trim() == right.trim(),
            _ => false,
        }
    }
}

impl Eq for BudgetModelSelector {}

impl Serialize for BudgetModelSelector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Model { model_id } => {
                let mut state = serializer.serialize_struct("BudgetModelSelector", 2)?;
                state.serialize_field("kind", "model")?;
                state.serialize_field("model_id", model_id)?;
                state.end()
            }
            Self::UpstreamModel { upstream_model } => {
                let mut state = serializer.serialize_struct("BudgetModelSelector", 2)?;
                state.serialize_field("kind", "upstream_model")?;
                state.serialize_field("upstream_model", upstream_model.trim())?;
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for BudgetModelSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct BudgetModelSelectorWire {
            kind: String,
            model_id: Option<Uuid>,
            upstream_model: Option<String>,
        }

        let wire = BudgetModelSelectorWire::deserialize(deserializer)?;
        match wire.kind.as_str() {
            "model" => wire
                .model_id
                .map(|model_id| Self::Model { model_id })
                .ok_or_else(|| serde::de::Error::missing_field("model_id")),
            "upstream_model" => {
                let upstream_model = wire
                    .upstream_model
                    .ok_or_else(|| serde::de::Error::missing_field("upstream_model"))?;
                let upstream_model = upstream_model.trim();
                if upstream_model.is_empty() {
                    return Err(serde::de::Error::custom("upstream_model cannot be empty"));
                }
                Ok(Self::UpstreamModel {
                    upstream_model: upstream_model.to_string(),
                })
            }
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["model", "upstream_model"],
            )),
        }
    }
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
    pub source: BudgetSource,
    pub is_active: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[cfg(test)]
mod tests {
    use super::{BudgetModelSelector, BudgetScope};
    use uuid::Uuid;

    #[test]
    fn upstream_model_selector_uses_trimmed_identity() {
        assert_eq!(
            BudgetModelSelector::UpstreamModel {
                upstream_model: " gpt-4 ".to_string(),
            },
            BudgetModelSelector::UpstreamModel {
                upstream_model: "gpt-4".to_string(),
            }
        );
    }

    #[test]
    fn upstream_model_selector_serializes_trimmed_value() {
        let json = serde_json::to_value(BudgetModelSelector::UpstreamModel {
            upstream_model: " gpt-4 ".to_string(),
        })
        .expect("serialize selector");

        assert_eq!(json["upstream_model"], "gpt-4");
    }

    #[test]
    fn upstream_model_selector_deserializes_trimmed_value() {
        let selector: BudgetModelSelector =
            serde_json::from_str(r#"{"kind":"upstream_model","upstream_model":" gpt-4 "}"#)
                .expect("deserialize selector");

        assert_eq!(
            selector,
            BudgetModelSelector::UpstreamModel {
                upstream_model: "gpt-4".to_string(),
            }
        );
    }

    #[test]
    fn budget_scope_key_uses_trimmed_upstream_model() {
        let user_id = Uuid::new_v4();
        let scope = BudgetScope::UserModel {
            user_id,
            selector: BudgetModelSelector::UpstreamModel {
                upstream_model: " gpt-4 ".to_string(),
            },
        };

        assert_eq!(
            scope.scope_key(),
            format!("budget:v1:user:{user_id}:upstream_model:gpt-4")
        );
    }
}
