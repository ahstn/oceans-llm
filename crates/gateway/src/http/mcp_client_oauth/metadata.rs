use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use gateway_core::{McpOauthResource, SCOPE_MCP_AGGREGATE, SCOPE_MCP_PROXY, SUPPORTED_SCOPES};
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
    let server = match McpGatewayService::new(state.store.clone())
        .load_active_server(server_key)
        .await
    {
        Ok(server) => server,
        Err(_) => return not_found(),
    };
    Json(protected_resource_metadata(
        settings,
        &resource,
        server.display_name,
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
    let scopes_supported = match resource {
        McpOauthResource::Aggregate => vec![SCOPE_MCP_AGGREGATE],
        McpOauthResource::Server(_) => vec![SCOPE_MCP_PROXY],
    };
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
