//! Durable MCP session machinery shared by the aggregate `/mcp` route and the
//! Code Mode `/code-mode-mcp` route. Sessions are bound to the authenticated
//! API key and to the surface they were initialized against; cross-principal
//! and cross-surface reuse both present as "not found".

use axum::{
    Json,
    body::Body,
    http::{HeaderMap, HeaderValue, Response, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
};
use gateway_core::{
    AuthenticatedApiKey, McpAggregateSessionRepository, McpSessionSurface,
    NewMcpAggregateSessionRecord,
};
use gateway_mcp::{
    DEFAULT_PROTOCOL_VERSION, JsonRpcId, MCP_PROTOCOL_VERSION_HEADER, MCP_SESSION_ID_HEADER,
    server::{JSON_RPC_INVALID_PARAMS, initialize_result, json_rpc_error, json_rpc_success},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::http::state::AppState;

use super::mcp_error_response;

const SESSION_TTL_HOURS: i64 = 12;

pub(super) async fn initialize_session(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    surface: McpSessionSurface,
    id: JsonRpcId,
    protocol_version: String,
    server_name: &str,
) -> Response<Body> {
    let now = OffsetDateTime::now_utc();
    let session_id = Uuid::new_v4();
    let raw_token = signed_session_token(&state.identity_token_secret, session_id);
    let token_hash = token_hash(&raw_token);
    let session = NewMcpAggregateSessionRecord {
        session_id,
        token_hash,
        surface,
        api_key_id: auth.id,
        owner_kind: auth.owner_kind,
        owner_user_id: auth.owner_user_id,
        owner_team_id: auth.owner_team_id,
        owner_service_account_id: auth.owner_service_account_id,
        protocol_version: if protocol_version.trim().is_empty() {
            DEFAULT_PROTOCOL_VERSION.to_string()
        } else {
            protocol_version
        },
        expires_at: now + Duration::hours(SESSION_TTL_HOURS),
        created_at: now,
    };
    match state.store.create_mcp_aggregate_session(&session).await {
        Ok(_) => json_rpc_response(
            StatusCode::OK,
            json_rpc_success(
                id,
                initialize_result(server_name, env!("CARGO_PKG_VERSION")),
            )
            .unwrap_or_else(serialization_error),
            Some(raw_token),
        ),
        Err(error) => mcp_error_response(error.into()),
    }
}

pub(super) async fn handle_initialized_notification(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    surface: McpSessionSurface,
    headers: &HeaderMap,
) -> Response<Body> {
    let Some((session_id, token_hash)) = session_identity(headers) else {
        return session_http_error(StatusCode::BAD_REQUEST, None, "MCP session id is required");
    };
    match validate_session(state, auth, surface, &token_hash, false).await {
        Ok(session) if session.session_id == session_id => {
            match state
                .store
                .update_mcp_aggregate_session_initialized(
                    session_id,
                    &token_hash,
                    OffsetDateTime::now_utc(),
                )
                .await
            {
                Ok(Some(_)) => StatusCode::ACCEPTED.into_response(),
                Ok(None) => {
                    session_http_error(StatusCode::NOT_FOUND, None, "MCP session was not found")
                }
                Err(error) => mcp_error_response(error.into()),
            }
        }
        Ok(_) => session_http_error(StatusCode::NOT_FOUND, None, "MCP session was not found"),
        Err(response) => response,
    }
}

pub(super) async fn handle_session_delete(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    surface: McpSessionSurface,
    headers: &HeaderMap,
) -> Response<Body> {
    let Some((session_id, token_hash)) = session_identity(headers) else {
        return session_http_error(StatusCode::BAD_REQUEST, None, "MCP session id is required");
    };
    match validate_session(state, auth, surface, &token_hash, false).await {
        Ok(session) if session.session_id == session_id => {
            match state
                .store
                .revoke_mcp_aggregate_session(session_id, &token_hash, OffsetDateTime::now_utc())
                .await
            {
                Ok(true) => StatusCode::ACCEPTED.into_response(),
                Ok(false) => {
                    session_http_error(StatusCode::NOT_FOUND, None, "MCP session was not found")
                }
                Err(error) => mcp_error_response(error.into()),
            }
        }
        Ok(_) => session_http_error(StatusCode::NOT_FOUND, None, "MCP session was not found"),
        Err(response) => response,
    }
}

pub(super) async fn validate_request_session(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    surface: McpSessionSurface,
    headers: &HeaderMap,
    id: &JsonRpcId,
) -> Result<gateway_core::McpAggregateSessionRecord, Response<Body>> {
    let Some((session_id, token_hash)) = session_identity(headers) else {
        return Err(session_http_error(
            StatusCode::BAD_REQUEST,
            Some(id.clone()),
            "MCP session id is required",
        ));
    };
    let session = validate_session(state, auth, surface, &token_hash, true).await?;
    if session.session_id != session_id {
        return Err(session_http_error(
            StatusCode::NOT_FOUND,
            Some(id.clone()),
            "MCP session was not found",
        ));
    }
    match state
        .store
        .touch_mcp_aggregate_session(session_id, &token_hash, OffsetDateTime::now_utc())
        .await
    {
        Ok(Some(session)) => Ok(session),
        Ok(None) => Err(session_http_error(
            StatusCode::NOT_FOUND,
            Some(id.clone()),
            "MCP session was not found",
        )),
        Err(error) => Err(mcp_error_response(error.into())),
    }
}

async fn validate_session(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    surface: McpSessionSurface,
    token_hash: &str,
    require_initialized: bool,
) -> Result<gateway_core::McpAggregateSessionRecord, Response<Body>> {
    let session = match state
        .store
        .get_mcp_aggregate_session_by_token_hash(token_hash)
        .await
    {
        Ok(Some(session)) => session,
        Ok(None) => {
            return Err(session_http_error(
                StatusCode::NOT_FOUND,
                None,
                "MCP session was not found",
            ));
        }
        Err(error) => return Err(mcp_error_response(error.into())),
    };
    // Surface mismatch is indistinguishable from "not found": a session
    // initialized at `/mcp` must never validate at `/code-mode-mcp` (or vice
    // versa), exactly like cross-principal reuse.
    if session.surface != surface
        || session.api_key_id != auth.id
        || session.owner_kind != auth.owner_kind
        || session.owner_user_id != auth.owner_user_id
        || session.owner_team_id != auth.owner_team_id
        || session.owner_service_account_id != auth.owner_service_account_id
    {
        return Err(session_http_error(
            StatusCode::NOT_FOUND,
            None,
            "MCP session was not found",
        ));
    }
    let now = OffsetDateTime::now_utc();
    if session.revoked_at.is_some() || session.expires_at <= now {
        return Err(session_http_error(
            StatusCode::NOT_FOUND,
            None,
            "MCP session was not found",
        ));
    }
    if require_initialized && !session.initialized {
        return Err(session_http_error(
            StatusCode::BAD_REQUEST,
            None,
            "MCP session has not completed initialization",
        ));
    }
    Ok(session)
}

pub(super) fn session_identity(headers: &HeaderMap) -> Option<(Uuid, String)> {
    let raw = headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())?;
    let (raw_id, _signature) = raw.split_once('.')?;
    let session_id = Uuid::parse_str(raw_id).ok()?;
    Some((session_id, token_hash(raw)))
}

pub(super) fn signed_session_token(secret: &str, session_id: Uuid) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"mcp-session:");
    hasher.update(secret.as_bytes());
    hasher.update(b":");
    hasher.update(session_id.as_bytes());
    let signature = hex_string(&hasher.finalize());
    format!("{session_id}.{}", &signature[..32])
}

pub(super) fn token_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex_string(&hasher.finalize())
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn session_http_error(
    status: StatusCode,
    id: Option<JsonRpcId>,
    message: &str,
) -> Response<Body> {
    json_rpc_response(
        status,
        json_rpc_error(id, JSON_RPC_INVALID_PARAMS, message),
        None,
    )
}

pub(super) fn json_rpc_response(
    status: StatusCode,
    payload: Value,
    session_id: Option<String>,
) -> Response<Body> {
    let mut response = (status, Json(payload)).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response.headers_mut().insert(
        MCP_PROTOCOL_VERSION_HEADER,
        HeaderValue::from_static(DEFAULT_PROTOCOL_VERSION),
    );
    if let Some(session_id) = session_id
        && let Ok(value) = HeaderValue::from_str(&session_id)
    {
        response.headers_mut().insert(MCP_SESSION_ID_HEADER, value);
    }
    response
}

pub(super) fn serialization_error(error: serde_json::Error) -> Value {
    json_rpc_error(None, JSON_RPC_INVALID_PARAMS, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_session_token_is_visible_ascii_and_parseable() {
        let session_id = Uuid::new_v4();
        let token = signed_session_token("secret", session_id);
        assert!(token.bytes().all(|byte| (0x21..=0x7e).contains(&byte)));
        let mut headers = HeaderMap::new();
        headers.insert(
            MCP_SESSION_ID_HEADER,
            HeaderValue::from_str(&token).unwrap(),
        );
        let (parsed_id, parsed_hash) = session_identity(&headers).expect("session identity");
        assert_eq!(parsed_id, session_id);
        assert_eq!(parsed_hash, token_hash(&token));
    }
}
