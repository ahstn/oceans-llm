//! Shared types for the gateway's OAuth authorization server, which MCP
//! clients use to obtain `oat_` access tokens for `/mcp` routes.
//!
//! See `docs/adr/2026-10-15-mcp-client-oauth-authorization.md`.

use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{domain::UserStatus, error::StoreError};

pub const OAUTH_ACCESS_TOKEN_PREFIX: &str = "oat_";
pub const OAUTH_REFRESH_TOKEN_PREFIX: &str = "ort_";

/// Allows the aggregate endpoint `/mcp`.
pub const SCOPE_MCP_AGGREGATE: &str = "mcp:aggregate";
/// Allows per-server routes `/mcp/{server_key}` within the token's resource.
pub const SCOPE_MCP_PROXY: &str = "mcp:proxy";
pub const SUPPORTED_SCOPES: [&str; 2] = [SCOPE_MCP_AGGREGATE, SCOPE_MCP_PROXY];

const MCP_PATH: &str = "/mcp";

/// An MCP route the gateway protects, identified by its RFC 8707 resource URI.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum McpOauthResource {
    /// `{issuer}/mcp`: the aggregate endpoint and, with `mcp:proxy`, every server.
    Aggregate,
    /// `{issuer}/mcp/{server_key}`: one per-server route.
    Server(String),
}

impl McpOauthResource {
    /// Parses a request path such as `/mcp` or `/mcp/github`.
    #[must_use]
    pub fn from_path(path: &str) -> Option<Self> {
        let path = path.strip_suffix('/').unwrap_or(path);
        if path == MCP_PATH {
            return Some(Self::Aggregate);
        }
        let server_key = path
            .strip_prefix(MCP_PATH)?
            .strip_prefix('/')?
            .to_ascii_lowercase();
        is_server_key_segment(&server_key).then_some(Self::Server(server_key))
    }

    #[must_use]
    pub fn path(&self) -> String {
        match self {
            Self::Aggregate => MCP_PATH.to_string(),
            Self::Server(server_key) => format!("{MCP_PATH}/{server_key}"),
        }
    }

    /// The canonical resource URI under `issuer`, an origin without a trailing slash.
    #[must_use]
    pub fn uri(&self, issuer: &str) -> String {
        format!("{issuer}{}", self.path())
    }

    /// The scope a token needs to call this route.
    #[must_use]
    pub fn required_scope(&self) -> &'static str {
        match self {
            Self::Aggregate => SCOPE_MCP_AGGREGATE,
            Self::Server(_) => SCOPE_MCP_PROXY,
        }
    }

    /// Whether a token issued for `self` may be presented on `route`.
    #[must_use]
    pub fn covers(&self, route: &Self) -> bool {
        match self {
            Self::Aggregate => true,
            Self::Server(_) => self == route,
        }
    }

    /// Whether `scope` may be granted for this resource.
    #[must_use]
    pub fn allows_scope(&self, scope: &str) -> bool {
        match self {
            Self::Aggregate => SUPPORTED_SCOPES.contains(&scope),
            Self::Server(_) => scope == SCOPE_MCP_PROXY,
        }
    }
}

/// Server keys are lowercase letters, digits, hyphen, and underscore.
fn is_server_key_segment(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

/// How a gateway request authenticated, recorded on MCP invocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayAuthMethod {
    ApiKey,
    Oauth,
}

impl GatewayAuthMethod {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Oauth => "oauth",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "api_key" => Some(Self::ApiKey),
            "oauth" => Some(Self::Oauth),
            _ => None,
        }
    }
}

/// Splits a space-delimited OAuth scope string, dropping duplicates and
/// keeping first-seen order. Linear in the input, since `scope` arrives on
/// unauthenticated requests.
#[must_use]
pub fn parse_scope_list(raw: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    raw.split(' ')
        .filter(|scope| !scope.is_empty() && seen.insert(*scope))
        .map(str::to_string)
        .collect()
}

/// How a client authenticates at the token and revocation endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayOauthClientAuthMethod {
    /// Public client: PKCE only.
    None,
    ClientSecretPost,
    ClientSecretBasic,
}

impl GatewayOauthClientAuthMethod {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ClientSecretPost => "client_secret_post",
            Self::ClientSecretBasic => "client_secret_basic",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "client_secret_post" => Some(Self::ClientSecretPost),
            "client_secret_basic" => Some(Self::ClientSecretBasic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayOauthClientRecord {
    pub client_id: String,
    /// Only `dynamic` (RFC 7591) today.
    pub registration_kind: String,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<String>,
    pub token_endpoint_auth_method: GatewayOauthClientAuthMethod,
    /// Argon2 hash; set only for confidential clients.
    pub client_secret_hash: Option<String>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl GatewayOauthClientRecord {
    #[must_use]
    pub fn allows_grant_type(&self, grant_type: &str) -> bool {
        self.grant_types.iter().any(|value| value == grant_type)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayOauthAuthorizationStatus {
    /// Waiting on the consent screen.
    Pending,
    /// Approved; holds a code that has not been redeemed.
    Approved,
    Denied,
    /// The code was redeemed.
    Consumed,
}

impl GatewayOauthAuthorizationStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Consumed => "consumed",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "denied" => Some(Self::Denied),
            "consumed" => Some(Self::Consumed),
            _ => None,
        }
    }
}

/// One `/oauth/authorize` call, from consent through code redemption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayOauthAuthorizationRequestRecord {
    pub request_id: Uuid,
    pub client_id: String,
    pub redirect_uri: String,
    /// Whether the client sent `redirect_uri`; the token request must then
    /// repeat it exactly (RFC 6749 section 4.1.3).
    pub redirect_uri_supplied: bool,
    pub state: Option<String>,
    pub code_challenge: String,
    /// Canonical resource URI.
    pub resource: String,
    /// Space-delimited scopes.
    pub scope: String,
    pub status: GatewayOauthAuthorizationStatus,
    pub user_id: Option<Uuid>,
    pub user_session_id: Option<Uuid>,
    pub code_hash: Option<String>,
    pub code_expires_at: Option<OffsetDateTime>,
    /// The grant created when the code was redeemed, revoked if it is replayed.
    pub grant_id: Option<Uuid>,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub decided_at: Option<OffsetDateTime>,
    pub consumed_at: Option<OffsetDateTime>,
}

/// The consent outcome written to a pending authorization request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayOauthAuthorizationDecision {
    Approved {
        user_id: Uuid,
        user_session_id: Uuid,
        code_hash: String,
        code_expires_at: OffsetDateTime,
    },
    Denied {
        user_id: Uuid,
    },
}

/// One approved consent: a token family bound to the approving admin session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayOauthGrantRecord {
    pub grant_id: Uuid,
    pub client_id: String,
    pub user_id: Uuid,
    pub user_session_id: Uuid,
    pub resource: String,
    pub scope: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
    pub revoked_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayOauthTokenKind {
    Access,
    Refresh,
}

impl GatewayOauthTokenKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Access => "access",
            Self::Refresh => "refresh",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "access" => Some(Self::Access),
            "refresh" => Some(Self::Refresh),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayOauthTokenRecord {
    /// SHA-256 hex of the raw token.
    pub token_hash: String,
    pub grant_id: Uuid,
    pub kind: GatewayOauthTokenKind,
    /// Space-delimited; never wider than the grant's scope.
    pub scope: String,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    /// Set when a refresh token is rotated.
    pub consumed_at: Option<OffsetDateTime>,
}

/// A token with everything needed to decide whether it is still valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayOauthTokenContext {
    pub token: GatewayOauthTokenRecord,
    pub grant: GatewayOauthGrantRecord,
    pub session_expires_at: OffsetDateTime,
    pub session_revoked_at: Option<OffsetDateTime>,
    pub user_status: UserStatus,
}

impl GatewayOauthTokenContext {
    /// Whether the grant, its approving session, and its user are all still live.
    #[must_use]
    pub fn grant_is_live(&self, now: OffsetDateTime) -> bool {
        self.grant.revoked_at.is_none()
            && self.session_revoked_at.is_none()
            && self.session_expires_at > now
            && self.user_status == UserStatus::Active
    }
}

/// A live grant as its user sees it: one authorized MCP client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayOauthGrantSummary {
    pub grant_id: Uuid,
    pub client_id: String,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub resource: String,
    pub scope: String,
    pub created_at: OffsetDateTime,
    /// When the client last received a token, at sign-in or refresh.
    pub last_token_issued_at: OffsetDateTime,
    /// When the client's last usable token expires. Tokens never outlive
    /// the approving session.
    pub expires_at: OffsetDateTime,
}

/// Rows changed by [`GatewayOauthRepository::purge_gateway_oauth_records`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GatewayOauthPurgeCounts {
    /// Grants marked revoked because their approving session ended.
    pub retired_grants: u64,
    pub clients: u64,
    pub authorization_requests: u64,
    pub tokens: u64,
}

/// The admin session that approves a consent, and its user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayOauthSessionState {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub expires_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
    pub user_status: UserStatus,
}

impl GatewayOauthSessionState {
    #[must_use]
    pub fn is_live(&self, now: OffsetDateTime) -> bool {
        self.revoked_at.is_none() && self.expires_at > now && self.user_status == UserStatus::Active
    }
}

#[async_trait]
pub trait GatewayOauthRepository: Send + Sync {
    async fn create_gateway_oauth_client(
        &self,
        client: &GatewayOauthClientRecord,
    ) -> Result<(), StoreError>;

    async fn get_gateway_oauth_client(
        &self,
        client_id: &str,
    ) -> Result<Option<GatewayOauthClientRecord>, StoreError>;

    async fn create_gateway_oauth_authorization_request(
        &self,
        request: &GatewayOauthAuthorizationRequestRecord,
    ) -> Result<(), StoreError>;

    async fn get_gateway_oauth_authorization_request(
        &self,
        request_id: Uuid,
    ) -> Result<Option<GatewayOauthAuthorizationRequestRecord>, StoreError>;

    async fn get_gateway_oauth_authorization_request_by_code_hash(
        &self,
        code_hash: &str,
    ) -> Result<Option<GatewayOauthAuthorizationRequestRecord>, StoreError>;

    /// Records consent on a request that is still pending and unexpired.
    /// Returns `false` when another decision won or the request expired.
    async fn decide_gateway_oauth_authorization_request(
        &self,
        request_id: Uuid,
        decision: &GatewayOauthAuthorizationDecision,
        decided_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;

    /// Marks an approved request consumed and records the grant it created.
    /// Returns `false` when the code was already redeemed.
    async fn consume_gateway_oauth_authorization_code(
        &self,
        request_id: Uuid,
        grant_id: Uuid,
        consumed_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;

    async fn create_gateway_oauth_grant(
        &self,
        grant: &GatewayOauthGrantRecord,
    ) -> Result<(), StoreError>;

    /// Stores `tokens` in one transaction: either all of them or none.
    async fn create_gateway_oauth_tokens(
        &self,
        tokens: &[GatewayOauthTokenRecord],
    ) -> Result<(), StoreError>;

    async fn get_gateway_oauth_token_context(
        &self,
        token_hash: &str,
    ) -> Result<Option<GatewayOauthTokenContext>, StoreError>;

    async fn get_gateway_oauth_session_state(
        &self,
        user_session_id: Uuid,
    ) -> Result<Option<GatewayOauthSessionState>, StoreError>;

    /// Consumes an unconsumed refresh token. Returns `false` on reuse.
    async fn consume_gateway_oauth_refresh_token(
        &self,
        token_hash: &str,
        consumed_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;

    async fn revoke_gateway_oauth_grant(
        &self,
        grant_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;

    async fn revoke_gateway_oauth_grants_for_user(
        &self,
        user_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<u64, StoreError>;

    /// The user's grants that still work at `now`: not revoked, an active
    /// user, a live approving session, and at least one unexpired, unconsumed
    /// token. This mirrors the liveness check token validation applies.
    /// With `aggregate_resource`, only grants bound to it or one of its
    /// per-server resources count, so grants from a previous issuer cannot
    /// fill the page; `None` lists grants for any issuer. Newest first, at
    /// most `limit` grants.
    async fn list_live_gateway_oauth_grants_for_user(
        &self,
        user_id: Uuid,
        aggregate_resource: Option<&str>,
        now: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<GatewayOauthGrantSummary>, StoreError>;

    /// Revokes `grant_id` only if it belongs to `user_id`.
    async fn revoke_gateway_oauth_grant_for_user(
        &self,
        grant_id: Uuid,
        user_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError>;

    /// Marks grants revoked (`session_ended`) once their approving session
    /// has been revoked or expired before `expired_before`, so dead grants
    /// leave the live-grant index. Then deletes authorization requests and
    /// tokens that expired before `expired_before`, and dynamically
    /// registered clients created before `unused_clients_before` that never
    /// received a grant and have no remaining authorization request. Grants
    /// are kept as an audit trail. Each step works in bounded batches, so a
    /// backlog never becomes one long transaction, and a run does a bounded
    /// number of batches; the next run continues where it stopped.
    ///
    /// A consumed code or refresh token is the only link from a replay to
    /// its grant, so rows of a grant that can still be live are kept: one
    /// that is not revoked and whose approving session had not ended by
    /// `expired_before`.
    async fn purge_gateway_oauth_records(
        &self,
        unused_clients_before: OffsetDateTime,
        expired_before: OffsetDateTime,
    ) -> Result<GatewayOauthPurgeCounts, StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mcp_route_paths() {
        assert_eq!(
            McpOauthResource::from_path("/mcp"),
            Some(McpOauthResource::Aggregate)
        );
        assert_eq!(
            McpOauthResource::from_path("/mcp/"),
            Some(McpOauthResource::Aggregate)
        );
        assert_eq!(
            McpOauthResource::from_path("/mcp/github"),
            Some(McpOauthResource::Server("github".to_string()))
        );
        assert_eq!(
            McpOauthResource::from_path("/mcp/GitHub"),
            Some(McpOauthResource::Server("github".to_string()))
        );
        assert_eq!(McpOauthResource::from_path("/mcp/a/b"), None);
        assert_eq!(McpOauthResource::from_path("/mcp/a.b"), None);
        assert_eq!(McpOauthResource::from_path("/mcpx"), None);
        assert_eq!(McpOauthResource::from_path("/v1/mcp"), None);
        assert_eq!(McpOauthResource::from_path("/mcp/%2e%2e"), None);
    }

    #[test]
    fn aggregate_resource_covers_every_route_but_server_resource_covers_one() {
        let aggregate = McpOauthResource::Aggregate;
        let github = McpOauthResource::Server("github".to_string());
        let docs = McpOauthResource::Server("docs".to_string());
        assert!(aggregate.covers(&aggregate));
        assert!(aggregate.covers(&github));
        assert!(github.covers(&github));
        assert!(!github.covers(&docs));
        assert!(!github.covers(&aggregate));
    }

    #[test]
    fn server_resources_only_allow_the_proxy_scope() {
        let github = McpOauthResource::Server("github".to_string());
        assert!(github.allows_scope(SCOPE_MCP_PROXY));
        assert!(!github.allows_scope(SCOPE_MCP_AGGREGATE));
        assert!(McpOauthResource::Aggregate.allows_scope(SCOPE_MCP_AGGREGATE));
        assert!(McpOauthResource::Aggregate.allows_scope(SCOPE_MCP_PROXY));
        assert!(!McpOauthResource::Aggregate.allows_scope("offline_access"));
    }

    #[test]
    fn scope_list_drops_duplicates_and_blanks() {
        assert_eq!(
            parse_scope_list("mcp:proxy  mcp:proxy mcp:aggregate"),
            vec!["mcp:proxy".to_string(), "mcp:aggregate".to_string()]
        );
    }
}
