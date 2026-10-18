use gateway_core::{
    GatewayOauthClientAuthMethod, GatewayOauthClientRecord, GatewayOauthRepository,
    hash_gateway_key_secret,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;

use super::{
    McpClientOauthService,
    error::OauthError,
    redirect::validate_redirect_uri,
    secrets::{generate_token, run_argon2},
};

pub(super) const MAX_REDIRECT_URIS: usize = 10;
const MAX_TEXT_LENGTH: usize = 200;
pub(super) const MAX_URI_LENGTH: usize = 2048;
const CLIENT_SECRET_PREFIX: &str = "ocs_";
const SUPPORTED_GRANT_TYPES: [&str; 2] = ["authorization_code", "refresh_token"];

/// RFC 7591 client metadata. Unknown members are ignored, as section 2 asks.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ClientRegistrationRequest {
    pub redirect_uris: Option<Vec<String>>,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub grant_types: Option<Vec<String>>,
    pub response_types: Option<Vec<String>>,
    pub token_endpoint_auth_method: Option<String>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    /// OpenID Connect `native` or `web`. Accepted and not enforced: the
    /// redirect URI rules already apply to both.
    pub application_type: Option<String>,
}

/// RFC 7591 section 3.2.1 response.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClientRegistrationResponse {
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub client_id_issued_at: i64,
    /// `0`: the secret does not expire. Present only with a secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_expires_at: Option<i64>,
    pub redirect_uris: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<String>,
    pub grant_types: Vec<String>,
    pub response_types: Vec<String>,
    pub token_endpoint_auth_method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<String>,
}

impl<R> McpClientOauthService<R>
where
    R: GatewayOauthRepository,
{
    /// RFC 7591 open registration. A registered client can do nothing until a
    /// user approves it on the consent screen.
    pub async fn register_client(
        &self,
        request: ClientRegistrationRequest,
        now: OffsetDateTime,
    ) -> Result<ClientRegistrationResponse, OauthError> {
        let redirect_uris = request.redirect_uris.unwrap_or_default();
        if redirect_uris.is_empty() {
            return Err(OauthError::invalid_redirect_uri(
                "at least one redirect URI is required",
            ));
        }
        if redirect_uris.len() > MAX_REDIRECT_URIS {
            return Err(OauthError::invalid_redirect_uri(format!(
                "at most {MAX_REDIRECT_URIS} redirect URIs are allowed"
            )));
        }
        for redirect_uri in &redirect_uris {
            validate_redirect_uri(redirect_uri)?;
        }
        let grant_types = validate_grant_types(request.grant_types)?;
        validate_response_types(request.response_types)?;
        let auth_method = parse_auth_method(request.token_endpoint_auth_method.as_deref())?;
        let client_name = optional_text(request.client_name, "client_name")?;
        let client_uri = optional_https_uri(request.client_uri, "client_uri")?;
        let logo_uri = optional_https_uri(request.logo_uri, "logo_uri")?;
        let software_id = optional_text(request.software_id, "software_id")?;
        let software_version = optional_text(request.software_version, "software_version")?;

        let client_secret = (auth_method != GatewayOauthClientAuthMethod::None)
            .then(|| generate_token(CLIENT_SECRET_PREFIX));
        let client_secret_hash = match client_secret.clone() {
            Some(secret) => Some(
                run_argon2(move || hash_gateway_key_secret(&secret))
                    .await?
                    .map_err(OauthError::from)?,
            ),
            None => None,
        };
        let record = GatewayOauthClientRecord {
            client_id: Uuid::new_v4().to_string(),
            registration_kind: "dynamic".to_string(),
            client_name,
            client_uri,
            logo_uri,
            redirect_uris,
            grant_types,
            token_endpoint_auth_method: auth_method,
            client_secret_hash,
            software_id,
            software_version,
            created_at: now,
            updated_at: now,
        };
        self.repo.create_gateway_oauth_client(&record).await?;
        Ok(ClientRegistrationResponse {
            client_secret_expires_at: client_secret.as_ref().map(|_| 0),
            client_secret,
            client_id: record.client_id,
            client_id_issued_at: now.unix_timestamp(),
            redirect_uris: record.redirect_uris,
            client_name: record.client_name,
            client_uri: record.client_uri,
            logo_uri: record.logo_uri,
            grant_types: record.grant_types,
            response_types: vec!["code".to_string()],
            token_endpoint_auth_method: auth_method.as_str(),
            software_id: record.software_id,
            software_version: record.software_version,
        })
    }
}

/// Defaults to `authorization_code`, the only flow the gateway runs.
pub(super) fn validate_grant_types(
    grant_types: Option<Vec<String>>,
) -> Result<Vec<String>, OauthError> {
    let requested = grant_types.unwrap_or_else(|| vec!["authorization_code".to_string()]);
    let mut grant_types: Vec<String> = Vec::new();
    for grant_type in requested {
        if !grant_types.contains(&grant_type) {
            grant_types.push(grant_type);
        }
    }
    if let Some(unsupported) = grant_types
        .iter()
        .find(|grant_type| !SUPPORTED_GRANT_TYPES.contains(&grant_type.as_str()))
    {
        return Err(OauthError::invalid_client_metadata(format!(
            "grant type `{unsupported}` is not supported"
        )));
    }
    if !grant_types
        .iter()
        .any(|value| value == "authorization_code")
    {
        return Err(OauthError::invalid_client_metadata(
            "grant_types must include authorization_code",
        ));
    }
    Ok(grant_types)
}

pub(super) fn validate_response_types(
    response_types: Option<Vec<String>>,
) -> Result<(), OauthError> {
    match response_types.as_deref() {
        None => Ok(()),
        Some(values) if !values.is_empty() && values.iter().all(|value| value == "code") => Ok(()),
        Some(_) => Err(OauthError::invalid_client_metadata(
            "response_types may only contain code",
        )),
    }
}

/// RFC 7591 section 2: `client_secret_basic` when omitted.
fn parse_auth_method(raw: Option<&str>) -> Result<GatewayOauthClientAuthMethod, OauthError> {
    GatewayOauthClientAuthMethod::from_db(raw.unwrap_or("client_secret_basic")).ok_or_else(|| {
        OauthError::invalid_client_metadata(
            "token_endpoint_auth_method must be none, client_secret_post, or client_secret_basic",
        )
    })
}

pub(super) fn optional_text(
    value: Option<String>,
    field: &str,
) -> Result<Option<String>, OauthError> {
    let Some(value) = value.map(|value| value.trim().to_string()) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > MAX_TEXT_LENGTH || value.chars().any(char::is_control) {
        return Err(OauthError::invalid_client_metadata(format!(
            "{field} must be at most {MAX_TEXT_LENGTH} printable characters"
        )));
    }
    Ok(Some(value))
}

pub(super) fn optional_https_uri(
    value: Option<String>,
    field: &str,
) -> Result<Option<String>, OauthError> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let valid = value.len() <= MAX_URI_LENGTH
        && Url::parse(&value).is_ok_and(|url| url.scheme() == "https" && url.host().is_some());
    if !valid {
        return Err(OauthError::invalid_client_metadata(format!(
            "{field} must be an https URL"
        )));
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_types_default_to_the_code_flow_and_reject_others() {
        assert_eq!(
            validate_grant_types(None).unwrap(),
            vec!["authorization_code"]
        );
        assert_eq!(
            validate_grant_types(Some(vec![
                "authorization_code".into(),
                "refresh_token".into()
            ]))
            .unwrap()
            .len(),
            2
        );
        assert_eq!(
            validate_grant_types(Some(vec!["client_credentials".into()]))
                .unwrap_err()
                .error,
            "invalid_client_metadata"
        );
        assert_eq!(
            validate_grant_types(Some(vec!["refresh_token".into()]))
                .unwrap_err()
                .error,
            "invalid_client_metadata"
        );
    }

    #[test]
    fn auth_method_defaults_to_client_secret_basic() {
        assert_eq!(
            parse_auth_method(None).unwrap(),
            GatewayOauthClientAuthMethod::ClientSecretBasic
        );
        assert_eq!(
            parse_auth_method(Some("none")).unwrap(),
            GatewayOauthClientAuthMethod::None
        );
        assert!(parse_auth_method(Some("private_key_jwt")).is_err());
    }

    #[test]
    fn metadata_text_and_uris_are_bounded() {
        assert_eq!(
            optional_text(Some("  Editor ".into()), "client_name").unwrap(),
            Some("Editor".to_string())
        );
        assert!(optional_text(Some("a\nb".into()), "client_name").is_err());
        assert!(optional_text(Some("x".repeat(201)), "client_name").is_err());
        assert!(optional_https_uri(Some("http://example.com".into()), "client_uri").is_err());
        assert!(validate_response_types(Some(vec!["token".into()])).is_err());
        assert!(validate_response_types(Some(vec!["code".into()])).is_ok());
    }
}
