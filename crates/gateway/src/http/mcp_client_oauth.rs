//! The gateway as an OAuth protected resource and authorization server for
//! MCP clients. See `docs/adr/2026-10-15-mcp-client-oauth-authorization.md`.

mod authorize;
mod consent;
mod form;
mod metadata;
mod registration;
mod revocation;
mod token;

use axum::http::HeaderValue;
use gateway_core::{AuthError, GatewayError, McpOauthResource};
use gateway_service::{McpClientOauthService, McpClientOauthSettings};
use gateway_store::AnyStore;

use crate::http::state::AppState;

pub use authorize::oauth_authorize;
pub use consent::{
    __path_decide_oauth_authorization_request, __path_get_oauth_authorization_request,
    OauthAuthorizationDecisionRequest, OauthAuthorizationDecisionResponse,
    OauthAuthorizationRequestView, OauthResourceKind, OauthScopeView,
    decide_oauth_authorization_request, get_oauth_authorization_request,
};
pub use metadata::{
    oauth_authorization_server_metadata, oauth_protected_resource_metadata_aggregate,
    oauth_protected_resource_metadata_server,
};
pub(crate) use registration::MAX_REGISTRATION_BODY_BYTES;
pub use registration::oauth_register;
pub use revocation::oauth_revoke;
pub use token::oauth_token;

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
mod flow_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod mcp_route_tests;
#[cfg(test)]
mod registration_tests;
#[cfg(test)]
mod tests;
