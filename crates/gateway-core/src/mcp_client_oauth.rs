//! Shared types for the gateway's OAuth authorization server, which MCP
//! clients use to obtain `oat_` access tokens for `/mcp` routes.
//!
//! See `docs/adr/2026-10-15-mcp-client-oauth-authorization.md`.

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

/// Splits a space-delimited OAuth scope string, dropping duplicates.
#[must_use]
pub fn parse_scope_list(raw: &str) -> Vec<String> {
    let mut scopes: Vec<String> = Vec::new();
    for scope in raw.split(' ').filter(|scope| !scope.is_empty()) {
        if !scopes.iter().any(|existing| existing == scope) {
            scopes.push(scope.to_string());
        }
    }
    scopes
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
