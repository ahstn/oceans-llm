use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
};

use gateway_core::{ExternalMcpAuthMode, ExternalMcpServerRecord, GatewayError};
use gateway_mcp::McpQueryCredential;
use serde_json::{Map, Value};
use url::Url;

const DISCOVERY_SECRET_ENV_PREFIX: &str = "OCEANS_MCP_DISCOVERY_";
/// The placeholder a static header `value_template` must contain once.
pub const API_KEY_TEMPLATE_PLACEHOLDER: &str = "{{api_key}}";
/// The provider key of OAuth flows whose authorization server is configured on
/// the MCP server row instead of in `mcp.oauth.providers`.
pub const MCP_SERVER_OAUTH_PROVIDER_KEY: &str = "mcp_server";

const OAUTH_OBO_FIELDS: &[&str] = &[
    "token_exchange",
    "token_type",
    "provider_key",
    "resource",
    "resource_indicator",
    "scopes",
    "discovery_auth",
    "discovery_tool_allowlist",
    "issuer",
    "authorization_endpoint",
    "token_endpoint",
    "registration_endpoint",
    "client_registration",
    "client_id",
    "client_secret_ref",
    "token_endpoint_auth_method",
    "authorization_response_iss_parameter_supported",
];
const SERVER_OAUTH_FIELDS: &[&str] = &[
    "issuer",
    "authorization_endpoint",
    "token_endpoint",
    "registration_endpoint",
    "client_registration",
    "client_id",
    "client_secret_ref",
    "token_endpoint_auth_method",
];

/// How a client authenticates to an OAuth token endpoint (RFC 7591 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpOauthTokenEndpointAuthMethod {
    ClientSecretBasic,
    ClientSecretPost,
    None,
}

impl McpOauthTokenEndpointAuthMethod {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClientSecretBasic => "client_secret_basic",
            Self::ClientSecretPost => "client_secret_post",
            Self::None => "none",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "client_secret_basic" => Some(Self::ClientSecretBasic),
            "client_secret_post" => Some(Self::ClientSecretPost),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// How Oceans obtains its OAuth client for a server-level authorization server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpOauthClientRegistration {
    /// RFC 7591 dynamic client registration on first use.
    Dynamic,
    /// The gateway-hosted Client ID Metadata Document URL is the client ID.
    MetadataDocument,
    /// An admin-supplied client ID and optional secret reference.
    Manual {
        client_id: String,
        client_secret_ref: Option<String>,
    },
}

impl McpOauthClientRegistration {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dynamic => "dynamic",
            Self::MetadataDocument => "metadata_document",
            Self::Manual { .. } => "manual",
        }
    }
}

/// An authorization server configured on the MCP server row, usually filled in
/// from OAuth metadata discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerOauthEndpoints {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub registration: McpOauthClientRegistration,
    pub token_endpoint_auth_method: Option<McpOauthTokenEndpointAuthMethod>,
    /// RFC 9207: the authorization server always returns `iss` with the
    /// authorization response, so a response without it is refused.
    pub authorization_response_iss_parameter_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOauthServerConfig {
    /// A `mcp.oauth.providers` key, or [`MCP_SERVER_OAUTH_PROVIDER_KEY`] when
    /// `server_oauth` is set.
    pub provider_key: String,
    pub resource: String,
    /// Scopes the connection must be granted. Empty means the provider's
    /// default scopes are requested and none are enforced.
    pub scopes: Vec<String>,
    /// Whether authorization and token requests carry the RFC 8707 `resource`.
    pub resource_indicator: bool,
    pub server_oauth: Option<McpServerOauthEndpoints>,
    pub discovery_tool_allowlist: Option<BTreeSet<String>>,
}

pub fn validate_mcp_auth_config(
    auth_mode: ExternalMcpAuthMode,
    auth_config: &Map<String, Value>,
) -> Result<(), GatewayError> {
    match auth_mode {
        ExternalMcpAuthMode::None => ensure_allowed_auth_fields(auth_config, &[]),
        ExternalMcpAuthMode::GatewayStaticHeader => {
            ensure_allowed_auth_fields(
                auth_config,
                &["header_name", "value_template", "query_param", "secret_ref"],
            )?;
            static_credential_placement(auth_config)?;
            validate_secret_ref(required_secret_ref(auth_config)?)?;
            Ok(())
        }
        ExternalMcpAuthMode::GatewayBearerToken => {
            ensure_allowed_auth_fields(auth_config, &["secret_ref"])?;
            validate_secret_ref(required_secret_ref(auth_config)?)?;
            Ok(())
        }
        ExternalMcpAuthMode::UserPassthrough => {
            ensure_allowed_auth_fields(auth_config, &["header", "token_type"])
        }
        ExternalMcpAuthMode::OauthObo => {
            ensure_allowed_auth_fields(auth_config, OAUTH_OBO_FIELDS)?;
            if let Some(discovery_auth) = auth_config.get("discovery_auth")
                && discovery_auth.as_str() != Some("none")
            {
                return Err(GatewayError::InvalidRequest(
                    "auth_config.discovery_auth must be `none` when set".to_string(),
                ));
            }
            // `discovery_auth` alone keeps the legacy token-label configuration valid.
            if auth_config.keys().any(|field| {
                !matches!(
                    field.as_str(),
                    "token_exchange" | "token_type" | "discovery_auth" | "discovery_tool_allowlist"
                )
            }) {
                let _ = mcp_oauth_server_config(auth_config)?;
            }
            Ok(())
        }
    }
}

pub fn mcp_oauth_server_config(
    auth_config: &Map<String, Value>,
) -> Result<McpOauthServerConfig, GatewayError> {
    let resource = required_string(auth_config, "resource")?;
    validate_oauth_url(resource, "resource")?;
    let server_oauth = server_oauth_endpoints(auth_config)?;
    let provider_key = match (auth_config.get("provider_key"), &server_oauth) {
        (Some(_), Some(_)) => {
            return Err(GatewayError::InvalidRequest(
                "auth_config.provider_key cannot be combined with server OAuth endpoints"
                    .to_string(),
            ));
        }
        (Some(_), None) => {
            let key = required_string(auth_config, "provider_key")?;
            if key == MCP_SERVER_OAUTH_PROVIDER_KEY {
                return Err(GatewayError::InvalidRequest(format!(
                    "auth_config.provider_key `{MCP_SERVER_OAUTH_PROVIDER_KEY}` is reserved"
                )));
            }
            key.to_string()
        }
        (None, Some(_)) => MCP_SERVER_OAUTH_PROVIDER_KEY.to_string(),
        (None, None) => {
            return Err(GatewayError::InvalidRequest(
                "auth_config.provider_key or auth_config.issuer is required".to_string(),
            ));
        }
    };
    let scopes = match auth_config.get("scopes") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or_else(|| {
                GatewayError::InvalidRequest("auth_config.scopes must be an array".to_string())
            })?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .filter(|scope| {
                        !scope.trim().is_empty() && !scope.chars().any(char::is_whitespace)
                    })
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| {
                        GatewayError::InvalidRequest(
                            "auth_config.scopes must contain non-empty scope strings".to_string(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
    };
    let resource_indicator = match auth_config.get("resource_indicator") {
        None => true,
        Some(value) => value.as_bool().ok_or_else(|| {
            GatewayError::InvalidRequest(
                "auth_config.resource_indicator must be a boolean".to_string(),
            )
        })?,
    };
    let discovery_tool_allowlist = auth_config
        .get("discovery_tool_allowlist")
        .map(parse_discovery_tool_allowlist)
        .transpose()?;
    Ok(McpOauthServerConfig {
        provider_key,
        resource: resource.to_string(),
        scopes,
        resource_indicator,
        server_oauth,
        discovery_tool_allowlist,
    })
}

fn server_oauth_endpoints(
    auth_config: &Map<String, Value>,
) -> Result<Option<McpServerOauthEndpoints>, GatewayError> {
    if !SERVER_OAUTH_FIELDS
        .iter()
        .any(|field| auth_config.contains_key(*field))
    {
        return Ok(None);
    }
    let issuer = required_string(auth_config, "issuer")?;
    let authorization_endpoint = required_string(auth_config, "authorization_endpoint")?;
    let token_endpoint = required_string(auth_config, "token_endpoint")?;
    validate_oauth_url(issuer, "issuer")?;
    validate_oauth_url(authorization_endpoint, "authorization_endpoint")?;
    validate_oauth_url(token_endpoint, "token_endpoint")?;
    let registration_endpoint = optional_string(auth_config, "registration_endpoint")?;
    if let Some(endpoint) = registration_endpoint {
        validate_oauth_url(endpoint, "registration_endpoint")?;
    }
    let client_id = optional_string(auth_config, "client_id")?;
    let client_secret_ref = optional_string(auth_config, "client_secret_ref")?;
    let registration = match required_string(auth_config, "client_registration")? {
        "dynamic" => {
            if registration_endpoint.is_none() {
                return Err(GatewayError::InvalidRequest(
                    "client_registration `dynamic` requires auth_config.registration_endpoint"
                        .to_string(),
                ));
            }
            McpOauthClientRegistration::Dynamic
        }
        "metadata_document" => McpOauthClientRegistration::MetadataDocument,
        "manual" => McpOauthClientRegistration::Manual {
            client_id: client_id
                .ok_or_else(|| {
                    GatewayError::InvalidRequest(
                        "client_registration `manual` requires auth_config.client_id".to_string(),
                    )
                })?
                .to_string(),
            client_secret_ref: client_secret_ref
                .map(|secret_ref| validate_secret_ref(secret_ref).map(ToOwned::to_owned))
                .transpose()?,
        },
        other => {
            return Err(GatewayError::InvalidRequest(format!(
                "auth_config.client_registration `{other}` must be dynamic, metadata_document, or manual"
            )));
        }
    };
    if !matches!(registration, McpOauthClientRegistration::Manual { .. })
        && (client_id.is_some() || client_secret_ref.is_some())
    {
        return Err(GatewayError::InvalidRequest(
            "auth_config.client_id and client_secret_ref require client_registration `manual`"
                .to_string(),
        ));
    }
    let token_endpoint_auth_method = optional_string(auth_config, "token_endpoint_auth_method")?
        .map(|value| {
            McpOauthTokenEndpointAuthMethod::parse(value).ok_or_else(|| {
                GatewayError::InvalidRequest(format!(
                    "auth_config.token_endpoint_auth_method `{value}` must be client_secret_basic, client_secret_post, or none"
                ))
            })
        })
        .transpose()?;
    let authorization_response_iss_parameter_supported =
        match auth_config.get("authorization_response_iss_parameter_supported") {
            None => false,
            Some(value) => value.as_bool().ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "auth_config.authorization_response_iss_parameter_supported must be a boolean"
                        .to_string(),
                )
            })?,
        };
    Ok(Some(McpServerOauthEndpoints {
        issuer: issuer.to_string(),
        authorization_endpoint: authorization_endpoint.to_string(),
        token_endpoint: token_endpoint.to_string(),
        registration_endpoint: registration_endpoint.map(ToOwned::to_owned),
        registration,
        token_endpoint_auth_method,
        authorization_response_iss_parameter_supported,
    }))
}

/// OAuth URLs must use https. Plain http is accepted only for loopback hosts so
/// a local authorization server can be used in development and tests.
fn validate_oauth_url(value: &str, field: &str) -> Result<(), GatewayError> {
    let parsed = Url::parse(value).map_err(|error| {
        GatewayError::InvalidRequest(format!("auth_config.{field} is invalid: {error}"))
    })?;
    if parsed.host().is_none() || (parsed.scheme() != "https" && !is_loopback_http_url(&parsed)) {
        return Err(GatewayError::InvalidRequest(format!(
            "auth_config.{field} must be an https URL with a host"
        )));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() || parsed.fragment().is_some() {
        return Err(GatewayError::InvalidRequest(format!(
            "auth_config.{field} must not contain user information or a fragment"
        )));
    }
    Ok(())
}

fn parse_discovery_tool_allowlist(value: &Value) -> Result<BTreeSet<String>, GatewayError> {
    let items = value.as_array().ok_or_else(|| {
        GatewayError::InvalidRequest(
            "auth_config.discovery_tool_allowlist must be an array".to_string(),
        )
    })?;
    let mut allowlist = BTreeSet::new();
    for value in items {
        let tool = value
            .as_str()
            .filter(|tool| !tool.trim().is_empty() && tool.trim() == *tool)
            .ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "auth_config.discovery_tool_allowlist must contain non-empty tool names"
                        .to_string(),
                )
            })?;
        if !allowlist.insert(tool.to_string()) {
            return Err(GatewayError::InvalidRequest(
                "auth_config.discovery_tool_allowlist must not contain duplicates".to_string(),
            ));
        }
    }
    if allowlist.is_empty() {
        return Err(GatewayError::InvalidRequest(
            "auth_config.discovery_tool_allowlist cannot be empty".to_string(),
        ));
    }
    Ok(allowlist)
}

#[must_use]
pub fn supports_public_discovery(server: &ExternalMcpServerRecord) -> bool {
    server.auth_mode.supports_gateway_discovery()
        || (server.auth_mode == ExternalMcpAuthMode::OauthObo
            && server
                .auth_config
                .get("discovery_auth")
                .and_then(Value::as_str)
                == Some("none"))
}

pub fn validate_gateway_managed_server_url(
    value: &str,
    auth_mode: ExternalMcpAuthMode,
) -> Result<(), GatewayError> {
    let sends_gateway_credentials =
        auth_mode.supports_gateway_discovery() || auth_mode == ExternalMcpAuthMode::OauthObo;
    if !sends_gateway_credentials || auth_mode == ExternalMcpAuthMode::None {
        return Ok(());
    }
    let url = Url::parse(value)
        .map_err(|error| GatewayError::InvalidRequest(format!("server_url is invalid: {error}")))?;
    if url.scheme() != "https" && !is_loopback_http_url(&url) {
        return Err(GatewayError::InvalidRequest(
            "MCP credentials require an https server_url unless the host is loopback".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_mcp_server_auth_destination(
    server_url: &str,
    auth_mode: ExternalMcpAuthMode,
    auth_config: &Map<String, Value>,
) -> Result<(), GatewayError> {
    validate_gateway_managed_server_url(server_url, auth_mode)?;
    if auth_mode == ExternalMcpAuthMode::GatewayStaticHeader
        && let StaticCredentialPlacement::Query(name) = static_credential_placement(auth_config)?
    {
        let server_url = Url::parse(server_url).map_err(|error| {
            GatewayError::InvalidRequest(format!("server_url is invalid: {error}"))
        })?;
        if server_url.query_pairs().any(|(key, _)| key == name) {
            return Err(GatewayError::InvalidRequest(format!(
                "server_url must not already contain the `{name}` query parameter"
            )));
        }
    }
    if auth_mode != ExternalMcpAuthMode::OauthObo || !auth_config.contains_key("resource") {
        return Ok(());
    }

    let server_url = Url::parse(server_url)
        .map_err(|error| GatewayError::InvalidRequest(format!("server_url is invalid: {error}")))?;
    let resource = mcp_oauth_server_config(auth_config)?;
    let resource_url = Url::parse(&resource.resource).map_err(|error| {
        GatewayError::InvalidRequest(format!("auth_config.resource is invalid: {error}"))
    })?;
    if server_url.origin() != resource_url.origin() {
        return Err(GatewayError::InvalidRequest(
            "OAuth server_url must use the same origin as auth_config.resource".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn is_loopback_http_url(url: &Url) -> bool {
    if url.scheme() != "http" {
        return false;
    }
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

pub fn gateway_mcp_upstream_headers(
    server: &ExternalMcpServerRecord,
) -> Result<Option<BTreeMap<String, String>>, GatewayError> {
    validate_mcp_server_auth_destination(
        &server.server_url,
        server.auth_mode,
        &server.auth_config,
    )?;
    match server.auth_mode {
        ExternalMcpAuthMode::None => Ok(None),
        ExternalMcpAuthMode::GatewayStaticHeader => {
            match static_credential_placement(&server.auth_config)? {
                StaticCredentialPlacement::Header { name, template } => {
                    let secret = resolve_secret_ref(required_secret_ref(&server.auth_config)?)?;
                    let value = match template {
                        Some(template) => template.replace(API_KEY_TEMPLATE_PLACEHOLDER, &secret),
                        None => secret,
                    };
                    Ok(Some(BTreeMap::from([(name.to_string(), value)])))
                }
                StaticCredentialPlacement::Query(_) => Ok(None),
            }
        }
        ExternalMcpAuthMode::GatewayBearerToken => {
            let secret = resolve_secret_ref(required_secret_ref(&server.auth_config)?)?;
            Ok(Some(BTreeMap::from([(
                "Authorization".to_string(),
                format!("Bearer {secret}"),
            )])))
        }
        ExternalMcpAuthMode::UserPassthrough | ExternalMcpAuthMode::OauthObo => Ok(None),
    }
}

/// The gateway-managed API key that goes in the upstream URL query, when the
/// server's static credential uses `query_param`.
pub fn gateway_mcp_upstream_query(
    server: &ExternalMcpServerRecord,
) -> Result<Option<McpQueryCredential>, GatewayError> {
    if server.auth_mode != ExternalMcpAuthMode::GatewayStaticHeader {
        return Ok(None);
    }
    validate_mcp_server_auth_destination(
        &server.server_url,
        server.auth_mode,
        &server.auth_config,
    )?;
    match static_credential_placement(&server.auth_config)? {
        StaticCredentialPlacement::Query(name) => {
            let secret = resolve_secret_ref(required_secret_ref(&server.auth_config)?)?;
            Ok(Some(McpQueryCredential::new(name, secret)))
        }
        StaticCredentialPlacement::Header { .. } => Ok(None),
    }
}

enum StaticCredentialPlacement<'a> {
    Header {
        name: &'a str,
        template: Option<&'a str>,
    },
    Query(&'a str),
}

fn static_credential_placement(
    auth_config: &Map<String, Value>,
) -> Result<StaticCredentialPlacement<'_>, GatewayError> {
    let header_name = optional_string(auth_config, "header_name")?;
    let query_param = optional_string(auth_config, "query_param")?;
    let template = optional_string(auth_config, "value_template")?;
    match (header_name, query_param) {
        (Some(name), None) => {
            validate_static_header_name(name)?;
            if let Some(template) = template {
                validate_value_template(template)?;
            }
            Ok(StaticCredentialPlacement::Header { name, template })
        }
        (None, Some(name)) => {
            if template.is_some() {
                return Err(GatewayError::InvalidRequest(
                    "auth_config.value_template only applies to header_name".to_string(),
                ));
            }
            validate_query_param_name(name)?;
            Ok(StaticCredentialPlacement::Query(name))
        }
        _ => Err(GatewayError::InvalidRequest(
            "exactly one of auth_config.header_name or auth_config.query_param is required"
                .to_string(),
        )),
    }
}

fn validate_value_template(template: &str) -> Result<(), GatewayError> {
    if template.matches(API_KEY_TEMPLATE_PLACEHOLDER).count() != 1 {
        return Err(GatewayError::InvalidRequest(format!(
            "auth_config.value_template must contain {API_KEY_TEMPLATE_PLACEHOLDER} exactly once"
        )));
    }
    if template.chars().any(char::is_control) || template.trim() != template {
        return Err(GatewayError::InvalidRequest(
            "auth_config.value_template must not contain control characters or surrounding whitespace"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_query_param_name(name: &str) -> Result<(), GatewayError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
    {
        return Err(GatewayError::InvalidRequest(
            "auth_config.query_param must be 1-64 letters, digits, '-', '_', '.', or '~'"
                .to_string(),
        ));
    }
    Ok(())
}

pub fn normalize_mcp_server_key(value: &str) -> Result<String, GatewayError> {
    let key = value.trim().to_ascii_lowercase();
    if !(3..=64).contains(&key.len()) {
        return Err(GatewayError::InvalidRequest(
            "server_key must be 3-64 characters".to_string(),
        ));
    }
    if !key.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
    }) {
        return Err(GatewayError::InvalidRequest(
            "server_key may only contain lowercase letters, digits, hyphen, and underscore"
                .to_string(),
        ));
    }
    Ok(key)
}

fn ensure_allowed_auth_fields(
    auth_config: &Map<String, Value>,
    allowed_fields: &[&str],
) -> Result<(), GatewayError> {
    for key in auth_config.keys() {
        if !allowed_fields.contains(&key.as_str()) {
            return Err(GatewayError::InvalidRequest(format!(
                "auth_config.{key} is not allowed for this auth mode"
            )));
        }
    }
    Ok(())
}

fn required_string<'a>(
    auth_config: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, GatewayError> {
    auth_config
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| GatewayError::InvalidRequest(format!("auth_config.{field} is required")))
}

fn optional_string<'a>(
    auth_config: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, GatewayError> {
    match auth_config.get(field) {
        None => Ok(None),
        Some(_) => required_string(auth_config, field).map(Some),
    }
}

fn required_secret_ref(auth_config: &Map<String, Value>) -> Result<&str, GatewayError> {
    required_string(auth_config, "secret_ref")
}

fn validate_static_header_name(header_name: &str) -> Result<&str, GatewayError> {
    if header_name.trim() != header_name {
        return Err(GatewayError::InvalidRequest(
            "auth_config.header_name must not contain leading or trailing whitespace".to_string(),
        ));
    }
    reqwest::header::HeaderName::from_bytes(header_name.as_bytes()).map_err(|error| {
        GatewayError::InvalidRequest(format!("auth_config.header_name is invalid: {error}"))
    })?;
    Ok(header_name)
}

fn validate_secret_ref(secret_ref: &str) -> Result<&str, GatewayError> {
    if secret_ref.trim() != secret_ref {
        return Err(GatewayError::InvalidRequest(
            "auth_config.secret_ref must not contain leading or trailing whitespace".to_string(),
        ));
    }
    let env_name = secret_env_name(secret_ref)?;
    if !env_name.starts_with(DISCOVERY_SECRET_ENV_PREFIX) {
        return Err(GatewayError::InvalidRequest(format!(
            "secret_ref environment variable must start with {DISCOVERY_SECRET_ENV_PREFIX}"
        )));
    }
    Ok(secret_ref)
}

pub(crate) fn resolve_secret_ref(secret_ref: &str) -> Result<String, GatewayError> {
    validate_secret_ref(secret_ref)?;
    let env_name = secret_env_name(secret_ref)?;
    std::env::var(env_name).map_err(|_| {
        GatewayError::InvalidRequest(format!(
            "secret_ref env/{env_name} is not available for MCP use"
        ))
    })
}

fn secret_env_name(secret_ref: &str) -> Result<&str, GatewayError> {
    let env_name = secret_ref.strip_prefix("env/").ok_or_else(|| {
        GatewayError::InvalidRequest(
            "secret_ref must reference an environment variable as env/NAME".to_string(),
        )
    })?;
    if env_name.is_empty() {
        return Err(GatewayError::InvalidRequest(
            "secret_ref environment variable name cannot be empty".to_string(),
        ));
    }
    if !env_name
        .bytes()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(GatewayError::InvalidRequest(
            "secret_ref environment variable name may only contain uppercase letters, digits, and underscore".to_string(),
        ));
    }
    Ok(env_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::{ExternalMcpAuthMode, ExternalMcpServerStatus, ExternalMcpTransport};
    use serde_json::{Map, json};
    use time::OffsetDateTime;
    use uuid::Uuid;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => unsafe {
                    std::env::set_var(self.key, previous);
                },
                None => unsafe {
                    std::env::remove_var(self.key);
                },
            }
        }
    }

    #[test]
    fn gateway_managed_auth_requires_secret_refs() {
        let mode = ExternalMcpAuthMode::GatewayBearerToken;
        let mut config = Map::new();
        assert!(validate_mcp_auth_config(mode, &config).is_err());
        config.insert("secret_ref".to_string(), json!("env/MCP_TOKEN"));
        assert!(validate_mcp_auth_config(mode, &config).is_err());
        config.insert(
            "secret_ref".to_string(),
            json!("env/OCEANS_MCP_DISCOVERY_MCP_TOKEN"),
        );
        assert!(validate_mcp_auth_config(mode, &config).is_ok());
    }

    #[test]
    fn gateway_static_header_auth_validates_header_name() {
        let mode = ExternalMcpAuthMode::GatewayStaticHeader;
        let mut config = Map::from_iter([
            ("header_name".to_string(), json!(" X-Api-Key ")),
            (
                "secret_ref".to_string(),
                json!("env/OCEANS_MCP_DISCOVERY_API_KEY"),
            ),
        ]);
        assert!(validate_mcp_auth_config(mode, &config).is_err());

        config.insert("header_name".to_string(), json!("bad header"));
        assert!(validate_mcp_auth_config(mode, &config).is_err());

        config.insert("header_name".to_string(), json!("X-Api-Key"));
        assert!(validate_mcp_auth_config(mode, &config).is_ok());
    }

    #[test]
    fn upstream_header_resolver_only_returns_configured_gateway_credentials() {
        let mut config = Map::from_iter([
            ("header_name".to_string(), json!("X-Upstream-Key")),
            (
                "secret_ref".to_string(),
                json!("env/OCEANS_MCP_DISCOVERY_STATIC_HEADER_TEST_KEY"),
            ),
        ]);
        let _env_guard = EnvVarGuard::set(
            "OCEANS_MCP_DISCOVERY_STATIC_HEADER_TEST_KEY",
            "upstream-secret",
        );
        let server = server_record(ExternalMcpAuthMode::GatewayStaticHeader, config.clone());
        let headers = gateway_mcp_upstream_headers(&server)
            .expect("headers")
            .expect("some headers");
        assert_eq!(
            headers.get("X-Upstream-Key"),
            Some(&"upstream-secret".to_string())
        );
        assert!(!headers.contains_key("Authorization"));
        assert!(!headers.contains_key("x-oceans-api-key"));

        config.clear();
        let server = server_record(ExternalMcpAuthMode::None, config);
        assert!(
            gateway_mcp_upstream_headers(&server)
                .expect("headers")
                .is_none()
        );
    }

    #[test]
    fn upstream_header_resolver_revalidates_gateway_managed_https() {
        let _env_guard = EnvVarGuard::set(
            "OCEANS_MCP_DISCOVERY_BEARER_URL_TEST_KEY",
            "upstream-secret",
        );
        let mut server = server_record(
            ExternalMcpAuthMode::GatewayBearerToken,
            Map::from_iter([(
                "secret_ref".to_string(),
                json!("env/OCEANS_MCP_DISCOVERY_BEARER_URL_TEST_KEY"),
            )]),
        );
        server.server_url = "http://example.test/mcp".to_string();

        let error = gateway_mcp_upstream_headers(&server).expect_err("http must fail");
        assert_eq!(error.error_code(), "invalid_request");
    }

    #[test]
    fn oauth_obo_accepts_complete_google_config_with_public_discovery() {
        let config = Map::from_iter([
            ("provider_key".to_string(), json!("google")),
            (
                "resource".to_string(),
                json!("https://drivemcp.googleapis.com/mcp/v1"),
            ),
            (
                "scopes".to_string(),
                json!(["https://www.googleapis.com/auth/drive.readonly"]),
            ),
            ("discovery_auth".to_string(), json!("none")),
            (
                "discovery_tool_allowlist".to_string(),
                json!(["get_file_metadata", "search_files"]),
            ),
        ]);
        let mut server = server_record(ExternalMcpAuthMode::OauthObo, config.clone());
        server.server_url = "https://drivemcp.googleapis.com/mcp/v1".to_string();

        validate_mcp_auth_config(ExternalMcpAuthMode::OauthObo, &config)
            .expect("valid OAuth config");
        validate_mcp_server_auth_destination(
            &server.server_url,
            server.auth_mode,
            &server.auth_config,
        )
        .expect("matching OAuth resource origin");
        assert!(supports_public_discovery(&server));
        assert_eq!(
            mcp_oauth_server_config(&config).expect("parsed config"),
            McpOauthServerConfig {
                provider_key: "google".to_string(),
                resource: "https://drivemcp.googleapis.com/mcp/v1".to_string(),
                scopes: vec!["https://www.googleapis.com/auth/drive.readonly".to_string()],
                resource_indicator: true,
                server_oauth: None,
                discovery_tool_allowlist: Some(BTreeSet::from([
                    "get_file_metadata".to_string(),
                    "search_files".to_string(),
                ])),
            }
        );

        let mut off_resource_server = server.clone();
        off_resource_server.server_url = "https://attacker.example/mcp".to_string();
        assert!(
            validate_mcp_server_auth_destination(
                &off_resource_server.server_url,
                off_resource_server.auth_mode,
                &off_resource_server.auth_config,
            )
            .is_err()
        );
        off_resource_server.server_url = "https://drivemcp.googleapis.com/another-path".to_string();
        validate_mcp_server_auth_destination(
            &off_resource_server.server_url,
            off_resource_server.auth_mode,
            &off_resource_server.auth_config,
        )
        .expect("same-origin OAuth destination");

        let mut insecure_server = server;
        insecure_server.server_url = "http://example.test/mcp".to_string();
        assert!(
            validate_gateway_managed_server_url(
                &insecure_server.server_url,
                insecure_server.auth_mode,
            )
            .is_err()
        );
        insecure_server.server_url = "http://127.0.0.1:8080/mcp".to_string();
        validate_gateway_managed_server_url(&insecure_server.server_url, insecure_server.auth_mode)
            .expect("loopback OAuth development endpoint");
    }

    #[test]
    fn oauth_obo_rejects_incomplete_or_credentialed_discovery_config() {
        let missing_provider = Map::from_iter([(
            "resource".to_string(),
            json!("https://drivemcp.googleapis.com/mcp/v1"),
        )]);
        assert!(
            validate_mcp_auth_config(ExternalMcpAuthMode::OauthObo, &missing_provider).is_err()
        );

        let invalid_discovery = Map::from_iter([("discovery_auth".to_string(), json!("user"))]);
        assert!(
            validate_mcp_auth_config(ExternalMcpAuthMode::OauthObo, &invalid_discovery).is_err()
        );

        let insecure_resource = Map::from_iter([
            ("provider_key".to_string(), json!("google")),
            (
                "resource".to_string(),
                json!("http://drivemcp.googleapis.com/mcp/v1"),
            ),
            (
                "scopes".to_string(),
                json!(["https://www.googleapis.com/auth/drive.readonly"]),
            ),
        ]);
        assert!(
            validate_mcp_auth_config(ExternalMcpAuthMode::OauthObo, &insecure_resource).is_err()
        );

        let default_discovery = Map::from_iter([
            ("provider_key".to_string(), json!("google")),
            (
                "resource".to_string(),
                json!("https://drivemcp.googleapis.com/mcp/v1"),
            ),
            (
                "scopes".to_string(),
                json!(["https://www.googleapis.com/auth/drive.readonly"]),
            ),
        ]);
        let server = server_record(ExternalMcpAuthMode::OauthObo, default_discovery);
        assert!(!supports_public_discovery(&server));
    }

    #[test]
    fn static_header_value_templates_wrap_the_secret() {
        let _env_guard = EnvVarGuard::set("OCEANS_MCP_DISCOVERY_TEMPLATE_TEST_KEY", "abc123");
        let config = Map::from_iter([
            ("header_name".to_string(), json!("Authorization")),
            ("value_template".to_string(), json!("Token {{api_key}}")),
            (
                "secret_ref".to_string(),
                json!("env/OCEANS_MCP_DISCOVERY_TEMPLATE_TEST_KEY"),
            ),
        ]);
        validate_mcp_auth_config(ExternalMcpAuthMode::GatewayStaticHeader, &config)
            .expect("valid template");
        let server = server_record(ExternalMcpAuthMode::GatewayStaticHeader, config.clone());
        let headers = gateway_mcp_upstream_headers(&server)
            .expect("headers")
            .expect("some headers");
        assert_eq!(
            headers.get("Authorization"),
            Some(&"Token abc123".to_string())
        );
        assert!(
            gateway_mcp_upstream_query(&server)
                .expect("query")
                .is_none()
        );

        for template in [
            "Token",
            "{{api_key}} {{api_key}}",
            "Token {{api_key}}\r\n",
            " {{api_key}}",
        ] {
            let mut invalid = config.clone();
            invalid.insert("value_template".to_string(), json!(template));
            assert!(
                validate_mcp_auth_config(ExternalMcpAuthMode::GatewayStaticHeader, &invalid)
                    .is_err(),
                "accepted template {template:?}"
            );
        }
    }

    #[test]
    fn static_query_parameter_keys_stay_out_of_headers_and_debug_output() {
        let _env_guard = EnvVarGuard::set("OCEANS_MCP_DISCOVERY_QUERY_TEST_KEY", "query-secret");
        let config = Map::from_iter([
            ("query_param".to_string(), json!("api_key")),
            (
                "secret_ref".to_string(),
                json!("env/OCEANS_MCP_DISCOVERY_QUERY_TEST_KEY"),
            ),
        ]);
        validate_mcp_auth_config(ExternalMcpAuthMode::GatewayStaticHeader, &config)
            .expect("valid query config");
        let server = server_record(ExternalMcpAuthMode::GatewayStaticHeader, config.clone());

        assert!(
            gateway_mcp_upstream_headers(&server)
                .expect("headers")
                .is_none()
        );
        let credential = gateway_mcp_upstream_query(&server)
            .expect("query")
            .expect("query credential");
        assert_eq!(credential.name(), "api_key");
        let url = credential.apply(&Url::parse(&server.server_url).expect("url"));
        assert_eq!(
            url.as_str(),
            "https://example.test/mcp?api_key=query-secret"
        );
        assert!(!format!("{credential:?}").contains("query-secret"));
        // The secret never lands in the stored server URL.
        assert!(!server.server_url.contains("query-secret"));

        // A server URL that already carries the parameter would send it twice.
        assert!(
            validate_mcp_server_auth_destination(
                "https://example.test/mcp?api_key=inline",
                ExternalMcpAuthMode::GatewayStaticHeader,
                &config,
            )
            .is_err()
        );

        for invalid in [
            Map::from_iter([
                ("query_param".to_string(), json!("api key")),
                (
                    "secret_ref".to_string(),
                    json!("env/OCEANS_MCP_DISCOVERY_X"),
                ),
            ]),
            Map::from_iter([
                ("query_param".to_string(), json!("api_key")),
                ("header_name".to_string(), json!("X-Api-Key")),
                (
                    "secret_ref".to_string(),
                    json!("env/OCEANS_MCP_DISCOVERY_X"),
                ),
            ]),
            Map::from_iter([
                ("query_param".to_string(), json!("api_key")),
                ("value_template".to_string(), json!("Token {{api_key}}")),
                (
                    "secret_ref".to_string(),
                    json!("env/OCEANS_MCP_DISCOVERY_X"),
                ),
            ]),
        ] {
            assert!(
                validate_mcp_auth_config(ExternalMcpAuthMode::GatewayStaticHeader, &invalid)
                    .is_err()
            );
        }
    }

    #[test]
    fn server_level_oauth_config_names_its_authorization_server() {
        let dynamic = Map::from_iter([
            (
                "resource".to_string(),
                json!("https://mcp.linear.example/mcp"),
            ),
            ("issuer".to_string(), json!("https://auth.linear.example")),
            (
                "authorization_endpoint".to_string(),
                json!("https://auth.linear.example/authorize"),
            ),
            (
                "token_endpoint".to_string(),
                json!("https://auth.linear.example/token"),
            ),
            (
                "registration_endpoint".to_string(),
                json!("https://auth.linear.example/register"),
            ),
            ("client_registration".to_string(), json!("dynamic")),
        ]);
        let config = mcp_oauth_server_config(&dynamic).expect("dynamic config");
        assert_eq!(config.provider_key, MCP_SERVER_OAUTH_PROVIDER_KEY);
        assert!(config.scopes.is_empty());
        assert!(config.resource_indicator);
        let endpoints = config.server_oauth.expect("server OAuth");
        assert_eq!(endpoints.registration, McpOauthClientRegistration::Dynamic);
        assert_eq!(endpoints.token_endpoint_auth_method, None);

        let mut manual = dynamic.clone();
        manual.insert("client_registration".to_string(), json!("manual"));
        manual.insert("client_id".to_string(), json!("admin-client"));
        manual.insert(
            "client_secret_ref".to_string(),
            json!("env/OCEANS_MCP_DISCOVERY_LINEAR_SECRET"),
        );
        manual.insert("resource_indicator".to_string(), json!(false));
        let config = mcp_oauth_server_config(&manual).expect("manual config");
        assert!(!config.resource_indicator);
        assert_eq!(
            config.server_oauth.expect("server OAuth").registration,
            McpOauthClientRegistration::Manual {
                client_id: "admin-client".to_string(),
                client_secret_ref: Some("env/OCEANS_MCP_DISCOVERY_LINEAR_SECRET".to_string()),
            }
        );

        let invalid = [
            // Dynamic registration needs somewhere to register.
            (
                "registration_endpoint",
                None,
                Some(("client_registration", json!("dynamic"))),
            ),
            // Manual registration needs a client ID.
            (
                "client_id",
                None,
                Some(("client_registration", json!("manual"))),
            ),
            // A client ID only makes sense for manual registration.
            (
                "client_id",
                Some(json!("x")),
                Some(("client_registration", json!("dynamic"))),
            ),
            // A configured provider and a server-level server cannot both apply.
            ("provider_key", Some(json!("google")), None),
            (
                "token_endpoint",
                Some(json!("http://auth.linear.example/token")),
                None,
            ),
            ("client_registration", Some(json!("implicit")), None),
            (
                "token_endpoint_auth_method",
                Some(json!("private_key_jwt")),
                None,
            ),
        ];
        for (field, value, extra) in invalid {
            let mut config = dynamic.clone();
            match value {
                Some(value) => config.insert(field.to_string(), value),
                None => config.remove(field),
            };
            if let Some((key, value)) = extra {
                config.insert(key.to_string(), value);
            }
            assert!(
                validate_mcp_auth_config(ExternalMcpAuthMode::OauthObo, &config).is_err(),
                "accepted {field}={:?}",
                config.get(field)
            );
        }
    }

    fn server_record(
        auth_mode: ExternalMcpAuthMode,
        auth_config: Map<String, Value>,
    ) -> ExternalMcpServerRecord {
        let now = OffsetDateTime::now_utc();
        ExternalMcpServerRecord {
            team_destinations: false,
            ownership: Default::default(),
            mcp_server_id: Uuid::new_v4(),
            server_key: "github".to_string(),
            display_name: "GitHub".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://example.test/mcp".to_string(),
            auth_mode,
            auth_config,
            timeout_ms: 30_000,
            status: ExternalMcpServerStatus::Active,
            last_discovery_status: None,
            last_discovery_at: None,
            last_successful_discovery_at: None,
            last_error_summary: None,
            last_tool_count: None,
            created_at: now,
            updated_at: now,
            disabled_at: None,
            approve_tool_changes: false,
        }
    }
}
