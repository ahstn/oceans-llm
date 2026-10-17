use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex as StdMutex, OnceLock},
    time::Duration,
};

use async_trait::async_trait;
use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpServerRecord, ExternalMcpServerStatus, ExternalMcpToolRecord, ExternalMcpTransport,
    GatewayError, McpRegistryRepository, McpServerOwnership, McpServerOwnershipGuard,
    McpServerVisibility, McpToolMetadata, NewExternalMcpServerRecord, StoreError,
    TransferExternalMcpServerOwnershipRecord, UpdateExternalMcpServerRecord,
    UpsertExternalMcpToolRecord,
};
use gateway_mcp::{McpClientError, NormalizedMcpTool, StreamableHttpClient};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::sync::Mutex as AsyncMutex;
use url::Url;
use uuid::Uuid;

use crate::admin_audit::AuditProgress;
use crate::mcp_upstream_auth::{
    gateway_mcp_upstream_headers, gateway_mcp_upstream_query, mcp_oauth_server_config,
    normalize_mcp_server_key, supports_public_discovery, validate_mcp_auth_config,
    validate_mcp_server_auth_destination,
};
use crate::mcp_upstream_http::McpUpstreamClients;

const DEFAULT_DISCOVERY_TIMEOUT_MS: i64 = 30_000;
const MIN_DISCOVERY_TIMEOUT_MS: i64 = 1_000;
const MAX_DISCOVERY_TIMEOUT_MS: i64 = 120_000;
const MAX_ERROR_SUMMARY_CHARS: usize = 512;
static DISCOVERY_LOCKS: OnceLock<StdMutex<HashMap<Uuid, Arc<AsyncMutex<()>>>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedMcpServerCatalogEntry {
    pub catalog_key: String,
    pub display_name: String,
    pub description: Option<String>,
    pub transport: String,
    pub server_url: String,
    pub auth_mode: String,
    #[serde(default)]
    pub auth_config: Map<String, Value>,
    pub documentation_url: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CreateExternalMcpServerInput {
    pub recommended_catalog_key: Option<String>,
    pub server_key: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub server_url: Option<String>,
    pub transport: Option<String>,
    pub auth_mode: Option<String>,
    pub auth_config: Map<String, Value>,
    pub timeout_ms: Option<i64>,
    /// Resolved by [`crate::McpOwnership::new_server_ownership`].
    pub ownership: McpServerOwnership,
}

#[derive(Debug, Clone)]
pub struct UpdateExternalMcpServerInput {
    pub display_name: String,
    pub description: Option<String>,
    pub server_url: String,
    pub auth_mode: String,
    pub auth_config: Map<String, Value>,
    pub timeout_ms: Option<i64>,
    /// `None` keeps the current contact; a blank value clears it.
    pub owner_contact: Option<String>,
    /// `None` keeps the current visibility. Callers check the change with
    /// [`crate::McpOwnership::authorize_visibility_change`] first.
    pub visibility: Option<McpServerVisibility>,
    /// The owner the caller was authorized against; the save is rejected if the
    /// server has moved since.
    pub expected_owner_team_id: Option<Uuid>,
    /// The visibility the caller was authorized against.
    pub expected_visibility: McpServerVisibility,
    /// Refuse a URL change, under the write lock, while credentials other than
    /// the owning team's own encrypted ones remain. Set for team admins.
    pub retarget_owner_credentials_only: bool,
    /// The URL and auth settings the save was authorized against.
    pub expected_server: Option<McpServerOwnershipGuard>,
}

#[derive(Debug, Clone)]
pub struct McpDiscoveryResult {
    pub server: ExternalMcpServerRecord,
    pub status: ExternalMcpDiscoveryStatus,
    pub error_summary: Option<String>,
    pub tools: Vec<ExternalMcpToolRecord>,
}

#[async_trait]
pub trait McpDiscoveryClient: Send + Sync {
    async fn list_tools(
        &self,
        server: &ExternalMcpServerRecord,
        headers: Option<&BTreeMap<String, String>>,
    ) -> Result<Vec<NormalizedMcpTool>, McpClientError>;
}

/// Lists tools over HTTP, through the client [`McpUpstreamClients`] picks for
/// the server, so a team-owned server's discovery reaches only the addresses
/// its URL allows.
#[derive(Debug, Clone, Default)]
pub struct HttpMcpDiscoveryClient {
    clients: McpUpstreamClients,
}

impl HttpMcpDiscoveryClient {
    #[must_use]
    pub fn new(clients: McpUpstreamClients) -> Self {
        Self { clients }
    }
}

#[async_trait]
impl McpDiscoveryClient for HttpMcpDiscoveryClient {
    async fn list_tools(
        &self,
        server: &ExternalMcpServerRecord,
        headers: Option<&BTreeMap<String, String>>,
    ) -> Result<Vec<NormalizedMcpTool>, McpClientError> {
        let timeout = Duration::from_millis(server.timeout_ms.max(1) as u64);
        let query_credential = gateway_mcp_upstream_query(server)
            .map_err(|error| McpClientError::InvalidHeader(error.to_string()))?;
        let client = self
            .clients
            .for_server(server)
            .map_err(|error| McpClientError::InvalidUrl {
                url: server.server_url.clone(),
                message: error.to_string(),
            })?
            .clone();
        StreamableHttpClient::with_http_client(&server.server_url, client, timeout)?
            .with_query_credential(query_credential)
            .list_tools(headers)
            .await
    }
}

#[derive(Clone)]
pub struct McpRegistryService<R, C = HttpMcpDiscoveryClient> {
    repo: Arc<R>,
    client: Arc<C>,
}

impl<R> McpRegistryService<R, HttpMcpDiscoveryClient>
where
    R: McpRegistryRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self {
            repo,
            client: Arc::new(HttpMcpDiscoveryClient::default()),
        }
    }
}

impl<R, C> McpRegistryService<R, C>
where
    R: McpRegistryRepository,
    C: McpDiscoveryClient,
{
    #[must_use]
    pub fn with_client(repo: Arc<R>, client: Arc<C>) -> Self {
        Self { repo, client }
    }

    pub fn recommended_servers(
        &self,
    ) -> Result<Vec<RecommendedMcpServerCatalogEntry>, GatewayError> {
        load_recommended_catalog()
    }

    pub async fn list_servers(
        &self,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, GatewayError> {
        self.repo
            .list_external_mcp_servers(include_disabled)
            .await
            .map_err(Into::into)
    }

    /// The servers a team admin of `team_id` sees: the team's own and every
    /// published one.
    pub async fn list_servers_visible_to_team(
        &self,
        team_id: Uuid,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, GatewayError> {
        self.repo
            .list_external_mcp_servers_visible_to_team(team_id, include_disabled)
            .await
            .map_err(Into::into)
    }

    /// One owner's servers: a team's, or the platform's for `None`.
    pub async fn list_servers_owned_by(
        &self,
        owner_team_id: Option<Uuid>,
        include_disabled: bool,
    ) -> Result<Vec<ExternalMcpServerRecord>, GatewayError> {
        self.repo
            .list_external_mcp_servers_owned_by(owner_team_id, include_disabled)
            .await
            .map_err(Into::into)
    }

    pub async fn create_server(
        &self,
        input: CreateExternalMcpServerInput,
    ) -> Result<ExternalMcpServerRecord, GatewayError> {
        let ownership = input.ownership.clone();
        if ownership.owner_team_id.is_none() && ownership.visibility == McpServerVisibility::Team {
            return Err(GatewayError::InvalidRequest(
                "a platform-owned MCP server is always visible to the organisation".to_string(),
            ));
        }
        let resolved = self.resolve_create_input(input)?;
        let server_key = normalize_mcp_server_key(&resolved.server_key)?;
        if self
            .repo
            .get_external_mcp_server_by_key(&server_key)
            .await?
            .is_some()
        {
            return Err(GatewayError::Store(StoreError::Conflict(format!(
                "external MCP server key `{server_key}` already exists"
            ))));
        }

        validate_server_url(&resolved.server_url)?;
        let transport = parse_transport(&resolved.transport)?;
        let auth_mode = parse_auth_mode(&resolved.auth_mode)?;
        validate_mcp_auth_config(auth_mode, &resolved.auth_config)?;
        validate_mcp_server_auth_destination(
            &resolved.server_url,
            auth_mode,
            &resolved.auth_config,
        )?;
        let timeout_ms = validate_timeout_ms(resolved.timeout_ms)?;
        let now = OffsetDateTime::now_utc();

        self.repo
            .create_external_mcp_server(&NewExternalMcpServerRecord {
                ownership,
                server_key,
                display_name: resolved.display_name,
                description: resolved.description,
                transport,
                server_url: resolved.server_url,
                auth_mode,
                auth_config: resolved.auth_config,
                timeout_ms,
                created_at: now,
            })
            .await
            .map_err(Into::into)
    }

    /// Saves the server's settings. Nothing is written when they already match, and
    /// the result then carries no previous record. Also returns the users' OAuth
    /// bindings the save revoked, because the endpoints that issued them changed.
    pub async fn update_server(
        &self,
        mcp_server_id: Uuid,
        input: UpdateExternalMcpServerInput,
    ) -> Result<
        (
            gateway_core::Updated<ExternalMcpServerRecord>,
            Vec<crate::RedactedMcpCredentialBinding>,
        ),
        GatewayError,
    > {
        let existing = self.server_or_not_found(mcp_server_id).await?;
        validate_server_url(&input.server_url)?;
        let auth_mode = parse_auth_mode(&input.auth_mode)?;
        validate_mcp_auth_config(auth_mode, &input.auth_config)?;
        validate_mcp_server_auth_destination(&input.server_url, auth_mode, &input.auth_config)?;
        let timeout_ms = validate_timeout_ms(input.timeout_ms)?;
        // An omitted contact is resolved by the store, under its write lock.
        let owner_contact = input
            .owner_contact
            .map(|contact| crate::normalize_owner_contact(Some(contact)))
            .transpose()?;
        let visibility = input.visibility.unwrap_or(existing.ownership.visibility);
        if existing.ownership.owner_team_id.is_none() && visibility == McpServerVisibility::Team {
            return Err(GatewayError::InvalidRequest(
                "a platform-owned MCP server is always visible to the organisation".to_string(),
            ));
        }

        let update = self
            .repo
            .update_external_mcp_server(&UpdateExternalMcpServerRecord {
                mcp_server_id,
                display_name: validate_display_name(input.display_name)?,
                description: input.description,
                server_url: input.server_url,
                auth_mode,
                auth_config: input.auth_config,
                timeout_ms,
                owner_contact,
                visibility,
                retarget_owner_credentials_only: input.retarget_owner_credentials_only,
                expected_server: input.expected_server,
                expected_owner_team_id: input.expected_owner_team_id,
                expected_visibility: input.expected_visibility,
                updated_at: OffsetDateTime::now_utc(),
            })
            .await?;
        Ok((
            gateway_core::Updated {
                current: update.current,
                previous: update.previous,
            },
            update
                .revoked_oauth_bindings
                .into_iter()
                .map(crate::mcp_credentials::redact_binding)
                .collect(),
        ))
    }

    /// Moves the server to `ownership`, which callers resolve with
    /// [`crate::McpOwnership::transfer_ownership`] first. With `expected`, only
    /// while the server still has it; a newer change makes it a conflict.
    pub async fn transfer_ownership_if(
        &self,
        mcp_server_id: Uuid,
        ownership: McpServerOwnership,
        expected: Option<McpServerOwnership>,
    ) -> Result<gateway_core::Updated<ExternalMcpServerRecord>, GatewayError> {
        self.repo
            .transfer_external_mcp_server_ownership(&TransferExternalMcpServerOwnershipRecord {
                mcp_server_id,
                ownership,
                expected_ownership: expected,
                updated_at: OffsetDateTime::now_utc(),
                require_no_stranded_access: true,
            })
            .await
            .map_err(Into::into)
    }

    /// Disables the server; the flag is `false` when it was already disabled.
    pub async fn disable_server(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<(ExternalMcpServerRecord, bool), GatewayError> {
        self.repo
            .disable_external_mcp_server(mcp_server_id, OffsetDateTime::now_utc())
            .await
            .map_err(Into::into)
    }

    /// Disables the server only while `expected_owner_team_id` still owns it, so
    /// a transfer in between cannot let the former team disable it.
    pub async fn disable_server_for_owner(
        &self,
        mcp_server_id: Uuid,
        expected_owner_team_id: Option<Uuid>,
    ) -> Result<(ExternalMcpServerRecord, bool), GatewayError> {
        self.repo
            .disable_external_mcp_server_for_owner(
                mcp_server_id,
                expected_owner_team_id,
                OffsetDateTime::now_utc(),
            )
            .await
            .map_err(Into::into)
    }

    pub async fn list_tools(
        &self,
        mcp_server_id: Uuid,
        include_inactive: bool,
    ) -> Result<Vec<ExternalMcpToolRecord>, GatewayError> {
        self.server_or_not_found(mcp_server_id).await?;
        self.repo
            .list_external_mcp_tools(mcp_server_id, include_inactive)
            .await
            .map_err(Into::into)
    }

    pub async fn refresh_discovery(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<McpDiscoveryResult, GatewayError> {
        self.refresh_discovery_tracked(mcp_server_id, &AuditProgress::default(), None)
            .await
    }

    /// Like [`Self::refresh_discovery`]; `progress` is marked once the discovery run
    /// is stored, so an audit entry can record a refresh whose reload fails afterwards.
    ///
    /// With `expected_server`, the server the caller authorized, the refresh
    /// runs only if the server still matches it when loaded, and its result is
    /// stored only while it still does. Discovery can take a while, so a
    /// transfer or settings change in between makes the refresh a conflict.
    pub async fn refresh_discovery_tracked(
        &self,
        mcp_server_id: Uuid,
        progress: &AuditProgress,
        expected_server: Option<McpServerOwnershipGuard>,
    ) -> Result<McpDiscoveryResult, GatewayError> {
        let discovery_lock = discovery_lock_for(mcp_server_id);
        let _guard = discovery_lock.lock().await;
        let server = self.server_or_not_found(mcp_server_id).await?;
        // The server must still be the one the caller authorized: the same
        // owner, visibility, URL and auth settings. The result is stored only
        // while it still is, checked under the store's write lock.
        if let Some(guard) = &expected_server
            && !guard.matches(&gateway_core::McpServerGuardRow::of(&server))
        {
            return Err(owner_changed_during_discovery(mcp_server_id));
        }
        let persist = DiscoveryPersist {
            progress,
            expected_server,
        };
        if server.status == ExternalMcpServerStatus::Disabled {
            return self
                .record_discovery_failure(
                    server,
                    ExternalMcpDiscoveryStatus::Disabled,
                    OffsetDateTime::now_utc(),
                    Some("external MCP server is disabled".to_string()),
                    json!({"reason": "disabled"}),
                    &persist,
                )
                .await;
        }
        if !supports_public_discovery(&server) {
            let auth_mode = server.auth_mode.as_str().to_string();
            return self
                .record_discovery_failure(
                    server,
                    ExternalMcpDiscoveryStatus::AuthRequired,
                    OffsetDateTime::now_utc(),
                    Some(
                        "server auth mode requires per-user credentials for discovery".to_string(),
                    ),
                    json!({"auth_mode": auth_mode}),
                    &persist,
                )
                .await;
        }

        let started_at = OffsetDateTime::now_utc();
        // The query credential is resolved again by the client; checking it here
        // records a missing secret as a configuration failure.
        let headers = match gateway_mcp_upstream_headers(&server)
            .and_then(|headers| gateway_mcp_upstream_query(&server).map(|_| headers))
        {
            Ok(headers) => headers,
            Err(error) => {
                return self
                    .record_discovery_failure(
                        server,
                        ExternalMcpDiscoveryStatus::Failed,
                        started_at,
                        Some(bounded_error_summary(error.to_string())),
                        json!({"reason": "invalid_discovery_auth_config"}),
                        &persist,
                    )
                    .await;
            }
        };
        match self.client.list_tools(&server, headers.as_ref()).await {
            Ok(tools) => {
                let finished_at = OffsetDateTime::now_utc();
                let discovered_tool_count = tools.len() as i64;
                let discovery_tool_allowlist = mcp_oauth_server_config(&server.auth_config)
                    .ok()
                    .and_then(|config| config.discovery_tool_allowlist);
                let upserts = tools
                    .iter()
                    .filter(|tool| {
                        discovery_tool_allowlist
                            .as_ref()
                            .is_none_or(|allowlist| allowlist.contains(&tool.name))
                    })
                    .map(|tool| UpsertExternalMcpToolRecord {
                        mcp_server_id: server.mcp_server_id,
                        upstream_name: tool.name.clone(),
                        display_name: tool.name.clone(),
                        description: tool.description.clone(),
                        input_schema: tool.input_schema.clone(),
                        metadata: McpToolMetadata {
                            title: tool.title.clone(),
                            annotations: tool.annotations.clone(),
                            output_schema: tool.output_schema.clone(),
                        },
                        schema_hash: tool.schema_hash.clone(),
                    })
                    .collect::<Vec<_>>();
                let schema_set_hash = schema_set_hash(&upserts);
                let run = ExternalMcpDiscoveryRunRecord {
                    discovery_run_id: Uuid::new_v4(),
                    mcp_server_id: server.mcp_server_id,
                    status: ExternalMcpDiscoveryStatus::Success,
                    started_at,
                    finished_at,
                    discovered_tool_count,
                    active_tool_count: upserts.len() as i64,
                    schema_set_hash: Some(schema_set_hash),
                    error_summary: None,
                    details: Map::from_iter([(
                        "scope_compatible_tool_count".to_string(),
                        json!(upserts.len()),
                    )]),
                    expected_server: persist.expected_server.clone(),
                };
                let stored_tools = self
                    .repo
                    .record_external_mcp_discovery_success(&run, &upserts)
                    .await
                    .map_err(discovery_conflict(mcp_server_id))?;
                progress.landed_on(server.mcp_server_id);
                let refreshed_server = self.server_or_not_found(server.mcp_server_id).await?;
                Ok(McpDiscoveryResult {
                    server: refreshed_server,
                    status: ExternalMcpDiscoveryStatus::Success,
                    error_summary: None,
                    tools: stored_tools,
                })
            }
            Err(error) => {
                let summary = bounded_error_summary(discovery_error_summary(&error));
                self.record_discovery_failure(
                    server,
                    ExternalMcpDiscoveryStatus::Failed,
                    started_at,
                    Some(summary),
                    json!({"client_error": classify_client_error(&error)}),
                    &persist,
                )
                .await
            }
        }
    }

    fn resolve_create_input(
        &self,
        input: CreateExternalMcpServerInput,
    ) -> Result<ResolvedCreateInput, GatewayError> {
        let catalog_entry = input
            .recommended_catalog_key
            .as_deref()
            .map(|catalog_key| {
                load_recommended_catalog()?
                    .into_iter()
                    .find(|entry| entry.catalog_key == catalog_key)
                    .ok_or_else(|| {
                        GatewayError::InvalidRequest(format!(
                            "recommended MCP server `{catalog_key}` not found"
                        ))
                    })
            })
            .transpose()?;

        let catalog = catalog_entry.as_ref();
        let server_key = input
            .server_key
            .or_else(|| catalog.map(|entry| entry.catalog_key.clone()))
            .ok_or_else(|| GatewayError::InvalidRequest("server_key is required".to_string()))?;
        let display_name = input
            .display_name
            .or_else(|| catalog.map(|entry| entry.display_name.clone()))
            .ok_or_else(|| GatewayError::InvalidRequest("display_name is required".to_string()))?;
        let server_url = input
            .server_url
            .or_else(|| catalog.map(|entry| entry.server_url.clone()))
            .ok_or_else(|| GatewayError::InvalidRequest("server_url is required".to_string()))?;
        let transport = input
            .transport
            .or_else(|| catalog.map(|entry| entry.transport.clone()))
            .unwrap_or_else(|| ExternalMcpTransport::StreamableHttp.as_str().to_string());
        let request_overrides_auth_mode = input.auth_mode.is_some();
        let auth_mode = input
            .auth_mode
            .or_else(|| catalog.map(|entry| entry.auth_mode.clone()))
            .unwrap_or_else(|| ExternalMcpAuthMode::None.as_str().to_string());
        let auth_config = if input.auth_config.is_empty() && !request_overrides_auth_mode {
            catalog
                .map(|entry| entry.auth_config.clone())
                .unwrap_or_default()
        } else {
            input.auth_config
        };

        Ok(ResolvedCreateInput {
            server_key,
            display_name: validate_display_name(display_name)?,
            description: input
                .description
                .or_else(|| catalog.and_then(|entry| entry.description.clone())),
            server_url,
            transport,
            auth_mode,
            auth_config,
            timeout_ms: input.timeout_ms,
        })
    }

    async fn server_or_not_found(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<ExternalMcpServerRecord, GatewayError> {
        self.repo
            .get_external_mcp_server(mcp_server_id)
            .await?
            .ok_or_else(|| {
                GatewayError::Store(StoreError::NotFound(format!(
                    "external MCP server `{mcp_server_id}` not found"
                )))
            })
    }

    /// Re-reads the owner right before a discovery result is stored, so a refresh
    /// that started before a transfer does not write to a server another team
    /// now owns. The re-read is not in the store's write, but it narrows the
    /// window from the whole upstream call to one query.
    async fn record_discovery_failure(
        &self,
        server: ExternalMcpServerRecord,
        status: ExternalMcpDiscoveryStatus,
        started_at: OffsetDateTime,
        error_summary: Option<String>,
        details: Value,
        persist: &DiscoveryPersist<'_>,
    ) -> Result<McpDiscoveryResult, GatewayError> {
        let now = OffsetDateTime::now_utc();
        let run = ExternalMcpDiscoveryRunRecord {
            discovery_run_id: Uuid::new_v4(),
            mcp_server_id: server.mcp_server_id,
            status,
            started_at,
            finished_at: now,
            discovered_tool_count: 0,
            active_tool_count: server.last_tool_count.unwrap_or(0),
            schema_set_hash: None,
            error_summary,
            details: value_object(details)?,
            expected_server: persist.expected_server.clone(),
        };
        self.repo
            .record_external_mcp_discovery_failure(&run)
            .await
            .map_err(discovery_conflict(server.mcp_server_id))?;
        persist.progress.landed_on(server.mcp_server_id);
        let refreshed_server = self.server_or_not_found(server.mcp_server_id).await?;
        let tools = self
            .repo
            .list_external_mcp_tools(server.mcp_server_id, false)
            .await?;
        Ok(McpDiscoveryResult {
            server: refreshed_server,
            status,
            error_summary: run.error_summary,
            tools,
        })
    }
}

/// Where a refresh records its result: the audit progress to mark, and the
/// server state the result may be stored against.
struct DiscoveryPersist<'a> {
    progress: &'a AuditProgress,
    expected_server: Option<McpServerOwnershipGuard>,
}

fn owner_changed_during_discovery(mcp_server_id: Uuid) -> GatewayError {
    GatewayError::Store(StoreError::Conflict(format!(
        "external MCP server `{mcp_server_id}` changed owner or settings during discovery; refresh it again"
    )))
}

/// Reports a guarded write the store refused as a change during discovery.
fn discovery_conflict(mcp_server_id: Uuid) -> impl Fn(StoreError) -> GatewayError {
    move |error| match error {
        StoreError::Conflict(_) => owner_changed_during_discovery(mcp_server_id),
        other => other.into(),
    }
}

struct ResolvedCreateInput {
    server_key: String,
    display_name: String,
    description: Option<String>,
    server_url: String,
    transport: String,
    auth_mode: String,
    auth_config: Map<String, Value>,
    timeout_ms: Option<i64>,
}

fn load_recommended_catalog() -> Result<Vec<RecommendedMcpServerCatalogEntry>, GatewayError> {
    serde_json::from_str(include_str!("../data/recommended_mcp_servers.json")).map_err(|error| {
        GatewayError::Internal(format!("invalid recommended MCP catalog: {error}"))
    })
}

fn discovery_lock_for(mcp_server_id: Uuid) -> Arc<AsyncMutex<()>> {
    let locks = DISCOVERY_LOCKS.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .expect("MCP discovery lock registry mutex poisoned");
    locks
        .entry(mcp_server_id)
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

fn validate_display_name(value: String) -> Result<String, GatewayError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return Err(GatewayError::InvalidRequest(
            "display_name must be 1-120 characters".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn validate_server_url(value: &str) -> Result<(), GatewayError> {
    let url = Url::parse(value)
        .map_err(|error| GatewayError::InvalidRequest(format!("server_url is invalid: {error}")))?;
    match url.scheme() {
        "http" | "https" => Ok(()),
        _ => Err(GatewayError::InvalidRequest(
            "server_url must use http or https".to_string(),
        )),
    }
}

fn parse_transport(value: &str) -> Result<ExternalMcpTransport, GatewayError> {
    ExternalMcpTransport::from_db(value).ok_or_else(|| {
        GatewayError::InvalidRequest(format!(
            "unsupported external MCP transport `{value}`; only streamable_http is supported"
        ))
    })
}

fn parse_auth_mode(value: &str) -> Result<ExternalMcpAuthMode, GatewayError> {
    ExternalMcpAuthMode::from_db(value)
        .ok_or_else(|| GatewayError::InvalidRequest(format!("unsupported MCP auth mode `{value}`")))
}

fn validate_timeout_ms(value: Option<i64>) -> Result<i64, GatewayError> {
    let timeout_ms = value.unwrap_or(DEFAULT_DISCOVERY_TIMEOUT_MS);
    if !(MIN_DISCOVERY_TIMEOUT_MS..=MAX_DISCOVERY_TIMEOUT_MS).contains(&timeout_ms) {
        return Err(GatewayError::InvalidRequest(format!(
            "timeout_ms must be between {MIN_DISCOVERY_TIMEOUT_MS} and {MAX_DISCOVERY_TIMEOUT_MS}"
        )));
    }
    Ok(timeout_ms)
}

fn schema_set_hash(tools: &[UpsertExternalMcpToolRecord]) -> String {
    let mut entries = tools
        .iter()
        .map(|tool| format!("{}\0{}", tool.upstream_name, tool.schema_hash))
        .collect::<Vec<_>>();
    entries.sort();
    let digest = Sha256::digest(entries.join("\n").as_bytes());
    format!("sha256:{digest:x}")
}

fn bounded_error_summary(value: String) -> String {
    if value.len() <= MAX_ERROR_SUMMARY_CHARS {
        return value;
    }
    value.chars().take(MAX_ERROR_SUMMARY_CHARS).collect()
}

fn discovery_error_summary(error: &McpClientError) -> String {
    match error {
        McpClientError::Http { status, .. } => {
            format!("MCP upstream returned HTTP {status}")
        }
        McpClientError::JsonRpc(error) => format!("MCP JSON-RPC error {}", error.code),
        other => other.to_string(),
    }
}

fn classify_client_error(error: &McpClientError) -> &'static str {
    match error {
        McpClientError::InvalidUrl { .. } => "invalid_url",
        McpClientError::InvalidHeader(_) => "invalid_header",
        McpClientError::Timeout => "timeout",
        McpClientError::Transport(_) => "transport",
        McpClientError::Http { .. } => "http",
        McpClientError::ResponseTooLarge { .. } => "response_too_large",
        McpClientError::JsonRpc(_) => "json_rpc",
        McpClientError::InvalidResponse { .. } => "invalid_response",
        McpClientError::InvalidToolSchema { .. } => "invalid_tool_schema",
    }
}

fn value_object(value: Value) -> Result<Map<String, Value>, GatewayError> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(GatewayError::Internal(
            "MCP discovery details must be a JSON object".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommended_catalog_loads() {
        let entries = load_recommended_catalog().expect("catalog");
        assert!(entries.iter().any(|entry| entry.catalog_key == "github"));
        for (key, resource, required_scope) in [
            (
                "google_drive",
                "https://drivemcp.googleapis.com/mcp/v1",
                "https://www.googleapis.com/auth/drive.readonly",
            ),
            (
                "google_docs",
                "https://docsmcp.googleapis.com/mcp/v1",
                "https://www.googleapis.com/auth/documents.readonly",
            ),
        ] {
            let entry = entries
                .iter()
                .find(|entry| entry.catalog_key == key)
                .expect("Google Workspace catalog entry");
            assert_eq!(
                entry
                    .auth_config
                    .get("provider_key")
                    .and_then(Value::as_str),
                Some("google")
            );
            assert_eq!(
                entry.auth_config.get("resource").and_then(Value::as_str),
                Some(resource)
            );
            assert_eq!(
                entry
                    .auth_config
                    .get("discovery_auth")
                    .and_then(Value::as_str),
                Some("none")
            );
            let scopes = entry
                .auth_config
                .get("scopes")
                .and_then(Value::as_array)
                .expect("Google Workspace scopes");
            assert_eq!(scopes.len(), 1);
            assert_eq!(scopes[0].as_str(), Some(required_scope));
            let allowlist = entry
                .auth_config
                .get("discovery_tool_allowlist")
                .and_then(Value::as_array)
                .expect("scope-compatible discovery tool allowlist");
            let tool_names = allowlist
                .iter()
                .map(|value| value.as_str().expect("tool name"))
                .collect::<Vec<_>>();
            if key == "google_drive" {
                assert_eq!(
                    tool_names,
                    vec![
                        "download_file_content",
                        "get_file_metadata",
                        "get_file_permissions",
                        "list_recent_files",
                        "read_file_content",
                        "search_files",
                    ]
                );
            } else {
                assert_eq!(tool_names, vec!["read_doc"]);
            }
        }
        let notion = entries
            .iter()
            .find(|entry| entry.catalog_key == "notion")
            .expect("Notion catalog entry");
        assert!(!notion.auth_config.contains_key("provider_key"));
    }

    #[test]
    fn server_keys_are_normalized_and_limited() {
        assert_eq!(normalize_mcp_server_key(" GitHub ").unwrap(), "github");
        assert!(normalize_mcp_server_key("bad key").is_err());
    }

    #[test]
    fn discovery_error_summary_omits_upstream_json_rpc_message() {
        let summary =
            discovery_error_summary(&McpClientError::JsonRpc(gateway_mcp::JsonRpcErrorObject {
                code: -32000,
                message: "remote controlled diagnostic".to_string(),
                data: None,
            }));

        assert_eq!(summary, "MCP JSON-RPC error -32000");
    }

    #[test]
    fn catalog_auth_config_is_not_inherited_when_request_overrides_auth_mode() {
        let service = McpRegistryService::with_client(
            Arc::new(NoopMcpRegistryRepository),
            Arc::new(NoopMcpDiscoveryClient),
        );
        let resolved = service
            .resolve_create_input(CreateExternalMcpServerInput {
                recommended_catalog_key: Some("github".to_string()),
                server_key: Some("github-none".to_string()),
                display_name: Some("GitHub".to_string()),
                description: None,
                server_url: None,
                transport: None,
                auth_mode: Some(ExternalMcpAuthMode::None.as_str().to_string()),
                auth_config: Map::new(),
                timeout_ms: None,
                ownership: McpServerOwnership::default(),
            })
            .expect("resolve input");

        assert_eq!(resolved.auth_mode, ExternalMcpAuthMode::None.as_str());
        assert!(resolved.auth_config.is_empty());
    }

    struct NoopMcpDiscoveryClient;

    #[async_trait]
    impl McpDiscoveryClient for NoopMcpDiscoveryClient {
        async fn list_tools(
            &self,
            _server: &ExternalMcpServerRecord,
            _headers: Option<&BTreeMap<String, String>>,
        ) -> Result<Vec<NormalizedMcpTool>, McpClientError> {
            Ok(Vec::new())
        }
    }

    struct NoopMcpRegistryRepository;

    #[async_trait]
    impl McpRegistryRepository for NoopMcpRegistryRepository {
        async fn list_external_mcp_servers(
            &self,
            _include_disabled: bool,
        ) -> Result<Vec<ExternalMcpServerRecord>, StoreError> {
            unimplemented!()
        }

        async fn get_external_mcp_server(
            &self,
            _mcp_server_id: Uuid,
        ) -> Result<Option<ExternalMcpServerRecord>, StoreError> {
            unimplemented!()
        }

        async fn get_external_mcp_server_by_key(
            &self,
            _server_key: &str,
        ) -> Result<Option<ExternalMcpServerRecord>, StoreError> {
            unimplemented!()
        }

        async fn create_external_mcp_server(
            &self,
            _input: &NewExternalMcpServerRecord,
        ) -> Result<ExternalMcpServerRecord, StoreError> {
            unimplemented!()
        }

        async fn update_external_mcp_server(
            &self,
            _input: &UpdateExternalMcpServerRecord,
        ) -> Result<gateway_core::ExternalMcpServerUpdate, StoreError> {
            unimplemented!()
        }

        async fn disable_external_mcp_server(
            &self,
            _mcp_server_id: Uuid,
            _disabled_at: OffsetDateTime,
        ) -> Result<(ExternalMcpServerRecord, bool), StoreError> {
            unimplemented!()
        }

        async fn list_external_mcp_tools(
            &self,
            _mcp_server_id: Uuid,
            _include_inactive: bool,
        ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
            unimplemented!()
        }

        async fn record_external_mcp_discovery_success(
            &self,
            _run: &ExternalMcpDiscoveryRunRecord,
            _tools: &[UpsertExternalMcpToolRecord],
        ) -> Result<Vec<ExternalMcpToolRecord>, StoreError> {
            unimplemented!()
        }

        async fn record_external_mcp_discovery_failure(
            &self,
            _run: &ExternalMcpDiscoveryRunRecord,
        ) -> Result<(), StoreError> {
            unimplemented!()
        }
    }
}
