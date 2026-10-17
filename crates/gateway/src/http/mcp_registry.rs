mod tool_policy;
mod tool_reviews;
mod views;

#[cfg(test)]
mod tests;

use std::time::Instant;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_client_config::{
    DEFAULT_GATEWAY_BASE_URL, normalize_gateway_base_url, render_mcp_client_configs,
};
use gateway_core::{
    GatewayError, McpAccessRepository, McpGrantSubject, McpRegistryRepository,
    McpToolGrantSubjectKind, McpToolProjection, NewMcpToolsetRecord, UpdateMcpToolsetRecord,
    UpsertMcpToolGrantRecord,
};
use gateway_service::{
    AuditAction, AuditEvent, AuditProgress, CreateExternalMcpServerInput, McpCredentialService,
    McpRegistryService, UpdateExternalMcpServerInput, UpsertMcpCredentialBindingInput,
    admin_audit::actions,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{Envelope, OpenAiErrorEnvelopeView, envelope},
    audit::AuditRequestMeta,
    error::AppError,
    state::AppState,
};
use crate::observability::McpDiscoveryRefreshMetric;
pub use tool_policy::*;
pub use tool_reviews::*;
use views::*;

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/connection-info",
    responses(
        (status = 200, body = Envelope<McpConnectionInfoPayload>),
        (status = 500, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn get_mcp_connection_info(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<McpConnectionInfoPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let gateway_base_url = state
        .client_config_gateway_base_url
        .as_ref()
        .as_deref()
        .unwrap_or(DEFAULT_GATEWAY_BASE_URL);
    let invalid_base_url = || {
        AppError(GatewayError::Internal(
            "GATEWAY_CLIENT_CONFIG_BASE_URL must be an HTTP(S) URL with a host and no credentials, query, or fragment for MCP connections".to_string(),
        ))
    };
    let parsed = url::Url::parse(gateway_base_url).map_err(|_| invalid_base_url())?;
    if gateway_base_url.trim() != gateway_base_url
        || !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(invalid_base_url());
    }
    let endpoint = format!("{}/mcp", normalize_gateway_base_url(gateway_base_url));
    let client_configurations = render_mcp_client_configs(&endpoint)
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(envelope(McpConnectionInfoPayload {
        endpoint,
        client_configurations,
    })))
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
    meta: AuditRequestMeta,
    Json(request): Json<CreateMcpServerRequest>,
) -> Result<Json<Envelope<McpServerPayload>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
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
            approve_tool_changes: request.approve_tool_changes,
        })
        .await?;
    let server = map_server(server);
    state
        .audit()
        .record(
            &meta.user_context(&actor),
            server_event(actions::MCP_SERVER_CREATED, &server).after(server_audit_summary(&server)),
        )
        .await;
    Ok(Json(envelope(McpServerPayload { server })))
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
    meta: AuditRequestMeta,
    Path(server_id): Path<String>,
    Json(request): Json<UpdateMcpServerRequest>,
) -> Result<Json<Envelope<McpServerPayload>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let service = McpRegistryService::new(state.store.clone());
    let update = service
        .update_server(
            server_id,
            UpdateExternalMcpServerInput {
                display_name: request.display_name,
                description: request.description,
                server_url: request.server_url,
                auth_mode: request.auth_mode,
                auth_config: request.auth_config,
                timeout_ms: request.timeout_ms,
                approve_tool_changes: request.approve_tool_changes,
            },
        )
        .await?;
    let server = map_server(update.current);
    // The store compares the unredacted settings under the write lock and writes
    // nothing when they match, so a resave records nothing while rotating a
    // credential inside the URL still counts even though the summary hides it.
    if let Some(previous) = update.previous {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                server_event(actions::MCP_SERVER_UPDATED, &server)
                    .before(server_audit_summary(&map_server(previous)))
                    .after(server_audit_summary(&server)),
            )
            .await;
    }
    Ok(Json(envelope(McpServerPayload { server })))
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
    meta: AuditRequestMeta,
    Path(server_id): Path<String>,
) -> Result<Json<Envelope<McpServerPayload>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let before = server_before_summary(&state, server_id).await?;
    let service = McpRegistryService::new(state.store.clone());
    let (server, disabled) = service.disable_server(server_id).await?;
    let server = map_server(server);
    // Disabling a disabled server is not a transition. The store decides this in
    // its update, so concurrent requests record one entry.
    if disabled {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                server_event(actions::MCP_SERVER_DISABLED, &server)
                    .before(serde_json::json!({ "status": before["status"] }))
                    .after(serde_json::json!({ "status": "disabled" })),
            )
            .await;
    }
    Ok(Json(envelope(McpServerPayload { server })))
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
    path = "/api/v1/admin/mcp/oauth-discovery",
    request_body = McpOauthDiscoveryRequest,
    responses(
        (status = 200, body = Envelope<McpOauthDiscoveryPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
/// Reads an MCP server's OAuth metadata so an admin can register it by URL.
/// Nothing is stored; the admin saves the suggested auth config, or an
/// edited copy, with the server.
pub async fn discover_mcp_server_oauth(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<McpOauthDiscoveryRequest>,
) -> Result<Json<Envelope<McpOauthDiscoveryPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let discovery = state
        .mcp_oauth_runtime
        .discover(request.server_url.trim())
        .await?;
    let mut warnings = discovery.warnings.clone();
    if state.mcp_oauth_runtime.public_base_url().is_none() {
        warnings.push(
            "mcp.oauth.public_base_url is not configured, so users cannot connect yet.".to_string(),
        );
    }
    Ok(Json(envelope(McpOauthDiscoveryPayload {
        suggested_auth_config: discovery.suggested_auth_config(),
        callback_url: state
            .mcp_oauth_runtime
            .public_base_url()
            .map(|base| format!("{base}/api/v1/mcp/oauth/callback")),
        client_metadata_document_url: state.mcp_oauth_runtime.client_metadata_document_url().ok(),
        resource: discovery.resource,
        resource_metadata_url: discovery.resource_metadata_url,
        authorization_servers: discovery.authorization_servers,
        issuer: discovery.issuer,
        authorization_server_metadata_url: discovery.authorization_server_metadata_url,
        authorization_endpoint: discovery.authorization_endpoint,
        token_endpoint: discovery.token_endpoint,
        registration_endpoint: discovery.registration_endpoint,
        scopes_supported: discovery.scopes_supported,
        default_scopes: discovery.default_scopes,
        token_endpoint_auth_methods_supported: discovery.token_endpoint_auth_methods_supported,
        code_challenge_methods_supported: discovery.code_challenge_methods_supported,
        client_id_metadata_document_supported: discovery.client_id_metadata_document_supported,
        recommended_client_registration: discovery.recommended_client_registration.to_string(),
        resource_indicator: discovery.resource_indicator,
        warnings,
    })))
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
#[tracing::instrument(skip(state, headers, meta), fields(mcp_server_id = %server_id))]
pub async fn refresh_mcp_server_discovery(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(server_id): Path<String>,
) -> Result<Json<Envelope<McpDiscoveryRefreshPayload>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let service = McpRegistryService::new(state.store.clone());
    let started_at = Instant::now();
    let progress = AuditProgress::default();
    let result = service
        .refresh_discovery_tracked(server_id, &progress)
        .await;
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
            // A refresh that reached the server but failed discovery still
            // rewrote the server's discovery state, so it is audited too.
            let event = AuditEvent::new(actions::MCP_SERVER_DISCOVERY_REFRESHED)
                .target(server_id)
                .label(result.server.server_key.clone())
                .meta("status", status)
                .meta("error_summary", &result.error_summary)
                .after(serde_json::json!({ "tool_count": result.tools.len() }));
            state
                .audit()
                .record(&meta.user_context(&actor), event)
                .await;
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
            // The discovery run was stored, but reloading the server or tools failed.
            if progress.has_landed() {
                let event = AuditEvent::new(actions::MCP_SERVER_DISCOVERY_REFRESHED)
                    .target(server_id)
                    .meta("error", error.error_code())
                    .meta("outcome", "partial");
                state
                    .audit()
                    .record(&meta.user_context(&actor), event)
                    .await;
            }
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
    meta: AuditRequestMeta,
    Json(request): Json<CreateMcpToolsetRequest>,
) -> Result<Json<Envelope<McpToolsetPayload>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
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
                toolset_event(actions::MCP_TOOLSET_CREATED, &map_toolset(toolset.clone()))
                    .after(map_toolset(toolset.clone()))
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
    let actor = require_platform_admin(&state, &headers).await?;
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
                toolset_event(actions::MCP_TOOLSET_UPDATED, &map_toolset(toolset.clone()))
                    .before(toolset_details(&previous))
                    .after(toolset_details(&toolset)),
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
    let actor = require_platform_admin(&state, &headers).await?;
    let toolset = state
        .audit()
        .audited(
            &meta.user_context(&actor),
            state
                .store
                .disable_mcp_toolset(toolset_id, OffsetDateTime::now_utc()),
            |toolset| {
                toolset_event(actions::MCP_TOOLSET_DISABLED, &map_toolset(toolset.clone()))
                    .after(serde_json::json!({ "status": toolset.status.as_str() }))
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
    require_platform_admin(&state, &headers).await?;
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
    let actor = require_platform_admin(&state, &headers).await?;
    let sorted_ids = |tools: &[gateway_core::McpToolsetToolRecord]| {
        let mut ids: Vec<String> = tools
            .iter()
            .map(|tool| tool.mcp_tool_id.to_string())
            .collect();
        ids.sort();
        ids
    };
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
                match state.store.get_mcp_toolset(toolset_id).await? {
                    Some(toolset) => {
                        toolset_event(actions::MCP_TOOLSET_TOOLS_REPLACED, &map_toolset(toolset))
                    }
                    None => AuditEvent::new(actions::MCP_TOOLSET_TOOLS_REPLACED).target(toolset_id),
                }
                .before(serde_json::json!({ "tool_ids": before }))
                .after(serde_json::json!({ "tool_ids": after })),
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
    meta: AuditRequestMeta,
    Json(request): Json<UpsertMcpGrantRequest>,
) -> Result<Json<Envelope<McpGrantPayload>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let record = UpsertMcpToolGrantRecord {
        subject_kind: parse_grant_subject_kind(&request.subject_kind)?,
        subject_id: request.subject_id,
        target_kind: parse_grant_target_kind(&request.target_kind)?,
        target_id: request.target_id,
        updated_at: OffsetDateTime::now_utc(),
    };
    let (grant, created) = state.store.upsert_mcp_tool_grant(&record).await?;
    let grant_id = grant.grant_id;
    let grant = map_grant(grant);
    // Repeating an active grant changes no access, so only a new grant is recorded.
    if created {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                AuditEvent::new(actions::MCP_GRANT_SET)
                    .target(grant_id)
                    .after(&grant),
            )
            .await;
    }
    Ok(Json(envelope(McpGrantPayload { grant })))
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
    meta: AuditRequestMeta,
    Json(request): Json<UpsertMcpGrantRequest>,
) -> Result<Json<Envelope<McpGrantsPayload>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let subject_kind = parse_grant_subject_kind(&request.subject_kind)?;
    let target_kind = parse_grant_target_kind(&request.target_kind)?;
    // The store returns the id of the grant it revoked, so the entry targets the
    // same id as that grant's `mcp_grant.set`. Nothing active means no change.
    let revoked = state
        .store
        .revoke_mcp_tool_grant(
            subject_kind,
            request.subject_id,
            target_kind,
            request.target_id,
            OffsetDateTime::now_utc(),
        )
        .await?;
    if let Some(grant_id) = revoked {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                AuditEvent::new(actions::MCP_GRANT_REVOKED)
                    .target(grant_id)
                    .before(serde_json::json!({
                        "subject_kind": subject_kind.as_str(),
                        "subject_id": request.subject_id,
                        "target_kind": target_kind.as_str(),
                        "target_id": request.target_id,
                    })),
            )
            .await;
    }
    Ok(Json(envelope(McpGrantsPayload { items: Vec::new() })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/mcp/credential-bindings",
    params(McpCredentialBindingsQuery),
    responses(
        (status = 200, body = Envelope<McpCredentialBindingsPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn list_mcp_credential_bindings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<McpCredentialBindingsQuery>,
) -> Result<Json<Envelope<McpCredentialBindingsPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let owner_scope_kind = query
        .owner_scope_kind
        .as_deref()
        .map(parse_credential_owner_scope_kind)
        .transpose()?;
    let service = McpCredentialService::new(state.store.clone());
    let items = service
        .list_bindings(
            query.server_id,
            owner_scope_kind,
            query.owner_scope_id,
            query.include_revoked,
        )
        .await?
        .into_iter()
        .map(map_credential_binding)
        .collect();
    Ok(Json(envelope(McpCredentialBindingsPayload { items })))
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/mcp/credential-bindings",
    request_body = UpsertMcpCredentialBindingRequest,
    responses(
        (status = 200, body = Envelope<McpCredentialBindingPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn upsert_mcp_credential_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Json(request): Json<UpsertMcpCredentialBindingRequest>,
) -> Result<Json<Envelope<McpCredentialBindingPayload>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let service = McpCredentialService::new(state.store.clone());
    // The store reports whether its write replaced a binding: a caller-chosen ID
    // that does not exist yet is inserted, so it is a create.
    let (binding, replaced) = service
        .upsert_binding(UpsertMcpCredentialBindingInput {
            credential_binding_id: request.credential_binding_id,
            mcp_server_id: request.server_id,
            owner_scope_kind: parse_credential_owner_scope_kind(&request.owner_scope_kind)?,
            owner_user_id: request.owner_user_id,
            owner_team_id: request.owner_team_id,
            owner_service_account_id: request.owner_service_account_id,
            material_kind: parse_credential_material_kind(&request.material_kind)?,
            header_name: request.header_name,
            secret: request.secret,
            secret_ref: request.secret_ref,
            expires_at: request.expires_at,
            metadata: request.metadata,
        })
        .await?;
    // The service returns bindings without their secrets; only those views are
    // recorded. A replacement also records the binding it overwrote, read in the
    // same transaction, so the entry shows what access changed.
    let binding = map_credential_binding(binding);
    let replaced = replaced.map(map_credential_binding);
    let mut event = AuditEvent::new(actions::MCP_CREDENTIAL_BINDING_SET)
        .target(binding.id())
        .label(binding.owner_scope_key().to_string())
        .meta(
            "operation",
            if replaced.is_some() {
                "replace"
            } else {
                "create"
            },
        )
        .after(credential_binding_audit_summary(&binding));
    if let Some(replaced) = &replaced {
        event = event.before(credential_binding_audit_summary(replaced));
    }
    state
        .audit()
        .record(&meta.user_context(&actor), event)
        .await;
    Ok(Json(envelope(McpCredentialBindingPayload { binding })))
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/mcp/credential-bindings/{credential_binding_id}",
    params(("credential_binding_id" = String, Path, description = "MCP credential binding identifier")),
    responses((status = 200, body = Envelope<McpCredentialBindingsPayload>)),
    security(("session_cookie" = []))
)]
pub async fn revoke_mcp_credential_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(credential_binding_id): Path<String>,
) -> Result<Json<Envelope<McpCredentialBindingsPayload>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let credential_binding_id = parse_uuid(&credential_binding_id, "credential_binding_id")?;
    let service = McpCredentialService::new(state.store.clone());
    // The revoke returns the binding it changed, so the entry names its owner and
    // keeps its redacted settings. A missing or already-revoked binding records
    // nothing.
    if let Some(binding) = service.revoke_binding(credential_binding_id).await? {
        let binding = map_credential_binding(binding);
        // The returned row already carries the revocation, so `before` leaves out
        // what the revoke wrote: it matched only an active binding.
        let mut before = credential_binding_audit_summary(&binding);
        let revoked_at = before.get("revoked_at").cloned().unwrap_or_default();
        if let Some(fields) = before.as_object_mut() {
            fields.insert("revoked_at".to_string(), serde_json::Value::Null);
            fields.remove("updated_at");
        }
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                AuditEvent::new(actions::MCP_CREDENTIAL_BINDING_REVOKED)
                    .target(binding.id())
                    .label(binding.owner_scope_key().to_string())
                    .before(before)
                    .after(serde_json::json!({ "revoked_at": revoked_at })),
            )
            .await;
    }
    Ok(Json(envelope(McpCredentialBindingsPayload {
        items: Vec::new(),
    })))
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
    let selected = preview_subjects(&query);
    if selected.is_empty() {
        return Err(GatewayError::InvalidRequest(
            "at least one access preview subject is required".to_string(),
        )
        .into());
    }
    // Preview what a real caller gets: a key with its owner and team, a user
    // or service account with its team.
    let subjects = gateway_service::McpAccess::new(state.store.clone())
        .preview_subjects(&selected)
        .await?;
    let resolution = state
        .store
        .resolve_mcp_access_for_subjects(&subjects, query.server_id, McpToolProjection::Full)
        .await?;
    Ok(Json(envelope(McpEffectiveAccessPayload {
        referenced_server_count: resolution.referenced_server_count,
        exposed_tool_count: resolution.exposed_tool_count,
        filtered_tool_count: resolution.filtered_tool_count,
        tools: resolution.allowed_tools.into_iter().map(map_tool).collect(),
    })))
}

fn preview_subjects(query: &McpEffectiveAccessQuery) -> Vec<McpGrantSubject> {
    [
        query
            .api_key_id
            .map(|subject_id| (McpToolGrantSubjectKind::ApiKey, subject_id)),
        query
            .user_id
            .map(|subject_id| (McpToolGrantSubjectKind::User, subject_id)),
        query
            .service_account_id
            .map(|subject_id| (McpToolGrantSubjectKind::ServiceAccount, subject_id)),
        query
            .team_id
            .map(|subject_id| (McpToolGrantSubjectKind::Team, subject_id)),
    ]
    .into_iter()
    .flatten()
    .map(|(subject_kind, subject_id)| McpGrantSubject {
        subject_kind,
        subject_id,
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

fn server_event(action: AuditAction, server: &McpServerView) -> AuditEvent {
    AuditEvent::new(action)
        .target(server.id())
        .label(server.server_key().to_string())
}

fn toolset_event(action: AuditAction, toolset: &McpToolsetView) -> AuditEvent {
    AuditEvent::new(action)
        .target(toolset.id())
        .label(toolset.toolset_key().to_string())
}

async fn server_before_summary(
    state: &AppState,
    server_id: Uuid,
) -> Result<serde_json::Value, AppError> {
    Ok(state
        .store
        .get_external_mcp_server(server_id)
        .await?
        .map(|server| server_audit_summary(&map_server(server)))
        .unwrap_or(serde_json::Value::Null))
}

/// The audited details of a tool set.
fn toolset_details(toolset: &gateway_core::McpToolsetRecord) -> serde_json::Value {
    serde_json::json!({
        "display_name": toolset.display_name,
        "description": toolset.description,
    })
}
