use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex, Weak},
    time::Duration as StdDuration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gateway_core::{
    ExternalMcpServerRecord, GatewayError, McpOauthClientRecord,
    McpUpstreamCredentialBindingRecord, McpUpstreamCredentialRepository, ProviderError,
    RefreshMcpOauthCredentialBindingRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::mcp_credentials::{decrypt_binding_secret, decrypt_credential_secret, encrypt_secret};
use crate::mcp_oauth_discovery::{McpOauthDiscovery, discover_mcp_oauth, register_oauth_client};
use crate::mcp_upstream_auth::{
    McpOauthClientRegistration, McpOauthServerConfig, McpOauthTokenEndpointAuthMethod,
    McpServerOauthEndpoints, is_loopback_http_url, mcp_oauth_server_config, resolve_secret_ref,
};
use crate::outbound_fetch::{
    DEFAULT_OUTBOUND_MAX_BODY_BYTES, DEFAULT_OUTBOUND_TIMEOUT, InvalidDomainName,
    OutboundFetchError, ScopedOutboundFetchers, normalize_domains,
};

const REFRESH_EARLY_SECONDS: i64 = 300;
const REFRESH_LEASE_SECONDS: i64 = 90;
const REFRESH_LEASE_WAIT_SECONDS: u64 = 65;
/// The access-token lifetime assumed when a refreshable grant omits `expires_in`.
const DEFAULT_EXPIRES_IN_SECONDS: i64 = 3600;
const SERVER_CALLBACK_PATH: &str = "/api/v1/mcp/oauth/callback";
const CLIENT_METADATA_DOCUMENT_PATH: &str = "/api/v1/mcp/oauth/client-metadata.json";
const TOKEN_REQUEST_TIMEOUT: StdDuration = StdDuration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpOauthProviderType {
    Google,
    /// Any OAuth 2.0 authorization server with explicit endpoints.
    Oauth2,
}

#[derive(Clone)]
pub struct McpOauthProvider {
    pub key: String,
    pub provider_type: McpOauthProviderType,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub authorization_url: String,
    pub token_url: String,
    pub token_endpoint_auth_method: McpOauthTokenEndpointAuthMethod,
    /// Requested when the MCP server row sets no scopes.
    pub scopes: Vec<String>,
}

impl std::fmt::Debug for McpOauthProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpOauthProvider")
            .field("key", &self.key)
            .field("provider_type", &self.provider_type)
            .field("client_id", &self.client_id)
            .field("authorization_url", &self.authorization_url)
            .field("token_url", &self.token_url)
            .finish_non_exhaustive()
    }
}

/// Who chose a client's token endpoint, which decides whether token requests
/// go through the SSRF guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenEndpointSource {
    /// `mcp.oauth.providers`: the gateway's own configuration.
    GatewayConfig,
    /// The MCP server row, usually from upstream metadata discovery.
    Upstream,
}

/// The OAuth client that a connection uses, from a configured provider or
/// from the authorization server on the MCP server row.
#[derive(Clone)]
pub struct McpOauthClient {
    pub provider_key: String,
    pub client_id: String,
    client_secret: Option<String>,
    pub authorization_url: String,
    pub token_url: String,
    pub token_endpoint_auth_method: McpOauthTokenEndpointAuthMethod,
    pub redirect_uri: String,
    /// The issuer bound into token bundles for server-level clients.
    pub issuer: Option<String>,
    default_scopes: Vec<String>,
    google: bool,
    token_endpoint_source: TokenEndpointSource,
    /// The URL that scopes this client's token requests, never one an
    /// upstream supplied: the admin-entered MCP server URL, or for a
    /// team-owned server the token endpoint itself, so a team cannot widen an
    /// endpoint it edited to the private networks its server's URL reaches.
    scope_url: String,
}

impl std::fmt::Debug for McpOauthClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpOauthClient")
            .field("provider_key", &self.provider_key)
            .field("client_id", &self.client_id)
            .field("token_url", &self.token_url)
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

impl McpOauthClient {
    /// The scopes to request: the server's own, or the provider defaults.
    #[must_use]
    pub fn requested_scopes(&self, config: &McpOauthServerConfig) -> Vec<String> {
        if config.scopes.is_empty() {
            self.default_scopes.clone()
        } else {
            config.scopes.clone()
        }
    }

    /// Builds the browser redirect that starts the authorization code flow.
    pub fn authorization_url(
        &self,
        config: &McpOauthServerConfig,
        state: &str,
        pkce_challenge: &str,
    ) -> Result<String, GatewayError> {
        let mut url = url::Url::parse(&self.authorization_url)
            .map_err(|error| GatewayError::Internal(error.to_string()))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("response_type", "code");
            pairs.append_pair("client_id", &self.client_id);
            pairs.append_pair("redirect_uri", &self.redirect_uri);
            let scopes = self.requested_scopes(config);
            if !scopes.is_empty() {
                pairs.append_pair("scope", &scopes.join(" "));
            }
            pairs.append_pair("state", state);
            pairs.append_pair("code_challenge", pkce_challenge);
            pairs.append_pair("code_challenge_method", "S256");
            if self.google {
                pairs.append_pair("access_type", "offline");
                pairs.append_pair("prompt", "consent");
                pairs.append_pair("include_granted_scopes", "true");
            }
            if config.resource_indicator {
                pairs.append_pair("resource", &config.resource);
            }
        }
        Ok(url.to_string())
    }

    /// Adds client authentication and the form to a token request.
    fn token_request(
        &self,
        request: reqwest::RequestBuilder,
        mut form: Vec<(&'static str, String)>,
    ) -> reqwest::RequestBuilder {
        match (self.token_endpoint_auth_method, &self.client_secret) {
            (McpOauthTokenEndpointAuthMethod::ClientSecretBasic, Some(secret)) => {
                // RFC 6749 §2.3.1: form-encode both parts before Basic encoding.
                let credentials = format!(
                    "{}:{}",
                    form_urlencode(&self.client_id),
                    form_urlencode(secret)
                );
                return request
                    .header(
                        reqwest::header::AUTHORIZATION,
                        format!("Basic {}", BASE64.encode(credentials)),
                    )
                    .form(&form);
            }
            (McpOauthTokenEndpointAuthMethod::ClientSecretPost, Some(secret)) => {
                form.push(("client_id", self.client_id.clone()));
                form.push(("client_secret", secret.clone()));
            }
            _ => form.push(("client_id", self.client_id.clone())),
        }
        request.form(&form)
    }
}

/// The token endpoint, checked where the request is sent: client secrets and
/// codes travel only over https, or plain http to this machine for local
/// development. Configuration and discovery check this too.
fn secure_token_url(raw: &str) -> Result<url::Url, GatewayError> {
    let url = url::Url::parse(raw).map_err(|error| {
        GatewayError::InvalidRequest(format!("token endpoint is invalid: {error}"))
    })?;
    if url.scheme() == "https" || is_loopback_http_url(&url) {
        Ok(url)
    } else {
        Err(GatewayError::InvalidRequest(
            "token endpoint must use https".to_string(),
        ))
    }
}

/// The URL whose host scopes requests to `endpoint`. A platform admin wrote a
/// platform-owned server's URL and stands behind every endpoint it uses. A team
/// admin may have edited a team-owned server's endpoints, which were checked
/// only when saved, so each is scoped by its own URL: a hostname reaches public
/// addresses unless listed in `mcp.oauth.private_network_hosts`.
fn endpoint_scope_url<'a>(server: &'a ExternalMcpServerRecord, endpoint: &'a str) -> &'a str {
    if server.ownership.owner_team_id.is_some() {
        endpoint
    } else {
        &server.server_url
    }
}

fn discovery_fetchers(private_network_hosts: &[String]) -> ScopedOutboundFetchers {
    ScopedOutboundFetchers::new(
        DEFAULT_OUTBOUND_TIMEOUT,
        DEFAULT_OUTBOUND_MAX_BODY_BYTES,
        private_network_hosts,
    )
}

fn token_fetchers(private_network_hosts: &[String]) -> ScopedOutboundFetchers {
    ScopedOutboundFetchers::new(
        TOKEN_REQUEST_TIMEOUT,
        DEFAULT_OUTBOUND_MAX_BODY_BYTES,
        private_network_hosts,
    )
}

fn form_urlencode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOauthTokenBundle {
    pub version: u8,
    pub access_token: String,
    /// Absent when the provider issues no refresh token; the connection then
    /// lasts until the access token expires.
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub provider_key: String,
    pub resource: String,
    pub granted_scopes: Vec<String>,
    /// The authorization server of a server-level client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
}

#[derive(Debug, Clone)]
pub struct McpOauthTokenGrant {
    pub bundle: McpOauthTokenBundle,
    /// `None` when the access token does not expire.
    pub expires_at: Option<OffsetDateTime>,
}

#[derive(Clone)]
pub struct McpOauthRuntime {
    public_base_url: Option<String>,
    providers: BTreeMap<String, McpOauthProvider>,
    /// Token requests for configured providers, whose endpoints are gateway
    /// configuration.
    client: reqwest::Client,
    /// Discovery and registration, scoped to the MCP server's address class.
    discovery_fetchers: ScopedOutboundFetchers,
    /// Token requests for server-level clients, scoped the same way.
    token_fetchers: ScopedOutboundFetchers,
    refresh_locks: Arc<Mutex<HashMap<Uuid, Weak<AsyncMutex<()>>>>>,
}

impl McpOauthRuntime {
    #[must_use]
    pub fn new(public_base_url: Option<String>, providers: Vec<McpOauthProvider>) -> Self {
        Self {
            public_base_url,
            providers: providers
                .into_iter()
                .map(|provider| (provider.key.clone(), provider))
                .collect(),
            client: reqwest::Client::builder()
                .timeout(TOKEN_REQUEST_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("default MCP OAuth HTTP client configuration must be valid"),
            discovery_fetchers: discovery_fetchers(&[]),
            token_fetchers: token_fetchers(&[]),
            refresh_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Lets servers on these hosts, and their subdomains, have discovery,
    /// registration, and token requests reach private-network addresses.
    /// Other hostnames reach public addresses only, whatever they resolve to.
    pub fn with_private_network_hosts(
        mut self,
        hosts: Vec<String>,
    ) -> Result<Self, InvalidDomainName> {
        let hosts = normalize_domains(hosts)?;
        self.discovery_fetchers = discovery_fetchers(&hosts);
        self.token_fetchers = token_fetchers(&hosts);
        Ok(self)
    }

    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.public_base_url.is_some() && !self.providers.is_empty()
    }

    /// The externally reachable gateway origin, without a trailing slash.
    #[must_use]
    pub fn public_base_url(&self) -> Option<&str> {
        self.public_base_url.as_deref()
    }

    #[must_use]
    pub fn connection_unavailable_reason(&self, provider_key: &str) -> Option<&'static str> {
        if self.public_base_url.is_none() {
            Some("public_base_url_not_configured")
        } else if !self.providers.contains_key(provider_key) {
            Some("provider_not_configured")
        } else {
            None
        }
    }

    /// Why a user cannot start a connection for this server, if they cannot.
    #[must_use]
    pub fn connection_unavailable_reason_for(
        &self,
        config: &McpOauthServerConfig,
    ) -> Option<&'static str> {
        if config.server_oauth.is_some() {
            self.public_base_url
                .is_none()
                .then_some("public_base_url_not_configured")
        } else {
            self.connection_unavailable_reason(&config.provider_key)
        }
    }

    /// The redirect URI for a server's connections. Configured providers keep
    /// their own callback path; server-level clients share one.
    pub fn callback_url_for(&self, config: &McpOauthServerConfig) -> Result<String, GatewayError> {
        if config.server_oauth.is_some() {
            Ok(format!(
                "{}{SERVER_CALLBACK_PATH}",
                self.require_public_base_url()?
            ))
        } else {
            self.callback_url(&config.provider_key)
        }
    }

    /// The gateway-hosted Client ID Metadata Document URL, which is the
    /// client ID for servers that use `metadata_document` registration.
    pub fn client_metadata_document_url(&self) -> Result<String, GatewayError> {
        Ok(format!(
            "{}{CLIENT_METADATA_DOCUMENT_PATH}",
            self.require_public_base_url()?
        ))
    }

    /// The Client ID Metadata Document served at
    /// [`Self::client_metadata_document_url`].
    #[must_use]
    pub fn client_metadata_document(&self) -> Option<serde_json::Value> {
        let base = self.public_base_url.as_deref()?;
        Some(serde_json::json!({
            "client_id": format!("{base}{CLIENT_METADATA_DOCUMENT_PATH}"),
            "client_name": "Oceans LLM Gateway",
            "client_uri": base,
            "redirect_uris": [format!("{base}{SERVER_CALLBACK_PATH}")],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }))
    }

    /// Runs OAuth metadata discovery against an MCP server URL.
    pub async fn discover(&self, server_url: &str) -> Result<McpOauthDiscovery, GatewayError> {
        let fetcher = self.discovery_fetchers.for_trusted_url(server_url);
        discover_mcp_oauth(fetcher, server_url).await
    }

    /// Sends a token request and reads the response.
    ///
    /// A server-level client's token endpoint came from upstream metadata or
    /// the server's auth config, so it goes through the SSRF guard, scoped to
    /// the admin-entered MCP server URL, or for a team-owned server to the
    /// endpoint's own URL. A configured provider's endpoint is the
    /// gateway's own configuration and uses the plain client.
    ///
    /// A failure to reach the endpoint is a [`ProviderError::Transport`]; an
    /// endpoint the guard refuses is an invalid request, since retrying will
    /// not help.
    async fn send_token_request(
        &self,
        client: &McpOauthClient,
        form: Vec<(&'static str, String)>,
    ) -> Result<(reqwest::StatusCode, Vec<u8>), GatewayError> {
        let token_url = secure_token_url(&client.token_url)?;
        if client.token_endpoint_source == TokenEndpointSource::GatewayConfig {
            let response = client
                .token_request(self.client.post(token_url), form)
                .send()
                .await
                .map_err(oauth_transport_error)?;
            let status = response.status();
            let body = response.bytes().await.map_err(oauth_transport_error)?;
            return Ok((status, body.to_vec()));
        }
        let fetcher = self.token_fetchers.for_trusted_url(&client.scope_url);
        let request = fetcher
            .request(reqwest::Method::POST, &token_url)
            .map_err(oauth_fetch_error)?;
        let response = fetcher
            .send(client.token_request(request, form))
            .await
            .map_err(oauth_fetch_error)?;
        let status = response.status();
        let body = response.body().await.map_err(oauth_fetch_error)?;
        Ok((status, body))
    }

    /// Like [`Self::discover`], but every URL the gateway fetches or will call
    /// must reach a public address. Team admins discover this way.
    pub async fn discover_public(
        &self,
        server_url: &str,
    ) -> Result<McpOauthDiscovery, GatewayError> {
        // The public scope checks and pins every address at connect time, for
        // the server and every hop the upstream names, whatever DNS returns.
        let fetcher = self
            .discovery_fetchers
            .for_scope(crate::outbound_fetch::OutboundAddressScope::Public);
        discover_mcp_oauth(fetcher, server_url).await
    }

    fn require_public_base_url(&self) -> Result<&str, GatewayError> {
        self.public_base_url.as_deref().ok_or_else(|| {
            GatewayError::InvalidRequest("mcp.oauth.public_base_url is not configured".to_string())
        })
    }

    /// Resolves the OAuth client for a server. A `dynamic` server registers a
    /// client on first use when `register` is set; refreshes pass `false` so a
    /// missing client asks the user to reconnect instead of minting a new one.
    pub async fn resolve_client<R>(
        &self,
        repo: &R,
        server: &ExternalMcpServerRecord,
        config: &McpOauthServerConfig,
        register: bool,
    ) -> Result<McpOauthClient, GatewayError>
    where
        R: McpUpstreamCredentialRepository + ?Sized,
    {
        let redirect_uri = self.callback_url_for(config)?;
        let Some(endpoints) = &config.server_oauth else {
            let provider = self.provider(&config.provider_key)?;
            return Ok(McpOauthClient {
                provider_key: provider.key.clone(),
                client_id: provider.client_id.clone(),
                client_secret: provider.client_secret.clone(),
                authorization_url: provider.authorization_url.clone(),
                token_url: provider.token_url.clone(),
                token_endpoint_auth_method: provider.token_endpoint_auth_method,
                redirect_uri,
                issuer: None,
                default_scopes: provider.scopes.clone(),
                google: provider.provider_type == McpOauthProviderType::Google,
                token_endpoint_source: TokenEndpointSource::GatewayConfig,
                scope_url: server.server_url.clone(),
            });
        };
        let (client_id, client_secret, method) = match &endpoints.registration {
            McpOauthClientRegistration::Manual {
                client_id,
                client_secret_ref,
            } => {
                let secret = client_secret_ref
                    .as_deref()
                    .map(resolve_secret_ref)
                    .transpose()?;
                let method = endpoints
                    .token_endpoint_auth_method
                    .unwrap_or(if secret.is_some() {
                        McpOauthTokenEndpointAuthMethod::ClientSecretBasic
                    } else {
                        McpOauthTokenEndpointAuthMethod::None
                    });
                (client_id.clone(), secret, method)
            }
            McpOauthClientRegistration::MetadataDocument => (
                self.client_metadata_document_url()?,
                None,
                McpOauthTokenEndpointAuthMethod::None,
            ),
            McpOauthClientRegistration::Dynamic => {
                let registered = self
                    .registered_client(repo, server, config, endpoints, &redirect_uri, register)
                    .await?;
                (
                    registered.client_id,
                    registered.client_secret,
                    registered.method,
                )
            }
        };
        Ok(McpOauthClient {
            provider_key: config.provider_key.clone(),
            client_id,
            client_secret,
            authorization_url: endpoints.authorization_endpoint.clone(),
            token_url: endpoints.token_endpoint.clone(),
            token_endpoint_auth_method: method,
            redirect_uri,
            issuer: Some(endpoints.issuer.clone()),
            default_scopes: Vec::new(),
            google: false,
            token_endpoint_source: TokenEndpointSource::Upstream,
            scope_url: endpoint_scope_url(server, &endpoints.token_endpoint).to_string(),
        })
    }

    async fn registered_client<R>(
        &self,
        repo: &R,
        server: &ExternalMcpServerRecord,
        config: &McpOauthServerConfig,
        endpoints: &McpServerOauthEndpoints,
        redirect_uri: &str,
        register: bool,
    ) -> Result<StoredClient, GatewayError>
    where
        R: McpUpstreamCredentialRepository + ?Sized,
    {
        let mcp_server_id = server.mcp_server_id;
        let now = OffsetDateTime::now_utc();
        if let Some(stored) = repo.get_mcp_oauth_client(mcp_server_id).await? {
            let expired = stored
                .client_secret_expires_at
                .is_some_and(|expires_at| expires_at <= now);
            if stored.issuer == endpoints.issuer && stored.redirect_uri == redirect_uri && !expired
            {
                return StoredClient::decrypt(stored);
            }
            if !register {
                return Err(GatewayError::McpCredentialRequired {
                    server_key: String::new(),
                });
            }
            // The authorization server or callback changed, or the secret
            // expired: replace the stale registration.
            repo.delete_mcp_oauth_client(mcp_server_id, &stored.client_id)
                .await?;
        } else if !register {
            return Err(GatewayError::McpCredentialRequired {
                server_key: String::new(),
            });
        }
        let registration_endpoint =
            endpoints.registration_endpoint.as_deref().ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "dynamic client registration requires a registration_endpoint".to_string(),
                )
            })?;
        let registered = register_oauth_client(
            self.discovery_fetchers
                .for_trusted_url(endpoint_scope_url(server, registration_endpoint)),
            registration_endpoint,
            redirect_uri,
            endpoints
                .token_endpoint_auth_method
                .unwrap_or(McpOauthTokenEndpointAuthMethod::ClientSecretBasic),
            &config.scopes,
        )
        .await?;
        let encrypted = if registered.client_secret.is_some()
            || registered.registration_access_token.is_some()
        {
            let secrets = serde_json::to_string(&StoredClientSecrets {
                client_secret: registered.client_secret.clone(),
                registration_access_token: registered.registration_access_token.clone(),
            })
            .map_err(|error| GatewayError::Internal(error.to_string()))?;
            Some(encrypt_secret(&secrets)?)
        } else {
            None
        };
        let stored = repo
            .insert_mcp_oauth_client_if_absent(&McpOauthClientRecord {
                mcp_server_id,
                issuer: endpoints.issuer.clone(),
                redirect_uri: redirect_uri.to_string(),
                client_id: registered.client_id,
                token_endpoint_auth_method: registered
                    .token_endpoint_auth_method
                    .as_str()
                    .to_string(),
                secret_ciphertext: encrypted.as_ref().map(|value| value.ciphertext.clone()),
                secret_nonce: encrypted.as_ref().map(|value| value.nonce.clone()),
                secret_key_id: encrypted.as_ref().map(|value| value.key_id.to_string()),
                client_secret_expires_at: registered
                    .client_secret_expires_at
                    .and_then(|value| OffsetDateTime::from_unix_timestamp(value).ok()),
                created_at: now,
                updated_at: now,
            })
            .await?;
        // A concurrent registration may have won; use whichever row is stored.
        StoredClient::decrypt(stored)
    }

    pub fn provider(&self, key: &str) -> Result<&McpOauthProvider, GatewayError> {
        self.providers.get(key).ok_or_else(|| {
            GatewayError::InvalidRequest(format!("MCP OAuth provider `{key}` is not configured"))
        })
    }

    pub fn callback_url(&self, provider_key: &str) -> Result<String, GatewayError> {
        let base = self.public_base_url.as_deref().ok_or_else(|| {
            GatewayError::InvalidRequest("mcp.oauth.public_base_url is not configured".to_string())
        })?;
        Ok(format!("{base}/api/v1/mcp/oauth/{provider_key}/callback"))
    }

    pub async fn exchange_code(
        &self,
        client: &McpOauthClient,
        config: &McpOauthServerConfig,
        code: &str,
        pkce_verifier: &str,
        required_scopes: &[String],
    ) -> Result<McpOauthTokenGrant, GatewayError> {
        let mut form = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code.to_string()),
            ("redirect_uri", client.redirect_uri.clone()),
            ("code_verifier", pkce_verifier.to_string()),
        ];
        if config.resource_indicator {
            form.push(("resource", config.resource.clone()));
        }
        let (status, body) = self.send_token_request(client, form).await?;
        let mut grant = parse_token_response(
            status,
            &body,
            &client.provider_key,
            &config.resource,
            required_scopes,
            None,
            client.google,
        )?;
        grant.bundle.issuer.clone_from(&client.issuer);
        Ok(grant)
    }

    pub async fn refresh_binding<R>(
        &self,
        repo: &R,
        binding: McpUpstreamCredentialBindingRecord,
        server: &ExternalMcpServerRecord,
    ) -> Result<McpUpstreamCredentialBindingRecord, GatewayError>
    where
        R: McpUpstreamCredentialRepository + ?Sized,
    {
        let lock = {
            let mut locks = self.refresh_locks.lock().map_err(|_| {
                GatewayError::Internal("MCP OAuth refresh lock was poisoned".to_string())
            })?;
            locks.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = locks
                .get(&binding.credential_binding_id)
                .and_then(Weak::upgrade)
            {
                lock
            } else {
                let lock = Arc::new(AsyncMutex::new(()));
                locks.insert(binding.credential_binding_id, Arc::downgrade(&lock));
                lock
            }
        };
        let _guard = match lock.try_lock() {
            Ok(guard) => guard,
            Err(_) if has_unexpired_access_token(&binding) => return Ok(binding),
            Err(_) => lock.lock().await,
        };
        let (current, lease_token) = match acquire_refresh_lease(repo, &binding, server).await? {
            RefreshLease::Current(current) => return Ok(current),
            RefreshLease::Acquired {
                binding,
                lease_token,
            } => (binding, lease_token),
        };
        let leased_binding_id = current.credential_binding_id;
        let result = self.refresh_binding_with_lease(repo, current, server).await;
        if let Err(error) = repo
            .release_mcp_oauth_refresh_lease(leased_binding_id, lease_token)
            .await
        {
            tracing::warn!(
                credential_binding_id = %leased_binding_id,
                error = %error,
                "failed to release MCP OAuth refresh lease"
            );
        }
        result
    }

    async fn refresh_binding_with_lease<R>(
        &self,
        repo: &R,
        current: McpUpstreamCredentialBindingRecord,
        server: &ExternalMcpServerRecord,
    ) -> Result<McpUpstreamCredentialBindingRecord, GatewayError>
    where
        R: McpUpstreamCredentialRepository + ?Sized,
    {
        let raw =
            decrypt_binding_secret(&current).map_err(|_| GatewayError::McpCredentialRequired {
                server_key: server.server_key.clone(),
            })?;
        let bundle: McpOauthTokenBundle =
            serde_json::from_str(&raw).map_err(|_| GatewayError::McpCredentialExpired {
                server_key: server.server_key.clone(),
            })?;
        validate_oauth_bundle_for_server(&bundle, server)?;
        let Some(refresh_token) = bundle.refresh_token.clone() else {
            // Without a refresh token the user must connect again once the
            // access token expires.
            if has_unexpired_access_token(&current) {
                return Ok(current);
            }
            return Err(GatewayError::McpCredentialExpired {
                server_key: server.server_key.clone(),
            });
        };
        let config = mcp_oauth_server_config(&server.auth_config)
            .map_err(|_| credential_required(server))?;
        let client = self
            .resolve_client(repo, server, &config, false)
            .await
            .map_err(|error| match error {
                GatewayError::McpCredentialRequired { .. } => credential_required(server),
                error => error,
            })?;
        let mut form = vec![
            ("grant_type", "refresh_token".to_string()),
            ("refresh_token", refresh_token.clone()),
        ];
        if config.resource_indicator {
            form.push(("resource", bundle.resource.clone()));
        }
        let (status, body) = match self.send_token_request(&client, form).await {
            Ok(response) => response,
            // Only an unreachable endpoint keeps the current token; a refused
            // one is a configuration problem and surfaces as one.
            Err(error) if is_transport_error(&error) && has_unexpired_access_token(&current) => {
                tracing::warn!(error = %error, "using unexpired access token after OAuth refresh transport failure");
                return Ok(current);
            }
            Err(error) => return Err(error),
        };
        if !status.is_success() {
            let error_code = serde_json::from_slice::<TokenResponse>(&body)
                .ok()
                .and_then(|parsed| parsed.error);
            if error_code.as_deref() == Some("invalid_grant") {
                let expected_ciphertext = current
                    .secret_ciphertext
                    .as_deref()
                    .ok_or_else(|| credential_required(server))?;
                if repo
                    .revoke_mcp_oauth_credential_if_unchanged(
                        current.credential_binding_id,
                        expected_ciphertext,
                        OffsetDateTime::now_utc(),
                    )
                    .await?
                {
                    return Err(credential_required(server));
                }
                return current_binding_after_refresh_race(repo, &current, server).await;
            }
            if is_transient_oauth_status(status) && has_unexpired_access_token(&current) {
                tracing::warn!(
                    status = status.as_u16(),
                    "using unexpired access token after transient OAuth refresh failure"
                );
                return Ok(current);
            }
            return Err(oauth_endpoint_error(status));
        }
        let mut grant = parse_token_response(
            status,
            &body,
            &bundle.provider_key,
            &bundle.resource,
            &bundle.granted_scopes,
            Some(refresh_token),
            client.google,
        )?;
        grant.bundle.issuer = bundle.issuer;
        persist_refreshed_binding(repo, current, grant, server).await
    }
}

pub(crate) fn validate_oauth_bundle_for_server(
    bundle: &McpOauthTokenBundle,
    server: &ExternalMcpServerRecord,
) -> Result<(), GatewayError> {
    let config =
        mcp_oauth_server_config(&server.auth_config).map_err(|_| credential_required(server))?;
    let issuer = config
        .server_oauth
        .as_ref()
        .map(|endpoints| endpoints.issuer.as_str());
    if bundle.provider_key != config.provider_key
        || bundle.resource != config.resource
        || (issuer.is_some() && bundle.issuer.as_deref() != issuer)
    {
        return Err(credential_required(server));
    }
    ensure_required_scopes(&config.scopes, &bundle.granted_scopes)
        .map_err(|_| credential_required(server))
}

struct StoredClient {
    client_id: String,
    client_secret: Option<String>,
    method: McpOauthTokenEndpointAuthMethod,
}

#[derive(Serialize, Deserialize)]
struct StoredClientSecrets {
    client_secret: Option<String>,
    registration_access_token: Option<String>,
}

impl StoredClient {
    fn decrypt(record: McpOauthClientRecord) -> Result<Self, GatewayError> {
        let method = McpOauthTokenEndpointAuthMethod::parse(&record.token_endpoint_auth_method)
            .ok_or_else(|| {
                GatewayError::Internal("stored MCP OAuth client has an unknown auth method".into())
            })?;
        let client_secret = match (
            record.secret_ciphertext.as_deref(),
            record.secret_nonce.as_deref(),
            record.secret_key_id.as_deref(),
        ) {
            (Some(ciphertext), Some(nonce), Some(key_id)) => {
                let secrets: StoredClientSecrets =
                    serde_json::from_str(&decrypt_credential_secret(ciphertext, nonce, key_id)?)
                        .map_err(|error| GatewayError::Internal(error.to_string()))?;
                secrets.client_secret
            }
            _ => None,
        };
        Ok(Self {
            client_id: record.client_id,
            client_secret,
            method,
        })
    }
}

fn credential_required(server: &ExternalMcpServerRecord) -> GatewayError {
    GatewayError::McpCredentialRequired {
        server_key: server.server_key.clone(),
    }
}

enum RefreshLease {
    Current(McpUpstreamCredentialBindingRecord),
    Acquired {
        binding: McpUpstreamCredentialBindingRecord,
        lease_token: Uuid,
    },
}

async fn acquire_refresh_lease<R>(
    repo: &R,
    original: &McpUpstreamCredentialBindingRecord,
    server: &ExternalMcpServerRecord,
) -> Result<RefreshLease, GatewayError>
where
    R: McpUpstreamCredentialRepository + ?Sized,
{
    let deadline = tokio::time::Instant::now() + StdDuration::from_secs(REFRESH_LEASE_WAIT_SECONDS);
    loop {
        let current = repo
            .get_active_mcp_upstream_credential_binding(
                original.mcp_server_id,
                &original.owner_scope_key,
            )
            .await?
            .ok_or_else(|| GatewayError::McpCredentialRequired {
                server_key: server.server_key.clone(),
            })?;
        if !needs_refresh(&current) {
            return Ok(RefreshLease::Current(current));
        }
        let now = OffsetDateTime::now_utc();
        let lease_token = Uuid::new_v4();
        if repo
            .try_acquire_mcp_oauth_refresh_lease(
                current.credential_binding_id,
                lease_token,
                now,
                now + Duration::seconds(REFRESH_LEASE_SECONDS),
            )
            .await?
        {
            return Ok(RefreshLease::Acquired {
                binding: current,
                lease_token,
            });
        }
        if has_unexpired_access_token(&current) {
            return Ok(RefreshLease::Current(current));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ProviderError::Timeout.into());
        }
        tokio::time::sleep(StdDuration::from_millis(100)).await;
    }
}

#[must_use]
pub fn needs_refresh(binding: &McpUpstreamCredentialBindingRecord) -> bool {
    binding.expires_at.is_some_and(|expires_at| {
        expires_at <= OffsetDateTime::now_utc() + Duration::seconds(REFRESH_EARLY_SECONDS)
    })
}

fn has_unexpired_access_token(binding: &McpUpstreamCredentialBindingRecord) -> bool {
    binding
        .expires_at
        .is_some_and(|expires_at| expires_at > OffsetDateTime::now_utc())
}

async fn persist_refreshed_binding<R>(
    repo: &R,
    current: McpUpstreamCredentialBindingRecord,
    grant: McpOauthTokenGrant,
    server: &ExternalMcpServerRecord,
) -> Result<McpUpstreamCredentialBindingRecord, GatewayError>
where
    R: McpUpstreamCredentialRepository + ?Sized,
{
    let secret = serde_json::to_string(&grant.bundle)
        .map_err(|error| GatewayError::Internal(error.to_string()))?;
    let encrypted = encrypt_secret(&secret)?;
    let expected_secret_ciphertext = current
        .secret_ciphertext
        .clone()
        .ok_or_else(|| credential_required(server))?;
    let refreshed = repo
        .compare_and_swap_mcp_oauth_credential_refresh(&RefreshMcpOauthCredentialBindingRecord {
            credential_binding_id: current.credential_binding_id,
            expected_secret_ciphertext,
            secret_ciphertext: encrypted.ciphertext,
            secret_nonce: encrypted.nonce,
            secret_key_id: encrypted.key_id.to_string(),
            // A refreshable grant always has an expiry; see `parse_token_response`.
            expires_at: grant.expires_at.unwrap_or_else(|| {
                OffsetDateTime::now_utc() + Duration::seconds(DEFAULT_EXPIRES_IN_SECONDS)
            }),
            metadata: oauth_bundle_metadata(&grant.bundle),
            updated_at: OffsetDateTime::now_utc(),
        })
        .await?;
    match refreshed {
        Some(binding) => Ok(binding),
        None => current_binding_after_refresh_race(repo, &current, server).await,
    }
}

async fn current_binding_after_refresh_race<R>(
    repo: &R,
    current: &McpUpstreamCredentialBindingRecord,
    server: &ExternalMcpServerRecord,
) -> Result<McpUpstreamCredentialBindingRecord, GatewayError>
where
    R: McpUpstreamCredentialRepository + ?Sized,
{
    repo.get_active_mcp_upstream_credential_binding(current.mcp_server_id, &current.owner_scope_key)
        .await?
        .ok_or_else(|| credential_required(server))
}

#[must_use]
pub(crate) fn oauth_bundle_metadata(
    bundle: &McpOauthTokenBundle,
) -> Map<String, serde_json::Value> {
    Map::from_iter([
        (
            "oauth_bundle_version".to_string(),
            serde_json::json!(bundle.version),
        ),
        (
            "oauth_provider_key".to_string(),
            serde_json::json!(bundle.provider_key),
        ),
        ("resource".to_string(), serde_json::json!(bundle.resource)),
        (
            "granted_scopes".to_string(),
            serde_json::json!(bundle.granted_scopes),
        ),
        (
            "refreshable".to_string(),
            serde_json::json!(bundle.refresh_token.is_some()),
        ),
    ])
    .into_iter()
    .chain(
        bundle
            .issuer
            .as_ref()
            .map(|issuer| ("oauth_issuer".to_string(), serde_json::json!(issuer))),
    )
    .collect()
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    token_type: Option<String>,
    scope: Option<String>,
    error: Option<String>,
}

fn parse_token_response(
    status: reqwest::StatusCode,
    body: &[u8],
    provider_key: &str,
    resource: &str,
    required_scopes: &[String],
    prior_refresh_token: Option<String>,
    require_refresh_token: bool,
) -> Result<McpOauthTokenGrant, GatewayError> {
    let parsed: TokenResponse =
        serde_json::from_slice(body).map_err(|_| ProviderError::UpstreamHttp {
            status: status.as_u16(),
            body: "OAuth token endpoint returned an invalid response".to_string(),
        })?;
    if !status.is_success() || parsed.error.is_some() {
        return Err(oauth_endpoint_error(status));
    }
    let access_token = parsed.access_token.ok_or_else(|| {
        GatewayError::Internal("OAuth token response omitted access_token".to_string())
    })?;
    let refresh_token = parsed.refresh_token.or(prior_refresh_token);
    if require_refresh_token && refresh_token.is_none() {
        return Err(GatewayError::InvalidRequest(
            "OAuth provider did not return a refresh token; reconnect with consent".to_string(),
        ));
    }
    let granted_scopes = parsed
        .scope
        .map(|scopes| scopes.split_whitespace().map(ToOwned::to_owned).collect())
        .unwrap_or_else(|| required_scopes.to_vec());
    ensure_required_scopes(required_scopes, &granted_scopes)?;
    // A refreshable token without `expires_in` is refreshed hourly; a token
    // that cannot be refreshed and states no lifetime does not expire.
    let expires_in = parsed
        .expires_in
        .filter(|value| *value > 0)
        .or(refresh_token
            .is_some()
            .then_some(DEFAULT_EXPIRES_IN_SECONDS));
    Ok(McpOauthTokenGrant {
        bundle: McpOauthTokenBundle {
            version: 1,
            access_token,
            refresh_token,
            token_type: parsed.token_type.unwrap_or_else(|| "Bearer".to_string()),
            provider_key: provider_key.to_string(),
            resource: resource.to_string(),
            granted_scopes,
            issuer: None,
        },
        expires_at: expires_in
            .map(|seconds| OffsetDateTime::now_utc() + Duration::seconds(seconds)),
    })
}

fn ensure_required_scopes(required: &[String], granted: &[String]) -> Result<(), GatewayError> {
    let granted = granted.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let missing = required
        .iter()
        .filter(|scope| !granted.contains(scope.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(GatewayError::InvalidRequest(format!(
            "OAuth grant is missing required scopes: {}",
            missing.join(", ")
        )))
    }
}

fn oauth_transport_error(error: reqwest::Error) -> GatewayError {
    ProviderError::Transport(error.to_string()).into()
}

fn oauth_fetch_error(error: OutboundFetchError) -> GatewayError {
    if error.is_transient() {
        ProviderError::Transport(format!("OAuth token request failed: {error}")).into()
    } else {
        GatewayError::InvalidRequest(format!("OAuth token endpoint is not allowed: {error}"))
    }
}

fn is_transport_error(error: &GatewayError) -> bool {
    matches!(error, GatewayError::Provider(ProviderError::Transport(_)))
}

fn oauth_endpoint_error(status: reqwest::StatusCode) -> GatewayError {
    if is_transient_oauth_status(status) {
        ProviderError::UpstreamHttp {
            status: status.as_u16(),
            body: "OAuth token endpoint failed".to_string(),
        }
        .into()
    } else {
        ProviderError::Transport("OAuth token endpoint rejected the request".to_string()).into()
    }
}

fn is_transient_oauth_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

#[cfg(test)]
mod tests;
