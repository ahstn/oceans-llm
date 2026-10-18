//! Admin endpoints for MCP tool sets: organisation-wide groups of tools that
//! platform admins grant as one.

use super::*;

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
    require_mcp_platform_scope(&state, &headers).await?;
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
    meta: AuditRequestMeta,
    Json(request): Json<CreateMcpToolsetRequest>,
) -> Result<Json<Envelope<McpToolsetPayload>>, AppError> {
    let (actor, scope) = require_mcp_platform_scope(&state, &headers).await?;
    let record = NewMcpToolsetRecord {
        toolset_key: request.toolset_key,
        display_name: request.display_name,
        description: request.description,
        created_at: OffsetDateTime::now_utc(),
    };
    let toolset = state
        .audit()
        .audited(
            &meta.user_context(&actor),
            state.store.create_mcp_toolset(&record),
            |toolset| {
                scope.annotate(
                    toolset_event(actions::MCP_TOOLSET_CREATED, &map_toolset(toolset.clone()))
                        .after(map_toolset(toolset.clone())),
                )
            },
        )
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
    meta: AuditRequestMeta,
    Path(toolset_id): Path<Uuid>,
    Json(request): Json<UpdateMcpToolsetRequest>,
) -> Result<Json<Envelope<McpToolsetPayload>>, AppError> {
    let (actor, scope) = require_mcp_platform_scope(&state, &headers).await?;
    let record = UpdateMcpToolsetRecord {
        toolset_id,
        display_name: request.display_name,
        description: request.description,
        updated_at: OffsetDateTime::now_utc(),
    };
    let update = state.store.update_mcp_toolset(&record).await?;
    let toolset = update.current;
    // The store writes nothing when the details already match, so a resave records nothing.
    if let Some(previous) = update.previous {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                scope.annotate(
                    toolset_event(actions::MCP_TOOLSET_UPDATED, &map_toolset(toolset.clone()))
                        .before(toolset_details(&previous))
                        .after(toolset_details(&toolset)),
                ),
            )
            .await;
    }
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
    meta: AuditRequestMeta,
    Path(toolset_id): Path<Uuid>,
) -> Result<Json<Envelope<McpToolsetPayload>>, AppError> {
    let (actor, scope) = require_mcp_platform_scope(&state, &headers).await?;
    let toolset = state
        .audit()
        .audited(
            &meta.user_context(&actor),
            state
                .store
                .disable_mcp_toolset(toolset_id, OffsetDateTime::now_utc()),
            |toolset| {
                scope.annotate(
                    toolset_event(actions::MCP_TOOLSET_DISABLED, &map_toolset(toolset.clone()))
                        .after(serde_json::json!({ "status": toolset.status.as_str() })),
                )
            },
        )
        .await?;
    Ok(Json(envelope(McpToolsetPayload {
        toolset: map_toolset(toolset),
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/toolsets/{toolset_id}/tools",
    params(("toolset_id" = String, Path, description = "MCP tool set identifier")),
    responses(
        (status = 200, body = Envelope<McpToolsetToolsPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn list_mcp_toolset_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(toolset_id): Path<String>,
) -> Result<Json<Envelope<McpToolsetToolsPayload>>, AppError> {
    require_mcp_platform_scope(&state, &headers).await?;
    let toolset_id = parse_uuid(&toolset_id, "toolset_id")?;
    let tools = state.store.list_mcp_toolset_tools(toolset_id).await?;
    Ok(Json(envelope(McpToolsetToolsPayload {
        tool_ids: tools
            .into_iter()
            .map(|tool| tool.mcp_tool_id.to_string())
            .collect(),
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
    meta: AuditRequestMeta,
    Path(toolset_id): Path<Uuid>,
    Json(request): Json<ReplaceMcpToolsetToolsRequest>,
) -> Result<Json<Envelope<McpToolsetToolsPayload>>, AppError> {
    let (actor, scope) = require_mcp_platform_scope(&state, &headers).await?;
    let sorted_ids = |tools: &[gateway_core::McpToolsetToolRecord]| {
        let mut ids: Vec<String> = tools
            .iter()
            .map(|tool| tool.mcp_tool_id.to_string())
            .collect();
        ids.sort();
        ids
    };
    // Tool sets are granted organisation-wide; the store refuses team-only
    // tools under the same write, so a concurrent narrowing cannot slip one in.
    let update = state
        .store
        .replace_mcp_toolset_tools(toolset_id, &request.tool_ids, OffsetDateTime::now_utc())
        .await?;
    let tools = update.current;
    // The store writes nothing when the membership already matches, so a resave
    // changes no access and records nothing.
    if let Some(previous) = update.previous {
        let (before, after) = (sorted_ids(&previous), sorted_ids(&tools));
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                // Labelled like the other tool set entries, so it stays readable
                // after the tool set is renamed or disabled.
                scope.annotate(
                    match state.store.get_mcp_toolset(toolset_id).await? {
                        Some(toolset) => toolset_event(
                            actions::MCP_TOOLSET_TOOLS_REPLACED,
                            &map_toolset(toolset),
                        ),
                        None => {
                            AuditEvent::new(actions::MCP_TOOLSET_TOOLS_REPLACED).target(toolset_id)
                        }
                    }
                    .before(serde_json::json!({ "tool_ids": before }))
                    .after(serde_json::json!({ "tool_ids": after })),
                ),
            )
            .await;
    }
    Ok(Json(envelope(McpToolsetToolsPayload {
        tool_ids: tools
            .into_iter()
            .map(|tool| tool.mcp_tool_id.to_string())
            .collect(),
    })))
}

pub(super) fn toolset_event(action: AuditAction, toolset: &McpToolsetView) -> AuditEvent {
    AuditEvent::new(action)
        .target(toolset.id())
        .label(toolset.toolset_key().to_string())
}

/// The audited details of a tool set.
pub(super) fn toolset_details(toolset: &gateway_core::McpToolsetRecord) -> serde_json::Value {
    serde_json::json!({
        "display_name": toolset.display_name,
        "description": toolset.description,
    })
}
