use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use gateway_core::{
    GatewayError, McpOauthResource, SCOPE_MCP_AGGREGATE, SCOPE_MCP_PROXY, StoreError,
};
use gateway_service::PendingAuthorization;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{oauth_service, require_active_session};
use crate::http::{
    admin_contract::{OpenAiErrorEnvelopeView, format_timestamp},
    error::AppError,
    state::AppState,
};

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OauthResourceKind {
    /// `/mcp`: every MCP server the user can reach.
    Aggregate,
    /// `/mcp/{server_key}`: one server.
    Server,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OauthScopeView {
    pub scope: String,
    pub description: String,
}

/// What the consent screen shows for a pending authorization request.
#[derive(Debug, Serialize, ToSchema)]
pub struct OauthAuthorizationRequestView {
    pub request_id: String,
    pub client_id: String,
    /// Self-asserted by the client at registration.
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub redirect_uri: String,
    /// The host the browser returns to; the consent screen must show it.
    pub redirect_host: String,
    /// Every registered redirect URI is on this device, so the client's
    /// identity cannot be verified.
    pub loopback_redirect_only: bool,
    pub resource: String,
    pub resource_kind: OauthResourceKind,
    pub server_key: Option<String>,
    pub server_display_name: Option<String>,
    pub scopes: Vec<OauthScopeView>,
    pub expires_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct OauthAuthorizationDecisionRequest {
    pub approve: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OauthAuthorizationDecisionResponse {
    /// The client redirect carrying the code, or the `access_denied` error.
    pub redirect_to: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/oauth/authorization-requests/{request_id}",
    params(("request_id" = String, Path, description = "Authorization request identifier")),
    responses(
        (status = 200, body = OauthAuthorizationRequestView),
        (status = 401, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView),
        (status = 409, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn get_oauth_authorization_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<OauthAuthorizationRequestView>, AppError> {
    require_active_session(&state, &headers).await?;
    let service = oauth_service(&state).ok_or_else(not_enabled)?;
    let request_id = parse_request_id(&request_id)?;
    let pending = service
        .pending_authorization(request_id, OffsetDateTime::now_utc())
        .await?;
    Ok(Json(authorization_request_view(pending)))
}

#[utoipa::path(
    post,
    path = "/api/v1/oauth/authorization-requests/{request_id}/decision",
    request_body = OauthAuthorizationDecisionRequest,
    params(("request_id" = String, Path, description = "Authorization request identifier")),
    responses(
        (status = 200, body = OauthAuthorizationDecisionResponse),
        (status = 401, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView),
        (status = 409, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn decide_oauth_authorization_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
    Json(decision): Json<OauthAuthorizationDecisionRequest>,
) -> Result<Json<OauthAuthorizationDecisionResponse>, AppError> {
    let (user_id, session_id) = require_active_session(&state, &headers).await?;
    let service = oauth_service(&state).ok_or_else(not_enabled)?;
    let request_id = parse_request_id(&request_id)?;
    let redirect_to = service
        .decide_authorization(
            request_id,
            user_id,
            session_id,
            decision.approve,
            OffsetDateTime::now_utc(),
        )
        .await?;
    Ok(Json(OauthAuthorizationDecisionResponse { redirect_to }))
}

fn parse_request_id(raw: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| {
        AppError(GatewayError::Store(StoreError::NotFound(
            "authorization request was not found or has expired".to_string(),
        )))
    })
}

fn not_enabled() -> AppError {
    AppError(GatewayError::Store(StoreError::NotFound(
        "OAuth for MCP clients is not enabled on this gateway".to_string(),
    )))
}

fn authorization_request_view(pending: PendingAuthorization) -> OauthAuthorizationRequestView {
    let (resource_kind, server_key) = match &pending.resource {
        McpOauthResource::Aggregate => (OauthResourceKind::Aggregate, None),
        McpOauthResource::Server(server_key) => {
            (OauthResourceKind::Server, Some(server_key.clone()))
        }
    };
    OauthAuthorizationRequestView {
        request_id: pending.request.request_id.to_string(),
        client_id: pending.client.client_id,
        client_name: pending.client.client_name,
        client_uri: pending.client.client_uri,
        redirect_host: redirect_host(&pending.request.redirect_uri),
        redirect_uri: pending.request.redirect_uri,
        loopback_redirect_only: pending.loopback_redirect_only,
        resource: pending.request.resource,
        resource_kind,
        server_key,
        server_display_name: pending.server_display_name,
        scopes: pending
            .scopes
            .into_iter()
            .map(|scope| OauthScopeView {
                description: scope_description(&scope).to_string(),
                scope,
            })
            .collect(),
        expires_at: format_timestamp(pending.request.expires_at),
    }
}

fn redirect_host(redirect_uri: &str) -> String {
    url::Url::parse(redirect_uri)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?.to_string();
            Some(match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host,
            })
        })
        .unwrap_or_else(|| redirect_uri.to_string())
}

pub(super) fn scope_description(scope: &str) -> &'static str {
    match scope {
        SCOPE_MCP_AGGREGATE => "Search and call your MCP tools through the aggregate endpoint",
        SCOPE_MCP_PROXY => "Connect directly to MCP servers you have access to",
        _ => "Unknown scope",
    }
}
