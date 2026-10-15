//! Admin audit trail: request context for audit entries and the read API.
//!
//! Handlers build an [`AuditContext`] from the signed-in actor and
//! [`AuditRequestMeta`], then write through [`AppState::audit`]. See
//! `docs/adr/2026-10-15-admin-audit-log.md`.

use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, OnceLock},
};

use admin_ui::client_ip::{CLIENT_IP_HEADER, verify_client_ip};

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
/// Callers can supply `x-request-id`; bound it so it cannot crowd out the rest of
/// an entry's metadata under the summary size cap.
const MAX_REQUEST_ID_CHARS: usize = 128;

/// Where an admin request came from. Extracting it never fails.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditRequestMeta {
    pub request_ip: Option<String>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>,
}

impl AuditRequestMeta {
    /// `signing_key` verifies the admin UI proxy's signed client address. Without a
    /// valid signature the direct peer is recorded; forwarding headers are never trusted.
    #[must_use]
    pub fn from_parts(peer: Option<IpAddr>, headers: &HeaderMap, signing_key: &[u8]) -> Self {
        Self {
            request_ip: resolve_client_ip(peer, headers, signing_key, now_unix_secs())
                .map(|ip| ip.to_string()),
            user_agent: header_text(headers, "user-agent")
                .map(|value| value.chars().take(MAX_USER_AGENT_CHARS).collect()),
            request_id: header_text(headers, "x-request-id")
                .map(|value| value.chars().take(MAX_REQUEST_ID_CHARS).collect()),
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

impl FromRequestParts<AppState> for AuditRequestMeta {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(address)| address.ip());
        Ok(Self::from_parts(
            peer,
            &parts.headers,
            &state.client_ip_signing_key(),
        ))
    }
}

impl AppState {
    /// The audit boundary every control-plane mutation writes through.
    #[must_use]
    pub fn audit(&self) -> AdminAudit<AnyStore> {
        AdminAudit::new(self.store.clone())
    }

    /// Key shared with the admin UI proxy for signing the browser's address.
    ///
    /// A configured identity secret is shared by every replica, so a request the
    /// proxy signed on one replica verifies on another. The development fallback
    /// secret is public, so it is never used; a random per-process key replaces it.
    #[must_use]
    pub fn client_ip_signing_key(&self) -> Arc<[u8]> {
        client_ip_signing_key_for(&self.identity_token_secret)
    }
}

/// The fallback used when `GATEWAY_IDENTITY_TOKEN_SECRET` is unset. It is public.
pub const DEV_IDENTITY_TOKEN_SECRET: &str = "local-dev-identity-secret";

fn client_ip_signing_key_for(identity_token_secret: &str) -> Arc<[u8]> {
    static PROCESS_KEY: OnceLock<Arc<[u8]>> = OnceLock::new();
    if identity_token_secret.is_empty() || identity_token_secret == DEV_IDENTITY_TOKEN_SECRET {
        return PROCESS_KEY
            .get_or_init(|| {
                let mut key = Uuid::new_v4().as_bytes().to_vec();
                key.extend_from_slice(Uuid::new_v4().as_bytes());
                Arc::from(key)
            })
            .clone();
    }
    Arc::from(identity_token_secret.as_bytes())
}

/// Resolves the client address for an audit entry: the address the admin UI
/// proxy signed when its signature is valid and fresh, otherwise the direct peer.
///
/// `X-Forwarded-For` is ignored, because without a configured trusted-proxy chain
/// any client could choose its own value. Behind a load balancer the peer is the
/// balancer's address.
#[must_use]
pub fn resolve_client_ip(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    signing_key: &[u8],
    now_unix_secs: i64,
) -> Option<IpAddr> {
    headers
        .get(CLIENT_IP_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| verify_client_ip(signing_key, value, now_unix_secs))
        .or(peer)
}

fn now_unix_secs() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
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
    use admin_ui::client_ip::sign_client_ip;
    use axum::http::HeaderValue;

    use super::*;

    const KEY: &[u8] = b"test-secret";
    const NOW: i64 = 1_800_000_000;

    fn headers(pairs: &[(&'static str, String)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn the_public_development_secret_never_signs_client_addresses() {
        let key = client_ip_signing_key_for(DEV_IDENTITY_TOKEN_SECRET);
        assert_ne!(&*key, DEV_IDENTITY_TOKEN_SECRET.as_bytes());
        assert_eq!(key, client_ip_signing_key_for(""));
        let forged = sign_client_ip(
            DEV_IDENTITY_TOKEN_SECRET.as_bytes(),
            "198.51.100.1".parse().unwrap(),
            NOW,
        );
        let peer = "10.0.0.5".parse().ok();
        assert_eq!(
            resolve_client_ip(peer, &headers(&[(CLIENT_IP_HEADER, forged)]), &key, NOW),
            peer
        );
        assert_eq!(&*client_ip_signing_key_for("configured"), b"configured");
    }

    #[test]
    fn request_ids_are_bounded() {
        let meta = AuditRequestMeta::from_parts(
            None,
            &headers(&[("x-request-id", "r".repeat(10_000))]),
            KEY,
        );
        assert_eq!(meta.request_id.unwrap().len(), MAX_REQUEST_ID_CHARS);
    }

    #[test]
    fn a_valid_signed_address_wins_over_the_peer() {
        let signed = sign_client_ip(KEY, "203.0.113.9".parse().unwrap(), NOW);
        let peer = "127.0.0.1".parse().ok();
        assert_eq!(
            resolve_client_ip(peer, &headers(&[(CLIENT_IP_HEADER, signed)]), KEY, NOW),
            "203.0.113.9".parse().ok()
        );
    }

    #[test]
    fn forwarding_headers_and_bad_signatures_fall_back_to_the_peer() {
        let peer = "10.0.0.5".parse().ok();
        let forged = sign_client_ip(b"guessed", "198.51.100.1".parse().unwrap(), NOW);
        let spoofed = headers(&[
            ("x-forwarded-for", "198.51.100.1".to_string()),
            (CLIENT_IP_HEADER, forged),
        ]);
        assert_eq!(resolve_client_ip(peer, &spoofed, KEY, NOW), peer);
        assert_eq!(resolve_client_ip(None, &spoofed, KEY, NOW), None);
    }
}
