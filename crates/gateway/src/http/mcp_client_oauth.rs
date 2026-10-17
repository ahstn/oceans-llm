//! The gateway as an OAuth protected resource and authorization server for
//! MCP clients. See `docs/adr/2026-10-15-mcp-client-oauth-authorization.md`.

mod authorize;
mod authorized_clients;
mod consent;
mod form;
mod metadata;
mod registration;
mod revocation;
mod stream_lifetime;
mod token;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{
        HeaderMap, HeaderName, HeaderValue, Method,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    routing::{get, post},
};
use gateway_core::{AuthError, GatewayError, McpOauthResource, UserRecord, UserStatus};
use gateway_service::{McpClientOauthService, McpClientOauthSettings};
use gateway_store::AnyStore;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use crate::http::{
    error::AppError, identity::resolve_session_user_with_session_id, state::AppState,
};

pub use authorize::oauth_authorize;
pub use authorized_clients::{
    __path_admin_list_authorized_mcp_clients, __path_admin_revoke_authorized_mcp_clients,
    __path_list_my_authorized_mcp_clients, __path_revoke_my_authorized_mcp_client,
    AdminRevokeAuthorizedMcpClientsRequest, AuthorizedMcpClientView, AuthorizedMcpClientsView,
    RevokeAuthorizedMcpClientsResponse, admin_list_authorized_mcp_clients,
    admin_revoke_authorized_mcp_clients, list_my_authorized_mcp_clients,
    revoke_my_authorized_mcp_client,
};
pub use consent::{
    __path_decide_oauth_authorization_request, __path_get_oauth_authorization_request,
    OauthAuthorizationDecisionRequest, OauthAuthorizationDecisionResponse,
    OauthAuthorizationRequestView, OauthResourceKind, OauthScopeView,
    decide_oauth_authorization_request, get_oauth_authorization_request,
};
pub(crate) use form::MAX_FORM_BODY_BYTES;
pub use metadata::{
    oauth_authorization_server_metadata, oauth_protected_resource_metadata_aggregate,
    oauth_protected_resource_metadata_server,
};
pub(crate) use registration::MAX_REGISTRATION_BODY_BYTES;
pub use registration::oauth_register;
pub use revocation::oauth_revoke;
pub(crate) use stream_lifetime::bound_to_oauth_access;
pub use stream_lifetime::{OAUTH_STREAM_RECHECK_INTERVAL, OauthStreamWatcher};
pub use token::oauth_token;

/// The endpoints an MCP client calls itself: metadata, token, revocation, and
/// registration. A browser-based client calls them from its own origin, so
/// they allow any origin. They take no cookies, so the CORS policy grants no
/// credentials, and a page cannot use it to act as a signed-in user.
/// `/oauth/authorize` is a top-level navigation and the consent API is
/// cookie-authenticated, so neither is here.
pub(crate) fn public_router() -> Router<AppState> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("mcp-protocol-version"),
        ])
        .max_age(Duration::from_secs(24 * 60 * 60));
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(oauth_protected_resource_metadata_aggregate),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp/{server_key}",
            get(oauth_protected_resource_metadata_server),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth_authorization_server_metadata),
        )
        .route(
            "/oauth/token",
            post(oauth_token).layer(DefaultBodyLimit::max(MAX_FORM_BODY_BYTES)),
        )
        .route(
            "/oauth/revoke",
            post(oauth_revoke).layer(DefaultBodyLimit::max(MAX_FORM_BODY_BYTES)),
        )
        .route(
            "/oauth/register",
            post(oauth_register).layer(DefaultBodyLimit::max(MAX_REGISTRATION_BODY_BYTES)),
        )
        .layer(cors)
}

/// The signed-in active user and their `user_sessions` ID.
pub(super) async fn require_active_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(UserRecord, Uuid), AppError> {
    let (user, session_id) = resolve_session_user_with_session_id(state, headers)
        .await?
        .ok_or(AppError(AuthError::SessionRequired.into()))?;
    if user.status != UserStatus::Active {
        return Err(AppError(AuthError::InsufficientPrivileges.into()));
    }
    Ok((user, session_id))
}

/// The authorization server, or `None` when OAuth for MCP clients is disabled.
fn oauth_service(state: &AppState) -> Option<McpClientOauthService<AnyStore>> {
    state
        .mcp_client_oauth
        .clone()
        .map(|settings| McpClientOauthService::new(state.store.clone(), settings))
}

/// The `WWW-Authenticate` value for an MCP auth failure on `resource`, or
/// `None` when the error needs no OAuth challenge.
///
/// A `401` names the metadata document and the scope the route needs. It adds
/// `error="invalid_token"` only when a credential was presented, as RFC 6750
/// section 3.1 asks. A `403` for a missing scope uses `insufficient_scope`.
pub(crate) fn bearer_challenge(
    settings: &McpClientOauthSettings,
    resource: &McpOauthResource,
    error: &GatewayError,
) -> Option<HeaderValue> {
    let metadata_url = settings.protected_resource_metadata_url(resource);
    let challenge = match error {
        GatewayError::Auth(AuthError::OauthInsufficientScope { required }) => format!(
            r#"Bearer error="insufficient_scope", scope="{required}", resource_metadata="{metadata_url}""#
        ),
        error if error.http_status_code() == 401 => {
            let scope = resource.required_scope();
            let credential_missing = matches!(
                error,
                GatewayError::Auth(
                    AuthError::MissingAuthorizationHeader | AuthError::MissingBearerToken
                )
            );
            if credential_missing {
                format!(r#"Bearer resource_metadata="{metadata_url}", scope="{scope}""#)
            } else {
                format!(
                    r#"Bearer error="invalid_token", resource_metadata="{metadata_url}", scope="{scope}""#
                )
            }
        }
        _ => return None,
    };
    HeaderValue::from_str(&challenge).ok()
}

#[cfg(test)]
mod authorized_clients_tests;
#[cfg(test)]
mod flow_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod mcp_route_tests;
#[cfg(test)]
mod metadata_document_tests;
#[cfg(test)]
mod registration_tests;
#[cfg(test)]
mod tests;
