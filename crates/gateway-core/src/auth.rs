use uuid::Uuid;

use crate::domain::{ApiKeyModelGrantMode, ApiKeyOwnerKind};
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
    pub model_grant_mode: ApiKeyModelGrantMode,
    pub owner_kind: ApiKeyOwnerKind,
    pub owner_user_id: Option<Uuid>,
    pub owner_team_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
}

impl AuthenticatedApiKey {
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

pub const AUTHORIZATION_HEADER: &str = "authorization";
pub const X_API_KEY_HEADER: &str = "x-api-key";
pub const X_OCEANS_API_KEY_HEADER: &str = "x-oceans-api-key";
pub const X_LITELLM_API_KEY_HEADER: &str = "x-litellm-api-key";

/// The credential headers a route family accepts besides `Authorization` and
/// `x-litellm-api-key`, which every family accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialHeaderPolicy {
    /// OpenAI-compatible inference.
    Bearer,
    /// Anthropic Messages and model listings fall back to `x-api-key` when `Authorization` is
    /// absent.
    BearerOrXApiKey,
    /// MCP, batches and skills also accept `x-oceans-api-key`, which must match any other key.
    BearerOrOceansApiKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatewayCredential<'a> {
    pub token: &'a str,
    /// Lower-case name of the header that supplied the token.
    pub source: &'static str,
}

/// Picks the gateway credential from request headers. `header` looks up a lower-case header
/// name and fails when the value is not text.
///
/// `x-litellm-api-key` is the gateway credential whenever it is present, so `Authorization` and
/// `x-api-key` are ignored and may carry an unrelated upstream token. Only `x-oceans-api-key`
/// can conflict with it.
pub fn resolve_gateway_credential<'a>(
    policy: CredentialHeaderPolicy,
    header: impl Fn(&'static str) -> Result<Option<&'a str>, AuthError>,
) -> Result<Option<GatewayCredential<'a>>, AuthError> {
    let oceans = match policy {
        CredentialHeaderPolicy::BearerOrOceansApiKey => header(X_OCEANS_API_KEY_HEADER)?
            .map(non_empty_token)
            .transpose()?,
        _ => None,
    };
    let (token, source) = if let Some(raw) = header(X_LITELLM_API_KEY_HEADER)? {
        (optional_bearer_token(raw)?, X_LITELLM_API_KEY_HEADER)
    } else if let Some(raw) = header(AUTHORIZATION_HEADER)? {
        (extract_bearer_token(raw)?, AUTHORIZATION_HEADER)
    } else if let Some(token) = oceans {
        (token, X_OCEANS_API_KEY_HEADER)
    } else if policy == CredentialHeaderPolicy::BearerOrXApiKey
        && let Some(raw) = header(X_API_KEY_HEADER)?
    {
        (non_empty_token(raw)?, X_API_KEY_HEADER)
    } else {
        return Ok(None);
    };
    if oceans.is_some_and(|oceans| oceans != token) {
        return Err(AuthError::ConflictingApiKeyHeaders);
    }
    Ok(Some(GatewayCredential { token, source }))
}

fn optional_bearer_token(raw: &str) -> Result<&str, AuthError> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("bearer") {
        return Err(AuthError::MissingBearerToken);
    }
    match trimmed.split_once(' ') {
        Some((scheme, token)) if scheme.eq_ignore_ascii_case("bearer") => non_empty_token(token),
        _ => non_empty_token(trimmed),
    }
}

fn non_empty_token(raw: &str) -> Result<&str, AuthError> {
    let token = raw.trim();
    if token.is_empty() {
        return Err(AuthError::MissingBearerToken);
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
    use std::collections::BTreeMap;

    use super::{
        CredentialHeaderPolicy, GatewayCredential, extract_bearer_token, parse_gateway_api_key,
        resolve_gateway_credential,
    };
    use crate::error::AuthError;

    fn resolve(
        policy: CredentialHeaderPolicy,
        headers: &[(&'static str, &'static str)],
    ) -> Result<Option<GatewayCredential<'static>>, AuthError> {
        let headers = headers.iter().copied().collect::<BTreeMap<_, _>>();
        resolve_gateway_credential(policy, |name| Ok(headers.get(name).copied()))
    }

    #[test]
    fn litellm_key_wins_over_unrelated_authorization() {
        for value in ["gwk_a.b", "Bearer gwk_a.b", "bearer  gwk_a.b "] {
            let credential = resolve(
                CredentialHeaderPolicy::Bearer,
                &[
                    ("x-litellm-api-key", value),
                    ("authorization", "Bearer sk-ant-oat-unrelated"),
                ],
            )
            .expect("resolves")
            .expect("credential");
            assert_eq!(credential.token, "gwk_a.b");
            assert_eq!(credential.source, "x-litellm-api-key");
        }
        let credential = resolve(
            CredentialHeaderPolicy::BearerOrXApiKey,
            &[("x-litellm-api-key", "gwk_a.b"), ("x-api-key", "sk-other")],
        )
        .expect("resolves")
        .expect("credential");
        assert_eq!(credential.source, "x-litellm-api-key");
        // A malformed `Authorization` is never read when the alias is present.
        let credential = resolve(
            CredentialHeaderPolicy::Bearer,
            &[
                ("x-litellm-api-key", "gwk_a.b"),
                ("authorization", "Basic x"),
            ],
        )
        .expect("resolves")
        .expect("credential");
        assert_eq!(credential.token, "gwk_a.b");
    }

    #[test]
    fn litellm_key_conflicts_only_with_oceans_key() {
        let policy = CredentialHeaderPolicy::BearerOrOceansApiKey;
        assert!(matches!(
            resolve(
                policy,
                &[
                    ("x-litellm-api-key", "gwk_a.b"),
                    ("x-oceans-api-key", "gwk_c.d")
                ]
            ),
            Err(AuthError::ConflictingApiKeyHeaders)
        ));
        let credential = resolve(
            policy,
            &[
                ("x-litellm-api-key", "Bearer gwk_a.b"),
                ("x-oceans-api-key", "gwk_a.b"),
                ("authorization", "Bearer unrelated"),
            ],
        )
        .expect("resolves")
        .expect("credential");
        assert_eq!(credential.source, "x-litellm-api-key");
        // Routes that do not accept `x-oceans-api-key` ignore it.
        assert!(
            resolve(
                CredentialHeaderPolicy::Bearer,
                &[
                    ("x-litellm-api-key", "gwk_a.b"),
                    ("x-oceans-api-key", "gwk_c.d")
                ]
            )
            .is_ok()
        );
    }

    #[test]
    fn native_credential_headers_keep_their_rules() {
        let credential = resolve(
            CredentialHeaderPolicy::BearerOrXApiKey,
            &[
                ("authorization", "Bearer gwk_a.b"),
                ("x-api-key", "gwk_c.d"),
            ],
        )
        .expect("resolves")
        .expect("credential");
        assert_eq!(
            (credential.token, credential.source),
            ("gwk_a.b", "authorization")
        );
        let credential = resolve(
            CredentialHeaderPolicy::BearerOrXApiKey,
            &[("x-api-key", "gwk_c.d")],
        )
        .expect("resolves")
        .expect("credential");
        assert_eq!(credential.source, "x-api-key");
        assert!(matches!(
            resolve(CredentialHeaderPolicy::Bearer, &[("x-api-key", "gwk_c.d")]),
            Ok(None)
        ));
        assert!(matches!(
            resolve(
                CredentialHeaderPolicy::BearerOrOceansApiKey,
                &[
                    ("authorization", "Bearer gwk_a.b"),
                    ("x-oceans-api-key", "gwk_c.d")
                ]
            ),
            Err(AuthError::ConflictingApiKeyHeaders)
        ));
        let credential = resolve(
            CredentialHeaderPolicy::BearerOrOceansApiKey,
            &[("x-oceans-api-key", " gwk_c.d ")],
        )
        .expect("resolves")
        .expect("credential");
        assert_eq!(
            (credential.token, credential.source),
            ("gwk_c.d", "x-oceans-api-key")
        );
        assert!(matches!(
            resolve(
                CredentialHeaderPolicy::Bearer,
                &[("x-litellm-api-key", "Bearer ")]
            ),
            Err(AuthError::MissingBearerToken)
        ));
    }

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
