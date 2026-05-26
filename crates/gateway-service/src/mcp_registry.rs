use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpServerRecord, ExternalMcpServerStatus, ExternalMcpToolRecord, ExternalMcpTransport,
    GatewayError, McpRegistryRepository, NewExternalMcpServerRecord, StoreError,
    UpdateExternalMcpServerRecord, UpsertExternalMcpToolRecord,
};
use gateway_mcp::{McpClientError, NormalizedMcpTool, StreamableHttpClient};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;

const DEFAULT_DISCOVERY_TIMEOUT_MS: i64 = 30_000;
const MIN_DISCOVERY_TIMEOUT_MS: i64 = 1_000;
const MAX_DISCOVERY_TIMEOUT_MS: i64 = 120_000;
const MAX_ERROR_SUMMARY_CHARS: usize = 512;
const DISCOVERY_SECRET_ENV_PREFIX: &str = "OCEANS_MCP_DISCOVERY_";

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
}

#[derive(Debug, Clone)]
pub struct UpdateExternalMcpServerInput {
    pub display_name: String,
    pub description: Option<String>,
    pub server_url: String,
    pub auth_mode: String,
    pub auth_config: Map<String, Value>,
    pub timeout_ms: Option<i64>,
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

#[derive(Debug, Clone, Default)]
pub struct HttpMcpDiscoveryClient;

#[async_trait]
impl McpDiscoveryClient for HttpMcpDiscoveryClient {
    async fn list_tools(
        &self,
        server: &ExternalMcpServerRecord,
        headers: Option<&BTreeMap<String, String>>,
    ) -> Result<Vec<NormalizedMcpTool>, McpClientError> {
        let timeout = Duration::from_millis(server.timeout_ms.max(1) as u64);
        StreamableHttpClient::new(&server.server_url, timeout)?
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
            client: Arc::new(HttpMcpDiscoveryClient),
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

    pub async fn create_server(
        &self,
        input: CreateExternalMcpServerInput,
    ) -> Result<ExternalMcpServerRecord, GatewayError> {
        let resolved = self.resolve_create_input(input)?;
        let server_key = normalize_server_key(&resolved.server_key)?;
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
        validate_auth_config(auth_mode, &resolved.auth_config)?;
        validate_credentialed_server_url(&resolved.server_url, auth_mode)?;
        let timeout_ms = validate_timeout_ms(resolved.timeout_ms)?;
        let now = OffsetDateTime::now_utc();

        self.repo
            .create_external_mcp_server(&NewExternalMcpServerRecord {
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

    pub async fn update_server(
        &self,
        mcp_server_id: Uuid,
        input: UpdateExternalMcpServerInput,
    ) -> Result<ExternalMcpServerRecord, GatewayError> {
        self.server_or_not_found(mcp_server_id).await?;
        validate_server_url(&input.server_url)?;
        let auth_mode = parse_auth_mode(&input.auth_mode)?;
        validate_auth_config(auth_mode, &input.auth_config)?;
        validate_credentialed_server_url(&input.server_url, auth_mode)?;
        let timeout_ms = validate_timeout_ms(input.timeout_ms)?;

        self.repo
            .update_external_mcp_server(&UpdateExternalMcpServerRecord {
                mcp_server_id,
                display_name: validate_display_name(input.display_name)?,
                description: input.description,
                server_url: input.server_url,
                auth_mode,
                auth_config: input.auth_config,
                timeout_ms,
                updated_at: OffsetDateTime::now_utc(),
            })
            .await
            .map_err(Into::into)
    }

    pub async fn disable_server(
        &self,
        mcp_server_id: Uuid,
    ) -> Result<ExternalMcpServerRecord, GatewayError> {
        self.repo
            .disable_external_mcp_server(mcp_server_id, OffsetDateTime::now_utc())
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
        let server = self.server_or_not_found(mcp_server_id).await?;
        if server.status == ExternalMcpServerStatus::Disabled {
            return self
                .record_discovery_failure(
                    server,
                    ExternalMcpDiscoveryStatus::Disabled,
                    Some("external MCP server is disabled".to_string()),
                    json!({"reason": "disabled"}),
                )
                .await;
        }
        if !server.auth_mode.supports_gateway_discovery() {
            let auth_mode = server.auth_mode.as_str().to_string();
            return self
                .record_discovery_failure(
                    server,
                    ExternalMcpDiscoveryStatus::AuthRequired,
                    Some(
                        "server auth mode requires per-user credentials for discovery".to_string(),
                    ),
                    json!({"auth_mode": auth_mode}),
                )
                .await;
        }

        let headers = discovery_headers(&server)?;
        let started_at = OffsetDateTime::now_utc();
        match self.client.list_tools(&server, headers.as_ref()).await {
            Ok(tools) => {
                let finished_at = OffsetDateTime::now_utc();
                let upserts = tools
                    .iter()
                    .map(|tool| UpsertExternalMcpToolRecord {
                        mcp_server_id: server.mcp_server_id,
                        upstream_name: tool.name.clone(),
                        display_name: tool.name.clone(),
                        description: tool.description.clone(),
                        input_schema: tool.input_schema.clone(),
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
                    discovered_tool_count: upserts.len() as i64,
                    active_tool_count: upserts.len() as i64,
                    schema_set_hash: Some(schema_set_hash),
                    error_summary: None,
                    details: Map::new(),
                };
                let stored_tools = self
                    .repo
                    .record_external_mcp_discovery_success(&run, &upserts)
                    .await?;
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
                    Some(summary),
                    json!({"client_error": classify_client_error(&error)}),
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
        let auth_mode = input
            .auth_mode
            .or_else(|| catalog.map(|entry| entry.auth_mode.clone()))
            .unwrap_or_else(|| ExternalMcpAuthMode::None.as_str().to_string());
        let auth_config = if input.auth_config.is_empty() {
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

    async fn record_discovery_failure(
        &self,
        server: ExternalMcpServerRecord,
        status: ExternalMcpDiscoveryStatus,
        error_summary: Option<String>,
        details: Value,
    ) -> Result<McpDiscoveryResult, GatewayError> {
        let now = OffsetDateTime::now_utc();
        let run = ExternalMcpDiscoveryRunRecord {
            discovery_run_id: Uuid::new_v4(),
            mcp_server_id: server.mcp_server_id,
            status,
            started_at: now,
            finished_at: now,
            discovered_tool_count: 0,
            active_tool_count: server.last_tool_count.unwrap_or(0),
            schema_set_hash: None,
            error_summary,
            details: value_object(details)?,
        };
        self.repo
            .record_external_mcp_discovery_failure(&run)
            .await?;
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

fn normalize_server_key(value: &str) -> Result<String, GatewayError> {
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

fn validate_credentialed_server_url(
    value: &str,
    auth_mode: ExternalMcpAuthMode,
) -> Result<(), GatewayError> {
    if !auth_mode.supports_gateway_discovery() || auth_mode == ExternalMcpAuthMode::None {
        return Ok(());
    }
    let url = Url::parse(value)
        .map_err(|error| GatewayError::InvalidRequest(format!("server_url is invalid: {error}")))?;
    if url.scheme() != "https" {
        return Err(GatewayError::InvalidRequest(
            "gateway-managed MCP discovery credentials require an https server_url".to_string(),
        ));
    }
    Ok(())
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

fn validate_auth_config(
    auth_mode: ExternalMcpAuthMode,
    auth_config: &Map<String, Value>,
) -> Result<(), GatewayError> {
    match auth_mode {
        ExternalMcpAuthMode::None => ensure_allowed_auth_fields(auth_config, &[]),
        ExternalMcpAuthMode::GatewayStaticHeader => {
            ensure_allowed_auth_fields(auth_config, &["header_name", "secret_ref"])?;
            required_string(auth_config, "header_name")?;
            validate_discovery_secret_ref(required_secret_ref(auth_config)?)?;
            Ok(())
        }
        ExternalMcpAuthMode::GatewayBearerToken => {
            ensure_allowed_auth_fields(auth_config, &["secret_ref"])?;
            validate_discovery_secret_ref(required_secret_ref(auth_config)?)?;
            Ok(())
        }
        ExternalMcpAuthMode::UserPassthrough => {
            ensure_allowed_auth_fields(auth_config, &["header", "token_type"])
        }
        ExternalMcpAuthMode::OauthObo => {
            ensure_allowed_auth_fields(auth_config, &["token_exchange", "token_type"])
        }
    }
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

fn discovery_headers(
    server: &ExternalMcpServerRecord,
) -> Result<Option<BTreeMap<String, String>>, GatewayError> {
    match server.auth_mode {
        ExternalMcpAuthMode::None => Ok(None),
        ExternalMcpAuthMode::GatewayStaticHeader => {
            let header_name = required_string(&server.auth_config, "header_name")?;
            let secret = resolve_secret_ref(required_secret_ref(&server.auth_config)?)?;
            Ok(Some(BTreeMap::from([(header_name.to_string(), secret)])))
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

fn required_secret_ref(auth_config: &Map<String, Value>) -> Result<&str, GatewayError> {
    required_string(auth_config, "secret_ref")
}

fn validate_discovery_secret_ref(secret_ref: &str) -> Result<&str, GatewayError> {
    let env_name = discovery_secret_env_name(secret_ref)?;
    if !env_name.starts_with(DISCOVERY_SECRET_ENV_PREFIX) {
        return Err(GatewayError::InvalidRequest(format!(
            "secret_ref environment variable must start with {DISCOVERY_SECRET_ENV_PREFIX}"
        )));
    }
    Ok(secret_ref)
}

fn resolve_secret_ref(secret_ref: &str) -> Result<String, GatewayError> {
    validate_discovery_secret_ref(secret_ref)?;
    let env_name = discovery_secret_env_name(secret_ref)?;
    std::env::var(env_name).map_err(|_| {
        GatewayError::InvalidRequest(format!(
            "secret_ref env/{env_name} is not available for MCP discovery"
        ))
    })
}

fn discovery_secret_env_name(secret_ref: &str) -> Result<&str, GatewayError> {
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
    Ok(env_name)
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
        McpClientError::JsonRpc(error) => {
            format!("MCP JSON-RPC error {}: {}", error.code, error.message)
        }
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
    }

    #[test]
    fn server_keys_are_normalized_and_limited() {
        assert_eq!(normalize_server_key(" GitHub ").unwrap(), "github");
        assert!(normalize_server_key("bad key").is_err());
    }

    #[test]
    fn gateway_managed_auth_requires_secret_refs() {
        let mode = ExternalMcpAuthMode::GatewayBearerToken;
        let mut config = Map::new();
        assert!(validate_auth_config(mode, &config).is_err());
        config.insert("secret_ref".to_string(), json!("env/MCP_TOKEN"));
        assert!(validate_auth_config(mode, &config).is_err());
        config.insert(
            "secret_ref".to_string(),
            json!("env/OCEANS_MCP_DISCOVERY_MCP_TOKEN"),
        );
        assert!(validate_auth_config(mode, &config).is_ok());
    }
}
