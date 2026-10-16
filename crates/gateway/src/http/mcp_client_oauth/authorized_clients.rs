//! Listing and disconnecting the MCP clients a user has authorized.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    GatewayError, GatewayOauthGrantRecord, GatewayOauthGrantSummary, GatewayOauthRepository,
    McpOauthResource, StoreError, UserRecord, parse_scope_list,
};
use gateway_service::{AuditEvent, admin_audit::actions};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use gateway_service::mcp_client_oauth::MAX_LISTED_AUTHORIZED_CLIENTS;

use super::{consent::scope_description, oauth_service, require_active_session};
use crate::http::{
    admin_auth::{require_active_session as require_session_user, require_platform_admin},
    admin_contract::{OpenAiErrorEnvelopeView, format_timestamp},
    audit::AuditRequestMeta,
    error::AppError,
    mcp_client_oauth::{OauthResourceKind, OauthScopeView},
    state::AppState,
};

/// One MCP client that can still get tokens on the user's behalf.
#[derive(Debug, Serialize, ToSchema)]
pub struct AuthorizedMcpClientView {
    pub grant_id: String,
    pub client_id: String,
    /// Self-asserted by the client at registration.
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub resource: String,
    pub resource_kind: OauthResourceKind,
    pub server_key: Option<String>,
    pub scopes: Vec<OauthScopeView>,
    pub authorized_at: String,
    /// When the client last received a token, at sign-in or refresh.
    pub last_token_issued_at: String,
    /// When the client's last usable token expires; never later than the
    /// admin session that approved it.
    pub expires_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthorizedMcpClientsView {
    /// Whether OAuth sign-in for MCP clients is enabled on this gateway.
    /// While it is disabled, `items` still lists clients that would work
    /// again if it were re-enabled.
    pub enabled: bool,
    /// The newest authorized clients, at most 100.
    pub items: Vec<AuthorizedMcpClientView>,
    /// More clients are authorized than `items` lists.
    pub has_more: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RevokeAuthorizedMcpClientsResponse {
    pub revoked_count: u64,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuthorizedMcpClientsQuery {
    /// User whose authorized clients to list.
    pub user_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AdminRevokeAuthorizedMcpClientsRequest {
    pub user_id: Uuid,
    /// Revoke only this grant; omit to revoke every client the user authorized.
    pub grant_id: Option<Uuid>,
}

#[utoipa::path(
    get,
    path = "/api/v1/me/authorized-mcp-clients",
    responses(
        (status = 200, body = AuthorizedMcpClientsView),
        (status = 401, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn list_my_authorized_mcp_clients(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AuthorizedMcpClientsView>, AppError> {
    let (user_id, _) = require_active_session(&state, &headers).await?;
    authorized_clients_view(&state, user_id).await.map(Json)
}

#[utoipa::path(
    delete,
    path = "/api/v1/me/authorized-mcp-clients/{grant_id}",
    params(("grant_id" = String, Path, description = "Grant identifier")),
    responses(
        (status = 200, body = RevokeAuthorizedMcpClientsResponse),
        (status = 401, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn revoke_my_authorized_mcp_client(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(grant_id): Path<String>,
) -> Result<Json<RevokeAuthorizedMcpClientsResponse>, AppError> {
    let user = require_session_user(&state, &headers).await?;
    let grant_id = Uuid::parse_str(&grant_id).map_err(|_| grant_not_found())?;
    // Revocation does not depend on the runtime setting: a grant left live
    // while OAuth is disabled would work again once it is re-enabled.
    let revoked = state
        .store
        .revoke_gateway_oauth_grant_for_user(
            grant_id,
            user.user_id,
            "user_revoked",
            OffsetDateTime::now_utc(),
        )
        .await?
        .ok_or_else(grant_not_found)?;
    audit_grant_revoked(&state, &meta, &user, &revoked, "user_disconnect").await;
    Ok(Json(RevokeAuthorizedMcpClientsResponse {
        revoked_count: 1,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/authorized-clients",
    params(AuthorizedMcpClientsQuery),
    responses(
        (status = 200, body = AuthorizedMcpClientsView),
        (status = 401, body = OpenAiErrorEnvelopeView),
        (status = 403, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn admin_list_authorized_mcp_clients(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuthorizedMcpClientsQuery>,
) -> Result<Json<AuthorizedMcpClientsView>, AppError> {
    require_platform_admin(&state, &headers).await?;
    authorized_clients_view(&state, query.user_id)
        .await
        .map(Json)
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp/authorized-clients/revoke",
    request_body = AdminRevokeAuthorizedMcpClientsRequest,
    responses(
        (status = 200, body = RevokeAuthorizedMcpClientsResponse),
        (status = 401, body = OpenAiErrorEnvelopeView),
        (status = 403, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn admin_revoke_authorized_mcp_clients(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Json(request): Json<AdminRevokeAuthorizedMcpClientsRequest>,
) -> Result<Json<RevokeAuthorizedMcpClientsResponse>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    // Works while OAuth is disabled too, so a compromised client stays
    // disconnected when the feature is turned back on.
    let now = OffsetDateTime::now_utc();
    let revoked_count = match request.grant_id {
        Some(grant_id) => {
            let revoked = state
                .store
                .revoke_gateway_oauth_grant_for_user(
                    grant_id,
                    request.user_id,
                    "admin_revoked",
                    now,
                )
                .await?;
            if let Some(revoked) = &revoked {
                audit_grant_revoked(&state, &meta, &actor, revoked, "admin_revoke").await;
            }
            u64::from(revoked.is_some())
        }
        None => {
            let revoked_count = state
                .store
                .revoke_gateway_oauth_grants_for_user(request.user_id, "admin_revoked", now)
                .await?;
            // One entry for the bulk revoke; it has no single grant to target.
            if revoked_count > 0 {
                state
                    .audit()
                    .record(
                        &meta.user_context(&actor),
                        AuditEvent::new(actions::MCP_CLIENT_GRANT_REVOKED)
                            .meta("operation", "admin_revoke_all")
                            .meta("user_id", request.user_id)
                            .meta("revoked_count", revoked_count),
                    )
                    .await;
            }
            revoked_count
        }
    };
    Ok(Json(RevokeAuthorizedMcpClientsResponse { revoked_count }))
}

/// Records one revoked grant: which client lost access to whose account.
/// Holds no token or code, only the grant's identifiers and access.
async fn audit_grant_revoked(
    state: &AppState,
    meta: &AuditRequestMeta,
    actor: &UserRecord,
    grant: &GatewayOauthGrantRecord,
    operation: &str,
) {
    let client_name = state
        .store
        .get_gateway_oauth_client(&grant.client_id)
        .await
        .ok()
        .flatten()
        .and_then(|client| client.client_name);
    state
        .audit()
        .record(
            &meta.user_context(actor),
            AuditEvent::new(actions::MCP_CLIENT_GRANT_REVOKED)
                .target(grant.grant_id)
                .label(client_name.unwrap_or_else(|| grant.client_id.clone()))
                .meta("operation", operation)
                .meta("user_id", grant.user_id)
                .before(serde_json::json!({
                    "client_id": grant.client_id,
                    "resource": grant.resource,
                    "scope": grant.scope,
                    "authorized_at": format_timestamp(grant.created_at),
                })),
        )
        .await;
}

async fn authorized_clients_view(
    state: &AppState,
    user_id: Uuid,
) -> Result<AuthorizedMcpClientsView, AppError> {
    let Some(service) = oauth_service(state) else {
        return dormant_clients_view(state, user_id).await;
    };
    let listed = service
        .authorized_clients(user_id, OffsetDateTime::now_utc())
        .await?;
    let items = listed
        .clients
        .into_iter()
        .map(|(grant, resource)| authorized_client_view(grant, resource))
        .collect();
    Ok(AuthorizedMcpClientsView {
        enabled: true,
        items,
        has_more: listed.has_more,
    })
}

/// While OAuth is disabled no issuer is configured, so every live grant is
/// listed. These grants work again if OAuth is re-enabled, so users and
/// admins must still be able to find and revoke them.
async fn dormant_clients_view(
    state: &AppState,
    user_id: Uuid,
) -> Result<AuthorizedMcpClientsView, AppError> {
    let mut grants = state
        .store
        .list_live_gateway_oauth_grants_for_user(
            user_id,
            None,
            OffsetDateTime::now_utc(),
            MAX_LISTED_AUTHORIZED_CLIENTS + 1,
        )
        .await?;
    let has_more = grants.len() > MAX_LISTED_AUTHORIZED_CLIENTS as usize;
    grants.truncate(MAX_LISTED_AUTHORIZED_CLIENTS as usize);
    let items = grants
        .into_iter()
        .filter_map(|grant| {
            let resource = url::Url::parse(&grant.resource)
                .ok()
                .and_then(|url| McpOauthResource::from_path(url.path()))?;
            Some(authorized_client_view(grant, resource))
        })
        .collect();
    Ok(AuthorizedMcpClientsView {
        enabled: false,
        items,
        has_more,
    })
}

fn authorized_client_view(
    grant: GatewayOauthGrantSummary,
    resource: McpOauthResource,
) -> AuthorizedMcpClientView {
    let (resource_kind, server_key) = match resource {
        McpOauthResource::Server(server_key) => (OauthResourceKind::Server, Some(server_key)),
        McpOauthResource::Aggregate => (OauthResourceKind::Aggregate, None),
    };
    AuthorizedMcpClientView {
        grant_id: grant.grant_id.to_string(),
        client_id: grant.client_id,
        client_name: grant.client_name,
        client_uri: grant.client_uri,
        resource: grant.resource,
        resource_kind,
        server_key,
        scopes: parse_scope_list(&grant.scope)
            .into_iter()
            .map(|scope| OauthScopeView {
                description: scope_description(&scope).to_string(),
                scope,
            })
            .collect(),
        authorized_at: format_timestamp(grant.created_at),
        last_token_issued_at: format_timestamp(grant.last_token_issued_at),
        expires_at: format_timestamp(grant.expires_at),
    }
}

fn grant_not_found() -> AppError {
    AppError(GatewayError::Store(StoreError::NotFound(
        "authorized MCP client was not found".to_string(),
    )))
}
