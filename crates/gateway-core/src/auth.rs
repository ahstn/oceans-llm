use uuid::Uuid;

use crate::error::AuthError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedGatewayApiKey {
    pub public_id: String,
    pub secret: String,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedApiKey {
    pub id: Uuid,
    pub public_id: String,
    pub name: String,
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

pub fn parse_gateway_api_key(raw: &str) -> Result<ParsedGatewayApiKey, AuthError> {
    let trimmed = raw.trim();
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
