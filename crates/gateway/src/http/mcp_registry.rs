mod views;

#[cfg(test)]
mod ownership_tests;
#[cfg(test)]
mod tests;

use std::{sync::Arc, time::Instant};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use gateway_client_config::{
    DEFAULT_GATEWAY_BASE_URL, normalize_gateway_base_url, render_mcp_client_configs,
};
use gateway_core::{
    ExternalMcpServerRecord, GatewayError, IdentityRepository, McpAccessRepository,
    McpGrantSubject, McpRegistryRepository, McpServerOwnershipGuard, McpToolGrantSubjectKind,
    McpToolProjection, NewMcpToolsetRecord, UpdateMcpToolsetRecord, UpsertMcpToolGrantRecord,
};
use gateway_service::{
    AuditAction, AuditEvent, AuditProgress, CreateExternalMcpServerInput, HttpMcpDiscoveryClient,
    McpAdminScope, McpCredentialService, McpOwnership, McpRegistryService,
    RedactedMcpCredentialBinding, RequestedMcpServerOwnership, UpdateExternalMcpServerInput,
    UpsertMcpCredentialBindingInput, admin_audit::actions, authorize_destination,
    authorize_team_auth_config, parse_mcp_server_visibility, refuse_private_network_host,
};
use gateway_store::AnyStore;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::http::{
    admin_auth::{
        McpViewer, require_mcp_admin_scope, require_mcp_platform_scope, require_mcp_viewer,
    },
    admin_contract::{Envelope, OpenAiErrorEnvelopeView, envelope},
    audit::AuditRequestMeta,
    error::AppError,
    state::AppState,
};
use crate::observability::McpDiscoveryRefreshMetric;
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
    require_mcp_viewer(&state, &headers).await?;
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
    require_mcp_admin_scope(&state, &headers).await?;
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
    let scope = match require_mcp_viewer(&state, &headers).await? {
        McpViewer::Admin(scope) => scope,
        McpViewer::Member { user_id, team_id } => {
            return list_member_servers(&state, user_id, team_id, &query).await;
        }
    };
    let owner = query.owner.as_deref().map(parse_owner_filter).transpose()?;
    let service = McpRegistryService::new(state.store.clone());
    // The owner filter runs in the store query; visibility is still the caller's.
    let servers = match (owner, scope.managed_team_id()) {
        (Some(owner), _) => {
            service
                .list_servers_owned_by(owner.team_id(), query.include_disabled)
                .await?
        }
        // A team admin's registry is filtered to what they see in the query.
        (None, Some(team_id)) => {
            state
                .store
                .list_external_mcp_servers_visible_to_team(team_id, query.include_disabled)
                .await?
        }
        (None, None) => service.list_servers(query.include_disabled).await?,
    }
    .into_iter()
    .filter(|server| scope.can_view(server))
    .collect();
    let items = present_servers(&state, &scope, servers).await?;
    Ok(Json(envelope(McpServersPayload {
        items,
        can_publish_to_organisation: ownership(&state).can_publish(&scope),
    })))
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
    let (actor, scope) = require_mcp_admin_scope(&state, &headers).await?;
    // A catalog entry supplies its own public URL, so only a typed one is
    // checked. Catalog values count as inherited, like a platform admin's.
    let catalog = match request.recommended_catalog_key.as_deref() {
        Some(key) => McpRegistryService::new(state.store.clone())
            .recommended_servers()?
            .into_iter()
            .find(|entry| entry.catalog_key == key),
        None => None,
    };
    if let Some(server_url) = request.server_url.as_deref() {
        refuse_private_network_host(&scope, &state.mcp_http_clients, server_url)?;
        authorize_destination(&scope, server_url).await?;
    }
    let effective_url = request
        .server_url
        .clone()
        .or_else(|| catalog.as_ref().map(|entry| entry.server_url.clone()))
        .unwrap_or_default();
    let effective_auth_config = match (&catalog, request.auth_config.is_empty()) {
        (Some(entry), true) if request.auth_mode.is_none() => entry.auth_config.clone(),
        _ => request.auth_config.clone(),
    };
    authorize_team_auth_config(
        &scope,
        &effective_auth_config,
        catalog
            .as_ref()
            .map(|entry| (&entry.auth_config, entry.server_url.as_str())),
        &effective_url,
        &state.mcp_http_clients,
    )
    .await?;
    let ownership = ownership(&state)
        .new_server_ownership(
            &scope,
            RequestedMcpServerOwnership {
                owner_team_id: request.owner_team_id,
                owner_contact: request.owner_contact,
                visibility: request
                    .visibility
                    .as_deref()
                    .map(parse_mcp_server_visibility)
                    .transpose()?,
            },
            &actor.email,
        )
        .await?;
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
            ownership,
        })
        .await?;
    let view = map_server(server.clone());
    state
        .audit()
        .record(
            &meta.user_context(&actor),
            scope.annotate(
                server_event(actions::MCP_SERVER_CREATED, &view).after(server_audit_summary(&view)),
            ),
        )
        .await;
    let server = present_server(&state, &scope, server).await?;
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
    let (actor, scope) = require_mcp_admin_scope(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let ownership = ownership(&state);
    let existing = ownership.managed_server(&scope, server_id).await?;
    // A server a platform admin registered on an internal network stays
    // editable; only a new URL is checked.
    if request.server_url.trim() != existing.server_url {
        refuse_private_network_host(&scope, &state.mcp_http_clients, &request.server_url)?;
        authorize_destination(&scope, &request.server_url).await?;
        ownership
            .authorize_retarget(&scope, &existing, &request.server_url)
            .await?;
    }
    authorize_team_auth_config(
        &scope,
        &request.auth_config,
        Some((&existing.auth_config, &existing.server_url)),
        &request.server_url,
        &state.mcp_http_clients,
    )
    .await?;
    let visibility = request
        .visibility
        .as_deref()
        .map(parse_mcp_server_visibility)
        .transpose()?;
    if let Some(visibility) = visibility {
        ownership
            .authorize_visibility_change(&scope, &existing, visibility)
            .await?;
    }
    let service = McpRegistryService::new(state.store.clone());
    // Narrowing to `team` is applied first, on its own and only while the
    // ownership is still what was authorized above. The stranded-access check
    // runs after it, as a grant's check runs after the grant, so whichever
    // lands second sees the other. Undoing it then touches nothing else: the
    // URL, auth settings, discovery state and OAuth client are saved after.
    let mut expected_visibility = existing.ownership.visibility;
    let narrowed = match (existing.ownership.owner_team_id, visibility) {
        (Some(team_id), Some(gateway_core::McpServerVisibility::Team))
            if existing.ownership.visibility == gateway_core::McpServerVisibility::Organisation =>
        {
            let narrowed = gateway_core::McpServerOwnership {
                visibility: gateway_core::McpServerVisibility::Team,
                ..existing.ownership.clone()
            };
            service
                .transfer_ownership_if(
                    server_id,
                    narrowed.clone(),
                    Some(existing.ownership.clone()),
                )
                .await?;
            if let Err(error) = ownership
                .confirm_no_stranded_access(server_id, team_id)
                .await
            {
                undo_transfer(&state, &service, server_id, &existing.ownership, narrowed).await?;
                return Err(error.into());
            }
            expected_visibility = gateway_core::McpServerVisibility::Team;
            true
        }
        _ => false,
    };
    let saved = service
        .update_server(
            server_id,
            UpdateExternalMcpServerInput {
                display_name: request.display_name,
                description: request.description,
                server_url: request.server_url,
                auth_mode: request.auth_mode,
                auth_config: request.auth_config,
                timeout_ms: request.timeout_ms,
                owner_contact: request.owner_contact,
                visibility,
                // Ties the save to the owner authorized above, so a transfer in
                // between cannot let the former team overwrite the server.
                expected_owner_team_id: existing.ownership.owner_team_id,
                expected_visibility,
                // `authorize_retarget` checked this above; the store repeats it
                // under the write lock, so a credential added meanwhile counts.
                retarget_owner_credentials_only: !scope.is_platform_admin(),
                // The auth checks above ran against these settings; a newer
                // change makes the save a conflict rather than overwrite it.
                expected_server: Some(McpServerOwnershipGuard {
                    visibility: expected_visibility,
                    ..McpServerOwnershipGuard::of(&existing).with_auth_settings(&existing)
                }),
            },
        )
        .await;
    let update = match saved {
        Ok(update) => update,
        Err(error) => {
            // The save was rejected, so undo the narrowing written before it.
            if narrowed {
                let narrowed_ownership = gateway_core::McpServerOwnership {
                    visibility: gateway_core::McpServerVisibility::Team,
                    ..existing.ownership.clone()
                };
                undo_transfer(
                    &state,
                    &service,
                    server_id,
                    &existing.ownership,
                    narrowed_ownership,
                )
                .await?;
            }
            return Err(error.into());
        }
    };
    let current = update.current.clone();
    let server = map_server(update.current);
    // The store compares the unredacted settings under the write lock and writes
    // nothing when they match, so a resave records nothing while rotating a
    // credential inside the URL still counts even though the summary hides it.
    // A narrowing was written before the settings save, so the save's own
    // `previous` already has it; the entry's `before` is the server as
    // authorized, so the visibility change shows.
    let before = match (narrowed, update.previous) {
        (true, _) => Some(existing),
        (false, previous) => previous,
    };
    if let Some(before) = before {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                scope.annotate(
                    server_event(actions::MCP_SERVER_UPDATED, &server)
                        .before(server_audit_summary(&map_server(before)))
                        .after(server_audit_summary(&server)),
                ),
            )
            .await;
    }
    let server = present_server(&state, &scope, current).await?;
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
    let (actor, scope) = require_mcp_admin_scope(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let existing = ownership(&state).managed_server(&scope, server_id).await?;
    let expected_owner = existing.ownership.owner_team_id;
    let before = server_audit_summary(&map_server(existing));
    let service = McpRegistryService::new(state.store.clone());
    let (record, disabled) = service
        .disable_server_for_owner(server_id, expected_owner)
        .await?;
    let server = map_server(record.clone());
    // Disabling a disabled server is not a transition. The store decides this in
    // its update, so concurrent requests record one entry.
    if disabled {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                scope.annotate(
                    server_event(actions::MCP_SERVER_DISABLED, &server)
                        .before(serde_json::json!({ "status": before["status"] }))
                        .after(serde_json::json!({ "status": "disabled" })),
                ),
            )
            .await;
    }
    let server = present_server(&state, &scope, record).await?;
    Ok(Json(envelope(McpServerPayload { server })))
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/mcp/servers/{server_id}/owner",
    request_body = TransferMcpServerOwnershipRequest,
    params(("server_id" = String, Path, description = "External MCP server identifier")),
    responses(
        (status = 200, body = Envelope<McpServerPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 403, body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView),
        (status = 409, body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
/// Moves a server to another team, or takes it over for the platform.
/// Platform admins only.
pub async fn transfer_mcp_server_ownership(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(server_id): Path<String>,
    Json(request): Json<TransferMcpServerOwnershipRequest>,
) -> Result<Json<Envelope<McpServerPayload>>, AppError> {
    let (actor, scope) = require_mcp_platform_scope(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let ownership = ownership(&state);
    let existing = ownership.managed_server(&scope, server_id).await?;
    let resolved = ownership
        .transfer_ownership(
            &scope,
            &existing,
            RequestedMcpServerOwnership {
                owner_team_id: request.owner_team_id.ok_or_else(|| {
                    AppError(GatewayError::InvalidRequest(
                        "owner_team_id is required; use null to move the server to the platform"
                            .to_string(),
                    ))
                })?,
                owner_contact: request.owner_contact,
                visibility: request
                    .visibility
                    .as_deref()
                    .map(parse_mcp_server_visibility)
                    .transpose()?,
            },
        )
        .await?;
    let service = McpRegistryService::new(state.store.clone());
    // Written only while the server still has the ownership read above, so two
    // concurrent transfers cannot overwrite each other or audit a stale `before`.
    let update = service
        .transfer_ownership_if(server_id, resolved, Some(existing.ownership.clone()))
        .await?;
    // A grant that raced the narrowing check checks after its own write; this
    // side checks after the transfer, so whichever lands second sees the
    // other. Undo the transfer if access is now stranded.
    if let (Some(previous), Some(team_id), gateway_core::McpServerVisibility::Team) = (
        &update.previous,
        update.current.ownership.owner_team_id,
        update.current.ownership.visibility,
    ) && let Err(error) = ownership
        .confirm_no_stranded_access(server_id, team_id)
        .await
    {
        undo_transfer(
            &state,
            &service,
            server_id,
            &previous.ownership,
            update.current.ownership.clone(),
        )
        .await?;
        return Err(error.into());
    }
    // The store writes nothing when the ownership already matches.
    if let Some(previous) = update.previous {
        let current = map_server(update.current.clone());
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                scope.annotate(
                    server_event(actions::MCP_SERVER_OWNERSHIP_TRANSFERRED, &current)
                        .before(ownership_audit_summary(&previous.ownership))
                        .after(ownership_audit_summary(&update.current.ownership)),
                ),
            )
            .await;
    }
    let server = present_server(&state, &scope, update.current).await?;
    Ok(Json(envelope(McpServerPayload { server })))
}

/// Undoes a transfer or narrowing that would strand access, back to
/// `previous`. Only the owner and visibility it wrote must still hold: a
/// contact edit since does not stop the undo, while a newer transfer, which
/// ran its own check, wins and is left in place. An undo that cannot land is
/// reported rather than leaving the server moved behind a failed request.
async fn undo_transfer(
    state: &AppState,
    service: &McpRegistryService<AnyStore>,
    server_id: Uuid,
    previous: &gateway_core::McpServerOwnership,
    mut written: gateway_core::McpServerOwnership,
) -> Result<(), AppError> {
    for _ in 0..3 {
        match service
            .transfer_ownership_if(server_id, previous.clone(), Some(written.clone()))
            .await
        {
            Ok(_) => return Ok(()),
            Err(GatewayError::Store(gateway_core::StoreError::Conflict(_))) => {
                let Some(current) = state.store.get_external_mcp_server(server_id).await? else {
                    return Ok(());
                };
                if current.ownership.owner_team_id != written.owner_team_id
                    || current.ownership.visibility != written.visibility
                {
                    return Ok(());
                }
                written = current.ownership;
            }
            Err(other) => return Err(other.into()),
        }
    }
    Err(AppError(GatewayError::Store(
        gateway_core::StoreError::Conflict(format!(
            "external MCP server `{server_id}` kept changing while an ownership change was undone; check its owner and visibility"
        )),
    )))
}

fn ownership_audit_summary(ownership: &gateway_core::McpServerOwnership) -> serde_json::Value {
    serde_json::json!({
        "owner_team_id": ownership.owner_team_id,
        "owner_contact": ownership.owner_contact,
        "visibility": ownership.visibility.as_str(),
    })
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
    let viewer = require_mcp_viewer(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let admin_scope = match &viewer {
        McpViewer::Admin(scope) => Some(*scope),
        McpViewer::Member { .. } => None,
    };
    let member_tools = match viewer {
        McpViewer::Admin(scope) => {
            ownership(&state).visible_server(&scope, server_id).await?;
            None
        }
        // A member sees only the tools their grants let them call, on a server
        // they can connect to; anything else does not exist for them.
        McpViewer::Member { user_id, team_id } => {
            // Only this server is resolved and read, not the whole registry.
            let access = member_access(&state, user_id, team_id, Some(server_id)).await?;
            let active = access.servers.iter().any(|server| {
                server.mcp_server_id == server_id
                    && server.status != gateway_core::ExternalMcpServerStatus::Disabled
            });
            let allowed: Vec<_> = if active {
                access
                    .tools
                    .into_iter()
                    .map(|tool| tool.mcp_tool_id)
                    .collect()
            } else {
                Vec::new()
            };
            if allowed.is_empty() {
                return Err(AppError(GatewayError::Store(
                    gateway_core::StoreError::NotFound(format!(
                        "external MCP server `{server_id}` not found"
                    )),
                )));
            }
            Some(allowed)
        }
    };
    let service = McpRegistryService::new(state.store.clone());
    let items = service
        .list_tools(server_id, query.include_inactive)
        .await?
        .into_iter()
        .filter(|tool| {
            member_tools
                .as_ref()
                .is_none_or(|allowed| allowed.contains(&tool.mcp_tool_id))
        })
        .map(map_tool)
        .collect();
    // Checked again after the read: a transfer to another team in between must
    // not hand the former owner the new owner's tools.
    if let Some(scope) = &admin_scope {
        ownership(&state).visible_server(scope, server_id).await?;
    }
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
    let (_, scope) = require_mcp_admin_scope(&state, &headers).await?;
    // A team admin's discovery checks every hop, including URLs the upstream
    // supplies, not just the one they typed.
    let discovery = if scope.is_platform_admin() {
        state
            .mcp_oauth_runtime
            .discover(request.server_url.trim())
            .await?
    } else {
        state
            .mcp_oauth_runtime
            .discover_public(request.server_url.trim())
            .await?
    };
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
    let (actor, scope) = require_mcp_admin_scope(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let server = ownership(&state).managed_server(&scope, server_id).await?;
    // Re-checked on each refresh, since the host may now resolve elsewhere. A
    // host listed in `mcp.oauth.private_network_hosts` may resolve privately;
    // the guarded client still holds it to private networks when it connects.
    if !state
        .mcp_http_clients
        .is_private_network_host(&server.server_url)
    {
        authorize_destination(&scope, &server.server_url).await?;
    }
    let service = McpRegistryService::with_client(
        state.store.clone(),
        Arc::new(HttpMcpDiscoveryClient::new(state.mcp_http_clients.clone())),
    );
    let started_at = Instant::now();
    let progress = AuditProgress::default();
    let result = service
        .refresh_discovery_tracked(
            server_id,
            &progress,
            // Pinned to the server the destination check above ran against.
            Some(McpServerOwnershipGuard::of(&server).with_auth_settings(&server)),
        )
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
                .record(&meta.user_context(&actor), scope.annotate(event))
                .await;
            // The server is reloaded after the guarded write; a transfer since
            // then must not hand the new owner's settings to the former team.
            if !scope.can_manage(&result.server) {
                return Err(AppError(GatewayError::Store(
                    gateway_core::StoreError::Conflict(format!(
                        "external MCP server `{server_id}` changed owner during discovery; reload it"
                    )),
                )));
            }
            let owner_team_name =
                owner_team_name(&state, result.server.ownership.owner_team_id).await?;
            Ok(Json(envelope(map_discovery_result(
                result,
                owner_team_name,
            ))))
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
                    .record(&meta.user_context(&actor), scope.annotate(event))
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
    let (_, scope) = require_mcp_admin_scope(&state, &headers).await?;
    let subject_kind = query
        .subject_kind
        .as_deref()
        .map(parse_grant_subject_kind)
        .transpose()?;
    let items = ownership(&state)
        .list_grants(&scope, subject_kind, query.subject_id)
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
    let (actor, scope) = require_mcp_admin_scope(&state, &headers).await?;
    let mut record = UpsertMcpToolGrantRecord {
        subject_kind: parse_grant_subject_kind(&request.subject_kind)?,
        subject_id: request.subject_id,
        target_kind: parse_grant_target_kind(&request.target_kind)?,
        target_id: request.target_id,
        updated_at: OffsetDateTime::now_utc(),
        expected_server: None,
        expected_subject_team: None,
    };
    let authorized_server = ownership(&state)
        .authorize_grant(
            &scope,
            McpGrantSubject {
                subject_kind: record.subject_kind,
                subject_id: record.subject_id,
            },
            record.target_kind,
            record.target_id,
        )
        .await?;
    // The store writes the grant only while the server still has the owner
    // and visibility it was authorized against, and the subject is still in
    // the team it was checked in, so a transfer, narrowing or team move in
    // between is a conflict rather than a grant nobody allowed.
    if let Some((server, subject_team)) = &authorized_server {
        record.expected_server = Some(McpServerOwnershipGuard::of(server));
        record.expected_subject_team = Some(*subject_team);
    }
    let (grant, created) = state.store.upsert_mcp_tool_grant(&record).await?;
    let grant_id = grant.grant_id;
    let grant = map_grant(grant);
    // Repeating an active grant changes no access, so only a new grant is recorded.
    if created {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                scope.annotate(
                    AuditEvent::new(actions::MCP_GRANT_SET)
                        .target(grant_id)
                        .after(&grant),
                ),
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
    let (actor, scope) = require_mcp_admin_scope(&state, &headers).await?;
    let subject_kind = parse_grant_subject_kind(&request.subject_kind)?;
    let target_kind = parse_grant_target_kind(&request.target_kind)?;
    let authorized_server = ownership(&state)
        .authorize_grant_revoke(&scope, target_kind, request.target_id)
        .await?;
    // The store returns the id of the grant it revoked, so the entry targets the
    // same id as that grant's `mcp_grant.set`. Nothing active means no change.
    // A team admin's revoke holds only while the server keeps the owner it was
    // authorized against, so a former team cannot revoke after a transfer.
    let revoked = match &authorized_server {
        Some(server) => {
            state
                .store
                .revoke_mcp_tool_grant_guarded(
                    subject_kind,
                    request.subject_id,
                    request.target_id,
                    &McpServerOwnershipGuard::of(server),
                    OffsetDateTime::now_utc(),
                )
                .await?
        }
        None => {
            state
                .store
                .revoke_mcp_tool_grant(
                    subject_kind,
                    request.subject_id,
                    target_kind,
                    request.target_id,
                    OffsetDateTime::now_utc(),
                )
                .await?
        }
    };
    if let Some(grant_id) = revoked {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                scope.annotate(
                    AuditEvent::new(actions::MCP_GRANT_REVOKED)
                        .target(grant_id)
                        .before(serde_json::json!({
                            "subject_kind": subject_kind.as_str(),
                            "subject_id": request.subject_id,
                            "target_kind": target_kind.as_str(),
                            "target_id": request.target_id,
                        })),
                ),
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
    let (_, scope) = require_mcp_admin_scope(&state, &headers).await?;
    if let Some(server_id) = query.server_id {
        ownership(&state).managed_server(&scope, server_id).await?;
    }
    let owner_scope_kind = query
        .owner_scope_kind
        .as_deref()
        .map(parse_credential_owner_scope_kind)
        .transpose()?;
    let service = McpCredentialService::new(state.store.clone());
    // A team admin's list is limited to their team's servers in the query
    // itself, a named server included, so a transfer after the check above
    // cannot hand them the new owner's bindings.
    let bindings = match scope.managed_team_id() {
        Some(team_id) => service
            .list_bindings_for_owner_team(
                team_id,
                owner_scope_kind,
                query.owner_scope_id,
                query.include_revoked,
            )
            .await?
            .into_iter()
            .filter(|binding| {
                query
                    .server_id
                    .is_none_or(|server_id| binding.mcp_server_id == server_id)
            })
            .collect(),
        None => {
            service
                .list_bindings(
                    query.server_id,
                    owner_scope_kind,
                    query.owner_scope_id,
                    query.include_revoked,
                )
                .await?
        }
    };
    let items = bindings.into_iter().map(map_credential_binding).collect();
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
    let (actor, scope) = require_mcp_admin_scope(&state, &headers).await?;
    let owner_scope_kind = parse_credential_owner_scope_kind(&request.owner_scope_kind)?;
    let ownership = ownership(&state);
    let server = ownership.managed_server(&scope, request.server_id).await?;
    ownership
        .authorize_credential_binding(
            &scope,
            &server,
            owner_scope_kind,
            request.owner_team_id,
            request.owner_service_account_id,
            request.secret_ref.is_some(),
        )
        .await?;
    let service = McpCredentialService::new(state.store.clone());
    // Replacing a binding by id must not reach one the caller could not set.
    if let Some(binding_id) = request.credential_binding_id
        && let Some(existing) = find_credential_binding(&service, binding_id).await?
    {
        // A binding stays on its server; the store refuses a move too.
        if existing.mcp_server_id != request.server_id {
            return Err(AppError(GatewayError::InvalidRequest(
                "a credential binding cannot move to another MCP server; revoke it and create a new one"
                    .to_string(),
            )));
        }
        let existing_server = ownership
            .managed_server(&scope, existing.mcp_server_id)
            .await?;
        ownership
            .authorize_credential_binding(
                &scope,
                &existing_server,
                existing.owner_scope_kind,
                existing.owner_team_id,
                existing.owner_service_account_id,
                false,
            )
            .await?;
    }
    // The store reports whether its write replaced a binding: a caller-chosen ID
    // that does not exist yet is inserted, so it is a create.
    let (binding, replaced) = service
        .upsert_binding(UpsertMcpCredentialBindingInput {
            credential_binding_id: request.credential_binding_id,
            mcp_server_id: request.server_id,
            owner_scope_kind,
            owner_user_id: request.owner_user_id,
            owner_team_id: request.owner_team_id,
            owner_service_account_id: request.owner_service_account_id,
            material_kind: parse_credential_material_kind(&request.material_kind)?,
            header_name: request.header_name,
            secret: request.secret,
            secret_ref: request.secret_ref,
            expires_at: request.expires_at,
            metadata: request.metadata,
            // As for grants: written only while the server keeps the owner and
            // visibility the binding was authorized against.
            // Pinned to the URL too, so a retarget that lands first refuses
            // the write rather than store a credential for a new destination.
            expected_server: Some(
                McpServerOwnershipGuard::of(&server).with_server_url(&server.server_url),
            ),
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
        .record(&meta.user_context(&actor), scope.annotate(event))
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
    let (actor, scope) = require_mcp_admin_scope(&state, &headers).await?;
    let credential_binding_id = parse_uuid(&credential_binding_id, "credential_binding_id")?;
    let service = McpCredentialService::new(state.store.clone());
    // A team admin may revoke any binding on their team's servers. A binding on
    // a server they cannot see is reported as missing.
    let authorized_server = if scope.is_platform_admin() {
        None
    } else {
        let Some(existing) = find_credential_binding(&service, credential_binding_id).await? else {
            return Err(
                GatewayError::Store(gateway_core::StoreError::NotFound(format!(
                    "MCP credential binding `{credential_binding_id}` not found"
                )))
                .into(),
            );
        };
        Some(
            ownership(&state)
                .managed_server(&scope, existing.mcp_server_id)
                .await?,
        )
    };
    // The revoke returns the binding it changed, so the entry names its owner and
    // keeps its redacted settings. A missing or already-revoked binding records
    // nothing. A team admin's revoke holds only while the server keeps the owner
    // it was authorized against.
    let revoked = match &authorized_server {
        Some(server) => {
            service
                .revoke_binding_guarded(credential_binding_id, &McpServerOwnershipGuard::of(server))
                .await?
        }
        None => service.revoke_binding(credential_binding_id).await?,
    };
    if let Some(binding) = revoked {
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
                scope.annotate(
                    AuditEvent::new(actions::MCP_CREDENTIAL_BINDING_REVOKED)
                        .target(binding.id())
                        .label(binding.owner_scope_key().to_string())
                        .before(before)
                        .after(serde_json::json!({ "revoked_at": revoked_at })),
                ),
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
    let (_, scope) = require_mcp_admin_scope(&state, &headers).await?;
    let subjects = preview_subjects(&query);
    if subjects.is_empty() {
        return Err(GatewayError::InvalidRequest(
            "at least one access preview subject is required".to_string(),
        )
        .into());
    }
    // A team admin previews their own team's subjects on one of their servers,
    // so the preview never shows access granted elsewhere.
    if let Some(team_id) = scope.managed_team_id() {
        let server_id = query.server_id.ok_or_else(|| {
            AppError(GatewayError::InvalidRequest(
                "server_id is required to preview access".to_string(),
            ))
        })?;
        let ownership = ownership(&state);
        ownership.managed_server(&scope, server_id).await?;
        for subject in &subjects {
            if ownership.subject_team(subject.clone()).await? != Some(team_id) {
                return Err(AppError(GatewayError::Auth(
                    gateway_core::AuthError::InsufficientPrivileges,
                )));
            }
        }
    }
    let mut resolution = state
        .store
        .resolve_mcp_access_for_subjects(&subjects, query.server_id, McpToolProjection::Full)
        .await?;
    if let Some(api_key_id) = query.api_key_id {
        limit_to_api_key_access(&state, api_key_id, &mut resolution).await?;
    }
    // As at runtime, a team-only server's tools count only for its own team.
    let caller_team = preview_caller_team(&state, &query).await?;
    let hidden = state
        .store
        .list_team_only_mcp_server_ids_outside(caller_team)
        .await?
        .into_iter()
        .collect();
    gateway_service::mcp_access::retain_reachable_tools(&mut resolution, &hidden);
    Ok(Json(envelope(McpEffectiveAccessPayload {
        referenced_server_count: resolution.referenced_server_count,
        exposed_tool_count: resolution.exposed_tool_count,
        filtered_tool_count: resolution.filtered_tool_count,
        tools: resolution.allowed_tools.into_iter().map(map_tool).collect(),
    })))
}

/// Narrows a preview to what the API key can call at runtime: nothing once it
/// is revoked or without an MCP scope, and with a server list only those
/// servers' tools. Grants alone
/// would report tools the gateway refuses that key.
async fn limit_to_api_key_access(
    state: &AppState,
    api_key_id: Uuid,
    resolution: &mut gateway_core::McpAccessResolution,
) -> Result<(), AppError> {
    use gateway_core::{AdminApiKeyRepository, SCOPE_MCP_AGGREGATE, SCOPE_MCP_PROXY};
    let Some(key) = AdminApiKeyRepository::get_api_key_by_id(&*state.store, api_key_id).await?
    else {
        return Ok(());
    };
    let callable = key.revoked_at.is_none()
        && (key.access.has_scope(SCOPE_MCP_AGGREGATE) || key.access.has_scope(SCOPE_MCP_PROXY));
    if !callable {
        resolution.allowed_tools.clear();
    } else if key.access.mcp_server_keys.is_some() {
        let server_keys: std::collections::HashMap<Uuid, String> = state
            .store
            .list_external_mcp_servers(true)
            .await?
            .into_iter()
            .map(|server| (server.mcp_server_id, server.server_key))
            .collect();
        resolution.allowed_tools.retain(|tool| {
            server_keys
                .get(&tool.mcp_server_id)
                .is_some_and(|server_key| key.access.allows_mcp_server(server_key))
        });
    }
    let servers: std::collections::HashSet<Uuid> = resolution
        .allowed_tools
        .iter()
        .map(|tool| tool.mcp_server_id)
        .collect();
    resolution.referenced_server_count = servers.len() as i64;
    resolution.filtered_tool_count = resolution
        .exposed_tool_count
        .saturating_sub(resolution.allowed_tools.len() as i64);
    Ok(())
}

/// The team the previewed caller acts for: an API key's (its own, or its
/// owner's), a user's current team, a service account's, or the team itself.
async fn preview_caller_team(
    state: &AppState,
    query: &McpEffectiveAccessQuery,
) -> Result<Option<Uuid>, AppError> {
    use gateway_core::AdminApiKeyRepository;
    let user_team = |user_id| async move {
        Ok::<_, AppError>(
            IdentityRepository::get_team_membership_for_user(&*state.store, user_id)
                .await?
                .map(|membership| membership.team_id),
        )
    };
    if let Some(api_key_id) = query.api_key_id {
        let Some(key) = AdminApiKeyRepository::get_api_key_by_id(&*state.store, api_key_id).await?
        else {
            return Ok(None);
        };
        return match (key.owner_team_id, key.owner_user_id) {
            (Some(team_id), _) => Ok(Some(team_id)),
            (None, Some(user_id)) => user_team(user_id).await,
            (None, None) => Ok(None),
        };
    }
    if let Some(user_id) = query.user_id {
        return user_team(user_id).await;
    }
    if let Some(service_account_id) = query.service_account_id {
        return Ok(IdentityRepository::get_service_account_by_id(
            &*state.store,
            service_account_id,
        )
        .await?
        .map(|service_account| service_account.team_id));
    }
    Ok(query.team_id)
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

/// The active tools a user can call. Grants to them or their team count as
/// they reach any of the user's MCP clients. A grant to one of their API keys
/// counts only while that key may call MCP at all and its server list, if it
/// has one, includes the tool's server, as runtime resolution enforces.
/// Restricted-key groups resolved at once for a member's access.
const MEMBER_ACCESS_CONCURRENCY: usize = 4;

/// Resolves which tools a member can call, as identities and lifecycle only
/// (callers need tool and server IDs, not metadata).
///
/// `server_id` limits the resolution to one server's tools.
/// The tools a member can call and the servers they are on, read by ID.
struct MemberAccess {
    tools: Vec<gateway_core::ExternalMcpToolRecord>,
    servers: Vec<gateway_core::ExternalMcpServerRecord>,
}

async fn member_access(
    state: &AppState,
    user_id: Uuid,
    team_id: Option<Uuid>,
    server_id: Option<Uuid>,
) -> Result<MemberAccess, AppError> {
    let mut tools = granted_member_tools(state, user_id, team_id, server_id).await?;
    let referenced: Vec<Uuid> = tools
        .iter()
        .map(|tool| tool.mcp_server_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    // Only the servers these tools are on are read. As at runtime, a team-only
    // server counts only for its own team's members.
    let mut servers = state
        .store
        .list_external_mcp_servers_by_ids(&referenced)
        .await?;
    servers.retain(|server| {
        server.ownership.visibility == gateway_core::McpServerVisibility::Organisation
            || server
                .ownership
                .owner_team_id
                .is_some_and(|owner| Some(owner) == team_id)
    });
    let reachable: std::collections::HashSet<Uuid> =
        servers.iter().map(|server| server.mcp_server_id).collect();
    tools.retain(|tool| reachable.contains(&tool.mcp_server_id));
    Ok(MemberAccess { tools, servers })
}

async fn granted_member_tools(
    state: &AppState,
    user_id: Uuid,
    team_id: Option<Uuid>,
    server_id: Option<Uuid>,
) -> Result<Vec<gateway_core::ExternalMcpToolRecord>, AppError> {
    use gateway_core::{AdminApiKeyRepository, SCOPE_MCP_AGGREGATE, SCOPE_MCP_PROXY};
    let keys: Vec<_> = state
        .store
        .list_api_keys_for_user_scope(user_id, None)
        .await?
        .into_iter()
        .filter(|key| key.owner_user_id == Some(user_id) && key.revoked_at.is_none())
        .filter(|key| {
            key.access.has_scope(SCOPE_MCP_AGGREGATE) || key.access.has_scope(SCOPE_MCP_PROXY)
        })
        .collect();
    let key_subject = |id| McpGrantSubject {
        subject_kind: McpToolGrantSubjectKind::ApiKey,
        subject_id: id,
    };
    // The user, their team and every key without a server list resolve in one
    // query; only a key limited to some servers needs its own, to filter it.
    let mut subjects = vec![McpGrantSubject {
        subject_kind: McpToolGrantSubjectKind::User,
        subject_id: user_id,
    }];
    if let Some(team_id) = team_id {
        subjects.push(McpGrantSubject {
            subject_kind: McpToolGrantSubjectKind::Team,
            subject_id: team_id,
        });
    }
    let (unrestricted, restricted): (Vec<_>, Vec<_>) = keys
        .into_iter()
        .partition(|key| key.access.mcp_server_keys.is_none());
    subjects.extend(unrestricted.iter().map(|key| key_subject(key.id)));
    let mut tools = state
        .store
        .resolve_mcp_access_for_subjects(&subjects, server_id, McpToolProjection::Access)
        .await?
        .allowed_tools;
    if restricted.is_empty() {
        return Ok(tools);
    }
    // Keys with the same server list resolve together, which is exact: their
    // grants are filtered by the one list they share. The groups resolve
    // concurrently, so a user's many restricted keys cost one round trip.
    let mut groups: std::collections::BTreeMap<Vec<String>, Vec<McpGrantSubject>> =
        std::collections::BTreeMap::new();
    for key in restricted {
        let allow_list = key.access.mcp_server_keys.clone().unwrap_or_default();
        groups
            .entry(allow_list)
            .or_default()
            .push(key_subject(key.id));
    }
    // A few groups at a time, so one user's many keys cannot fan out without
    // bound; results keep the groups' order.
    let groups: Vec<_> = groups.into_iter().collect();
    let mut resolutions = Vec::with_capacity(groups.len());
    for chunk in groups.chunks(MEMBER_ACCESS_CONCURRENCY) {
        resolutions.extend(
            futures_util::future::try_join_all(chunk.iter().map(|(_, subjects)| {
                state.store.resolve_mcp_access_for_subjects(
                    subjects,
                    server_id,
                    McpToolProjection::Access,
                )
            }))
            .await?,
        );
    }
    // Only the servers these tools are on are read, to check each key's list.
    let referenced: Vec<Uuid> = resolutions
        .iter()
        .flat_map(|resolution| {
            resolution
                .allowed_tools
                .iter()
                .map(|tool| tool.mcp_server_id)
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let server_keys: std::collections::HashMap<Uuid, String> = state
        .store
        .list_external_mcp_servers_by_ids(&referenced)
        .await?
        .into_iter()
        .map(|server| (server.mcp_server_id, server.server_key))
        .collect();
    let mut seen: std::collections::HashSet<Uuid> =
        tools.iter().map(|tool| tool.mcp_tool_id).collect();
    for ((allow_list, _), resolution) in groups.iter().zip(resolutions) {
        for tool in resolution.allowed_tools {
            let allowed = server_keys
                .get(&tool.mcp_server_id)
                .is_some_and(|server_key| allow_list.iter().any(|allowed| allowed == server_key));
            if allowed && seen.insert(tool.mcp_tool_id) {
                tools.push(tool);
            }
        }
    }
    Ok(tools)
}

/// A member's read-only server list: the servers they can connect to.
async fn list_member_servers(
    state: &AppState,
    user_id: Uuid,
    team_id: Option<Uuid>,
    query: &McpServersQuery,
) -> Result<Json<Envelope<McpServersPayload>>, AppError> {
    // Only the servers the member can reach are read, not the whole registry.
    let access = member_access(state, user_id, team_id, None).await?;
    let owner = query.owner.as_deref().map(parse_owner_filter).transpose()?;
    let servers: Vec<_> = access
        .servers
        .into_iter()
        .filter(|server| {
            query.include_disabled
                || server.status != gateway_core::ExternalMcpServerStatus::Disabled
        })
        .filter(|server| owner.is_none_or(|owner| owner.matches(server)))
        .collect();
    let names: std::collections::HashMap<Uuid, String> = if servers
        .iter()
        .any(|server| server.ownership.owner_team_id.is_some())
    {
        gateway_core::AdminIdentityRepository::list_teams(&*state.store)
            .await?
            .into_iter()
            .map(|team| (team.team_id, team.team_name))
            .collect()
    } else {
        std::collections::HashMap::new()
    };
    let items = servers
        .into_iter()
        .map(|server| {
            let name = server
                .ownership
                .owner_team_id
                .and_then(|team_id| names.get(&team_id).cloned());
            map_server(server).for_viewer(name, false)
        })
        .collect();
    Ok(Json(envelope(McpServersPayload {
        items,
        can_publish_to_organisation: false,
    })))
}

fn ownership(state: &AppState) -> McpOwnership<AnyStore> {
    McpOwnership::new(state.store.clone(), state.mcp_organisation_publishing)
}

/// The `owner` query of the server list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerFilter {
    /// `owner=platform`: platform-owned servers only.
    Platform,
    /// `owner=<team id>`: one team's servers.
    Team(Uuid),
}

impl OwnerFilter {
    fn matches(self, server: &ExternalMcpServerRecord) -> bool {
        server.ownership.owner_team_id == self.team_id()
    }

    /// The owning team the filter names; `None` for the platform.
    fn team_id(self) -> Option<Uuid> {
        match self {
            Self::Platform => None,
            Self::Team(team_id) => Some(team_id),
        }
    }
}

fn parse_owner_filter(raw: &str) -> Result<OwnerFilter, AppError> {
    if raw == "platform" {
        return Ok(OwnerFilter::Platform);
    }
    parse_uuid(raw, "owner").map(OwnerFilter::Team)
}

async fn owner_team_name(
    state: &AppState,
    owner_team_id: Option<Uuid>,
) -> Result<Option<String>, AppError> {
    let Some(team_id) = owner_team_id else {
        return Ok(None);
    };
    Ok(IdentityRepository::get_team_by_id(&*state.store, team_id)
        .await?
        .map(|team| team.team_name))
}

async fn present_server(
    state: &AppState,
    scope: &McpAdminScope,
    server: ExternalMcpServerRecord,
) -> Result<McpServerView, AppError> {
    let can_manage = scope.can_manage(&server);
    let name = owner_team_name(state, server.ownership.owner_team_id).await?;
    Ok(map_server(server).for_viewer(name, can_manage))
}

async fn present_servers(
    state: &AppState,
    scope: &McpAdminScope,
    servers: Vec<ExternalMcpServerRecord>,
) -> Result<Vec<McpServerView>, AppError> {
    // Every owning team's name in one read, only when a team owns a server.
    let names: std::collections::HashMap<Uuid, String> = if servers
        .iter()
        .any(|server| server.ownership.owner_team_id.is_some())
    {
        gateway_core::AdminIdentityRepository::list_teams(&*state.store)
            .await?
            .into_iter()
            .map(|team| (team.team_id, team.team_name))
            .collect()
    } else {
        std::collections::HashMap::new()
    };
    Ok(servers
        .into_iter()
        .map(|server| {
            let name = server
                .ownership
                .owner_team_id
                .and_then(|team_id| names.get(&team_id).cloned());
            let can_manage = scope.can_manage(&server);
            map_server(server).for_viewer(name, can_manage)
        })
        .collect())
}

async fn find_credential_binding(
    service: &McpCredentialService<AnyStore>,
    credential_binding_id: Uuid,
) -> Result<Option<RedactedMcpCredentialBinding>, AppError> {
    Ok(service.binding(credential_binding_id).await?)
}

/// The audited details of a tool set.
fn toolset_details(toolset: &gateway_core::McpToolsetRecord) -> serde_json::Value {
    serde_json::json!({
        "display_name": toolset.display_name,
        "description": toolset.description,
    })
}
