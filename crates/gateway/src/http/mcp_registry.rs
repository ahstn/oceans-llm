mod views;

use std::time::Instant;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_core::{
    GatewayError, McpAccessRepository, McpToolGrantSubjectKind, NewMcpToolsetRecord,
    UpdateMcpToolsetRecord, UpsertMcpToolGrantRecord,
};
use gateway_service::{
    CreateExternalMcpServerInput, McpRegistryService, UpdateExternalMcpServerInput,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{Envelope, OpenAiErrorEnvelopeView, envelope},
    error::AppError,
    state::AppState,
};
use crate::observability::McpDiscoveryRefreshMetric;
use views::*;

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
#[tracing::instrument(skip(state, headers), fields(mcp_server_id = %server_id))]
pub async fn refresh_mcp_server_discovery(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(server_id): Path<String>,
) -> Result<Json<Envelope<McpDiscoveryRefreshPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let service = McpRegistryService::new(state.store.clone());
    let started_at = Instant::now();
    let result = service.refresh_discovery(server_id).await;
    let latency_seconds = started_at.elapsed().as_secs_f64();
    match result {
        Ok(result) => {
            let status = result.status.as_str();
            let metric_result = if status == "success" {
                "success"
            } else {
                "failure"
            };
            state
                .metrics
                .record_mcp_discovery_refresh(&McpDiscoveryRefreshMetric {
                    server_id: &server_id.to_string(),
                    result: metric_result,
                    status,
                    latency_seconds,
                });
            Ok(Json(envelope(map_discovery_result(result))))
        }
        Err(error) => {
            state
                .metrics
                .record_mcp_discovery_refresh(&McpDiscoveryRefreshMetric {
                    server_id: &server_id.to_string(),
                    result: "error",
                    status: error.error_code(),
                    latency_seconds,
                });
            Err(error.into())
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/toolsets",
    params(McpToolsetsQuery),
    responses((status = 200, body = Envelope<McpToolsetsPayload>)),
    security(("session_cookie" = []))
)]
pub async fn list_mcp_toolsets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<McpToolsetsQuery>,
) -> Result<Json<Envelope<McpToolsetsPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let items = state
        .store
        .list_mcp_toolsets(query.include_disabled)
        .await?
        .into_iter()
        .map(map_toolset)
        .collect();
    Ok(Json(envelope(McpToolsetsPayload { items })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp/toolsets",
    request_body = CreateMcpToolsetRequest,
    responses((status = 200, body = Envelope<McpToolsetPayload>)),
    security(("session_cookie" = []))
)]
pub async fn create_mcp_toolset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateMcpToolsetRequest>,
) -> Result<Json<Envelope<McpToolsetPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let toolset = state
        .store
        .create_mcp_toolset(&NewMcpToolsetRecord {
            toolset_key: request.toolset_key,
            display_name: request.display_name,
            description: request.description,
            created_at: OffsetDateTime::now_utc(),
        })
        .await?;
    Ok(Json(envelope(McpToolsetPayload {
        toolset: map_toolset(toolset),
    })))
}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/mcp/toolsets/{toolset_id}",
    request_body = UpdateMcpToolsetRequest,
    responses((status = 200, body = Envelope<McpToolsetPayload>)),
    security(("session_cookie" = []))
)]
pub async fn update_mcp_toolset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(toolset_id): Path<Uuid>,
    Json(request): Json<UpdateMcpToolsetRequest>,
) -> Result<Json<Envelope<McpToolsetPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let toolset = state
        .store
        .update_mcp_toolset(&UpdateMcpToolsetRecord {
            toolset_id,
            display_name: request.display_name,
            description: request.description,
            updated_at: OffsetDateTime::now_utc(),
        })
        .await?;
    Ok(Json(envelope(McpToolsetPayload {
        toolset: map_toolset(toolset),
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp/toolsets/{toolset_id}/disable",
    responses((status = 200, body = Envelope<McpToolsetPayload>)),
    security(("session_cookie" = []))
)]
pub async fn disable_mcp_toolset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(toolset_id): Path<Uuid>,
) -> Result<Json<Envelope<McpToolsetPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let toolset = state
        .store
        .disable_mcp_toolset(toolset_id, OffsetDateTime::now_utc())
        .await?;
    Ok(Json(envelope(McpToolsetPayload {
        toolset: map_toolset(toolset),
    })))
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/mcp/toolsets/{toolset_id}/tools",
    request_body = ReplaceMcpToolsetToolsRequest,
    responses((status = 200, body = Envelope<McpToolsetToolsPayload>)),
    security(("session_cookie" = []))
)]
pub async fn replace_mcp_toolset_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(toolset_id): Path<Uuid>,
    Json(request): Json<ReplaceMcpToolsetToolsRequest>,
) -> Result<Json<Envelope<McpToolsetToolsPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let tools = state
        .store
        .replace_mcp_toolset_tools(toolset_id, &request.tool_ids, OffsetDateTime::now_utc())
        .await?;
    Ok(Json(envelope(McpToolsetToolsPayload {
        tool_ids: tools
            .into_iter()
            .map(|tool| tool.mcp_tool_id.to_string())
            .collect(),
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/grants",
    params(McpGrantsQuery),
    responses((status = 200, body = Envelope<McpGrantsPayload>)),
    security(("session_cookie" = []))
)]
pub async fn list_mcp_grants(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<McpGrantsQuery>,
) -> Result<Json<Envelope<McpGrantsPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let subject_kind = query
        .subject_kind
        .as_deref()
        .map(parse_grant_subject_kind)
        .transpose()?;
    let items = state
        .store
        .list_mcp_tool_grants(subject_kind, query.subject_id)
        .await?
        .into_iter()
        .map(map_grant)
        .collect();
    Ok(Json(envelope(McpGrantsPayload { items })))
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/mcp/grants",
    request_body = UpsertMcpGrantRequest,
    responses((status = 200, body = Envelope<McpGrantPayload>)),
    security(("session_cookie" = []))
)]
pub async fn upsert_mcp_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UpsertMcpGrantRequest>,
) -> Result<Json<Envelope<McpGrantPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let grant = state
        .store
        .upsert_mcp_tool_grant(&UpsertMcpToolGrantRecord {
            subject_kind: parse_grant_subject_kind(&request.subject_kind)?,
            subject_id: request.subject_id,
            target_kind: parse_grant_target_kind(&request.target_kind)?,
            target_id: request.target_id,
            updated_at: OffsetDateTime::now_utc(),
        })
        .await?;
    Ok(Json(envelope(McpGrantPayload {
        grant: map_grant(grant),
    })))
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/mcp/grants",
    request_body = UpsertMcpGrantRequest,
    responses((status = 200, body = Envelope<McpGrantsPayload>)),
    security(("session_cookie" = []))
)]
pub async fn revoke_mcp_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UpsertMcpGrantRequest>,
) -> Result<Json<Envelope<McpGrantsPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    state
        .store
        .revoke_mcp_tool_grant(
            parse_grant_subject_kind(&request.subject_kind)?,
            request.subject_id,
            parse_grant_target_kind(&request.target_kind)?,
            request.target_id,
            OffsetDateTime::now_utc(),
        )
        .await?;
    Ok(Json(envelope(McpGrantsPayload { items: Vec::new() })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/effective-access",
    params(McpEffectiveAccessQuery),
    responses((status = 200, body = Envelope<McpEffectiveAccessPayload>)),
    security(("session_cookie" = []))
)]
pub async fn preview_mcp_effective_access(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<McpEffectiveAccessQuery>,
) -> Result<Json<Envelope<McpEffectiveAccessPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let mut subjects = Vec::new();
    if let Some(api_key_id) = query.api_key_id {
        subjects.push(gateway_core::McpGrantSubject {
            subject_kind: McpToolGrantSubjectKind::ApiKey,
            subject_id: api_key_id,
        });
    }
    if let Some(user_id) = query.user_id {
        subjects.push(gateway_core::McpGrantSubject {
            subject_kind: McpToolGrantSubjectKind::User,
            subject_id: user_id,
        });
    }
    if let Some(service_account_id) = query.service_account_id {
        subjects.push(gateway_core::McpGrantSubject {
            subject_kind: McpToolGrantSubjectKind::ServiceAccount,
            subject_id: service_account_id,
        });
    }
    if let Some(team_id) = query.team_id {
        subjects.push(gateway_core::McpGrantSubject {
            subject_kind: McpToolGrantSubjectKind::Team,
            subject_id: team_id,
        });
    }
    if subjects.is_empty() {
        return Err(GatewayError::InvalidRequest(
            "at least one access preview subject is required".to_string(),
        )
        .into());
    }
    let resolution = state
        .store
        .resolve_mcp_access_for_subjects(&subjects, query.server_id)
        .await?;
    Ok(Json(envelope(McpEffectiveAccessPayload {
        referenced_server_count: resolution.referenced_server_count,
        exposed_tool_count: resolution.exposed_tool_count,
        filtered_tool_count: resolution.filtered_tool_count,
        tools: resolution.allowed_tools.into_iter().map(map_tool).collect(),
    })))
}

fn parse_uuid(raw: &str, field_name: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| {
        AppError(GatewayError::InvalidRequest(format!(
            "{field_name} must be a valid uuid"
        )))
    })
}
