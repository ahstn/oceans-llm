//! Admin endpoints for MCP tool grants, and the access preview that shows what
//! a caller's grants and tag rules reach.

use super::*;

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
    let ownership = ownership(&state);
    let mut grants = ownership
        .list_grants(&scope, subject_kind, query.subject_id)
        .await?;
    // A team admin's list follows the servers their team owns when read. It is
    // read again and only grants in both reads are returned, so a server moved
    // away in between drops out; the caller's scope is then checked last.
    if scope.managed_team_id().is_some() {
        let still_listed: std::collections::HashSet<Uuid> = ownership
            .list_grants(&scope, subject_kind, query.subject_id)
            .await?
            .into_iter()
            .map(|grant| grant.grant_id)
            .collect();
        grants.retain(|grant| still_listed.contains(&grant.grant_id));
    }
    recheck_admin_scope(&state, &headers, &scope).await?;
    let items = grants.into_iter().map(map_grant).collect();
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
    let selected = preview_subjects(&query);
    if selected.is_empty() {
        return Err(GatewayError::InvalidRequest(
            "at least one access preview subject is required".to_string(),
        )
        .into());
    }
    // A key preview already includes the key's owner and team, and its scope
    // limits apply to everything it resolves. A key mixed with another
    // subject describes no real caller.
    if query.api_key_id.is_some() && selected.len() > 1 {
        return Err(GatewayError::InvalidRequest(
            "an API key access preview cannot include other subjects; its owner and team are included automatically".to_string(),
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
        for subject in &selected {
            if ownership.subject_team(subject.clone()).await? != Some(team_id) {
                return Err(AppError(GatewayError::Auth(
                    gateway_core::AuthError::InsufficientPrivileges,
                )));
            }
        }
    }
    // Preview what a real caller gets: a key with its owner and team, a user
    // or service account with its team. Every selected subject is checked
    // here, so a disabled service account is refused.
    let previews = gateway_service::McpAccess::new(state.store.clone());
    let mut preview = previews.preview_subjects(&selected).await?;
    // A service account acts only through its keys, which runtime refuses
    // without an active budget, so it adds nothing to the preview. Any other
    // subject selected with it still previews what it reaches.
    let account_can_act = match query.service_account_id {
        Some(service_account_id) => {
            service_account_can_authenticate(&state, service_account_id, None).await?
        }
        None => true,
    };
    let mut nothing_reached = false;
    if !account_can_act {
        let others: Vec<McpGrantSubject> = selected
            .iter()
            .filter(|subject| subject.subject_kind != McpToolGrantSubjectKind::ServiceAccount)
            .cloned()
            .collect();
        if others.is_empty() {
            nothing_reached = true;
        } else {
            preview = previews.preview_subjects(&others).await?;
        }
    }
    let resolution = state
        .store
        .resolve_mcp_access_for_subjects(
            &preview.subjects,
            query.server_id,
            McpToolProjection::Full,
        )
        .await?;
    let exposed_tool_count = resolution.exposed_tool_count;
    // The servers the resolved tools are on, read once by ID, rather than the
    // whole registry.
    let referenced: Vec<Uuid> = resolution
        .allowed_tools
        .iter()
        .map(|tool| tool.mcp_server_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let servers: std::collections::HashMap<Uuid, gateway_core::ExternalMcpServerRecord> = state
        .store
        .list_external_mcp_servers_by_ids(&referenced)
        .await?
        .into_iter()
        .map(|server| (server.mcp_server_id, server))
        .collect();
    // As at runtime, a team-only server's tools count only for its own team:
    // here, a team the previewed caller acts for. A previewed key's scopes and
    // server list then apply on top of its grants.
    let caller_teams: std::collections::HashSet<Uuid> = preview
        .subjects
        .iter()
        .filter(|subject| subject.subject_kind == McpToolGrantSubjectKind::Team)
        .map(|subject| subject.subject_id)
        .collect();
    let allowed_tools: Vec<_> = resolution
        .allowed_tools
        .into_iter()
        .filter(|_| !nothing_reached)
        .filter(|tool| {
            // A tool whose server was not read is left out, as runtime refuses it.
            servers.get(&tool.mcp_server_id).is_some_and(|server| {
                (server.ownership.visibility == gateway_core::McpServerVisibility::Organisation
                    || server
                        .ownership
                        .owner_team_id
                        .is_some_and(|owner| caller_teams.contains(&owner)))
                    && preview.allows_server(&server.server_key)
            })
        })
        .collect();
    // Checked again after every read: a team admin who lost their role or
    // team, or whose server moved, during the preview gets none of it.
    if let (Some(_), Some(server_id)) = (scope.managed_team_id(), query.server_id) {
        ownership(&state).managed_server(&scope, server_id).await?;
    }
    // The caller is read last, so a role or team change during the server
    // read is seen.
    recheck_admin_scope(&state, &headers, &scope).await?;
    Ok(Json(envelope(McpEffectiveAccessPayload {
        referenced_server_count: allowed_tools
            .iter()
            .map(|tool| tool.mcp_server_id)
            .collect::<std::collections::HashSet<_>>()
            .len() as i64,
        filtered_tool_count: exposed_tool_count.saturating_sub(allowed_tools.len() as i64),
        exposed_tool_count,
        tools: allowed_tools.into_iter().map(map_tool).collect(),
    })))
}

/// Whether keys of a service account can authenticate, as `Authenticator`
/// requires: the account is active, belongs to the key's team (when one is
/// named), and has an active budget.
pub(super) async fn service_account_can_authenticate(
    state: &AppState,
    service_account_id: Uuid,
    key_team_id: Option<Uuid>,
) -> Result<bool, AppError> {
    let Some(account) =
        IdentityRepository::get_service_account_by_id(&*state.store, service_account_id).await?
    else {
        return Ok(false);
    };
    if account.status != gateway_core::ServiceAccountStatus::Active
        || key_team_id.is_some_and(|team_id| team_id != account.team_id)
    {
        return Ok(false);
    }
    Ok(gateway_core::BudgetRepository::get_active_budget_by_scope(
        &*state.store,
        &gateway_core::BudgetScope::ServiceAccount { service_account_id },
    )
    .await?
    .is_some())
}

pub(super) fn preview_subjects(query: &McpEffectiveAccessQuery) -> Vec<McpGrantSubject> {
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
