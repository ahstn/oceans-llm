//! Binds upstream `Mcp-Session-Id` values to the gateway caller.
//!
//! The per-server proxy never shows the upstream session id as is. It hands
//! the client `<upstream id, base64url>.<tag>`, where the tag is an HMAC over
//! the server, the authenticated credential and its owner. A request whose
//! session id does not verify for its own caller is refused with `404`, which
//! MCP clients treat as an expired session, and never reaches the upstream.
//! Without this, on a server that uses one shared credential, any caller who
//! learned a session id could resume or delete another caller's session.

use axum::{
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, Response, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use gateway_core::{AuthenticatedApiKey, ExternalMcpServerRecord};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::{MCP_SESSION_ID, json_rpc::mcp_jsonrpc_error_response};

type HmacSha256 = Hmac<Sha256>;

const TAG_HEX_LEN: usize = 32;
/// The aggregate route answers an unknown session the same way.
const SESSION_NOT_FOUND_CODE: i64 = gateway_mcp::server::JSON_RPC_INVALID_PARAMS;

/// Signs and checks session ids for one caller on one server.
pub(super) struct SessionBinding<'a> {
    secret: &'a str,
    auth: &'a AuthenticatedApiKey,
    server: &'a ExternalMcpServerRecord,
}

impl<'a> SessionBinding<'a> {
    pub(super) fn new(
        secret: &'a str,
        auth: &'a AuthenticatedApiKey,
        server: &'a ExternalMcpServerRecord,
    ) -> Self {
        Self {
            secret,
            auth,
            server,
        }
    }

    fn tag(&self, upstream_session_id: &[u8]) -> String {
        let mut mac =
            HmacSha256::new_from_slice(self.secret.as_bytes()).expect("HMAC accepts any key");
        let optional = |id: Option<uuid::Uuid>| id.map(|id| id.to_string()).unwrap_or_default();
        for part in [
            "oceans-mcp-upstream-session:v1".to_string(),
            self.server.mcp_server_id.to_string(),
            optional(self.auth.api_key_id()),
            optional(self.auth.oauth_grant_id()),
            self.auth.owner_kind.as_str().to_string(),
            optional(self.auth.owner_user_id),
            optional(self.auth.owner_team_id),
            optional(self.auth.owner_service_account_id),
        ] {
            mac.update(part.as_bytes());
            mac.update(b"\0");
        }
        mac.update(upstream_session_id);
        mac.finalize().into_bytes()[..TAG_HEX_LEN / 2]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// The session id the client sees for an upstream session id.
    pub(super) fn bind(&self, upstream_session_id: &[u8]) -> String {
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(upstream_session_id),
            self.tag(upstream_session_id)
        )
    }

    /// The upstream session id behind `client_session_id`, when it was issued
    /// to this caller for this server.
    pub(super) fn unbind(&self, client_session_id: &str) -> Option<Vec<u8>> {
        let (encoded, tag) = client_session_id.rsplit_once('.')?;
        let upstream = URL_SAFE_NO_PAD.decode(encoded).ok()?;
        let expected = self.tag(&upstream);
        // Constant-time comparison: the tag is a MAC.
        let matches = tag.len() == expected.len()
            && tag
                .bytes()
                .zip(expected.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0;
        matches.then_some(upstream)
    }

    /// Replaces the client's session id with the upstream one. A session id
    /// that does not verify for this caller yields the `404` to return.
    pub(super) fn inbound_headers(
        &self,
        headers: &HeaderMap,
    ) -> Result<HeaderMap, Box<Response<Body>>> {
        let mut headers = headers.clone();
        let values = headers
            .get_all(MCP_SESSION_ID)
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        if values.is_empty() {
            return Ok(headers);
        }
        headers.remove(MCP_SESSION_ID);
        let [value] = values.as_slice() else {
            return Err(Box::new(session_not_found()));
        };
        let upstream = value
            .to_str()
            .ok()
            .and_then(|value| self.unbind(value))
            .and_then(|upstream| HeaderValue::from_bytes(&upstream).ok())
            .ok_or_else(|| Box::new(session_not_found()))?;
        headers.insert(HeaderName::from_static(MCP_SESSION_ID), upstream);
        Ok(headers)
    }

    /// Replaces an upstream session id in a response with the bound one.
    pub(super) fn outbound_response(&self, mut response: Response<Body>) -> Response<Body> {
        let values = response
            .headers()
            .get_all(MCP_SESSION_ID)
            .iter()
            .map(|value| self.bind(value.as_bytes()))
            .collect::<Vec<_>>();
        let headers = response.headers_mut();
        headers.remove(MCP_SESSION_ID);
        for value in values {
            if let Ok(value) = HeaderValue::from_str(&value) {
                headers.append(HeaderName::from_static(MCP_SESSION_ID), value);
            }
        }
        response
    }
}

fn session_not_found() -> Response<Body> {
    mcp_jsonrpc_error_response(
        StatusCode::NOT_FOUND,
        None,
        SESSION_NOT_FOUND_CODE,
        "MCP session was not found",
    )
}

#[cfg(test)]
mod tests {
    use gateway_core::{ApiKeyModelGrantMode, ApiKeyOwnerKind};
    use uuid::Uuid;

    use super::*;

    fn auth(id: Uuid) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id,
            public_id: "k".to_string(),
            name: "k".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::All,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(Uuid::nil()),
            owner_team_id: None,
            owner_service_account_id: None,
            access: Default::default(),
            oauth: None,
        }
    }

    fn server() -> ExternalMcpServerRecord {
        let now = time::OffsetDateTime::now_utc();
        ExternalMcpServerRecord {
            mcp_server_id: Uuid::new_v4(),
            server_key: "notes".to_string(),
            display_name: "Notes".to_string(),
            description: None,
            transport: gateway_core::ExternalMcpTransport::StreamableHttp,
            server_url: "http://127.0.0.1/mcp".to_string(),
            auth_mode: gateway_core::ExternalMcpAuthMode::None,
            auth_config: Default::default(),
            timeout_ms: 1_000,
            status: gateway_core::ExternalMcpServerStatus::Active,
            last_discovery_status: None,
            last_discovery_at: None,
            last_successful_discovery_at: None,
            last_error_summary: None,
            last_tool_count: None,
            created_at: now,
            updated_at: now,
            disabled_at: None,
            ownership: Default::default(),
            team_destinations: false,
            approve_tool_changes: false,
        }
    }

    #[test]
    fn a_bound_session_id_only_verifies_for_its_caller_and_server() {
        let server = server();
        let other_server = self::server();
        let (alice, bob) = (auth(Uuid::new_v4()), auth(Uuid::new_v4()));
        let issued = SessionBinding::new("secret", &alice, &server).bind(b"upstream-1");
        assert!(!issued.contains("upstream-1"));
        assert_eq!(
            SessionBinding::new("secret", &alice, &server).unbind(&issued),
            Some(b"upstream-1".to_vec())
        );
        assert_eq!(
            SessionBinding::new("secret", &bob, &server).unbind(&issued),
            None
        );
        assert_eq!(
            SessionBinding::new("secret", &alice, &other_server).unbind(&issued),
            None
        );
        assert_eq!(
            SessionBinding::new("rotated", &alice, &server).unbind(&issued),
            None
        );
        assert_eq!(
            SessionBinding::new("secret", &alice, &server).unbind("upstream-1"),
            None
        );
    }
}
