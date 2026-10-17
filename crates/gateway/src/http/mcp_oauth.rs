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
    AuditEvent, MCP_SERVER_OAUTH_PROVIDER_KEY, McpCredentialService, McpOauthServerConfig,
    admin_audit::actions, credential_owner_scope_key, mcp_oauth_server_config,
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
    audit::AuditRequestMeta,
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
    let team_id = user_team(&state, user.user_id).await?;
    let mut items = Vec::new();
    // Servers listed only because the user can reach them, rechecked below.
    let mut listed_for_reach = std::collections::HashSet::new();
    for server in state.store.list_external_mcp_servers(true).await? {
        let binding = bindings.get(&server.mcp_server_id);
        let oauth_binding = binding.filter(|binding| {
            binding.material_kind == McpUpstreamCredentialMaterialKind::OauthTokens
        });
        // A team-only server the user cannot reach stays hidden, as in the
        // member registry, unless they hold a binding they may still revoke.
        if oauth_binding.is_none() {
            if !user_can_reach(team_id, &server) {
                continue;
            }
            listed_for_reach.insert(server.mcp_server_id);
        }
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
    // Checked again after the reads, against the user's team and each server
    // as they are now: leaving the team, or the server moving, in between must
    // not show a team-only server the user no longer reaches.
    if !listed_for_reach.is_empty() {
        let current_team = user_team(&state, user.user_id).await?;
        let ids: Vec<Uuid> = listed_for_reach.iter().copied().collect();
        let still_reached: std::collections::HashSet<Uuid> = state
            .store
            .list_external_mcp_servers_by_ids(&ids)
            .await?
            .into_iter()
            .filter(|server| user_can_reach(current_team, server))
            .map(|server| server.mcp_server_id)
            .collect();
        items.retain(|item: &McpOauthConnectionView| {
            !listed_for_reach.contains(&item.server_id) || still_reached.contains(&item.server_id)
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
    let team_id = user_team(&state, user.user_id).await?;
    if !user_can_reach(team_id, &server) {
        return Err(
            GatewayError::Store(gateway_core::StoreError::NotFound(format!(
                "external MCP server `{server_id}` not found"
            )))
            .into(),
        );
    }
    let config = mcp_oauth_server_config(&server.auth_config)?;
    // A dynamic-registration server registers its client here, on first use.
    let client = state
        .mcp_oauth_runtime
        .resolve_client(state.store.as_ref(), &server, &config, true)
        .await?;
    let redirect_to = normalize_connection_redirect(request.redirect_to.as_deref());
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let csrf_state = CsrfToken::new_random();
    let authorization_url =
        client.authorization_url(&config, csrf_state.secret(), pkce_challenge.as_str())?;
    let now = OffsetDateTime::now_utc();
    // Stored only while the server keeps the settings this authorization URL
    // was built from; a settings change deletes its states under the same lock.
    state
        .store
        .create_mcp_oauth_state_guarded(
            &McpOauthStateRecord {
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
            },
            &gateway_core::McpServerOwnershipGuard::of(&server).with_auth_settings(&server),
        )
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
    meta: AuditRequestMeta,
    Path(provider_key): Path<String>,
    Query(query): Query<McpOauthCallbackQuery>,
) -> Result<Response, AppError> {
    complete_oauth_callback(&state, &headers, &meta, &provider_key, query).await
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
    meta: AuditRequestMeta,
    Query(query): Query<McpOauthCallbackQuery>,
) -> Result<Response, AppError> {
    complete_oauth_callback(
        &state,
        &headers,
        &meta,
        MCP_SERVER_OAUTH_PROVIDER_KEY,
        query,
    )
    .await
}

async fn complete_oauth_callback(
    state: &AppState,
    headers: &HeaderMap,
    meta: &AuditRequestMeta,
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
    let (grant, exchanged_with) = match exchange_callback_code(state, &transaction, code).await {
        Ok(exchanged) => exchanged,
        Err(error) => {
            tracing::warn!(error = %error, "MCP OAuth token exchange failed");
            return Ok(connection_error_redirect("provider_failure"));
        }
    };
    let credentials = McpCredentialService::new(state.store.clone());
    // Stored only while the server keeps the settings the code was exchanged
    // with. A settings change revokes users' tokens under the same lock, so a
    // bundle from replaced endpoints never lands after it.
    let connect = credentials.upsert_oauth_user_binding(
        transaction.mcp_server_id,
        transaction.user_id,
        grant,
        Some(exchanged_with.clone()),
    );
    // Token values never leave the service; the entry keeps binding metadata only.
    // A reconnect replaces the user's binding, so it also records what it replaced.
    let connected = state
        .audit()
        .audited(
            &meta.user_context(&user),
            connect,
            |(binding, replaced, _)| {
                let event = AuditEvent::new(actions::MCP_CREDENTIAL_BINDING_SET)
                    .target(binding.credential_binding_id)
                    .label(binding.owner_scope_key.clone())
                    .meta(
                        "operation",
                        if replaced.is_some() {
                            "oauth_reconnect"
                        } else {
                            "oauth_connect"
                        },
                    )
                    .meta("mcp_server_id", binding.mcp_server_id)
                    .after(oauth_binding_summary(binding));
                match replaced {
                    Some(replaced) => event.before(oauth_binding_summary(replaced)),
                    None => event,
                }
            },
        )
        .await;
    match connected {
        Ok((binding, _, written)) => {
            // Membership is not part of the write's guard, so check it again
            // now the binding is stored: a user who left the team meanwhile
            // keeps no connection to its team-only server.
            let team_id = user_team(state, transaction.user_id).await?;
            let reachable = exchanged_with.is_reachable_from(team_id);
            if !reachable {
                // Only this callback's own tokens are revoked: a later callback
                // that stored new ones is left alone. Recorded like a
                // disconnect, so the trail matches the set written just before.
                if credentials
                    .revoke_oauth_binding_if_unchanged(binding.credential_binding_id, &written)
                    .await?
                {
                    state
                        .audit()
                        .record(
                            &meta.user_context(&user),
                            AuditEvent::new(actions::MCP_CREDENTIAL_BINDING_REVOKED)
                                .target(binding.credential_binding_id)
                                .label(binding.owner_scope_key.clone())
                                .meta("operation", "oauth_access_lost")
                                .meta("mcp_server_id", binding.mcp_server_id)
                                .before(oauth_binding_summary(&binding)),
                        )
                        .await;
                }
                return Ok(connection_error_redirect("provider_failure"));
            }
            Ok(connection_success_redirect(&transaction.redirect_to))
        }
        // The server's settings changed during the flow; the user connects again.
        Err(GatewayError::Store(gateway_core::StoreError::Conflict(message))) => {
            tracing::warn!(%message, "MCP OAuth settings changed before the token was stored");
            Ok(connection_error_redirect("provider_failure"))
        }
        Err(error) => Err(error.into()),
    }
}

/// Exchanges the code with the client the flow started with. A server whose
/// OAuth settings changed during the flow fails instead of binding a token
/// for the old settings.
async fn exchange_callback_code(
    state: &AppState,
    transaction: &McpOauthStateRecord,
    code: &str,
) -> Result<
    (
        gateway_service::McpOauthTokenGrant,
        gateway_core::McpServerOwnershipGuard,
    ),
    GatewayError,
> {
    let server = load_oauth_server(state, transaction.mcp_server_id)
        .await
        .map_err(|error| error.0)?;
    // The user may have left the team, or the server moved, since the flow
    // started. The token write below is pinned to this server's ownership, so
    // a transfer after this check is refused there.
    let team_id = user_team(state, transaction.user_id)
        .await
        .map_err(|error| error.0)?;
    if !user_can_reach(team_id, &server) {
        return Err(GatewayError::InvalidRequest(
            "the MCP server is no longer available to this user".to_string(),
        ));
    }
    // A change to the server's auth settings deletes its OAuth states in the
    // same write. The state still existing after the reload above means the
    // code goes to the endpoints the flow started with, not replacements.
    if !state
        .store
        .mcp_oauth_state_exists(&transaction.state_hash)
        .await?
    {
        return Err(GatewayError::InvalidRequest(
            "MCP server OAuth settings changed during the connection".to_string(),
        ));
    }
    let config = mcp_oauth_server_config(&server.auth_config)?;
    if config.provider_key != transaction.provider_key || config.resource != transaction.resource {
        return Err(GatewayError::InvalidRequest(
            "MCP server OAuth settings changed during the connection".to_string(),
        ));
    }
    let client = state
        .mcp_oauth_runtime
        .resolve_client(state.store.as_ref(), &server, &config, false)
        .await?;
    let grant = state
        .mcp_oauth_runtime
        .exchange_code(
            &client,
            &config,
            code,
            &transaction.pkce_verifier,
            &transaction.scopes,
        )
        .await?;
    Ok((
        grant,
        gateway_core::McpServerOwnershipGuard::of(&server).with_auth_settings(&server),
    ))
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
    meta: AuditRequestMeta,
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
        let credentials = McpCredentialService::new(state.store.clone());
        // The revoke returns the binding it removed, so the entry records that
        // access, not just its id. An already-revoked binding records nothing.
        if let Some(revoked) = credentials
            .revoke_binding(binding.credential_binding_id)
            .await?
        {
            state
                .audit()
                .record(
                    &meta.user_context(&user),
                    AuditEvent::new(actions::MCP_CREDENTIAL_BINDING_REVOKED)
                        .target(revoked.credential_binding_id)
                        .label(revoked.owner_scope_key.clone())
                        .meta("operation", "oauth_disconnect")
                        .meta("mcp_server_id", server_id)
                        .before(oauth_binding_summary(&revoked)),
                )
                .await;
        }
    }
    Ok(Json(McpOauthRevokeResponse { revoked: true }))
}

/// The OAuth binding fields an entry keeps: never the token values.
fn oauth_binding_summary(
    binding: &gateway_service::RedactedMcpCredentialBinding,
) -> serde_json::Value {
    serde_json::json!({
        "material_kind": binding.material_kind.as_str(),
        "oauth_provider_key": binding.oauth_provider_key,
        "granted_scopes": binding.granted_scopes,
        "expires_at": binding.expires_at.map(format_timestamp),
    })
}

async fn require_session_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<gateway_core::UserRecord, AppError> {
    resolve_session_user(state, headers)
        .await?
        .ok_or_else(|| AppError(AuthError::SessionRequired.into()))
}

async fn user_team(state: &AppState, user_id: Uuid) -> Result<Option<Uuid>, AppError> {
    Ok(
        gateway_core::IdentityRepository::get_team_membership_for_user(&*state.store, user_id)
            .await?
            .map(|membership| membership.team_id),
    )
}

/// Whether a user may see and connect to `server`: one published to the
/// organisation, or a team-only server their current team owns. A grant does
/// not widen this, matching runtime access, which ignores a team-only server's
/// grants once their holder has left the team.
fn user_can_reach(team_id: Option<Uuid>, server: &gateway_core::ExternalMcpServerRecord) -> bool {
    server.ownership.is_reachable_from(team_id)
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
