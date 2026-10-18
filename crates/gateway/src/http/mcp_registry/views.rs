use gateway_core::mcp_tool_policy::{
    DESTRUCTIVE_HINT, EffectiveHints, IDEMPOTENT_HINT, OPEN_WORLD_HINT, READ_ONLY_HINT,
    effective_tags,
};
use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpServerRecord, ExternalMcpToolRecord, GatewayError,
    McpAnnotationOverrides, McpToolGrantRecord, McpToolGrantSubjectKind, McpToolGrantTargetKind,
    McpToolsetRecord, McpUpstreamCredentialMaterialKind, McpUpstreamCredentialOwnerScopeKind,
};
use gateway_service::{
    McpDiscoveryResult, RecommendedMcpServerCatalogEntry, RedactedMcpCredentialBinding,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::http::admin_contract::{AdminModelClientConfigView, format_timestamp};

#[derive(Debug, Serialize, ToSchema)]
pub struct McpConnectionInfoPayload {
    pub(super) endpoint: String,
    pub(super) client_configurations: Vec<AdminModelClientConfigView>,
}

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
    /// New and changed tools wait for an admin to approve them.
    approve_tool_changes: bool,
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
    /// Human-readable title declared by the upstream server.
    title: Option<String>,
    description: Option<String>,
    #[schema(additional_properties = true)]
    input_schema: Value,
    /// Schema of the tool's structured result, when the upstream declares one.
    #[schema(additional_properties = true)]
    output_schema: Option<Value>,
    /// Hints as the upstream server declared them.
    annotations: McpToolAnnotationsView,
    /// Hints an admin set. A missing value keeps the upstream's.
    annotation_overrides: McpToolAnnotationsView,
    /// Hints the gateway uses and serves: each override, else the upstream value.
    effective_annotations: McpToolAnnotationsView,
    /// Tags an admin attached to the tool.
    admin_tags: Vec<String>,
    /// Derived tags (`read_only`, `destructive`, `idempotent`, `open_world`)
    /// plus admin tags. Tag rules match these.
    effective_tags: Vec<String>,
    /// `approved`, `pending_change` (served as the approved version while a
    /// text change waits), `pending_review` or `rejected` (both withheld).
    approval_status: String,
    /// The served version; null until the tool is first approved.
    approved_schema_version: Option<i64>,
    /// The version waiting for review.
    pending_schema_version: Option<i64>,
    schema_hash: String,
    schema_version: i64,
    is_active: bool,
    first_discovered_at: String,
    last_discovered_at: String,
    deactivated_at: Option<String>,
}

/// The four behaviour hints. A missing value means the hint is not declared
/// at this level. Upstream hints are not verified by the gateway.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct McpToolAnnotationsView {
    #[serde(default)]
    pub(super) read_only_hint: Option<bool>,
    #[serde(default)]
    pub(super) destructive_hint: Option<bool>,
    #[serde(default)]
    pub(super) idempotent_hint: Option<bool>,
    #[serde(default)]
    pub(super) open_world_hint: Option<bool>,
}

impl McpToolAnnotationsView {
    fn from_hints(hint: impl Fn(&str) -> Option<bool>) -> Self {
        Self {
            read_only_hint: hint(READ_ONLY_HINT),
            destructive_hint: hint(DESTRUCTIVE_HINT),
            idempotent_hint: hint(IDEMPOTENT_HINT),
            open_world_hint: hint(OPEN_WORLD_HINT),
        }
    }

    pub(super) fn overrides(overrides: &McpAnnotationOverrides) -> Self {
        Self::from_hints(|name| overrides.get(name))
    }

    pub(super) fn to_overrides(&self) -> McpAnnotationOverrides {
        McpAnnotationOverrides {
            read_only: self.read_only_hint,
            destructive: self.destructive_hint,
            idempotent: self.idempotent_hint,
            open_world: self.open_world_hint,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpDiscoveryRefreshPayload {
    server: McpServerView,
    status: String,
    /// Diagnostic text for a failed discovery. Its wording may change; branch on `error_kind`.
    error_summary: Option<String>,
    /// Why a `failed` discovery failed: `transport`, `timeout`, `http`, `json_rpc`,
    /// `invalid_response`, `invalid_url`, `invalid_header`, `invalid_auth_config`,
    /// `response_too_large` or `invalid_tool_schema`. Absent for other statuses.
    error_kind: Option<String>,
    /// The upstream HTTP status when `error_kind` is `http`.
    error_http_status: Option<u16>,
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
    oauth_provider_key: Option<String>,
    granted_scopes: Vec<String>,
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
    /// Hold new and changed tools for review. Off when omitted.
    #[serde(default)]
    pub(super) approve_tool_changes: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct McpOauthDiscoveryRequest {
    pub(super) server_url: String,
}

/// What OAuth metadata discovery found for an MCP server URL.
#[derive(Debug, Serialize, ToSchema)]
pub struct McpOauthDiscoveryPayload {
    pub(super) resource: String,
    pub(super) resource_metadata_url: Option<String>,
    pub(super) authorization_servers: Vec<String>,
    pub(super) issuer: String,
    pub(super) authorization_server_metadata_url: String,
    pub(super) authorization_endpoint: String,
    pub(super) token_endpoint: String,
    pub(super) registration_endpoint: Option<String>,
    pub(super) scopes_supported: Vec<String>,
    pub(super) default_scopes: Vec<String>,
    pub(super) token_endpoint_auth_methods_supported: Vec<String>,
    pub(super) code_challenge_methods_supported: Vec<String>,
    pub(super) client_id_metadata_document_supported: bool,
    /// `dynamic`, `metadata_document`, or `manual`.
    pub(super) recommended_client_registration: String,
    pub(super) resource_indicator: bool,
    pub(super) callback_url: Option<String>,
    pub(super) client_metadata_document_url: Option<String>,
    pub(super) warnings: Vec<String>,
    /// An `oauth_obo` auth config to save with the server.
    #[schema(additional_properties = true)]
    pub(super) suggested_auth_config: Map<String, Value>,
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
    /// Hold new and changed tools for review. Kept as it is when omitted.
    #[serde(default)]
    pub(super) approve_tool_changes: Option<bool>,
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
        approve_tool_changes: server.approve_tool_changes,
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

/// Audit summary of a server. The auth config is already allowlisted by
/// [`map_server`]; credentials, query strings and fragments are stripped from the
/// server URL and from every URL in the auth config (OAuth issuer, endpoints and
/// resource).
pub(super) fn server_audit_summary(server: &McpServerView) -> Value {
    let mut summary = serde_json::to_value(server).unwrap_or(Value::Null);
    if let Some(object) = summary.as_object_mut() {
        for volatile in ["created_at", "updated_at", "last_discovery_at"] {
            object.remove(volatile);
        }
        object.insert(
            "server_url".to_string(),
            Value::String(audit_safe_url(&server.server_url)),
        );
        if let Some(Value::Object(auth_config)) = object.get_mut("auth_config") {
            for value in auth_config.values_mut() {
                if let Value::String(text) = value
                    && is_web_url(text)
                {
                    *text = audit_safe_url(text);
                }
            }
        }
    }
    summary
}

fn is_web_url(text: &str) -> bool {
    url::Url::parse(text).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// Audit summary of a credential binding. The view never holds secret material;
/// a `secret_ref` written as a URL is stripped of credentials and query like
/// other audited URLs, and plain references such as `env/NAME` are kept.
pub(super) fn credential_binding_audit_summary(binding: &McpCredentialBindingView) -> Value {
    let mut summary = serde_json::to_value(binding).unwrap_or(Value::Null);
    if let Some(object) = summary.as_object_mut()
        && let Some(Value::String(secret_ref)) = object.get_mut("secret_ref")
        && is_web_url(secret_ref)
    {
        *secret_ref = audit_safe_url(secret_ref);
    }
    summary
}

const AUDIT_REDACTED_SEGMENT: &str = "REDACTED";

pub(super) fn audit_safe_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut url) => {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            // Some servers take a token as a path segment, e.g. `/connect/<token>`.
            // Segments stay in their encoded form; `set_path` keeps existing escapes.
            let path = url.path_segments().map(|segments| {
                segments
                    .map(|segment| {
                        if gateway_service::is_secret_path_segment(segment) {
                            AUDIT_REDACTED_SEGMENT
                        } else {
                            segment
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("/")
            });
            if let Some(path) = path {
                url.set_path(&format!("/{path}"));
            }
            url.to_string()
        }
        Err(_) => "[unparseable]".to_string(),
    }
}

impl McpServerView {
    pub(super) fn id(&self) -> &str {
        &self.id
    }

    pub(super) fn server_key(&self) -> &str {
        &self.server_key
    }
}

impl McpToolsetView {
    pub(super) fn id(&self) -> &str {
        &self.id
    }

    pub(super) fn toolset_key(&self) -> &str {
        &self.toolset_key
    }
}

impl McpCredentialBindingView {
    pub(super) fn id(&self) -> &str {
        &self.id
    }

    pub(super) fn owner_scope_key(&self) -> &str {
        &self.owner_scope_key
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
        oauth_provider_key: binding.oauth_provider_key,
        granted_scopes: binding.granted_scopes,
    }
}

pub(super) fn map_tool(tool: ExternalMcpToolRecord) -> McpToolView {
    let effective = EffectiveHints::of(&tool);
    let effective_tags = effective_tags(&tool).into_iter().collect();
    McpToolView {
        id: tool.mcp_tool_id.to_string(),
        server_id: tool.mcp_server_id.to_string(),
        upstream_name: tool.upstream_name,
        display_name: tool.display_name,
        description: tool.description,
        input_schema: tool.input_schema,
        annotations: McpToolAnnotationsView::from_hints(|name| tool.metadata.hint(name)),
        annotation_overrides: McpToolAnnotationsView::overrides(&tool.policy.annotation_overrides),
        effective_annotations: McpToolAnnotationsView {
            read_only_hint: effective.read_only,
            destructive_hint: effective.destructive,
            idempotent_hint: effective.idempotent,
            open_world_hint: effective.open_world,
        },
        admin_tags: tool.policy.tags,
        effective_tags,
        approval_status: tool.review.approval_status.as_str().to_string(),
        approved_schema_version: tool.review.approved_schema_version,
        pending_schema_version: tool.review.pending_schema_version,
        title: tool.metadata.title,
        output_schema: tool.metadata.output_schema,
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
        error_kind: result.error.map(|error| error.kind.to_string()),
        error_http_status: result.error.and_then(|error| error.http_status),
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
        ExternalMcpAuthMode::GatewayStaticHeader => {
            &["header_name", "value_template", "query_param", "secret_ref"]
        }
        ExternalMcpAuthMode::GatewayBearerToken => &["secret_ref"],
        ExternalMcpAuthMode::UserPassthrough => &["header", "token_type"],
        ExternalMcpAuthMode::OauthObo => &[
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
        ],
    };
    allowed_fields
        .iter()
        .filter_map(|field| {
            auth_config
                .get(*field)
                .filter(|value| match *field {
                    "scopes" | "discovery_tool_allowlist" => value
                        .as_array()
                        .is_some_and(|items| items.iter().all(Value::is_string)),
                    "resource_indicator" => value.is_boolean(),
                    _ => value.is_string(),
                })
                .map(|value| ((*field).to_string(), value.clone()))
        })
        .collect()
}

#[cfg(test)]
mod audit_summary_tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn auth_config_urls_lose_credentials_and_queries() {
        let mut auth_config = Map::new();
        auth_config.insert(
            "resource".to_string(),
            json!("https://mcp.example/?access_key=secret"),
        );
        auth_config.insert(
            "token_endpoint".to_string(),
            json!("https://user:pass@auth.example/token#frag"),
        );
        auth_config.insert("provider_key".to_string(), json!("github"));
        let server = McpServerView {
            id: "s-1".to_string(),
            server_key: "tickets".to_string(),
            display_name: "Tickets".to_string(),
            description: None,
            transport: "streamable_http".to_string(),
            server_url: "https://mcp.example/mcp?token=abc".to_string(),
            auth_mode: "oauth_obo".to_string(),
            auth_config,
            timeout_ms: 1000,
            approve_tool_changes: false,
            status: "active".to_string(),
            last_discovery_status: None,
            last_discovery_at: None,
            last_successful_discovery_at: None,
            last_error_summary: None,
            last_tool_count: None,
            created_at: String::new(),
            updated_at: String::new(),
            disabled_at: None,
        };
        let summary = server_audit_summary(&server);
        assert_eq!(summary["server_url"], "https://mcp.example/mcp");
        assert_eq!(summary["auth_config"]["resource"], "https://mcp.example/");
        assert_eq!(
            summary["auth_config"]["token_endpoint"],
            "https://auth.example/token"
        );
        assert_eq!(summary["auth_config"]["provider_key"], "github");
        assert!(!summary.to_string().contains("secret"));
    }

    #[test]
    fn credential_binding_secret_refs_lose_url_credentials() {
        let binding = |secret_ref: &str| McpCredentialBindingView {
            id: "b-1".to_string(),
            server_id: "s-1".to_string(),
            owner_scope_kind: "user".to_string(),
            owner_scope_key: "user:1".to_string(),
            owner_user_id: Some("1".to_string()),
            owner_team_id: None,
            owner_service_account_id: None,
            material_kind: "bearer_token".to_string(),
            header_name: None,
            storage_kind: "reference".to_string(),
            secret_ref: Some(secret_ref.to_string()),
            expires_at: None,
            created_at: String::new(),
            updated_at: String::new(),
            last_used_at: None,
            revoked_at: None,
            oauth_provider_key: None,
            granted_scopes: Vec::new(),
        };
        let url = credential_binding_audit_summary(&binding(
            "https://vault.example/v1/secret/mcp?token=abc",
        ));
        assert_eq!(url["secret_ref"], "https://vault.example/v1/secret/mcp");
        let plain = credential_binding_audit_summary(&binding("env/OCEANS_MCP_DISCOVERY_X"));
        assert_eq!(plain["secret_ref"], "env/OCEANS_MCP_DISCOVERY_X");
    }

    #[test]
    fn url_path_tokens_are_redacted_and_ordinary_paths_kept() {
        assert_eq!(
            audit_safe_url("https://mcp.example/connect/a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8/sse"),
            "https://mcp.example/connect/REDACTED/sse"
        );
        assert_eq!(
            audit_safe_url("https://mcp.example/v1/mcp"),
            "https://mcp.example/v1/mcp"
        );
        let encoded = format!("https://mcp.example/connect/{}/sse", "%41".repeat(32));
        assert_eq!(
            audit_safe_url(&encoded),
            "https://mcp.example/connect/REDACTED/sse"
        );
        // Encoded segments that are not secrets keep their encoding.
        assert_eq!(
            audit_safe_url("https://mcp.example/team%20a/mcp"),
            "https://mcp.example/team%20a/mcp"
        );
    }
}
