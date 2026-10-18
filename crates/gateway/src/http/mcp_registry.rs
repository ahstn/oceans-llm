mod access;
mod credentials;
mod member;
mod tool_policy;
mod tool_reviews;
mod toolsets;
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
pub use access::*;
pub use credentials::*;
use member::*;
pub use tool_policy::*;
pub use tool_reviews::*;
pub use toolsets::*;
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
    // An admin's list shows full settings for the servers their scope manages,
    // so the scope is checked again after the reads: an admin who left or
    // changed team meanwhile gets the list for the viewer they are now.
    for _ in 0..3 {
        let scope = match require_mcp_viewer(&state, &headers).await? {
            McpViewer::Admin(scope) => scope,
            McpViewer::Member { user_id, team_id } => {
                return list_member_servers(&state, user_id, team_id, &query).await;
            }
        };
        let (listed, owners) = list_admin_servers(&state, scope, &query).await?;
        // The view of each server follows its owner as read, so a transfer in
        // between must not leave its former team's admin the full settings.
        // Only the ownership columns are read again.
        let ids: Vec<Uuid> = owners.keys().copied().collect();
        let unchanged = state
            .store
            .list_mcp_server_ownerships(&ids)
            .await?
            .iter()
            .all(|(mcp_server_id, ownership)| owners.get(mcp_server_id) == Some(ownership));
        // The viewer is read last, so a role or team change at any point
        // before it is seen.
        let same_viewer = matches!(
            require_mcp_viewer(&state, &headers).await?,
            McpViewer::Admin(current) if current == scope
        );
        if unchanged && same_viewer {
            return Ok(listed);
        }
    }
    Err(AppError(GatewayError::Store(
        gateway_core::StoreError::Conflict(
            "MCP servers or your access to them kept changing while they were listed; try again"
                .to_string(),
        ),
    )))
}

/// The registry as `scope` sees it, with the ownership each listed server
/// had when read.
async fn list_admin_servers(
    state: &AppState,
    scope: McpAdminScope,
    query: &McpServersQuery,
) -> Result<
    (
        Json<Envelope<McpServersPayload>>,
        std::collections::HashMap<Uuid, gateway_core::McpServerOwnership>,
    ),
    AppError,
> {
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
            service
                .list_servers_visible_to_team(team_id, query.include_disabled)
                .await?
        }
        (None, None) => service.list_servers(query.include_disabled).await?,
    }
    .into_iter()
    .filter(|server| scope.can_view(server))
    .collect::<Vec<_>>();
    let owners = servers
        .iter()
        .map(|server| (server.mcp_server_id, server.ownership.clone()))
        .collect();
    let items = present_servers(state, &scope, servers).await?;
    Ok((
        Json(envelope(McpServersPayload {
            items,
            can_publish_to_organisation: ownership(state).can_publish(&scope),
        })),
        owners,
    ))
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
    refuse_approval_change(&scope, false, request.approve_tool_changes)?;
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
            approve_tool_changes: request.approve_tool_changes,
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
    refuse_approval_change(
        &scope,
        existing.approve_tool_changes,
        request.approve_tool_changes,
    )?;
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
    // A narrowing to `team` is saved with the settings, in one write: the store
    // refuses it while access would be stranded, counted with the server row
    // locked, so a racing grant is either counted or refused by its own guard.
    let (update, revoked_oauth_bindings) = service
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
                expected_visibility: existing.ownership.visibility,
                // A platform admin's save vouches for any destinations a team
                // set, so the server's traffic no longer needs the guard.
                vouch_for_destinations: scope.is_platform_admin(),
                // `authorize_retarget` checked this above; the store repeats it
                // under the write lock, so a credential added meanwhile counts.
                retarget_owner_credentials_only: !scope.is_platform_admin(),
                // The auth checks above ran against these settings; a newer
                // change makes the save a conflict rather than overwrite it.
                expected_server: Some(
                    McpServerOwnershipGuard::of(&existing).with_auth_settings(&existing),
                ),
                approve_tool_changes: request.approve_tool_changes,
            },
        )
        .await?;
    let current = update.current.clone();
    let server = map_server(update.current);
    // The store compares the unredacted settings under the write lock and writes
    // nothing when they match, so a resave records nothing while rotating a
    // credential inside the URL still counts even though the summary hides it.
    if let Some(before) = update.previous {
        // A save that only vouches for a team's destinations changes no field
        // the summary shows, so the entry says so.
        let vouched = before.team_destinations && !current.team_destinations;
        let mut event = server_event(actions::MCP_SERVER_UPDATED, &server)
            .before(server_audit_summary(&map_server(before)))
            .after(server_audit_summary(&server));
        if vouched {
            event = event.meta("destinations_vouched", true);
        }
        state
            .audit()
            .record(&meta.user_context(&actor), scope.annotate(event))
            .await;
    }
    // Users' OAuth connections end with the endpoints that issued them. Each
    // revoke is recorded, so every connection's set has a matching revoke, in
    // a few multi-row writes however many users had connected.
    let revokes = revoked_oauth_bindings
        .into_iter()
        .map(|binding| {
            let binding = map_credential_binding(binding);
            let mut before = credential_binding_audit_summary(&binding);
            let revoked_at = before.get("revoked_at").cloned().unwrap_or_default();
            if let Some(fields) = before.as_object_mut() {
                fields.insert("revoked_at".to_string(), serde_json::Value::Null);
                fields.remove("updated_at");
            }
            scope.annotate(
                AuditEvent::new(actions::MCP_CREDENTIAL_BINDING_REVOKED)
                    .target(binding.id())
                    .label(binding.owner_scope_key().to_string())
                    .meta("operation", "server_settings_changed")
                    .meta("mcp_server_id", server_id)
                    .before(before)
                    .after(serde_json::json!({ "revoked_at": revoked_at })),
            )
        })
        .collect();
    state
        .audit()
        .record_all(&meta.user_context(&actor), revokes)
        .await;
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
        (status = 403, description = "The caller can see the server but not manage it", body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView),
        (status = 409, description = "The server changed owner since the caller was authorized", body = OpenAiErrorEnvelopeView)
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

/// Reactivates a disabled server. Its tools and grants apply again as they were before it was
/// disabled, and its discovery status resets to not run until discovery is refreshed. Whoever
/// can disable a server can enable it again: a team admin for their team's servers.
#[utoipa::path(
    post,
    path = "/api/v1/admin/mcp/servers/{server_id}/enable",
    params(("server_id" = String, Path, description = "External MCP server identifier")),
    responses(
        (status = 200, body = Envelope<McpServerPayload>),
        (status = 400, body = OpenAiErrorEnvelopeView),
        (status = 403, description = "The caller can see the server but not manage it", body = OpenAiErrorEnvelopeView),
        (status = 404, body = OpenAiErrorEnvelopeView),
        (status = 409, description = "The server changed owner since the caller was authorized", body = OpenAiErrorEnvelopeView)
    ),
    security(("session_cookie" = []))
)]
pub async fn enable_mcp_server(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(server_id): Path<String>,
) -> Result<Json<Envelope<McpServerPayload>>, AppError> {
    let (actor, scope) = require_mcp_admin_scope(&state, &headers).await?;
    let server_id = parse_uuid(&server_id, "server_id")?;
    let existing = ownership(&state).managed_server(&scope, server_id).await?;
    let expected_owner = existing.ownership.owner_team_id;
    let service = McpRegistryService::new(state.store.clone());
    let (record, enabled) = service
        .enable_server_for_owner(server_id, expected_owner)
        .await?;
    let server = map_server(record.clone());
    // Enabling an active server is not a transition. As with disable, the store decides this
    // in its update, so concurrent requests record one entry.
    if enabled {
        state
            .audit()
            .record(
                &meta.user_context(&actor),
                scope.annotate(
                    server_event(actions::MCP_SERVER_ENABLED, &server)
                        .before(serde_json::json!({ "status": "disabled" }))
                        .after(serde_json::json!({ "status": "active" })),
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
    // The store refuses a narrowing that would strand access inside this
    // write, with the server row locked, so there is nothing to undo here.
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
    let not_found = || {
        AppError(GatewayError::Store(gateway_core::StoreError::NotFound(
            format!("external MCP server `{server_id}` not found"),
        )))
    };
    let service = McpRegistryService::new(state.store.clone());
    let tools = match viewer {
        McpViewer::Admin(scope) => {
            ownership(&state).visible_server(&scope, server_id).await?;
            let tools = service
                .list_tools(server_id, query.include_inactive)
                .await?;
            // Checked again after the read, against the viewer as they are
            // now: a transfer, or the admin leaving the team, in between must
            // not hand them tools they no longer see.
            match require_mcp_viewer(&state, &headers).await? {
                McpViewer::Admin(current) => {
                    ownership(&state)
                        .visible_server(&current, server_id)
                        .await?;
                }
                McpViewer::Member { .. } => return Err(not_found()),
            }
            tools
        }
        // A member sees only the tools their grants let them call, on a server
        // they can connect to; anything else does not exist for them.
        McpViewer::Member { user_id, team_id } => {
            // The tools are read first and the member's access resolved after,
            // so a grant revoked, or the server moved or disabled, before the
            // resolution is seen. Only this server is resolved and read.
            let tools = service
                .list_tools(server_id, query.include_inactive)
                .await?;
            let access = member_access(&state, user_id, team_id, Some(server_id)).await?;
            let active = access.servers.iter().any(|server| {
                server.mcp_server_id == server_id
                    && server.status != gateway_core::ExternalMcpServerStatus::Disabled
            });
            // A set, so filtering the listed tools stays linear.
            let allowed: std::collections::HashSet<Uuid> = if active {
                access.tool_ids.into_iter().collect()
            } else {
                std::collections::HashSet::new()
            };
            // The server's ownership is read again, then the viewer last: a
            // server moved away, or the member leaving the team the access was
            // resolved for, during the reads above hides the tools.
            let still_reachable = state
                .store
                .list_mcp_server_ownerships(&[server_id])
                .await?
                .iter()
                .any(|(_, ownership)| ownership.is_reachable_from(team_id));
            let same_viewer = matches!(
                require_mcp_viewer(&state, &headers).await?,
                McpViewer::Member { user_id: current_user, team_id: current_team }
                    if (current_user, current_team) == (user_id, team_id)
            );
            if allowed.is_empty() || !still_reachable || !same_viewer {
                return Err(not_found());
            }
            tools
                .into_iter()
                .filter(|tool| allowed.contains(&tool.mcp_tool_id))
                .collect()
        }
    };
    let items = tools.into_iter().map(map_tool).collect();
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
        (status = 404, body = OpenAiErrorEnvelopeView),
        (status = 409, description = "The server was disabled, enabled, transferred or reconfigured while discovery ran", body = OpenAiErrorEnvelopeView)
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
            // Read before the recheck below, which must be the last read.
            let owner_team_name =
                owner_team_name(&state, result.server.ownership.owner_team_id).await?;
            // The server and the caller are read again now, after the audit
            // write and every other read: a transfer, or the caller leaving the
            // team, since the discovery write must not hand the new owner's
            // settings and tool schemas to the former team's admin.
            let current = state.store.get_external_mcp_server(server_id).await?;
            // The caller is read last, so a role or team change during the
            // server reload is seen.
            let current_scope = require_mcp_admin_scope(&state, &headers)
                .await
                .ok()
                .map(|(_, scope)| scope);
            let unchanged = matches!(
                (current_scope, current.as_ref()),
                (Some(now), Some(current))
                    if now == scope
                        && current.ownership == result.server.ownership
                        && scope.can_manage(current)
            );
            if !unchanged {
                return Err(AppError(GatewayError::Store(
                    gateway_core::StoreError::Conflict(format!(
                        "external MCP server `{server_id}` changed owner, or your access to it changed, during discovery; reload it"
                    )),
                )));
            }
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

/// Only platform admins review tool changes, so only they turn approval on or
/// off. A team admin may leave the setting out, or send the value it has.
fn refuse_approval_change(
    scope: &McpAdminScope,
    current: bool,
    requested: Option<bool>,
) -> Result<(), AppError> {
    if scope.is_platform_admin() || requested.is_none_or(|requested| requested == current) {
        return Ok(());
    }
    Err(AppError(GatewayError::Auth(
        gateway_core::AuthError::InsufficientPrivileges,
    )))
}

/// Confirms, after a read, that the caller still holds the admin scope the read
/// was authorized and filtered by. An admin who lost their role or changed team
/// in between gets a conflict to retry, never the former scope's data.
async fn recheck_admin_scope(
    state: &AppState,
    headers: &HeaderMap,
    scope: &McpAdminScope,
) -> Result<(), AppError> {
    let (_, current) = require_mcp_admin_scope(state, headers).await?;
    if current == *scope {
        Ok(())
    } else {
        Err(AppError(GatewayError::Store(
            gateway_core::StoreError::Conflict(
                "your MCP admin access changed during this request; reload and try again"
                    .to_string(),
            ),
        )))
    }
}

/// The distinct owning teams of `servers`, whose names a response needs.
fn owner_team_ids(servers: &[ExternalMcpServerRecord]) -> Vec<Uuid> {
    servers
        .iter()
        .filter_map(|server| server.ownership.owner_team_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
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
        gateway_core::AdminIdentityRepository::list_teams_by_ids(
            &*state.store,
            &owner_team_ids(&servers),
        )
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
