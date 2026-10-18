//! OAuth metadata discovery and dynamic client registration for upstream MCP
//! servers.
//!
//! Discovery follows the MCP authorization flow: an unauthenticated request
//! to the server yields a `WWW-Authenticate` challenge that names the
//! protected-resource metadata (RFC 9728). That metadata names the
//! authorization server, whose metadata comes from RFC 8414 with OpenID
//! Connect discovery as a fallback.
//!
//! Every URL after the admin-entered server URL comes from the upstream, so
//! requests go through an [`OutboundFetcher`] scoped to the server's own
//! address class: a public server cannot point the gateway at private or
//! loopback addresses.

use gateway_core::GatewayError;
use reqwest::{
    Method, StatusCode,
    header::{ACCEPT, CONTENT_TYPE, WWW_AUTHENTICATE},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use url::Url;

use crate::{
    mcp_upstream_auth::{McpOauthTokenEndpointAuthMethod, McpServerOauthEndpoints},
    outbound_fetch::{OutboundFetchError, OutboundFetcher, OutboundResponse},
};

const CLIENT_NAME: &str = "Oceans LLM Gateway";

/// What OAuth metadata discovery found for an MCP server URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOauthDiscovery {
    pub server_url: String,
    /// The RFC 9728 document that was read, if the server publishes one.
    pub resource_metadata_url: Option<String>,
    pub resource: String,
    pub authorization_servers: Vec<String>,
    pub issuer: String,
    /// Where the authorization-server metadata came from.
    pub authorization_server_metadata_url: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub scopes_supported: Vec<String>,
    /// Scopes to request when the admin does not set any: the challenge's
    /// `scope`, then the resource's `scopes_supported`.
    pub default_scopes: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub client_id_metadata_document_supported: bool,
    /// RFC 9207: the authorization server returns `iss` with every
    /// authorization response.
    pub authorization_response_iss_parameter_supported: bool,
    /// `dynamic`, `metadata_document`, or `manual`.
    pub recommended_client_registration: &'static str,
    pub token_endpoint_auth_method: McpOauthTokenEndpointAuthMethod,
    /// Whether the upstream identifies itself as an RFC 8707 resource, so
    /// requests must carry the `resource` parameter.
    pub resource_indicator: bool,
    pub warnings: Vec<String>,
}

impl McpOauthDiscovery {
    /// An `oauth_obo` auth config for this server that an admin can save as
    /// is, or adjust first.
    #[must_use]
    pub fn suggested_auth_config(&self) -> Map<String, Value> {
        let mut config = Map::from_iter([
            ("resource".to_string(), json!(self.resource)),
            (
                "resource_indicator".to_string(),
                json!(self.resource_indicator),
            ),
            ("issuer".to_string(), json!(self.issuer)),
            (
                "authorization_endpoint".to_string(),
                json!(self.authorization_endpoint),
            ),
            ("token_endpoint".to_string(), json!(self.token_endpoint)),
            (
                "client_registration".to_string(),
                json!(self.recommended_client_registration),
            ),
        ]);
        if let Some(endpoint) = &self.registration_endpoint {
            config.insert("registration_endpoint".to_string(), json!(endpoint));
        }
        if self.recommended_client_registration == "dynamic" {
            config.insert(
                "token_endpoint_auth_method".to_string(),
                json!(self.token_endpoint_auth_method.as_str()),
            );
        }
        if self.recommended_client_registration == "manual" {
            config.insert("client_id".to_string(), json!(""));
        }
        if !self.default_scopes.is_empty() {
            config.insert("scopes".to_string(), json!(self.default_scopes));
        }
        if self.authorization_response_iss_parameter_supported {
            config.insert(
                "authorization_response_iss_parameter_supported".to_string(),
                json!(true),
            );
        }
        config
    }
}

/// The parameters of a `Bearer` challenge in a `WWW-Authenticate` header.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BearerChallenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
}

/// Reads the `Bearer` challenge from a `WWW-Authenticate` header value that
/// may hold several challenges, such as `Basic realm="x", Bearer scope="a"`.
#[must_use]
pub fn parse_bearer_challenge(header: &str) -> Option<BearerChallenge> {
    let mut in_bearer = false;
    let mut found = false;
    let mut challenge = BearerChallenge::default();
    for item in split_outside_quotes(header) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let param = match item.split_once(char::is_whitespace) {
            // A token before whitespace that is not a `key=value` pair starts
            // a new challenge.
            Some((scheme, rest)) if !scheme.contains('=') => {
                in_bearer = scheme.eq_ignore_ascii_case("bearer");
                found |= in_bearer;
                rest.trim()
            }
            _ if !item.contains('=') => {
                in_bearer = item.eq_ignore_ascii_case("bearer");
                found |= in_bearer;
                continue;
            }
            _ => item,
        };
        if !in_bearer {
            continue;
        }
        let Some((key, value)) = param.split_once('=') else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim().to_ascii_lowercase().as_str() {
            "resource_metadata" => challenge.resource_metadata = Some(value),
            "scope" => challenge.scope = Some(value),
            "error" => challenge.error = Some(value),
            _ => {}
        }
    }
    found.then_some(challenge)
}

fn split_outside_quotes(value: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut start = 0;
    let mut in_quotes = false;
    let mut escaped = false;
    for (index, character) in value.char_indices() {
        match character {
            _ if escaped => escaped = false,
            '\\' if in_quotes => escaped = true,
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                items.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    items.push(&value[start..]);
    items
}

fn unquote(value: &str) -> String {
    let Some(inner) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return value.to_string();
    };
    let mut output = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            if let Some(next) = chars.next() {
                output.push(next);
            }
        } else {
            output.push(character);
        }
    }
    output
}

#[derive(Debug, Deserialize)]
struct ProtectedResourceMetadata {
    resource: Option<String>,
    #[serde(default)]
    authorization_servers: Vec<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AuthorizationServerMetadata {
    issuer: Option<String>,
    authorization_endpoint: Option<String>,
    token_endpoint: Option<String>,
    registration_endpoint: Option<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
    #[serde(default)]
    response_types_supported: Vec<String>,
    token_endpoint_auth_methods_supported: Option<Vec<String>>,
    code_challenge_methods_supported: Option<Vec<String>>,
    #[serde(default)]
    client_id_metadata_document_supported: bool,
    #[serde(default)]
    authorization_response_iss_parameter_supported: bool,
}

/// Discovers how to authorize against the MCP server at `server_url`.
///
/// `http` must be scoped for `server_url`, as
/// [`crate::outbound_fetch::OutboundAddressScope::of_trusted_url`] picks it.
pub async fn discover_mcp_oauth(
    http: &OutboundFetcher,
    server_url: &str,
) -> Result<McpOauthDiscovery, GatewayError> {
    let server = Url::parse(server_url)
        .map_err(|error| GatewayError::InvalidRequest(format!("server_url is invalid: {error}")))?;
    ensure_fetchable(http, &server, "server_url")?;
    let mut warnings = Vec::new();
    let challenge = probe_challenge(http, &server).await;

    let mut resource_metadata = None;
    for candidate in protected_resource_metadata_urls(&server, challenge.as_ref())? {
        ensure_fetchable(http, &candidate, "resource_metadata")?;
        if let Some(metadata) = fetch_json::<ProtectedResourceMetadata>(http, &candidate).await? {
            resource_metadata = Some((candidate, metadata));
            break;
        }
    }

    let (resource_metadata_url, resource, authorization_servers, resource_scopes) =
        match resource_metadata {
            Some((url, metadata)) => {
                let resource = metadata.resource.ok_or_else(|| {
                    GatewayError::InvalidRequest(
                        "protected-resource metadata does not name its resource".to_string(),
                    )
                })?;
                let resource_url = Url::parse(&resource).map_err(|error| {
                    GatewayError::InvalidRequest(format!(
                        "protected-resource metadata resource is invalid: {error}"
                    ))
                })?;
                if resource_url.origin() != server.origin() {
                    return Err(GatewayError::InvalidRequest(
                        "protected-resource metadata names a resource on another origin"
                            .to_string(),
                    ));
                }
                if metadata.authorization_servers.is_empty() {
                    return Err(GatewayError::InvalidRequest(
                        "protected-resource metadata lists no authorization servers".to_string(),
                    ));
                }
                if metadata.authorization_servers.len() > 1 {
                    warnings.push(format!(
                        "The server lists {} authorization servers; Oceans uses the first.",
                        metadata.authorization_servers.len()
                    ));
                }
                (
                    Some(url.to_string()),
                    resource,
                    metadata.authorization_servers,
                    metadata.scopes_supported,
                )
            }
            None => {
                // Servers from before RFC 9728 adoption host the
                // authorization-server metadata on their own origin.
                warnings.push(
                    "The server publishes no protected-resource metadata; Oceans assumed its origin is the authorization server."
                        .to_string(),
                );
                let origin = server.origin().ascii_serialization();
                (None, server_url.to_string(), vec![origin], Vec::new())
            }
        };

    let (metadata_url, issuer, metadata) =
        fetch_authorization_server_metadata(http, &authorization_servers[0]).await?;
    let endpoint = |value: Option<String>, field: &str| -> Result<String, GatewayError> {
        let value = value.ok_or_else(|| {
            GatewayError::InvalidRequest(format!("authorization server metadata has no {field}"))
        })?;
        let parsed = Url::parse(&value).map_err(|error| {
            GatewayError::InvalidRequest(format!("{field} is invalid: {error}"))
        })?;
        ensure_fetchable(http, &parsed, field)?;
        Ok(value)
    };
    let authorization_endpoint =
        endpoint(metadata.authorization_endpoint, "authorization_endpoint")?;
    let token_endpoint = endpoint(metadata.token_endpoint, "token_endpoint")?;
    let registration_endpoint = metadata
        .registration_endpoint
        .map(|value| endpoint(Some(value), "registration_endpoint"))
        .transpose()?;
    if !metadata.response_types_supported.is_empty()
        && !metadata
            .response_types_supported
            .iter()
            .any(|value| value == "code")
    {
        return Err(GatewayError::InvalidRequest(
            "authorization server does not support the authorization code flow".to_string(),
        ));
    }
    let code_challenge_methods_supported = metadata
        .code_challenge_methods_supported
        .unwrap_or_default();
    if code_challenge_methods_supported.is_empty() {
        warnings.push(
            "The authorization server does not advertise PKCE support; Oceans still sends an S256 challenge."
                .to_string(),
        );
    } else if !code_challenge_methods_supported
        .iter()
        .any(|value| value == "S256")
    {
        return Err(GatewayError::InvalidRequest(
            "authorization server does not support S256 PKCE".to_string(),
        ));
    }
    // RFC 8414 §2: the default when the field is absent.
    let token_endpoint_auth_methods_supported = metadata
        .token_endpoint_auth_methods_supported
        .unwrap_or_else(|| vec!["client_secret_basic".to_string()]);
    let recommended_client_registration = if registration_endpoint.is_some() {
        "dynamic"
    } else if metadata.client_id_metadata_document_supported {
        "metadata_document"
    } else {
        warnings.push(
            "The authorization server supports neither dynamic client registration nor client metadata documents. Register an OAuth app with the upstream and enter its client ID."
                .to_string(),
        );
        "manual"
    };
    let default_scopes = challenge
        .as_ref()
        .and_then(|challenge| challenge.scope.as_deref())
        .map(|scope| scope.split_whitespace().map(ToOwned::to_owned).collect())
        .filter(|scopes: &Vec<String>| !scopes.is_empty())
        .unwrap_or_else(|| resource_scopes.clone());
    let scopes_supported = if resource_scopes.is_empty() {
        metadata.scopes_supported
    } else {
        resource_scopes
    };
    Ok(McpOauthDiscovery {
        server_url: server_url.to_string(),
        resource_indicator: resource_metadata_url.is_some(),
        resource_metadata_url,
        resource,
        authorization_servers,
        issuer,
        authorization_server_metadata_url: metadata_url.to_string(),
        authorization_endpoint,
        token_endpoint,
        registration_endpoint,
        scopes_supported,
        default_scopes,
        token_endpoint_auth_method: choose_token_endpoint_auth_method(
            &token_endpoint_auth_methods_supported,
        ),
        token_endpoint_auth_methods_supported,
        code_challenge_methods_supported,
        client_id_metadata_document_supported: metadata.client_id_metadata_document_supported,
        authorization_response_iss_parameter_supported: metadata
            .authorization_response_iss_parameter_supported,
        recommended_client_registration,
        warnings,
    })
}

/// Whether `iss` names the `expected` issuer: the same string, or one that
/// differs from it only by a single trailing `/`.
///
/// RFC 9207 compares issuers as exact strings, and discovery now records the
/// metadata's own issuer. Earlier discovery recorded the protected resource's
/// `authorization_servers` entry, or the server's origin, after accepting
/// metadata whose issuer differed from it only by a trailing `/`. Rows saved
/// then keep working without a re-save, which would revoke every user's
/// connection. The tolerance is no mix-up risk: both strings name the same
/// host and path, whose metadata a save checks the endpoints against, so no
/// other authorization server can issue a code that passes. It applies only
/// where the `/` ends the path: an issuer has no query or fragment (RFC 8414),
/// and a `/` at the end of one names a different identifier.
#[must_use]
pub fn issuer_matches(expected: &str, iss: &str) -> bool {
    let ends_path = |issuer: &str| !issuer.contains(['?', '#']);
    // `longer` is `shorter` plus one `/`, and `shorter` has none to lose.
    let one_slash_more = |longer: &str, shorter: &str| {
        longer.strip_suffix('/') == Some(shorter) && !shorter.ends_with('/')
    };
    expected == iss
        || (ends_path(expected)
            && ends_path(iss)
            && (one_slash_more(expected, iss) || one_slash_more(iss, expected)))
}

/// Reads the RFC 8414 metadata of the authorization server `issuer`, falling
/// back to OpenID Connect discovery, and returns where it was found and the
/// issuer exactly as the metadata states it.
async fn fetch_authorization_server_metadata(
    http: &OutboundFetcher,
    issuer: &str,
) -> Result<(Url, String, AuthorizationServerMetadata), GatewayError> {
    let issuer_url = Url::parse(issuer).map_err(|error| {
        GatewayError::InvalidRequest(format!(
            "authorization server `{issuer}` is invalid: {error}"
        ))
    })?;
    ensure_fetchable(http, &issuer_url, "authorization server")?;
    for candidate in authorization_server_metadata_urls(&issuer_url) {
        let Some(mut metadata) =
            fetch_json::<AuthorizationServerMetadata>(http, &candidate).await?
        else {
            continue;
        };
        // Callbacks compare `iss` with the metadata's own string (RFC 9207).
        let Some(published) = metadata
            .issuer
            .take()
            .filter(|published| issuer_matches(issuer, published))
        else {
            return Err(GatewayError::InvalidRequest(format!(
                "authorization server metadata does not match issuer `{issuer}`"
            )));
        };
        return Ok((candidate, published, metadata));
    }
    Err(GatewayError::InvalidRequest(format!(
        "authorization server `{issuer}` publishes no OAuth or OpenID Connect metadata"
    )))
}

/// Checks that a server's configured endpoints are the ones its issuer
/// publishes. The callback's RFC 9207 check only proves which issuer sent a
/// code, so it protects the code only while the token endpoint it goes to is
/// that issuer's own. Without this, an admin could pair an honest issuer with
/// a token endpoint they control.
pub async fn verify_published_endpoints(
    http: &OutboundFetcher,
    endpoints: &McpServerOauthEndpoints,
) -> Result<(), GatewayError> {
    let issuer = &endpoints.issuer;
    let (_, _, metadata) = fetch_authorization_server_metadata(http, issuer)
        .await
        .map_err(|error| match error {
            GatewayError::InvalidRequest(message) => GatewayError::InvalidRequest(format!(
                "auth_config cannot be checked against the issuer's metadata: {message}. An authorization server configured on the server must publish metadata; use an `oauth2` provider in gateway configuration otherwise"
            )),
            other => other,
        })?;
    let checks = [
        (
            "authorization_endpoint",
            Some(endpoints.authorization_endpoint.as_str()),
            metadata.authorization_endpoint.as_deref(),
        ),
        (
            "token_endpoint",
            Some(endpoints.token_endpoint.as_str()),
            metadata.token_endpoint.as_deref(),
        ),
        (
            "registration_endpoint",
            endpoints.registration_endpoint.as_deref(),
            metadata.registration_endpoint.as_deref(),
        ),
    ];
    for (field, configured, published) in checks {
        let Some(configured) = configured else {
            continue;
        };
        if !published.is_some_and(|published| same_url(configured, published)) {
            return Err(GatewayError::InvalidRequest(format!(
                "auth_config.{field} is not the {field} that `{issuer}` publishes; run OAuth discovery again"
            )));
        }
    }
    Ok(())
}

/// Compares parsed URLs, so `HTTPS://AS.example:443/token` and
/// `https://as.example/token` are the same endpoint.
fn same_url(left: &str, right: &str) -> bool {
    match (Url::parse(left), Url::parse(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// Prefers a confidential method, since the gateway can keep a secret.
#[must_use]
pub fn choose_token_endpoint_auth_method(supported: &[String]) -> McpOauthTokenEndpointAuthMethod {
    [
        McpOauthTokenEndpointAuthMethod::ClientSecretBasic,
        McpOauthTokenEndpointAuthMethod::ClientSecretPost,
        McpOauthTokenEndpointAuthMethod::None,
    ]
    .into_iter()
    .find(|method| supported.iter().any(|value| value == method.as_str()))
    .unwrap_or(McpOauthTokenEndpointAuthMethod::ClientSecretBasic)
}

async fn probe_challenge(http: &OutboundFetcher, server: &Url) -> Option<BearerChallenge> {
    let request = http
        .request(Method::POST, server)
        .ok()?
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": gateway_mcp::CLIENT_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "oceans-llm", "version": env!("CARGO_PKG_VERSION")}
            }
        }));
    let response = http.send(request).await.ok()?;
    if !matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return None;
    }
    response
        .headers()
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(parse_bearer_challenge)
}

fn protected_resource_metadata_urls(
    server: &Url,
    challenge: Option<&BearerChallenge>,
) -> Result<Vec<Url>, GatewayError> {
    if let Some(url) = challenge.and_then(|challenge| challenge.resource_metadata.as_deref()) {
        let url = Url::parse(url).map_err(|error| {
            GatewayError::InvalidRequest(format!("resource_metadata URL is invalid: {error}"))
        })?;
        return Ok(vec![url]);
    }
    // RFC 9728 §3.1: insert the well-known segment between the host and path.
    let path = server.path().trim_end_matches('/');
    let mut urls = Vec::new();
    if !path.is_empty() {
        urls.push(well_known_url(server, "oauth-protected-resource", path));
    }
    urls.push(well_known_url(server, "oauth-protected-resource", ""));
    Ok(urls)
}

fn authorization_server_metadata_urls(issuer: &Url) -> Vec<Url> {
    let path = issuer.path().trim_end_matches('/');
    let mut urls = vec![
        well_known_url(issuer, "oauth-authorization-server", path),
        well_known_url(issuer, "openid-configuration", path),
    ];
    if !path.is_empty() {
        let mut appended = issuer.clone();
        appended.set_path(&format!("{path}/.well-known/openid-configuration"));
        appended.set_query(None);
        urls.push(appended);
    }
    urls
}

fn well_known_url(base: &Url, name: &str, path: &str) -> Url {
    let mut url = base.clone();
    url.set_path(&format!("/.well-known/{name}{path}"));
    url.set_query(None);
    url.set_fragment(None);
    url
}

/// Metadata URLs come from the upstream, so each one must pass the
/// fetcher's own rules: https, or plain http to loopback when the MCP server
/// is on loopback too, and an address within the server's scope. Naming the
/// field tells the admin which URL was refused. The authorization endpoint
/// is never fetched, but follows the same rule.
fn ensure_fetchable(http: &OutboundFetcher, url: &Url, field: &str) -> Result<(), GatewayError> {
    http.check_url(url).map_err(|error| {
        GatewayError::InvalidRequest(format!(
            "{field} is not allowed: {error}{}",
            private_network_hint(&error)
        ))
    })
}

/// Fetches a metadata document. `Ok(None)` means it is not published there,
/// or the host could not be reached. A URL the fetcher refuses is an error,
/// so the admin learns why discovery stopped.
async fn fetch_json<T: serde::de::DeserializeOwned>(
    http: &OutboundFetcher,
    url: &Url,
) -> Result<Option<T>, GatewayError> {
    let response = match http.get(url, "application/json").await {
        Ok(response) => response,
        Err(error) if error.is_transient() => return Ok(None),
        Err(error) => return Err(fetch_error(url, &error)),
    };
    if response.status() != StatusCode::OK {
        return Ok(None);
    }
    let body = read_body(url, response).await?;
    serde_json::from_slice(&body).map(Some).map_err(|_| {
        GatewayError::InvalidRequest(format!("OAuth metadata at {url} is not valid JSON"))
    })
}

async fn read_body(url: &Url, response: OutboundResponse) -> Result<Vec<u8>, GatewayError> {
    response
        .body()
        .await
        .map_err(|error| fetch_error(url, &error))
}

fn fetch_error(url: &Url, error: &OutboundFetchError) -> GatewayError {
    GatewayError::InvalidRequest(format!(
        "OAuth metadata at {url} cannot be fetched: {error}{}",
        private_network_hint(error)
    ))
}

/// A blocked address is usually an internal server the admin has not listed
/// as private, so say how to allow it.
fn private_network_hint(error: &OutboundFetchError) -> &'static str {
    if matches!(error, OutboundFetchError::BlockedAddress { .. }) {
        ". If the MCP server runs on a private network, list its host under mcp.oauth.private_network_hosts"
    } else {
        ""
    }
}

/// A client issued by RFC 7591 dynamic client registration.
#[derive(Clone, PartialEq, Eq)]
pub struct RegisteredOauthClient {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub registration_access_token: Option<String>,
    /// Seconds since the epoch; `None` when the secret does not expire.
    pub client_secret_expires_at: Option<i64>,
    pub token_endpoint_auth_method: McpOauthTokenEndpointAuthMethod,
}

impl std::fmt::Debug for RegisteredOauthClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegisteredOauthClient")
            .field("client_id", &self.client_id)
            .field("has_client_secret", &self.client_secret.is_some())
            .field("client_secret_expires_at", &self.client_secret_expires_at)
            .field(
                "token_endpoint_auth_method",
                &self.token_endpoint_auth_method,
            )
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize)]
struct RegistrationResponse {
    client_id: Option<String>,
    client_secret: Option<String>,
    registration_access_token: Option<String>,
    client_secret_expires_at: Option<i64>,
    token_endpoint_auth_method: Option<String>,
    error: Option<String>,
}

/// Registers Oceans as an OAuth client (RFC 7591).
///
/// `http` must be scoped for the MCP server the registration is for.
pub async fn register_oauth_client(
    http: &OutboundFetcher,
    registration_endpoint: &str,
    redirect_uri: &str,
    requested_method: McpOauthTokenEndpointAuthMethod,
    scopes: &[String],
) -> Result<RegisteredOauthClient, GatewayError> {
    let mut request = json!({
        "client_name": CLIENT_NAME,
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": requested_method.as_str(),
    });
    if !scopes.is_empty() {
        request["scope"] = json!(scopes.join(" "));
    }
    let endpoint = Url::parse(registration_endpoint).map_err(|error| {
        GatewayError::InvalidRequest(format!("registration_endpoint is invalid: {error}"))
    })?;
    let registration_failed = |error: OutboundFetchError| {
        GatewayError::InvalidRequest(format!(
            "dynamic client registration failed: {error}{}",
            private_network_hint(&error)
        ))
    };
    let request = http
        .request(Method::POST, &endpoint)
        .map_err(registration_failed)?
        .header(ACCEPT, "application/json")
        .json(&request);
    let response = http.send(request).await.map_err(registration_failed)?;
    let status = response.status();
    let body = response.body().await.map_err(registration_failed)?;
    let parsed: Option<RegistrationResponse> = serde_json::from_slice(&body).ok();
    if !status.is_success() {
        // Only the RFC 7591 error code is surfaced; descriptions can echo
        // request details back.
        let code = parsed
            .and_then(|parsed| parsed.error)
            .filter(|code| {
                code.bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            })
            .map(|code| format!(" ({code})"))
            .unwrap_or_default();
        return Err(GatewayError::InvalidRequest(format!(
            "dynamic client registration was rejected with HTTP {}{code}",
            status.as_u16()
        )));
    }
    let parsed = parsed.ok_or_else(|| {
        GatewayError::InvalidRequest(
            "dynamic client registration returned an invalid response".to_string(),
        )
    })?;
    let client_id = parsed
        .client_id
        .filter(|client_id| !client_id.trim().is_empty())
        .ok_or_else(|| {
            GatewayError::InvalidRequest(
                "dynamic client registration returned no client_id".to_string(),
            )
        })?;
    let client_secret = parsed.client_secret.filter(|secret| !secret.is_empty());
    let token_endpoint_auth_method = match parsed
        .token_endpoint_auth_method
        .as_deref()
        .and_then(McpOauthTokenEndpointAuthMethod::parse)
    {
        Some(method) => method,
        // A server that issues no secret registered a public client.
        None if client_secret.is_none() => McpOauthTokenEndpointAuthMethod::None,
        None => requested_method,
    };
    if token_endpoint_auth_method != McpOauthTokenEndpointAuthMethod::None
        && client_secret.is_none()
    {
        return Err(GatewayError::InvalidRequest(
            "dynamic client registration issued a confidential client without a secret".to_string(),
        ));
    }
    Ok(RegisteredOauthClient {
        client_id,
        client_secret,
        registration_access_token: parsed.registration_access_token,
        client_secret_expires_at: parsed.client_secret_expires_at.filter(|value| *value > 0),
        token_endpoint_auth_method,
    })
}

#[cfg(test)]
mod tests;
