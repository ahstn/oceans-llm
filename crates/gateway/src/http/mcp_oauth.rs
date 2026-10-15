use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, header::CACHE_CONTROL},
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use gateway_core::{
    AuthError, ExternalMcpAuthMode, ExternalMcpServerStatus, GatewayError, McpOauthStateRecord,
    McpRegistryRepository, McpUpstreamCredentialBindingRecord, McpUpstreamCredentialMaterialKind,
    McpUpstreamCredentialOwnerScopeKind, McpUpstreamCredentialRepository,
};
use gateway_service::{
    MCP_SERVER_OAUTH_PROVIDER_KEY, McpCredentialService, McpOauthServerConfig,
    credential_owner_scope_key, mcp_oauth_server_config,
};
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
    /// The configured provider key, or the authorization server's host for a
    /// server-level OAuth client.
    pub provider_label: String,
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
) -> Result<Response, AppError> {
    let user = require_session_user(&state, &headers).await?;
    let bindings: HashMap<Uuid, McpUpstreamCredentialBindingRecord> = state
        .store
        .list_mcp_upstream_credential_bindings(
            None,
            Some(McpUpstreamCredentialOwnerScopeKind::User),
            Some(user.user_id),
            false,
        )
        .await?
        .into_iter()
        .map(|binding| (binding.mcp_server_id, binding))
        .collect();
    let mut items = Vec::new();
    for server in state.store.list_external_mcp_servers(true).await? {
        let binding = bindings.get(&server.mcp_server_id);
        let oauth_binding = binding.filter(|binding| {
            binding.material_kind == McpUpstreamCredentialMaterialKind::OauthTokens
        });
        let config = if server.status == ExternalMcpServerStatus::Active
            && server.auth_mode == ExternalMcpAuthMode::OauthObo
        {
            mcp_oauth_server_config(&server.auth_config).ok()
        } else {
            None
        };
        if config.is_none() && oauth_binding.is_none() {
            continue;
        }
        let provider_key = config
            .as_ref()
            .map(|config| config.provider_key.clone())
            .or_else(|| {
                oauth_binding
                    .and_then(|binding| binding.metadata.get("oauth_provider_key"))
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "unknown".to_string());
        let availability_error = config.as_ref().map_or_else(
            || {
                oauth_binding
                    .map(|_| "This MCP server no longer accepts new OAuth connections".to_string())
            },
            |config| {
                state
                    .mcp_oauth_runtime
                    .connection_unavailable_reason_for(config)
                    .map(ToOwned::to_owned)
            },
        );
        let expires_at = oauth_binding.and_then(|binding| binding.expires_at);
        let status = match oauth_binding {
            None if availability_error.is_some() => McpOauthConnectionStatus::Unavailable,
            None => McpOauthConnectionStatus::Disconnected,
            Some(_) if config.is_none() => McpOauthConnectionStatus::Connected,
            // The gateway refuses this binding, so the user must consent again.
            Some(binding)
                if config
                    .as_ref()
                    .is_some_and(|config| binding_mismatches_server(&binding.metadata, config)) =>
            {
                McpOauthConnectionStatus::Expired
            }
            Some(binding) if has_refreshable_oauth_metadata(&binding.metadata) => {
                McpOauthConnectionStatus::Connected
            }
            Some(_) if expires_at.is_some_and(|expiry| expiry <= OffsetDateTime::now_utc()) => {
                McpOauthConnectionStatus::Expired
            }
            Some(_) => McpOauthConnectionStatus::Connected,
        };
        let granted_scopes = oauth_binding
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
        let provider_label = config
            .as_ref()
            .and_then(|config| config.server_oauth.as_ref())
            .and_then(|endpoints| url::Url::parse(&endpoints.issuer).ok())
            .and_then(|issuer| issuer.host_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| provider_key.clone());
        items.push(McpOauthConnectionView {
            server_id: server.mcp_server_id,
            server_key: server.server_key,
            display_name: server.display_name,
            provider_key,
            provider_label,
            required_scopes: config.map(|config| config.scopes).unwrap_or_default(),
            granted_scopes,
            status,
            expires_at: expires_at.map(format_timestamp),
            availability_error,
        });
    }
    Ok(([(CACHE_CONTROL, "no-store")], Json(items)).into_response())
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
    // A dynamic-registration server registers its client here, on first use.
    let client = state
        .mcp_oauth_runtime
        .resolve_client(state.store.as_ref(), server_id, &config, true)
        .await?;
    let redirect_to = normalize_connection_redirect(request.redirect_to.as_deref());
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let csrf_state = CsrfToken::new_random();
    let authorization_url =
        client.authorization_url(&config, csrf_state.secret(), pkce_challenge.as_str())?;
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
    Ok(Json(McpOauthStartResponse { authorization_url }))
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
    complete_oauth_callback(&state, &headers, &provider_key, query).await
}

#[utoipa::path(
    get,
    path = "/api/v1/mcp/oauth/callback",
    params(McpOauthCallbackQuery),
    responses(
        (status = 307, description = "Redirect to the connection page"),
        (status = 401, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
/// The callback for servers whose authorization server is configured on the
/// server row, including dynamically registered clients.
pub async fn mcp_oauth_server_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<McpOauthCallbackQuery>,
) -> Result<Response, AppError> {
    complete_oauth_callback(&state, &headers, MCP_SERVER_OAUTH_PROVIDER_KEY, query).await
}

async fn complete_oauth_callback(
    state: &AppState,
    headers: &HeaderMap,
    provider_key: &str,
    query: McpOauthCallbackQuery,
) -> Result<Response, AppError> {
    let user = require_session_user(state, headers).await?;
    let Some(state_token) = query.state.as_deref() else {
        return Ok(connection_error_redirect("state_invalid"));
    };
    let now = OffsetDateTime::now_utc();
    let Some(transaction) = state
        .store
        .consume_mcp_oauth_state(&token_hash(state_token), user.user_id, provider_key, now)
        .await?
    else {
        return Ok(connection_error_redirect("state_invalid"));
    };
    if let Some(error) = callback_transaction_error(&transaction, provider_key, user.user_id, now) {
        return Ok(connection_error_redirect(error));
    }
    if query.error.is_some() {
        return Ok(connection_error_redirect("access_denied"));
    }
    let Some(code) = query.code.as_deref() else {
        return Ok(connection_error_redirect("provider_failure"));
    };
    let grant = match exchange_callback_code(state, &transaction, code).await {
        Ok(grant) => grant,
        Err(error) => {
            tracing::warn!(error = %error, "MCP OAuth token exchange failed");
            return Ok(connection_error_redirect("provider_failure"));
        }
    };
    McpCredentialService::new(state.store.clone())
        .upsert_oauth_user_binding(transaction.mcp_server_id, transaction.user_id, grant)
        .await?;
    Ok(connection_success_redirect(&transaction.redirect_to))
}

/// Exchanges the code with the client the flow started with. A server whose
/// OAuth settings changed during the flow fails instead of binding a token
/// for the old settings.
async fn exchange_callback_code(
    state: &AppState,
    transaction: &McpOauthStateRecord,
    code: &str,
) -> Result<gateway_service::McpOauthTokenGrant, GatewayError> {
    let server = load_oauth_server(state, transaction.mcp_server_id)
        .await
        .map_err(|error| error.0)?;
    let config = mcp_oauth_server_config(&server.auth_config)?;
    if config.provider_key != transaction.provider_key || config.resource != transaction.resource {
        return Err(GatewayError::InvalidRequest(
            "MCP server OAuth settings changed during the connection".to_string(),
        ));
    }
    let client = state
        .mcp_oauth_runtime
        .resolve_client(state.store.as_ref(), server.mcp_server_id, &config, false)
        .await?;
    state
        .mcp_oauth_runtime
        .exchange_code(
            &client,
            &config,
            code,
            &transaction.pkce_verifier,
            &transaction.scopes,
        )
        .await
}

#[utoipa::path(
    get,
    path = "/api/v1/mcp/oauth/client-metadata.json",
    responses(
        (status = 200, description = "OAuth Client ID Metadata Document", body = Object),
        (status = 404, body = OpenAiErrorEnvelopeView)
    )
)]
/// The Client ID Metadata Document whose URL is Oceans' client ID at
/// authorization servers that accept metadata documents instead of dynamic
/// client registration. It is public by design.
pub async fn mcp_oauth_client_metadata_document(
    State(state): State<AppState>,
) -> Result<Response, AppError> {
    let document = state
        .mcp_oauth_runtime
        .client_metadata_document()
        .ok_or_else(|| {
            GatewayError::Store(gateway_core::StoreError::NotFound(
                "mcp.oauth.public_base_url is not configured".to_string(),
            ))
        })?;
    Ok(([(CACHE_CONTROL, "public, max-age=3600")], Json(document)).into_response())
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
            if !value.starts_with('/')
                || value.starts_with("//")
                || value.contains('\\')
                || value.chars().any(char::is_control)
            {
                return false;
            }
            let Ok(url) = url::Url::parse(&format!("https://oceans.invalid{value}")) else {
                return false;
            };
            url.path() == DEFAULT_CONNECTION_REDIRECT && url.fragment().is_none()
        })
        .unwrap_or(DEFAULT_CONNECTION_REDIRECT)
        .to_string()
}

/// Absolute URL of the workspace connections page that starts the OAuth flow
/// for `server_key`. Returns `None` unless the caller can connect the server
/// themselves: it must use user OAuth, its provider must be configured, and a
/// public base URL must be set.
pub(crate) fn connection_page_url(
    state: &AppState,
    server: &gateway_core::ExternalMcpServerRecord,
) -> Option<String> {
    if server.status != ExternalMcpServerStatus::Active
        || server.auth_mode != ExternalMcpAuthMode::OauthObo
    {
        return None;
    }
    let config = mcp_oauth_server_config(&server.auth_config).ok()?;
    let runtime = &state.mcp_oauth_runtime;
    if runtime.connection_unavailable_reason_for(&config).is_some() {
        return None;
    }
    let mut url = url::Url::parse(&format!(
        "{}{DEFAULT_CONNECTION_REDIRECT}",
        runtime.public_base_url()?
    ))
    .ok()?;
    url.query_pairs_mut()
        .append_pair("connect", &server.server_key);
    Some(url.to_string())
}

fn connection_success_redirect(redirect_to: &str) -> Response {
    let separator = if redirect_to.contains('?') { '&' } else { '?' };
    Redirect::temporary(&format!("{redirect_to}{separator}oauth=connected")).into_response()
}

fn connection_error_redirect(code: &str) -> Response {
    Redirect::temporary(&format!("{DEFAULT_CONNECTION_REDIRECT}?oauth_error={code}"))
        .into_response()
}

fn token_hash(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

fn callback_transaction_error(
    transaction: &McpOauthStateRecord,
    provider_key: &str,
    session_user_id: Uuid,
    now: OffsetDateTime,
) -> Option<&'static str> {
    if transaction.expires_at <= now {
        Some("state_expired")
    } else if transaction.provider_key != provider_key || session_user_id != transaction.user_id {
        Some("state_invalid")
    } else {
        None
    }
}

/// Whether a binding was granted for a different provider or resource, or lacks a
/// scope the server now requires. Missing metadata is not judged here; the gateway
/// checks the stored bundle when the binding is used.
fn binding_mismatches_server(
    metadata: &serde_json::Map<String, serde_json::Value>,
    config: &McpOauthServerConfig,
) -> bool {
    let text = |key: &str| metadata.get(key).and_then(serde_json::Value::as_str);
    let issuer = config
        .server_oauth
        .as_ref()
        .map(|endpoints| endpoints.issuer.as_str());
    if text("oauth_provider_key").is_some_and(|provider| provider != config.provider_key)
        || text("resource").is_some_and(|resource| resource != config.resource)
        || (issuer.is_some() && text("oauth_issuer") != issuer)
    {
        return true;
    }
    let Some(granted) = metadata
        .get("granted_scopes")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    config
        .scopes
        .iter()
        .any(|scope| !granted.iter().any(|item| item.as_str() == Some(scope)))
}

fn has_refreshable_oauth_metadata(metadata: &serde_json::Map<String, serde_json::Value>) -> bool {
    // Bundles without a refresh token record `refreshable: false`; earlier
    // bundles always had one and omit the flag.
    metadata
        .get("refreshable")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
        && metadata
            .get("oauth_bundle_version")
            .and_then(serde_json::Value::as_u64)
            == Some(1)
        && metadata
            .get("oauth_provider_key")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|provider| !provider.is_empty())
        && metadata
            .get("resource")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|resource| !resource.is_empty())
}

#[cfg(test)]
mod tests;
