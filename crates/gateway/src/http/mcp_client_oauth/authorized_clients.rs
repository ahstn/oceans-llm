//! Listing and disconnecting the MCP clients a user has authorized.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    GatewayError, GatewayOauthGrantSummary, McpOauthResource, StoreError, parse_scope_list,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::{consent::scope_description, oauth_service, require_active_session};
use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{OpenAiErrorEnvelopeView, format_timestamp},
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
    /// Access ends no later than the admin session that approved it.
    pub expires_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthorizedMcpClientsView {
    /// Whether OAuth sign-in for MCP clients is enabled on this gateway.
    pub enabled: bool,
    pub items: Vec<AuthorizedMcpClientView>,
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
    Path(grant_id): Path<String>,
) -> Result<Json<RevokeAuthorizedMcpClientsResponse>, AppError> {
    let (user_id, _) = require_active_session(&state, &headers).await?;
    let service = oauth_service(&state).ok_or_else(grant_not_found)?;
    let grant_id = Uuid::parse_str(&grant_id).map_err(|_| grant_not_found())?;
    if !service
        .revoke_authorized_client(user_id, grant_id, "user_revoked", OffsetDateTime::now_utc())
        .await?
    {
        return Err(grant_not_found());
    }
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
    Json(request): Json<AdminRevokeAuthorizedMcpClientsRequest>,
) -> Result<Json<RevokeAuthorizedMcpClientsResponse>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let Some(service) = oauth_service(&state) else {
        return Ok(Json(RevokeAuthorizedMcpClientsResponse {
            revoked_count: 0,
        }));
    };
    let now = OffsetDateTime::now_utc();
    let revoked_count = match request.grant_id {
        Some(grant_id) => u64::from(
            service
                .revoke_authorized_client(request.user_id, grant_id, "admin_revoked", now)
                .await?,
        ),
        None => {
            service
                .revoke_all_authorized_clients(request.user_id, "admin_revoked", now)
                .await?
        }
    };
    Ok(Json(RevokeAuthorizedMcpClientsResponse { revoked_count }))
}

async fn authorized_clients_view(
    state: &AppState,
    user_id: Uuid,
) -> Result<AuthorizedMcpClientsView, AppError> {
    let Some(service) = oauth_service(state) else {
        return Ok(AuthorizedMcpClientsView {
            enabled: false,
            items: Vec::new(),
        });
    };
    let grants = service
        .authorized_clients(user_id, OffsetDateTime::now_utc())
        .await?;
    let items = grants
        .into_iter()
        .map(|grant| {
            let resource = service.settings().parse_resource(&grant.resource);
            authorized_client_view(grant, resource)
        })
        .collect();
    Ok(AuthorizedMcpClientsView {
        enabled: true,
        items,
    })
}

fn authorized_client_view(
    grant: GatewayOauthGrantSummary,
    resource: Option<McpOauthResource>,
) -> AuthorizedMcpClientView {
    let (resource_kind, server_key) = match resource {
        Some(McpOauthResource::Server(server_key)) => (OauthResourceKind::Server, Some(server_key)),
        _ => (OauthResourceKind::Aggregate, None),
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
        expires_at: format_timestamp(grant.session_expires_at),
    }
}

fn grant_not_found() -> AppError {
    AppError(GatewayError::Store(StoreError::NotFound(
        "authorized MCP client was not found".to_string(),
    )))
}
