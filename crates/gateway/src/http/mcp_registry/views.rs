use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpServerRecord, ExternalMcpToolRecord, GatewayError,
    McpToolGrantRecord, McpToolGrantSubjectKind, McpToolGrantTargetKind, McpToolsetRecord,
    McpUpstreamCredentialMaterialKind, McpUpstreamCredentialOwnerScopeKind,
};
use gateway_service::{
    McpDiscoveryResult, RecommendedMcpServerCatalogEntry, RedactedMcpCredentialBinding,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::http::admin_contract::format_timestamp;

#[derive(Debug, Deserialize, IntoParams)]
pub struct McpServersQuery {
    #[serde(default)]
    pub(super) include_disabled: bool,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct McpToolsQuery {
    #[serde(default)]
    pub(super) include_inactive: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RecommendedMcpServersPayload {
    pub(super) items: Vec<RecommendedMcpServerView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RecommendedMcpServerView {
    catalog_key: String,
    display_name: String,
    description: Option<String>,
    transport: String,
    server_url: String,
    auth_mode: String,
    #[schema(additional_properties = true)]
    auth_config: Map<String, Value>,
    documentation_url: Option<String>,
    tags: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpServersPayload {
    pub(super) items: Vec<McpServerView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpServerPayload {
    pub(super) server: McpServerView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpServerView {
    id: String,
    server_key: String,
    display_name: String,
    description: Option<String>,
    transport: String,
    server_url: String,
    auth_mode: String,
    #[schema(additional_properties = true)]
    auth_config: Map<String, Value>,
    timeout_ms: i64,
    status: String,
    last_discovery_status: Option<String>,
    last_discovery_at: Option<String>,
    last_successful_discovery_at: Option<String>,
    last_error_summary: Option<String>,
    last_tool_count: Option<i64>,
    created_at: String,
    updated_at: String,
    disabled_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpToolsPayload {
    pub(super) items: Vec<McpToolView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpToolView {
    id: String,
    server_id: String,
    upstream_name: String,
    display_name: String,
    description: Option<String>,
    #[schema(additional_properties = true)]
    input_schema: Value,
    schema_hash: String,
    schema_version: i64,
    is_active: bool,
    first_discovered_at: String,
    last_discovered_at: String,
    deactivated_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpDiscoveryRefreshPayload {
    server: McpServerView,
    status: String,
    error_summary: Option<String>,
    tools: Vec<McpToolView>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct McpToolsetsQuery {
    #[serde(default)]
    pub(super) include_disabled: bool,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct McpGrantsQuery {
    pub(super) subject_kind: Option<String>,
    pub(super) subject_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct McpEffectiveAccessQuery {
    pub(super) api_key_id: Option<Uuid>,
    pub(super) user_id: Option<Uuid>,
    pub(super) service_account_id: Option<Uuid>,
    pub(super) team_id: Option<Uuid>,
    pub(super) server_id: Option<Uuid>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpToolsetsPayload {
    pub(super) items: Vec<McpToolsetView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpToolsetPayload {
    pub(super) toolset: McpToolsetView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpToolsetView {
    id: String,
    toolset_key: String,
    display_name: String,
    description: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
    disabled_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpToolsetToolsPayload {
    pub(super) tool_ids: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpGrantsPayload {
    pub(super) items: Vec<McpGrantView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpGrantPayload {
    pub(super) grant: McpGrantView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpGrantView {
    id: String,
    subject_kind: String,
    subject_id: String,
    target_kind: String,
    target_id: String,
    is_active: bool,
    created_at: String,
    updated_at: String,
    revoked_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpEffectiveAccessPayload {
    pub(super) referenced_server_count: i64,
    pub(super) exposed_tool_count: i64,
    pub(super) filtered_tool_count: i64,
    pub(super) tools: Vec<McpToolView>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct McpCredentialBindingsQuery {
    pub(super) server_id: Option<Uuid>,
    pub(super) owner_scope_kind: Option<String>,
    pub(super) owner_scope_id: Option<Uuid>,
    #[serde(default)]
    pub(super) include_revoked: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpCredentialBindingsPayload {
    pub(super) items: Vec<McpCredentialBindingView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpCredentialBindingPayload {
    pub(super) binding: McpCredentialBindingView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpCredentialBindingView {
    id: String,
    server_id: String,
    owner_scope_kind: String,
    owner_scope_key: String,
    owner_user_id: Option<String>,
    owner_team_id: Option<String>,
    owner_service_account_id: Option<String>,
    material_kind: String,
    header_name: Option<String>,
    storage_kind: String,
    secret_ref: Option<String>,
    expires_at: Option<String>,
    created_at: String,
    updated_at: String,
    last_used_at: Option<String>,
    revoked_at: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateMcpToolsetRequest {
    pub(super) toolset_key: String,
    pub(super) display_name: String,
    pub(super) description: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateMcpToolsetRequest {
    pub(super) display_name: String,
    pub(super) description: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ReplaceMcpToolsetToolsRequest {
    pub(super) tool_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpsertMcpGrantRequest {
    pub(super) subject_kind: String,
    pub(super) subject_id: Uuid,
    pub(super) target_kind: String,
    pub(super) target_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpsertMcpCredentialBindingRequest {
    pub(super) credential_binding_id: Option<Uuid>,
    pub(super) server_id: Uuid,
    pub(super) owner_scope_kind: String,
    pub(super) owner_user_id: Option<Uuid>,
    pub(super) owner_team_id: Option<Uuid>,
    pub(super) owner_service_account_id: Option<Uuid>,
    pub(super) material_kind: String,
    pub(super) header_name: Option<String>,
    pub(super) secret: Option<String>,
    pub(super) secret_ref: Option<String>,
    pub(super) expires_at: Option<time::OffsetDateTime>,
    #[serde(default)]
    #[schema(additional_properties = true)]
    pub(super) metadata: Map<String, Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateMcpServerRequest {
    pub(super) recommended_catalog_key: Option<String>,
    pub(super) server_key: Option<String>,
    pub(super) display_name: Option<String>,
    pub(super) description: Option<String>,
    pub(super) server_url: Option<String>,
    pub(super) transport: Option<String>,
    pub(super) auth_mode: Option<String>,
    #[serde(default)]
    #[schema(additional_properties = true)]
    pub(super) auth_config: Map<String, Value>,
    pub(super) timeout_ms: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateMcpServerRequest {
    pub(super) display_name: String,
    pub(super) description: Option<String>,
    pub(super) server_url: String,
    pub(super) auth_mode: String,
    #[serde(default)]
    #[schema(additional_properties = true)]
    pub(super) auth_config: Map<String, Value>,
    pub(super) timeout_ms: Option<i64>,
}

pub(super) fn map_recommended_server(
    entry: RecommendedMcpServerCatalogEntry,
) -> RecommendedMcpServerView {
    RecommendedMcpServerView {
        catalog_key: entry.catalog_key,
        display_name: entry.display_name,
        description: entry.description,
        transport: entry.transport,
        server_url: entry.server_url,
        auth_mode: entry.auth_mode,
        auth_config: entry.auth_config,
        documentation_url: entry.documentation_url,
        tags: entry.tags,
    }
}

pub(super) fn map_server(server: ExternalMcpServerRecord) -> McpServerView {
    let auth_config = sanitized_auth_config(server.auth_mode, &server.auth_config);
    McpServerView {
        id: server.mcp_server_id.to_string(),
        server_key: server.server_key,
        display_name: server.display_name,
        description: server.description,
        transport: server.transport.as_str().to_string(),
        server_url: server.server_url,
        auth_mode: server.auth_mode.as_str().to_string(),
        auth_config,
        timeout_ms: server.timeout_ms,
        status: server.status.as_str().to_string(),
        last_discovery_status: server
            .last_discovery_status
            .map(|status| status.as_str().to_string()),
        last_discovery_at: server.last_discovery_at.map(format_timestamp),
        last_successful_discovery_at: server.last_successful_discovery_at.map(format_timestamp),
        last_error_summary: server.last_error_summary,
        last_tool_count: server.last_tool_count,
        created_at: format_timestamp(server.created_at),
        updated_at: format_timestamp(server.updated_at),
        disabled_at: server.disabled_at.map(format_timestamp),
    }
}

pub(super) fn map_credential_binding(
    binding: RedactedMcpCredentialBinding,
) -> McpCredentialBindingView {
    McpCredentialBindingView {
        id: binding.credential_binding_id.to_string(),
        server_id: binding.mcp_server_id.to_string(),
        owner_scope_kind: binding.owner_scope_kind.as_str().to_string(),
        owner_scope_key: binding.owner_scope_key,
        owner_user_id: binding.owner_user_id.map(|value| value.to_string()),
        owner_team_id: binding.owner_team_id.map(|value| value.to_string()),
        owner_service_account_id: binding
            .owner_service_account_id
            .map(|value| value.to_string()),
        material_kind: binding.material_kind.as_str().to_string(),
        header_name: binding.header_name,
        storage_kind: binding.storage_kind.as_str().to_string(),
        secret_ref: binding.secret_ref,
        expires_at: binding.expires_at.map(format_timestamp),
        created_at: format_timestamp(binding.created_at),
        updated_at: format_timestamp(binding.updated_at),
        last_used_at: binding.last_used_at.map(format_timestamp),
        revoked_at: binding.revoked_at.map(format_timestamp),
    }
}

pub(super) fn map_tool(tool: ExternalMcpToolRecord) -> McpToolView {
    McpToolView {
        id: tool.mcp_tool_id.to_string(),
        server_id: tool.mcp_server_id.to_string(),
        upstream_name: tool.upstream_name,
        display_name: tool.display_name,
        description: tool.description,
        input_schema: tool.input_schema,
        schema_hash: tool.schema_hash,
        schema_version: tool.schema_version,
        is_active: tool.is_active,
        first_discovered_at: format_timestamp(tool.first_discovered_at),
        last_discovered_at: format_timestamp(tool.last_discovered_at),
        deactivated_at: tool.deactivated_at.map(format_timestamp),
    }
}

pub(super) fn map_discovery_result(result: McpDiscoveryResult) -> McpDiscoveryRefreshPayload {
    McpDiscoveryRefreshPayload {
        server: map_server(result.server),
        status: result.status.as_str().to_string(),
        error_summary: result.error_summary,
        tools: result.tools.into_iter().map(map_tool).collect(),
    }
}

pub(super) fn map_toolset(toolset: McpToolsetRecord) -> McpToolsetView {
    McpToolsetView {
        id: toolset.toolset_id.to_string(),
        toolset_key: toolset.toolset_key,
        display_name: toolset.display_name,
        description: toolset.description,
        status: toolset.status.as_str().to_string(),
        created_at: format_timestamp(toolset.created_at),
        updated_at: format_timestamp(toolset.updated_at),
        disabled_at: toolset.disabled_at.map(format_timestamp),
    }
}

pub(super) fn map_grant(grant: McpToolGrantRecord) -> McpGrantView {
    McpGrantView {
        id: grant.grant_id.to_string(),
        subject_kind: grant.subject_kind.as_str().to_string(),
        subject_id: grant.subject_id.to_string(),
        target_kind: grant.target_kind.as_str().to_string(),
        target_id: grant.target_id.to_string(),
        is_active: grant.is_active,
        created_at: format_timestamp(grant.created_at),
        updated_at: format_timestamp(grant.updated_at),
        revoked_at: grant.revoked_at.map(format_timestamp),
    }
}

pub(super) fn parse_grant_subject_kind(
    value: &str,
) -> Result<McpToolGrantSubjectKind, GatewayError> {
    McpToolGrantSubjectKind::from_db(value).ok_or_else(|| {
        GatewayError::InvalidRequest(format!(
            "invalid MCP grant subject_kind `{value}`; expected api_key, user, team, or service_account"
        ))
    })
}

pub(super) fn parse_grant_target_kind(value: &str) -> Result<McpToolGrantTargetKind, GatewayError> {
    McpToolGrantTargetKind::from_db(value).ok_or_else(|| {
        GatewayError::InvalidRequest(format!(
            "invalid MCP grant target_kind `{value}`; expected tool or toolset"
        ))
    })
}

pub(super) fn parse_credential_owner_scope_kind(
    value: &str,
) -> Result<McpUpstreamCredentialOwnerScopeKind, GatewayError> {
    McpUpstreamCredentialOwnerScopeKind::from_db(value).ok_or_else(|| {
        GatewayError::InvalidRequest(format!(
            "invalid MCP credential owner_scope_kind `{value}`; expected user, team, or service_account"
        ))
    })
}

pub(super) fn parse_credential_material_kind(
    value: &str,
) -> Result<McpUpstreamCredentialMaterialKind, GatewayError> {
    McpUpstreamCredentialMaterialKind::from_db(value).ok_or_else(|| {
        GatewayError::InvalidRequest(format!(
            "invalid MCP credential material_kind `{value}`; expected static_header, bearer_token, or oauth_tokens"
        ))
    })
}

fn sanitized_auth_config(
    auth_mode: ExternalMcpAuthMode,
    auth_config: &Map<String, Value>,
) -> Map<String, Value> {
    let allowed_fields: &[&str] = match auth_mode {
        ExternalMcpAuthMode::None => &[],
        ExternalMcpAuthMode::GatewayStaticHeader => &["header_name", "secret_ref"],
        ExternalMcpAuthMode::GatewayBearerToken => &["secret_ref"],
        ExternalMcpAuthMode::UserPassthrough => &["header", "token_type"],
        ExternalMcpAuthMode::OauthObo => &["token_exchange", "token_type"],
    };
    allowed_fields
        .iter()
        .filter_map(|field| {
            auth_config
                .get(*field)
                .map(|value| ((*field).to_string(), value.clone()))
        })
        .collect()
}
