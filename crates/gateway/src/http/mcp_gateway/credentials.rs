//! Next steps for MCP calls that fail because the caller has no usable
//! upstream credential.
//!
//! A user with an OAuth-capable server gets a `connect_url` that starts the
//! connection flow. Credentials that only an admin can bind get a message to
//! contact an administrator instead.

use axum::{
    Json,
    body::Body,
    http::{Response, StatusCode},
    response::IntoResponse,
};
use gateway_core::{
    ApiKeyOwnerKind, AuthenticatedApiKey, ExternalMcpServerRecord, GatewayError,
    OpenAiErrorEnvelope,
};
use gateway_mcp::server::call_tool_error_result;
use serde_json::{Value, json};

use super::{json_rpc::mcp_jsonrpc_result_response, mcp_error_response};
use crate::http::{mcp_oauth::connection_page_url, state::AppState};

pub(super) struct CredentialGuidance {
    pub message: String,
    pub connect_url: Option<String>,
}

/// Returns guidance when `error` is a missing or expired upstream credential.
pub(super) fn credential_guidance(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    server: &ExternalMcpServerRecord,
    error: &GatewayError,
) -> Option<CredentialGuidance> {
    let expired = match error {
        GatewayError::McpCredentialRequired { .. } => false,
        GatewayError::McpCredentialExpired { .. } => true,
        _ => return None,
    };
    // A user's own connection only resolves for keys that user owns.
    let connect_url = (auth.owner_kind == ApiKeyOwnerKind::User)
        .then(|| connection_page_url(state, server))
        .flatten();
    let next_step = match (&connect_url, expired) {
        (Some(url), false) => format!("Connect your account at {url}, then retry the call."),
        (Some(url), true) => format!("Reconnect your account at {url}, then retry the call."),
        (None, _) => {
            "Contact an Oceans administrator to set up a credential for this server.".to_string()
        }
    };
    Some(CredentialGuidance {
        message: format!("{error}. {next_step}"),
        connect_url,
    })
}

/// The error response for a failed upstream credential lookup on a non-tool
/// request. Credential errors get the guidance and `connect_url` added to the
/// usual 403 envelope; other errors are returned unchanged.
pub(super) fn upstream_auth_error_response(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    server: &ExternalMcpServerRecord,
    error: GatewayError,
) -> Response<Body> {
    let Some(guidance) = credential_guidance(state, auth, server, &error) else {
        return mcp_error_response(error);
    };
    let mut envelope = OpenAiErrorEnvelope::from_gateway_error(&error);
    envelope.error.message = guidance.message;
    let mut body = serde_json::to_value(envelope).unwrap_or_else(|_| json!({"error": {}}));
    if let (Some(url), Some(error)) = (guidance.connect_url, body["error"].as_object_mut()) {
        error.insert("connect_url".to_string(), Value::String(url));
    }
    (StatusCode::FORBIDDEN, Json(body)).into_response()
}

/// The error response for a failed upstream credential lookup on
/// `tools/call`. Credential errors become a tool result with `isError: true`,
/// so clients that only show tool output still show the next step. Other
/// errors are returned unchanged.
pub(super) fn tool_call_auth_error_response(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    server: &ExternalMcpServerRecord,
    id: Option<&Value>,
    error: GatewayError,
) -> Response<Body> {
    let Some(guidance) = credential_guidance(state, auth, server, &error) else {
        return mcp_error_response(error);
    };
    let mut structured = json!({"server_key": server.server_key});
    if let Some(url) = &guidance.connect_url {
        structured["connect_url"] = json!(url);
    }
    let result = call_tool_error_result(guidance.message, error.error_code(), structured);
    mcp_jsonrpc_result_response(id, json!(result))
}

#[cfg(test)]
mod tests;
