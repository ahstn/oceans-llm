use time::OffsetDateTime;
use uuid::Uuid;

use crate::domain::{ApiKeyModelGrantMode, ApiKeyOwnerKind};
use crate::error::AuthError;
use crate::mcp_client_oauth::{GatewayAuthMethod, McpOauthResource, OAUTH_ACCESS_TOKEN_PREFIX};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedGatewayApiKey {
    pub public_id: String,
    pub secret: String,
}

/// The OAuth grant behind an MCP client `oat_` access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OauthAccessContext {
    pub grant_id: Uuid,
    pub client_id: String,
    pub resource: McpOauthResource,
    pub scopes: Vec<String>,
    /// Lookup key of the presented access token, to recheck it later.
    pub token_hash: String,
    /// When the presented access token expires.
    pub expires_at: OffsetDateTime,
}

/// The caller of a gateway request.
///
/// Usually a `gwk_` API key. On MCP routes it may instead be an OAuth access
/// token, which acts as its user: `oauth` is then set, `owner_kind` is `User`,
/// and `id` is the grant ID rather than an `api_keys` row.
#[derive(Debug, Clone)]
pub struct AuthenticatedApiKey {
    /// The `api_keys` row, or the OAuth grant ID. Use [`Self::api_key_id`]
    /// wherever an `api_keys` foreign key is needed.
    pub id: Uuid,
    pub public_id: String,
    pub name: String,
    pub model_grant_mode: ApiKeyModelGrantMode,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
    pub oauth: Option<OauthAccessContext>,
}

impl AuthenticatedApiKey {
    #[must_use]
    pub fn auth_method(&self) -> GatewayAuthMethod {
        if self.oauth.is_some() {
            GatewayAuthMethod::Oauth
        } else {
            GatewayAuthMethod::ApiKey
        }
    }

    /// The `api_keys` row, or `None` for an OAuth access token.
    #[must_use]
    pub fn api_key_id(&self) -> Option<Uuid> {
        self.oauth.is_none().then_some(self.id)
    }

    #[must_use]
    pub fn oauth_grant_id(&self) -> Option<Uuid> {
        self.oauth.as_ref().map(|oauth| oauth.grant_id)
    }

    #[must_use]
    pub fn is_user_owned(&self) -> bool {
        self.owner_kind == ApiKeyOwnerKind::User
    }

    #[must_use]
    pub fn is_service_account_owned(&self) -> bool {
        self.owner_kind == ApiKeyOwnerKind::ServiceAccount
    }
}

pub fn extract_bearer_token(header: &str) -> Result<&str, AuthError> {
    let mut parts = header.splitn(2, ' ');
    let scheme = parts
        .next()
        .ok_or(AuthError::InvalidAuthorizationHeader)?
        .trim();
    let token = parts.next().ok_or(AuthError::MissingBearerToken)?.trim();

    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return Err(AuthError::InvalidAuthorizationHeader);
    }

    Ok(token)
}

/// Whether `raw` is an MCP client OAuth access token, which only MCP routes accept.
#[must_use]
pub fn is_oauth_access_token(raw: &str) -> bool {
    raw.trim().starts_with(OAUTH_ACCESS_TOKEN_PREFIX)
}

pub fn parse_gateway_api_key(raw: &str) -> Result<ParsedGatewayApiKey, AuthError> {
    let trimmed = raw.trim();
    if is_oauth_access_token(trimmed) {
        return Err(AuthError::OauthTokenNotAccepted);
    }
    if !trimmed.starts_with("gwk_") {
        return Err(AuthError::InvalidApiKeyFormat);
    }

    let mut parts = trimmed[4..].splitn(2, '.');
    let public_id = parts
        .next()
        .ok_or(AuthError::InvalidApiKeyFormat)?
        .trim()
        .to_string();
    let secret = parts
        .next()
        .ok_or(AuthError::InvalidApiKeyFormat)?
        .trim()
        .to_string();

    if public_id.is_empty() || secret.is_empty() {
        return Err(AuthError::InvalidApiKeyFormat);
    }

    Ok(ParsedGatewayApiKey { public_id, secret })
}

#[cfg(test)]
mod tests {
    use super::{extract_bearer_token, parse_gateway_api_key};

    #[test]
    fn parses_gateway_api_key() {
        let parsed = parse_gateway_api_key("gwk_abcd1234.secret-value").expect("must parse");
        assert_eq!(parsed.public_id, "abcd1234");
        assert_eq!(parsed.secret, "secret-value");
    }

    #[test]
    fn rejects_malformed_gateway_api_key() {
        assert!(parse_gateway_api_key("badprefix_foo.bar").is_err());
        assert!(parse_gateway_api_key("gwk_foo").is_err());
        assert!(parse_gateway_api_key("gwk_.bar").is_err());
        assert!(parse_gateway_api_key("gwk_foo.").is_err());
    }

    #[test]
    fn oauth_access_tokens_are_not_api_keys() {
        assert!(matches!(
            parse_gateway_api_key("oat_abc"),
            Err(crate::error::AuthError::OauthTokenNotAccepted)
        ));
        assert!(super::is_oauth_access_token(" oat_abc"));
        assert!(!super::is_oauth_access_token("gwk_abc.def"));
    }

    #[test]
    fn extracts_bearer_token() {
        let token = extract_bearer_token("Bearer abc123").expect("must parse");
        assert_eq!(token, "abc123");
    }

    #[test]
    fn rejects_invalid_bearer_header() {
        assert!(extract_bearer_token("Basic abc123").is_err());
        assert!(extract_bearer_token("Bearer").is_err());
        assert!(extract_bearer_token(" ").is_err());
    }
}
