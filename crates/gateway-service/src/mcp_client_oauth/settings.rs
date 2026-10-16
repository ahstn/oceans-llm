use std::net::IpAddr;

use gateway_core::McpOauthResource;
use time::Duration;
use url::Url;

pub const DEFAULT_ACCESS_TOKEN_TTL_SECONDS: i64 = 60 * 60;
pub const MAX_ACCESS_TOKEN_TTL_SECONDS: i64 = 24 * 60 * 60;
pub const DEFAULT_REFRESH_TOKEN_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
pub const MAX_REFRESH_TOKEN_TTL_SECONDS: i64 = 90 * 24 * 60 * 60;
pub const DEFAULT_UNUSED_CLIENT_RETENTION_DAYS: i64 = 7;
pub const MAX_UNUSED_CLIENT_RETENTION_DAYS: i64 = 365;
const MIN_TOKEN_TTL_SECONDS: i64 = 60;

/// Runtime settings for the gateway's OAuth authorization server.
///
/// The issuer is the gateway's public origin. Tokens are bound to resource URIs
/// built from it, so it is configured, never derived from request headers.
#[derive(Debug, Clone)]
pub struct McpClientOauthSettings {
    issuer: String,
    access_token_ttl: Duration,
    refresh_token_ttl: Duration,
    dynamic_client_registration: bool,
    unused_client_retention: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpClientOauthSettingsError {
    #[error("issuer must be an absolute URL: {0}")]
    InvalidIssuer(String),
    #[error("issuer must use https, or http on a loopback host")]
    InsecureIssuer,
    #[error("issuer must be an origin without user information, path, query, or fragment")]
    IssuerNotOrigin,
    #[error(
        "access_token_ttl_seconds must be between {MIN_TOKEN_TTL_SECONDS} and {MAX_ACCESS_TOKEN_TTL_SECONDS}"
    )]
    AccessTokenTtl,
    #[error(
        "refresh_token_ttl_seconds must be at least access_token_ttl_seconds and at most {MAX_REFRESH_TOKEN_TTL_SECONDS}"
    )]
    RefreshTokenTtl,
    #[error(
        "unused_client_retention_days must be between 1 and {MAX_UNUSED_CLIENT_RETENTION_DAYS}"
    )]
    UnusedClientRetention,
}

impl McpClientOauthSettings {
    pub fn new(
        issuer: &str,
        access_token_ttl_seconds: i64,
        refresh_token_ttl_seconds: i64,
        dynamic_client_registration: bool,
    ) -> Result<Self, McpClientOauthSettingsError> {
        let issuer = normalize_issuer(issuer)?;
        if !(MIN_TOKEN_TTL_SECONDS..=MAX_ACCESS_TOKEN_TTL_SECONDS)
            .contains(&access_token_ttl_seconds)
        {
            return Err(McpClientOauthSettingsError::AccessTokenTtl);
        }
        if refresh_token_ttl_seconds < access_token_ttl_seconds
            || refresh_token_ttl_seconds > MAX_REFRESH_TOKEN_TTL_SECONDS
        {
            return Err(McpClientOauthSettingsError::RefreshTokenTtl);
        }
        Ok(Self {
            issuer,
            access_token_ttl: Duration::seconds(access_token_ttl_seconds),
            refresh_token_ttl: Duration::seconds(refresh_token_ttl_seconds),
            dynamic_client_registration,
            unused_client_retention: Duration::days(DEFAULT_UNUSED_CLIENT_RETENTION_DAYS),
        })
    }

    /// How long a dynamically registered client may go without a grant
    /// before cleanup deletes it.
    pub fn with_unused_client_retention_days(
        mut self,
        days: i64,
    ) -> Result<Self, McpClientOauthSettingsError> {
        if !(1..=MAX_UNUSED_CLIENT_RETENTION_DAYS).contains(&days) {
            return Err(McpClientOauthSettingsError::UnusedClientRetention);
        }
        self.unused_client_retention = Duration::days(days);
        Ok(self)
    }

    #[must_use]
    pub fn unused_client_retention(&self) -> Duration {
        self.unused_client_retention
    }

    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    #[must_use]
    pub fn access_token_ttl(&self) -> Duration {
        self.access_token_ttl
    }

    #[must_use]
    pub fn refresh_token_ttl(&self) -> Duration {
        self.refresh_token_ttl
    }

    #[must_use]
    pub fn dynamic_client_registration(&self) -> bool {
        self.dynamic_client_registration
    }

    #[must_use]
    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.issuer)
    }

    #[must_use]
    pub fn resource_uri(&self, resource: &McpOauthResource) -> String {
        resource.uri(&self.issuer)
    }

    /// The RFC 9728 metadata URL for `resource`, using path insertion.
    #[must_use]
    pub fn protected_resource_metadata_url(&self, resource: &McpOauthResource) -> String {
        format!(
            "{}/.well-known/oauth-protected-resource{}",
            self.issuer,
            resource.path()
        )
    }

    /// Resolves an RFC 8707 `resource` value to one of the gateway's MCP
    /// resources. Scheme and host case and a trailing slash are ignored.
    #[must_use]
    pub fn parse_resource(&self, raw: &str) -> Option<McpOauthResource> {
        let url = Url::parse(raw.trim()).ok()?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.origin().ascii_serialization() != self.issuer
        {
            return None;
        }
        McpOauthResource::from_path(url.path())
    }
}

fn normalize_issuer(raw: &str) -> Result<String, McpClientOauthSettingsError> {
    let url = Url::parse(raw.trim())
        .map_err(|error| McpClientOauthSettingsError::InvalidIssuer(error.to_string()))?;
    if url.host().is_none() {
        return Err(McpClientOauthSettingsError::InvalidIssuer(
            "missing host".to_string(),
        ));
    }
    match url.scheme() {
        "https" => {}
        "http" if is_loopback_host(&url) => {}
        _ => return Err(McpClientOauthSettingsError::InsecureIssuer),
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(McpClientOauthSettingsError::IssuerNotOrigin);
    }
    Ok(url.origin().ascii_serialization())
}

pub(crate) fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
        Some(url::Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(issuer: &str) -> McpClientOauthSettings {
        McpClientOauthSettings::new(issuer, 3600, 86400, true).expect("settings")
    }

    #[test]
    fn issuer_must_be_https_or_loopback_http_origin() {
        assert_eq!(
            settings("https://Oceans.Example.com/").issuer(),
            "https://oceans.example.com"
        );
        assert_eq!(
            settings("http://127.0.0.1:38090").issuer(),
            "http://127.0.0.1:38090"
        );
        assert_eq!(
            McpClientOauthSettings::new("http://oceans.example.com", 3600, 86400, true)
                .unwrap_err(),
            McpClientOauthSettingsError::InsecureIssuer
        );
        assert_eq!(
            McpClientOauthSettings::new("https://oceans.example.com/gw", 3600, 86400, true)
                .unwrap_err(),
            McpClientOauthSettingsError::IssuerNotOrigin
        );
    }

    #[test]
    fn unused_client_retention_is_bounded() {
        let defaults = settings("https://o.example");
        assert_eq!(defaults.unused_client_retention(), Duration::days(7));
        assert_eq!(
            defaults
                .clone()
                .with_unused_client_retention_days(30)
                .unwrap()
                .unused_client_retention(),
            Duration::days(30)
        );
        assert_eq!(
            defaults.with_unused_client_retention_days(0).unwrap_err(),
            McpClientOauthSettingsError::UnusedClientRetention
        );
    }

    #[test]
    fn token_lifetimes_are_bounded() {
        assert_eq!(
            McpClientOauthSettings::new("https://o.example", 10, 86400, true).unwrap_err(),
            McpClientOauthSettingsError::AccessTokenTtl
        );
        assert_eq!(
            McpClientOauthSettings::new("https://o.example", 3600, 60, true).unwrap_err(),
            McpClientOauthSettingsError::RefreshTokenTtl
        );
    }

    #[test]
    fn resource_parsing_normalizes_case_and_trailing_slash() {
        let settings = settings("https://oceans.example.com");
        assert_eq!(
            settings.parse_resource("HTTPS://OCEANS.example.com/mcp/"),
            Some(McpOauthResource::Aggregate)
        );
        assert_eq!(
            settings.parse_resource("https://oceans.example.com/mcp/github"),
            Some(McpOauthResource::Server("github".to_string()))
        );
        assert_eq!(
            settings.parse_resource("https://oceans.example.com:443/mcp"),
            Some(McpOauthResource::Aggregate)
        );
    }

    #[test]
    fn resource_parsing_rejects_other_audiences() {
        let settings = settings("https://oceans.example.com");
        for raw in [
            "https://oceans.example.com",
            "https://oceans.example.com/v1/chat/completions",
            "https://other.example.com/mcp",
            "http://oceans.example.com/mcp",
            "https://oceans.example.com/mcp?x=1",
            "https://oceans.example.com/mcp#frag",
            "https://user@oceans.example.com/mcp",
            "mcp",
        ] {
            assert_eq!(settings.parse_resource(raw), None, "{raw}");
        }
    }

    #[test]
    fn metadata_url_uses_path_insertion() {
        let settings = settings("https://oceans.example.com");
        assert_eq!(
            settings.protected_resource_metadata_url(&McpOauthResource::Server("docs".into())),
            "https://oceans.example.com/.well-known/oauth-protected-resource/mcp/docs"
        );
    }
}
