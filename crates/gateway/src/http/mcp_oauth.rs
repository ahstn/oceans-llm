use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use gateway_core::{
    AuthError, ExternalMcpAuthMode, ExternalMcpServerStatus, GatewayError, McpOauthStateRecord,
    McpRegistryRepository, McpUpstreamCredentialMaterialKind, McpUpstreamCredentialRepository,
};
use gateway_service::{McpCredentialService, credential_owner_scope_key, mcp_oauth_server_config};
use gateway_store::GatewayStore;
use openidconnect::{CsrfToken, PkceCodeChallenge};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::http::{
    admin_contract::{OpenAiErrorEnvelopeView, format_timestamp},
    error::AppError,
    identity::resolve_session_user,
    state::AppState,
};

const MCP_OAUTH_STATE_TTL_MINUTES: i64 = 10;
const DEFAULT_CONNECTION_REDIRECT: &str = "/admin/account/connections";

#[derive(Debug, Deserialize, ToSchema)]
pub struct McpOauthStartRequest {
    pub redirect_to: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpOauthStartResponse {
    pub authorization_url: String,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct McpOauthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum McpOauthConnectionStatus {
    Connected,
    Expired,
    Disconnected,
    Unavailable,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpOauthConnectionView {
    pub server_id: Uuid,
    pub server_key: String,
    pub display_name: String,
    pub provider_key: String,
    pub required_scopes: Vec<String>,
    pub granted_scopes: Vec<String>,
    pub status: McpOauthConnectionStatus,
    pub expires_at: Option<String>,
    pub availability_error: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpOauthRevokeResponse {
    pub revoked: bool,
}

#[utoipa::path(
    get,
    path = "/api/v1/mcp/oauth/connections",
    responses(
        (status = 200, body = [McpOauthConnectionView]),
        (status = 401, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn list_mcp_oauth_connections(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<McpOauthConnectionView>>, AppError> {
    let user = require_session_user(&state, &headers).await?;
    let owner_scope_key = credential_owner_scope_key(
        gateway_core::McpUpstreamCredentialOwnerScopeKind::User,
        Some(user.user_id),
        None,
        None,
    )?;
    let mut items = Vec::new();
    for server in state.store.list_external_mcp_servers(false).await? {
        if server.status != ExternalMcpServerStatus::Active
            || server.auth_mode != ExternalMcpAuthMode::OauthObo
        {
            continue;
        }
        let Ok(config) = mcp_oauth_server_config(&server.auth_config) else {
            continue;
        };
        let binding = state
            .store
            .get_active_mcp_upstream_credential_binding(server.mcp_server_id, &owner_scope_key)
            .await?;
        let expires_at = binding.as_ref().and_then(|binding| binding.expires_at);
        let availability_error = state
            .mcp_oauth_runtime
            .connection_unavailable_reason(&config.provider_key);
        let status = match binding.as_ref() {
            None if availability_error.is_some() => McpOauthConnectionStatus::Unavailable,
            None => McpOauthConnectionStatus::Disconnected,
            Some(binding)
                if binding.material_kind != McpUpstreamCredentialMaterialKind::OauthTokens =>
            {
                McpOauthConnectionStatus::Disconnected
            }
            Some(_) if expires_at.is_some_and(|expiry| expiry <= OffsetDateTime::now_utc()) => {
                McpOauthConnectionStatus::Expired
            }
            Some(_) => McpOauthConnectionStatus::Connected,
        };
        let granted_scopes = binding
            .as_ref()
            .and_then(|binding| binding.metadata.get("granted_scopes"))
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        items.push(McpOauthConnectionView {
            server_id: server.mcp_server_id,
            server_key: server.server_key,
            display_name: server.display_name,
            provider_key: config.provider_key,
            required_scopes: config.scopes,
            granted_scopes,
            status,
            expires_at: expires_at.map(format_timestamp),
            availability_error: availability_error.map(ToOwned::to_owned),
        });
    }
    Ok(Json(items))
}

#[utoipa::path(
    post,
    path = "/api/v1/mcp/servers/{server_id}/oauth/start",
    request_body = McpOauthStartRequest,
    params(("server_id" = String, Path, description = "External MCP server identifier")),
    responses(
        (status = 200, body = McpOauthStartResponse),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 401, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn start_mcp_oauth_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
    Json(request): Json<McpOauthStartRequest>,
) -> Result<Json<McpOauthStartResponse>, AppError> {
    let user = require_session_user(&state, &headers).await?;
    let server = load_oauth_server(&state, server_id).await?;
    let config = mcp_oauth_server_config(&server.auth_config)?;
    let provider = state.mcp_oauth_runtime.provider(&config.provider_key)?;
    let redirect_uri = state.mcp_oauth_runtime.callback_url(&config.provider_key)?;
    let redirect_to = normalize_connection_redirect(request.redirect_to.as_deref());
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let csrf_state = CsrfToken::new_random();
    let mut authorization_url = url::Url::parse(&provider.authorization_url)
        .map_err(|error| GatewayError::Internal(error.to_string()))?;
    {
        let mut pairs = authorization_url.query_pairs_mut();
        pairs.append_pair("response_type", "code");
        pairs.append_pair("client_id", &provider.client_id);
        pairs.append_pair("redirect_uri", &redirect_uri);
        pairs.append_pair("scope", &config.scopes.join(" "));
        pairs.append_pair("state", csrf_state.secret());
        pairs.append_pair("code_challenge", pkce_challenge.as_str());
        pairs.append_pair("code_challenge_method", "S256");
        pairs.append_pair("access_type", "offline");
        pairs.append_pair("prompt", "consent");
        pairs.append_pair("include_granted_scopes", "true");
        pairs.append_pair("resource", &config.resource);
    }
    let now = OffsetDateTime::now_utc();
    state
        .store
        .create_mcp_oauth_state(&McpOauthStateRecord {
            state_hash: token_hash(csrf_state.secret()),
            user_id: user.user_id,
            mcp_server_id: server_id,
            provider_key: config.provider_key,
            pkce_verifier: pkce_verifier.secret().to_string(),
            redirect_to,
            resource: config.resource,
            scopes: config.scopes,
            expires_at: now + Duration::minutes(MCP_OAUTH_STATE_TTL_MINUTES),
            consumed_at: None,
            created_at: now,
        })
        .await?;
    Ok(Json(McpOauthStartResponse {
        authorization_url: authorization_url.to_string(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/mcp/oauth/{provider_key}/callback",
    params(
        ("provider_key" = String, Path, description = "OAuth provider key"),
        McpOauthCallbackQuery
    ),
    responses(
        (status = 307, description = "Redirect to the connection page"),
        (status = 401, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn mcp_oauth_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(provider_key): Path<String>,
    Query(query): Query<McpOauthCallbackQuery>,
) -> Result<Response, AppError> {
    let user = require_session_user(&state, &headers).await?;
    let Some(state_token) = query.state.as_deref() else {
        return Ok(connection_error_redirect("state_invalid"));
    };
    let now = OffsetDateTime::now_utc();
    let Some(transaction) = state
        .store
        .consume_mcp_oauth_state(&token_hash(state_token), now)
        .await?
    else {
        return Ok(connection_error_redirect("state_invalid"));
    };
    if transaction.expires_at <= now {
        return Ok(connection_error_redirect("state_expired"));
    }
    if transaction.provider_key != provider_key {
        return Ok(connection_error_redirect("state_invalid"));
    }
    if !callback_session_matches(Some(user.user_id), transaction.user_id) {
        return Ok(connection_error_redirect("state_invalid"));
    }
    if query.error.is_some() {
        return Ok(connection_error_redirect("access_denied"));
    }
    let Some(code) = query.code.as_deref() else {
        return Ok(connection_error_redirect("provider_failure"));
    };
    let redirect_uri = state.mcp_oauth_runtime.callback_url(&provider_key)?;
    let grant = match state
        .mcp_oauth_runtime
        .exchange_code(
            &provider_key,
            code,
            &redirect_uri,
            &transaction.pkce_verifier,
            &transaction.resource,
            &transaction.scopes,
        )
        .await
    {
        Ok(grant) => grant,
        Err(error) => {
            tracing::warn!(error = %error, "MCP OAuth token exchange failed");
            return Ok(connection_error_redirect("provider_failure"));
        }
    };
    McpCredentialService::new(state.store.clone())
        .upsert_oauth_user_binding(transaction.mcp_server_id, transaction.user_id, grant)
        .await?;
    Ok(
        Redirect::temporary(&format!("{}?oauth=connected", transaction.redirect_to))
            .into_response(),
    )
}

#[utoipa::path(
    delete,
    path = "/api/v1/mcp/servers/{server_id}/oauth/connection",
    params(("server_id" = String, Path, description = "External MCP server identifier")),
    responses(
        (status = 200, body = McpOauthRevokeResponse),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 401, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn revoke_mcp_oauth_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<Uuid>,
) -> Result<Json<McpOauthRevokeResponse>, AppError> {
    let user = require_session_user(&state, &headers).await?;
    let _ = load_oauth_server(&state, server_id).await?;
    let owner_scope_key = credential_owner_scope_key(
        gateway_core::McpUpstreamCredentialOwnerScopeKind::User,
        Some(user.user_id),
        None,
        None,
    )?;
    if let Some(binding) = state
        .store
        .get_active_mcp_upstream_credential_binding(server_id, &owner_scope_key)
        .await?
    {
        McpCredentialService::new(state.store.clone())
            .revoke_binding(binding.credential_binding_id)
            .await?;
    }
    Ok(Json(McpOauthRevokeResponse { revoked: true }))
}

async fn require_session_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<gateway_core::UserRecord, AppError> {
    resolve_session_user(state, headers)
        .await?
        .ok_or_else(|| AppError(AuthError::SessionRequired.into()))
}

async fn load_oauth_server(
    state: &AppState,
    server_id: Uuid,
) -> Result<gateway_core::ExternalMcpServerRecord, AppError> {
    let server = state
        .store
        .get_external_mcp_server(server_id)
        .await?
        .ok_or_else(|| {
            GatewayError::Store(gateway_core::StoreError::NotFound(format!(
                "external MCP server `{server_id}` not found"
            )))
        })?;
    if server.status != ExternalMcpServerStatus::Active
        || server.auth_mode != ExternalMcpAuthMode::OauthObo
    {
        return Err(GatewayError::InvalidRequest(
            "MCP server does not support a user OAuth connection".to_string(),
        )
        .into());
    }
    Ok(server)
}

fn normalize_connection_redirect(value: Option<&str>) -> String {
    value
        .filter(|value| {
            value.starts_with("/admin/account/connections")
                && !value.starts_with("//")
                && !value.contains('\\')
        })
        .unwrap_or(DEFAULT_CONNECTION_REDIRECT)
        .to_string()
}

fn connection_error_redirect(code: &str) -> Response {
    Redirect::temporary(&format!("{DEFAULT_CONNECTION_REDIRECT}?oauth_error={code}"))
        .into_response()
}

fn token_hash(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

fn callback_session_matches(session_user_id: Option<Uuid>, transaction_user_id: Uuid) -> bool {
    session_user_id == Some(transaction_user_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_redirect_is_limited_to_the_self_service_page() {
        assert_eq!(
            normalize_connection_redirect(Some("/admin/account/connections?from=drive")),
            "/admin/account/connections?from=drive"
        );
        assert_eq!(
            normalize_connection_redirect(Some("https://attacker.example")),
            DEFAULT_CONNECTION_REDIRECT
        );
        assert_eq!(
            normalize_connection_redirect(Some("//attacker.example")),
            DEFAULT_CONNECTION_REDIRECT
        );
    }

    #[test]
    fn oauth_state_is_stored_as_a_hash() {
        let hash = token_hash("secret-state");
        assert_ne!(hash, "secret-state");
        assert_eq!(hash, token_hash("secret-state"));
        assert_ne!(hash, token_hash("other-state"));
    }

    #[test]
    fn oauth_callback_requires_the_initiating_browser_session() {
        let initiator = Uuid::new_v4();
        assert!(callback_session_matches(Some(initiator), initiator));
        assert!(!callback_session_matches(Some(Uuid::new_v4()), initiator));
        assert!(!callback_session_matches(None, initiator));
    }
}
