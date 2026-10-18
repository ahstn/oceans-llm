use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use gateway_core::{GatewayError, McpOauthResource, SUPPORTED_SCOPES, StoreError};
use gateway_service::{McpClientOauthSettings, McpGatewayService};
use serde::Serialize;
use serde_json::json;

use crate::http::state::AppState;

const AGGREGATE_RESOURCE_NAME: &str = "Oceans MCP gateway";

/// RFC 9728 protected-resource metadata.
#[derive(Debug, Serialize)]
struct ProtectedResourceMetadata {
    resource: String,
    authorization_servers: Vec<String>,
    scopes_supported: Vec<&'static str>,
    bearer_methods_supported: [&'static str; 1],
    resource_name: String,
}

/// RFC 8414 authorization-server metadata.
#[derive(Debug, Serialize)]
struct AuthorizationServerMetadata {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    registration_endpoint: Option<String>,
    revocation_endpoint: String,
    scopes_supported: [&'static str; 2],
    response_types_supported: [&'static str; 1],
    response_modes_supported: [&'static str; 1],
    grant_types_supported: [&'static str; 2],
    token_endpoint_auth_methods_supported: [&'static str; 3],
    revocation_endpoint_auth_methods_supported: [&'static str; 3],
    code_challenge_methods_supported: [&'static str; 1],
    authorization_response_iss_parameter_supported: bool,
    client_id_metadata_document_supported: bool,
}

pub(super) const TOKEN_ENDPOINT_AUTH_METHODS: [&str; 3] =
    ["none", "client_secret_post", "client_secret_basic"];

/// `GET /.well-known/oauth-protected-resource/mcp`
pub async fn oauth_protected_resource_metadata_aggregate(
    State(state): State<AppState>,
) -> Response {
    let Some(settings) = state.mcp_client_oauth.as_deref() else {
        return not_enabled();
    };
    Json(protected_resource_metadata(
        settings,
        &McpOauthResource::Aggregate,
        AGGREGATE_RESOURCE_NAME.to_string(),
    ))
    .into_response()
}

/// `GET /.well-known/oauth-protected-resource/mcp/{server_key}`
pub async fn oauth_protected_resource_metadata_server(
    State(state): State<AppState>,
    Path(server_key): Path<String>,
) -> Response {
    let Some(settings) = state.mcp_client_oauth.as_deref() else {
        return not_enabled();
    };
    let Some(resource) = McpOauthResource::from_path(&format!("/mcp/{server_key}")) else {
        return not_found();
    };
    let McpOauthResource::Server(server_key) = &resource else {
        return not_found();
    };
    // This route is public, so only a server published to the organisation is
    // named. A team-only server, an inactive one and a missing one all get the
    // same metadata, named by the key the caller already sent, so the route
    // never confirms that a hidden server exists. Whether the signed-in user
    // may reach it is decided at consent and on every request.
    let resource_name = match McpGatewayService::new(state.store.clone())
        .load_active_server(server_key)
        .await
    {
        Ok(server)
            if server.ownership.visibility == gateway_core::McpServerVisibility::Organisation =>
        {
            server.display_name
        }
        Ok(_) | Err(GatewayError::Store(StoreError::NotFound(_))) => server_key.clone(),
        Err(error) => return lookup_failure(&error),
    };
    Json(protected_resource_metadata(
        settings,
        &resource,
        resource_name,
    ))
    .into_response()
}

/// `GET /.well-known/oauth-authorization-server`
pub async fn oauth_authorization_server_metadata(State(state): State<AppState>) -> Response {
    let Some(settings) = state.mcp_client_oauth.as_deref() else {
        return not_enabled();
    };
    Json(authorization_server_metadata(settings)).into_response()
}

fn protected_resource_metadata(
    settings: &McpClientOauthSettings,
    resource: &McpOauthResource,
    resource_name: String,
) -> ProtectedResourceMetadata {
    // Every scope the authorization endpoint accepts for this resource, so
    // discovery matches enforcement. The 401 challenge still names only the
    // scope the route needs, which is what clients should request first.
    let scopes_supported = SUPPORTED_SCOPES
        .into_iter()
        .filter(|scope| resource.allows_scope(scope))
        .collect();
    ProtectedResourceMetadata {
        resource: settings.resource_uri(resource),
        authorization_servers: vec![settings.issuer().to_string()],
        scopes_supported,
        bearer_methods_supported: ["header"],
        resource_name,
    }
}

fn authorization_server_metadata(settings: &McpClientOauthSettings) -> AuthorizationServerMetadata {
    AuthorizationServerMetadata {
        issuer: settings.issuer().to_string(),
        authorization_endpoint: settings.endpoint("/oauth/authorize"),
        token_endpoint: settings.endpoint("/oauth/token"),
        registration_endpoint: settings
            .dynamic_client_registration()
            .then(|| settings.endpoint("/oauth/register")),
        revocation_endpoint: settings.endpoint("/oauth/revoke"),
        scopes_supported: SUPPORTED_SCOPES,
        response_types_supported: ["code"],
        response_modes_supported: ["query"],
        grant_types_supported: ["authorization_code", "refresh_token"],
        token_endpoint_auth_methods_supported: TOKEN_ENDPOINT_AUTH_METHODS,
        revocation_endpoint_auth_methods_supported: TOKEN_ENDPOINT_AUTH_METHODS,
        code_challenge_methods_supported: ["S256"],
        authorization_response_iss_parameter_supported: true,
        client_id_metadata_document_supported: settings.client_id_metadata_documents().is_some(),
    }
}

fn not_enabled() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "not_found",
            "error_description": "OAuth for MCP clients is not enabled on this gateway"
        })),
    )
        .into_response()
}

/// A missing or malformed server is `404`. A store outage must not look like
/// a missing resource, or clients abandon discovery instead of retrying.
fn lookup_failure(error: &GatewayError) -> Response {
    if matches!(
        error,
        GatewayError::Store(StoreError::NotFound(_)) | GatewayError::InvalidRequest(_)
    ) {
        return not_found();
    }
    tracing::warn!(error = %error, "MCP OAuth resource metadata lookup failed");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "temporarily_unavailable",
            "error_description": "MCP server metadata could not be loaded"
        })),
    )
        .into_response()
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "not_found",
            "error_description": "MCP server was not found"
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_missing_server_is_not_found() {
        let status = |error: GatewayError| lookup_failure(&error).status();
        assert_eq!(
            status(GatewayError::Store(StoreError::NotFound("gone".into()))),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status(GatewayError::InvalidRequest("bad key".into())),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status(GatewayError::Store(StoreError::Unavailable("down".into()))),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            status(GatewayError::Store(StoreError::Query("timeout".into()))),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
