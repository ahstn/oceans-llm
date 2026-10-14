//! The gateway as an OAuth protected resource and authorization server for
//! MCP clients. See `docs/adr/2026-10-15-mcp-client-oauth-authorization.md`.

mod metadata;

use axum::http::HeaderValue;
use gateway_core::{AuthError, GatewayError, McpOauthResource};
use gateway_service::McpClientOauthSettings;

pub use metadata::{
    oauth_authorization_server_metadata, oauth_protected_resource_metadata_aggregate,
    oauth_protected_resource_metadata_server,
};

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
mod tests;
