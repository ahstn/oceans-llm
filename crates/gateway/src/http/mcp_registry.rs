use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    ExternalMcpAuthMode, ExternalMcpServerRecord, ExternalMcpToolRecord, GatewayError,
};
use gateway_service::{
    CreateExternalMcpServerInput, McpDiscoveryResult, McpRegistryService,
    RecommendedMcpServerCatalogEntry, UpdateExternalMcpServerInput,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{Envelope, OpenAiErrorEnvelopeView, envelope, format_timestamp},
    error::AppError,
    state::AppState,
};

#[derive(Debug, Deserialize, IntoParams)]
pub struct McpServersQuery {
    #[serde(default)]
    include_disabled: bool,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct McpToolsQuery {
    #[serde(default)]
    include_inactive: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RecommendedMcpServersPayload {
    items: Vec<RecommendedMcpServerView>,
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
    items: Vec<McpServerView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct McpServerPayload {
    server: McpServerView,
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
    items: Vec<McpToolView>,
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateMcpServerRequest {
    recommended_catalog_key: Option<String>,
    server_key: Option<String>,
    display_name: Option<String>,
    description: Option<String>,
    server_url: Option<String>,
    transport: Option<String>,
    auth_mode: Option<String>,
    #[serde(default)]
    #[schema(additional_properties = true)]
    auth_config: Map<String, Value>,
    timeout_ms: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateMcpServerRequest {
    display_name: String,
    description: Option<String>,
    server_url: String,
    auth_mode: String,
    #[serde(default)]
    #[schema(additional_properties = true)]
    auth_config: Map<String, Value>,
    timeout_ms: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/recommended-servers",
    responses((status = 200, body = Envelope<RecommendedMcpServersPayload>)),
    security(("session_cookie" = []))
)]
pub async fn list_recommended_mcp_servers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<RecommendedMcpServersPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let service = McpRegistryService::new(state.store.clone());
    let items = service
        .recommended_servers()?
        .into_iter()
        .map(map_recommended_server)
        .collect();
    Ok(Json(envelope(RecommendedMcpServersPayload { items })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/servers",
    params(McpServersQuery),
    responses((status = 200, body = Envelope<McpServersPayload>)),
    security(("session_cookie" = []))
)]
pub async fn list_mcp_servers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<McpServersQuery>,
) -> Result<Json<Envelope<McpServersPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let service = McpRegistryService::new(state.store.clone());
    let items = service
        .list_servers(query.include_disabled)
        .await?
        .into_iter()
        .map(map_server)
        .collect();
    Ok(Json(envelope(McpServersPayload { items })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp/servers",
    request_body = CreateMcpServerRequest,
    responses((status = 200, body = Envelope<McpServerPayload>)),
    security(("session_cookie" = []))
)]
pub async fn create_mcp_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateMcpServerRequest>,
) -> Result<Json<Envelope<McpServerPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let service = McpRegistryService::new(state.store.clone());
    let server = service
        .create_server(CreateExternalMcpServerInput {
            recommended_catalog_key: request.recommended_catalog_key,
            server_key: request.server_key,
            display_name: request.display_name,
            description: request.description,
            server_url: request.server_url,
            transport: request.transport,
            auth_mode: request.auth_mode,
            auth_config: request.auth_config,
            timeout_ms: request.timeout_ms,
        })
        .await?;
    Ok(Json(envelope(McpServerPayload {
        server: map_server(server),
    })))
}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/mcp/servers/{server_id}",
    request_body = UpdateMcpServerRequest,
    params(("server_id" = String, Path, description = "External MCP server identifier")),
    responses(
        (status = 200, body = Envelope<McpServerPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn update_mcp_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
    Json(request): Json<UpdateMcpServerRequest>,
) -> Result<Json<Envelope<McpServerPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let service = McpRegistryService::new(state.store.clone());
    let server = service
        .update_server(
            server_id,
            UpdateExternalMcpServerInput {
                display_name: request.display_name,
                description: request.description,
                server_url: request.server_url,
                auth_mode: request.auth_mode,
                auth_config: request.auth_config,
                timeout_ms: request.timeout_ms,
            },
        )
        .await?;
    Ok(Json(envelope(McpServerPayload {
        server: map_server(server),
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp/servers/{server_id}/disable",
    params(("server_id" = String, Path, description = "External MCP server identifier")),
    responses(
        (status = 200, body = Envelope<McpServerPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn disable_mcp_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<Json<Envelope<McpServerPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let service = McpRegistryService::new(state.store.clone());
    let server = service.disable_server(server_id).await?;
    Ok(Json(envelope(McpServerPayload {
        server: map_server(server),
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/servers/{server_id}/tools",
    params(
        ("server_id" = String, Path, description = "External MCP server identifier"),
        McpToolsQuery
    ),
    responses(
        (status = 200, body = Envelope<McpToolsPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn list_mcp_server_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
    Query(query): Query<McpToolsQuery>,
) -> Result<Json<Envelope<McpToolsPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let service = McpRegistryService::new(state.store.clone());
    let items = service
        .list_tools(server_id, query.include_inactive)
        .await?
        .into_iter()
        .map(map_tool)
        .collect();
    Ok(Json(envelope(McpToolsPayload { items })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp/servers/{server_id}/discovery-refresh",
    params(("server_id" = String, Path, description = "External MCP server identifier")),
    responses(
        (status = 200, body = Envelope<McpDiscoveryRefreshPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn refresh_mcp_server_discovery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<Json<Envelope<McpDiscoveryRefreshPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let service = McpRegistryService::new(state.store.clone());
    Ok(Json(envelope(map_discovery_result(
        service.refresh_discovery(server_id).await?,
    ))))
}

fn map_recommended_server(entry: RecommendedMcpServerCatalogEntry) -> RecommendedMcpServerView {
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

fn map_server(server: ExternalMcpServerRecord) -> McpServerView {
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

fn map_tool(tool: ExternalMcpToolRecord) -> McpToolView {
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

fn map_discovery_result(result: McpDiscoveryResult) -> McpDiscoveryRefreshPayload {
    McpDiscoveryRefreshPayload {
        server: map_server(result.server),
        status: result.status.as_str().to_string(),
        error_summary: result.error_summary,
        tools: result.tools.into_iter().map(map_tool).collect(),
    }
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

fn parse_uuid(raw: &str, field_name: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| {
        AppError(GatewayError::InvalidRequest(format!(
            "{field_name} must be a valid uuid"
        )))
    })
}
