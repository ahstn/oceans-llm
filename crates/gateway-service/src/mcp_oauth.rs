use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex, Weak},
    time::Duration as StdDuration,
};

use gateway_core::{
    ExternalMcpServerRecord, GatewayError, McpUpstreamCredentialBindingRecord,
    McpUpstreamCredentialRepository, ProviderError, RefreshMcpOauthCredentialBindingRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::mcp_credentials::{decrypt_binding_secret, encrypt_secret};
use crate::mcp_upstream_auth::mcp_oauth_server_config;

const REFRESH_EARLY_SECONDS: i64 = 300;
const REFRESH_LEASE_SECONDS: i64 = 90;
const REFRESH_LEASE_WAIT_SECONDS: u64 = 65;

#[derive(Debug, Clone)]
pub struct McpOauthProvider {
    pub key: String,
    pub client_id: String,
    pub client_secret: String,
    pub authorization_url: String,
    pub token_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOauthTokenBundle {
    pub version: u8,
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub provider_key: String,
    pub resource: String,
    pub granted_scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct McpOauthTokenGrant {
    pub bundle: McpOauthTokenBundle,
    pub expires_at: OffsetDateTime,
}

#[derive(Clone)]
pub struct McpOauthRuntime {
    public_base_url: Option<String>,
    providers: BTreeMap<String, McpOauthProvider>,
    client: reqwest::Client,
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
                .timeout(StdDuration::from_secs(60))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("default MCP OAuth HTTP client configuration must be valid"),
            refresh_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.public_base_url.is_some() && !self.providers.is_empty()
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
        provider_key: &str,
        code: &str,
        redirect_uri: &str,
        pkce_verifier: &str,
        resource: &str,
        required_scopes: &[String],
    ) -> Result<McpOauthTokenGrant, GatewayError> {
        let provider = self.provider(provider_key)?;
        let response = self
            .client
            .post(&provider.token_url)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("client_id", provider.client_id.as_str()),
                ("client_secret", provider.client_secret.as_str()),
                ("redirect_uri", redirect_uri),
                ("code_verifier", pkce_verifier),
                ("resource", resource),
            ])
            .send()
            .await
            .map_err(oauth_transport_error)?;
        parse_token_response(response, provider_key, resource, required_scopes, None).await
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
        let provider = self.provider(&bundle.provider_key)?;
        let response = self
            .client
            .post(&provider.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", bundle.refresh_token.as_str()),
                ("client_id", provider.client_id.as_str()),
                ("client_secret", provider.client_secret.as_str()),
                ("resource", bundle.resource.as_str()),
            ])
            .send()
            .await
            .map_err(oauth_transport_error)?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let error_code = serde_json::from_str::<TokenResponse>(&body)
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
            return Err(oauth_endpoint_error(status));
        }
        let grant = parse_token_response(
            response,
            &bundle.provider_key,
            &bundle.resource,
            &bundle.granted_scopes,
            Some(bundle.refresh_token),
        )
        .await?;
        persist_refreshed_binding(repo, current, grant, server).await
    }
}

pub(crate) fn validate_oauth_bundle_for_server(
    bundle: &McpOauthTokenBundle,
    server: &ExternalMcpServerRecord,
) -> Result<(), GatewayError> {
    let config =
        mcp_oauth_server_config(&server.auth_config).map_err(|_| credential_required(server))?;
    if bundle.provider_key != config.provider_key || bundle.resource != config.resource {
        return Err(credential_required(server));
    }
    ensure_required_scopes(&config.scopes, &bundle.granted_scopes)
        .map_err(|_| credential_required(server))
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
            expires_at: grant.expires_at,
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
    ])
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

async fn parse_token_response(
    response: reqwest::Response,
    provider_key: &str,
    resource: &str,
    required_scopes: &[String],
    prior_refresh_token: Option<String>,
) -> Result<McpOauthTokenGrant, GatewayError> {
    let status = response.status();
    let body = response.text().await.map_err(oauth_transport_error)?;
    let parsed: TokenResponse =
        serde_json::from_str(&body).map_err(|_| ProviderError::UpstreamHttp {
            status: status.as_u16(),
            body: "OAuth token endpoint returned an invalid response".to_string(),
        })?;
    if !status.is_success() || parsed.error.is_some() {
        return Err(oauth_endpoint_error(status));
    }
    let access_token = parsed.access_token.ok_or_else(|| {
        GatewayError::Internal("OAuth token response omitted access_token".to_string())
    })?;
    let refresh_token = parsed
        .refresh_token
        .or(prior_refresh_token)
        .ok_or_else(|| {
            GatewayError::InvalidRequest(
                "OAuth provider did not return a refresh token; reconnect with consent".to_string(),
            )
        })?;
    let granted_scopes = parsed
        .scope
        .map(|scopes| scopes.split_whitespace().map(ToOwned::to_owned).collect())
        .unwrap_or_else(|| required_scopes.to_vec());
    ensure_required_scopes(required_scopes, &granted_scopes)?;
    let expires_in = parsed.expires_in.filter(|value| *value > 0).unwrap_or(3600);
    Ok(McpOauthTokenGrant {
        bundle: McpOauthTokenBundle {
            version: 1,
            access_token,
            refresh_token,
            token_type: parsed.token_type.unwrap_or_else(|| "Bearer".to_string()),
            provider_key: provider_key.to_string(),
            resource: resource.to_string(),
            granted_scopes,
        },
        expires_at: OffsetDateTime::now_utc() + Duration::seconds(expires_in),
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

fn oauth_endpoint_error(status: reqwest::StatusCode) -> GatewayError {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        ProviderError::UpstreamHttp {
            status: status.as_u16(),
            body: "OAuth token endpoint failed".to_string(),
        }
        .into()
    } else {
        ProviderError::Transport("OAuth token endpoint rejected the request".to_string()).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::{ExternalMcpAuthMode, ExternalMcpServerStatus, ExternalMcpTransport};
    use serde_json::{Map, json};

    #[test]
    fn required_scope_check_accepts_superset_and_rejects_missing_scope() {
        let required = vec![
            "https://www.googleapis.com/auth/drive.readonly".to_string(),
            "https://www.googleapis.com/auth/documents.readonly".to_string(),
        ];
        let mut granted = required.clone();
        granted.push("openid".to_string());
        ensure_required_scopes(&required, &granted).expect("scope superset");

        let error = ensure_required_scopes(&required, &granted[1..])
            .expect_err("missing Drive scope must fail");
        assert_eq!(error.error_code(), "invalid_request");
    }

    #[test]
    fn oauth_endpoint_error_does_not_include_provider_response_details() {
        let error = oauth_endpoint_error(reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(error.error_code(), "upstream_transport");
        assert!(!error.to_string().contains("invalid_grant"));
    }

    #[test]
    fn oauth_bundle_must_match_the_current_server_contract() {
        let server = oauth_server();
        let bundle = McpOauthTokenBundle {
            version: 1,
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            token_type: "Bearer".to_string(),
            provider_key: "google".to_string(),
            resource: "https://drivemcp.googleapis.com/mcp/v1".to_string(),
            granted_scopes: vec!["https://www.googleapis.com/auth/drive.readonly".to_string()],
        };

        validate_oauth_bundle_for_server(&bundle, &server).expect("matching contract");

        let mut wrong_resource = bundle.clone();
        wrong_resource.resource = "https://docsmcp.googleapis.com/mcp/v1".to_string();
        assert_eq!(
            validate_oauth_bundle_for_server(&wrong_resource, &server)
                .expect_err("resource mismatch")
                .error_code(),
            "credential_required"
        );

        let mut missing_scope = bundle;
        missing_scope.granted_scopes.clear();
        assert_eq!(
            validate_oauth_bundle_for_server(&missing_scope, &server)
                .expect_err("scope mismatch")
                .error_code(),
            "credential_required"
        );
    }

    fn oauth_server() -> ExternalMcpServerRecord {
        ExternalMcpServerRecord {
            mcp_server_id: Uuid::new_v4(),
            server_key: "google_drive".to_string(),
            display_name: "Google Drive".to_string(),
            description: None,
            transport: ExternalMcpTransport::StreamableHttp,
            server_url: "https://drivemcp.googleapis.com/mcp/v1".to_string(),
            auth_mode: ExternalMcpAuthMode::OauthObo,
            auth_config: Map::from_iter([
                ("provider_key".to_string(), json!("google")),
                (
                    "resource".to_string(),
                    json!("https://drivemcp.googleapis.com/mcp/v1"),
                ),
                (
                    "scopes".to_string(),
                    json!(["https://www.googleapis.com/auth/drive.readonly"]),
                ),
            ]),
            timeout_ms: 30_000,
            status: ExternalMcpServerStatus::Active,
            last_discovery_status: None,
            last_discovery_at: None,
            last_successful_discovery_at: None,
            last_error_summary: None,
            last_tool_count: None,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
            disabled_at: None,
        }
    }
}
