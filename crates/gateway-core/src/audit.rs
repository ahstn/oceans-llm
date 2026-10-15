//! Admin audit trail records and repository contract.
//!
//! Audit entries explain administrator changes to the control plane. They are
//! separate from request logs, which explain model and tool traffic. See
//! `docs/adr/2026-10-15-admin-audit-log.md`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::StoreError;

pub const DEFAULT_AUDIT_LOG_PAGE_SIZE: u32 = 50;
pub const MAX_AUDIT_LOG_PAGE_SIZE: u32 = 200;

/// Who made an audited change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditActorKind {
    /// A signed-in user acting through an admin or self-service session.
    User,
    /// A gateway API key acting on the control plane.
    ApiKey,
    /// The gateway itself, such as startup seeding or a background job.
    System,
}

impl AuditActorKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ApiKey => "api_key",
            Self::System => "system",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "api_key" => Some(Self::ApiKey),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditActor {
    pub kind: AuditActorKind,
    pub user_id: Option<Uuid>,
    pub api_key_id: Option<Uuid>,
    /// Display snapshot, such as the user's email, taken when the entry is written.
    pub label: Option<String>,
}

impl AuditActor {
    #[must_use]
    pub fn user(user_id: Uuid, label: impl Into<String>) -> Self {
        Self {
            kind: AuditActorKind::User,
            user_id: Some(user_id),
            api_key_id: None,
            label: Some(label.into()),
        }
    }

    #[must_use]
    pub fn system(label: impl Into<String>) -> Self {
        Self {
            kind: AuditActorKind::System,
            user_id: None,
            api_key_id: None,
            label: Some(label.into()),
        }
    }
}

/// An audit entry ready to persist. Summaries and metadata must already be redacted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewAuditLogRecord {
    pub occurred_at: OffsetDateTime,
    pub actor: AuditActor,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub target_label: Option<String>,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub metadata: Value,
    pub request_ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditLogRecord {
    pub id: i64,
    pub occurred_at: OffsetDateTime,
    pub actor: AuditActor,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub target_label: Option<String>,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub metadata: Value,
    pub request_ip: Option<String>,
    pub user_agent: Option<String>,
}

/// Keyset position after the last entry of a page. Entries are ordered newest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditLogCursor {
    pub occurred_at_ms: i64,
    pub id: i64,
}

impl AuditLogCursor {
    /// Encodes the cursor as an opaque `<millis>_<id>` token for API responses.
    #[must_use]
    pub fn encode(self) -> String {
        format!("{}_{}", self.occurred_at_ms, self.id)
    }

    #[must_use]
    pub fn decode(token: &str) -> Option<Self> {
        let (occurred_at_ms, id) = token.split_once('_')?;
        Some(Self {
            occurred_at_ms: occurred_at_ms.parse().ok()?,
            id: id.parse().ok()?,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuditLogQuery {
    pub actor_user_id: Option<Uuid>,
    pub action: Option<String>,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub occurred_at_start: Option<OffsetDateTime>,
    pub occurred_at_end: Option<OffsetDateTime>,
    pub cursor: Option<AuditLogCursor>,
    pub limit: u32,
}

impl AuditLogQuery {
    #[must_use]
    pub fn effective_limit(&self) -> u32 {
        if self.limit == 0 {
            DEFAULT_AUDIT_LOG_PAGE_SIZE
        } else {
            self.limit.min(MAX_AUDIT_LOG_PAGE_SIZE)
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuditLogPage {
    pub items: Vec<AuditLogRecord>,
    /// Set when more entries exist after this page.
    pub next_cursor: Option<AuditLogCursor>,
}

#[async_trait]
pub trait AuditLogRepository: Send + Sync {
    async fn insert_audit_log(&self, record: &NewAuditLogRecord) -> Result<i64, StoreError>;

    async fn list_audit_logs(&self, query: &AuditLogQuery) -> Result<AuditLogPage, StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips_and_rejects_garbage() {
        let cursor = AuditLogCursor {
            occurred_at_ms: 1_800_000_000_123,
            id: 42,
        };
        assert_eq!(AuditLogCursor::decode(&cursor.encode()), Some(cursor));
        assert_eq!(AuditLogCursor::decode("nope"), None);
        assert_eq!(AuditLogCursor::decode("1_x"), None);
    }

    #[test]
    fn limit_defaults_and_clamps() {
        assert_eq!(
            AuditLogQuery::default().effective_limit(),
            DEFAULT_AUDIT_LOG_PAGE_SIZE
        );
        let query = AuditLogQuery {
            limit: 10_000,
            ..Default::default()
        };
        assert_eq!(query.effective_limit(), MAX_AUDIT_LOG_PAGE_SIZE);
    }
}
