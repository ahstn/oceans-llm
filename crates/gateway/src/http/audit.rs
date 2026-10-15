//! Admin audit trail: request context for audit entries and the read API.
//!
//! Handlers build an [`AuditContext`] from the signed-in actor and
//! [`AuditRequestMeta`], then write through [`AppState::audit`]. See
//! `docs/adr/2026-10-15-admin-audit-log.md`.

use std::net::{IpAddr, SocketAddr};

use axum::{
    Json,
    extract::{ConnectInfo, FromRequestParts, Query, State},
    http::{HeaderMap, request::Parts},
};
use gateway_core::{
    AuditActor, AuditActorKind, AuditLogCursor, AuditLogQuery, AuditLogRecord, AuditLogRepository,
    GatewayError, MAX_AUDIT_LOG_PAGE_SIZE, UserRecord,
};
use gateway_service::{AdminAudit, AuditContext, admin_audit::targets};
use gateway_store::AnyStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{Envelope, envelope, format_timestamp},
    error::AppError,
    state::AppState,
};

const MAX_USER_AGENT_CHARS: usize = 512;

/// Where an admin request came from. Extracting it never fails.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditRequestMeta {
    pub request_ip: Option<String>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>,
}

impl AuditRequestMeta {
    #[must_use]
    pub fn from_parts(peer: Option<IpAddr>, headers: &HeaderMap) -> Self {
        Self {
            request_ip: resolve_client_ip(peer, headers).map(|ip| ip.to_string()),
            user_agent: header_text(headers, "user-agent")
                .map(|value| value.chars().take(MAX_USER_AGENT_CHARS).collect()),
            request_id: header_text(headers, "x-request-id"),
        }
    }

    /// The audit context for a signed-in user acting through a session.
    #[must_use]
    pub fn user_context(&self, user: &UserRecord) -> AuditContext {
        self.context(AuditActor::user(user.user_id, user.email.clone()))
    }

    #[must_use]
    pub fn context(&self, actor: AuditActor) -> AuditContext {
        AuditContext::new(actor).with_request(
            self.request_ip.clone(),
            self.user_agent.clone(),
            self.request_id.clone(),
        )
    }
}

impl<S: Send + Sync> FromRequestParts<S> for AuditRequestMeta {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip());
        Ok(Self::from_parts(peer, &parts.headers))
    }
}

impl AppState {
    /// The audit boundary every control-plane mutation writes through.
    #[must_use]
    pub fn audit(&self) -> AdminAudit<AnyStore> {
        AdminAudit::new(self.store.clone())
    }
}

/// Resolves the client address for an audit entry.
///
/// A direct peer on a public address is the client. When the peer is a local or
/// private hop, such as the admin UI server or an ingress, `X-Forwarded-For` is
/// read right to left and the first public address wins; with only private
/// addresses in the chain, the left-most one is used.
#[must_use]
pub fn resolve_client_ip(peer: Option<IpAddr>, headers: &HeaderMap) -> Option<IpAddr> {
    if let Some(peer) = peer
        && !is_internal_hop(peer)
    {
        return Some(peer);
    }
    let forwarded: Vec<IpAddr> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(parse_forwarded_ip)
        .collect();
    forwarded
        .iter()
        .rev()
        .find(|ip| !is_internal_hop(**ip))
        .or_else(|| forwarded.first())
        .copied()
        .or(peer)
}

fn parse_forwarded_ip(raw: &str) -> Option<IpAddr> {
    let raw = raw.trim();
    raw.parse::<IpAddr>()
        .ok()
        .or_else(|| raw.parse::<SocketAddr>().ok().map(|address| address.ip()))
}

fn is_internal_hop(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(mapped) => is_internal_hop(IpAddr::V4(mapped)),
            None => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
        },
    }
}

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct AuditLogListQuery {
    /// Only entries made by this user.
    pub actor_user_id: Option<String>,
    /// Exact action name, such as `api_key.created`.
    pub action: Option<String>,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    /// RFC 3339 lower bound, inclusive.
    pub occurred_at_start: Option<String>,
    /// RFC 3339 upper bound, inclusive.
    pub occurred_at_end: Option<String>,
    /// `next_cursor` from the previous page.
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditLogPageView {
    pub items: Vec<AuditLogView>,
    /// Pass as `cursor` to fetch the next page. Absent on the last page.
    pub next_cursor: Option<String>,
    pub limit: u32,
    /// Known target types, for filter controls.
    pub target_types: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditLogView {
    pub id: String,
    pub occurred_at: String,
    pub actor: AuditActorView,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub target_label: Option<String>,
    #[schema(value_type = Option<Object>)]
    pub before: Option<Value>,
    #[schema(value_type = Option<Object>)]
    pub after: Option<Value>,
    #[schema(value_type = Object)]
    pub metadata: Value,
    pub request_ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditActorView {
    /// `user`, `api_key` or `system`.
    pub kind: String,
    pub user_id: Option<String>,
    pub api_key_id: Option<String>,
    pub label: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/audit-logs",
    params(AuditLogListQuery),
    responses((status = 200, body = Envelope<AuditLogPageView>)),
    security(("session_cookie" = [])),
    tag = "Audit"
)]
pub async fn list_audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditLogListQuery>,
) -> Result<Json<Envelope<AuditLogPageView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let query = AuditLogQuery {
        actor_user_id: non_empty(query.actor_user_id)
            .map(|raw| parse_uuid(&raw, "actor_user_id"))
            .transpose()?,
        action: non_empty(query.action),
        target_type: non_empty(query.target_type),
        target_id: non_empty(query.target_id),
        occurred_at_start: parse_timestamp(query.occurred_at_start, "occurred_at_start")?,
        occurred_at_end: parse_timestamp(query.occurred_at_end, "occurred_at_end")?,
        cursor: non_empty(query.cursor)
            .map(|raw| {
                AuditLogCursor::decode(&raw)
                    .ok_or_else(|| invalid_request("`cursor` is not a valid audit log cursor"))
            })
            .transpose()?,
        limit: query.limit.unwrap_or(0).min(MAX_AUDIT_LOG_PAGE_SIZE),
    };
    let limit = query.effective_limit();
    let page = state.store.list_audit_logs(&query).await?;
    Ok(Json(envelope(AuditLogPageView {
        items: page.items.into_iter().map(audit_log_view).collect(),
        next_cursor: page.next_cursor.map(AuditLogCursor::encode),
        limit,
        target_types: targets::ALL.iter().map(|kind| kind.to_string()).collect(),
    })))
}

fn audit_log_view(record: AuditLogRecord) -> AuditLogView {
    AuditLogView {
        id: record.id.to_string(),
        occurred_at: format_timestamp(record.occurred_at),
        actor: AuditActorView {
            kind: record.actor.kind.as_str().to_string(),
            user_id: record.actor.user_id.map(|id| id.to_string()),
            api_key_id: match record.actor.kind {
                AuditActorKind::ApiKey => record.actor.api_key_id.map(|id| id.to_string()),
                AuditActorKind::User | AuditActorKind::System => None,
            },
            label: record.actor.label,
        },
        action: record.action,
        target_type: record.target_type,
        target_id: record.target_id,
        target_label: record.target_label,
        before: record.before,
        after: record.after,
        metadata: record.metadata,
        request_ip: record.request_ip,
        user_agent: record.user_agent,
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_uuid(raw: &str, field: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| invalid_request(&format!("`{field}` must be a UUID")))
}

fn parse_timestamp(raw: Option<String>, field: &str) -> Result<Option<OffsetDateTime>, AppError> {
    non_empty(raw)
        .map(|raw| {
            OffsetDateTime::parse(&raw, &Rfc3339)
                .map_err(|_| invalid_request(&format!("`{field}` must be an RFC 3339 timestamp")))
        })
        .transpose()
}

fn invalid_request(message: &str) -> AppError {
    AppError(GatewayError::InvalidRequest(message.to_string()))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn forwarded(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn a_public_peer_is_the_client_and_forwarded_headers_are_ignored() {
        let peer = "198.51.100.4".parse().ok();
        assert_eq!(
            resolve_client_ip(peer, &forwarded("203.0.113.9")),
            "198.51.100.4".parse().ok()
        );
    }

    #[test]
    fn a_private_hop_yields_the_right_most_public_forwarded_address() {
        let peer = "127.0.0.1".parse().ok();
        assert_eq!(
            resolve_client_ip(peer, &forwarded("192.0.2.1, 203.0.113.9, 10.0.0.5")),
            "203.0.113.9".parse().ok()
        );
    }

    #[test]
    fn an_all_private_chain_yields_the_left_most_address() {
        let peer = "127.0.0.1".parse().ok();
        assert_eq!(
            resolve_client_ip(peer, &forwarded("10.1.2.3, 127.0.0.1")),
            "10.1.2.3".parse().ok()
        );
        assert_eq!(resolve_client_ip(peer, &HeaderMap::new()), peer);
    }

    #[test]
    fn garbage_forwarded_entries_are_skipped() {
        let peer = "::1".parse().ok();
        assert_eq!(
            resolve_client_ip(peer, &forwarded("unknown, 203.0.113.9:4430")),
            "203.0.113.9".parse().ok()
        );
    }
}
