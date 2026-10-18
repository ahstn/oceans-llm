//! Admin endpoints for upstream MCP credential bindings: the secrets the
//! gateway sends to an MCP server for a user, service account or team.

use super::*;

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
    let mut bindings: Vec<_> = match scope.managed_team_id() {
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
    // Each binding's server is read again, so one moved away from a team
    // admin's team in between drops out; the caller's scope is checked last.
    if let Some(team_id) = scope.managed_team_id() {
        let server_ids: Vec<Uuid> = bindings
            .iter()
            .map(|binding| binding.mcp_server_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let still_owned: std::collections::HashSet<Uuid> = state
            .store
            .list_mcp_server_ownerships(&server_ids)
            .await?
            .into_iter()
            .filter(|(_, ownership)| ownership.owned_by(team_id))
            .map(|(mcp_server_id, _)| mcp_server_id)
            .collect();
        bindings.retain(|binding| still_owned.contains(&binding.mcp_server_id));
    }
    recheck_admin_scope(&state, &headers, &scope).await?;
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
    // Replacing a binding by id must not reach one the caller could not set. A
    // caller-chosen ID that names no binding is pinned as absent, so a binding
    // created with it meanwhile is not overwritten on this authorization.
    let mut expected_replaced = request.credential_binding_id.map(|_| None);
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
                // Authorized under its actual storage: a team admin may revoke a
                // `secret_ref` binding but not replace it.
                existing.secret_ref.is_some(),
            )
            .await?;
        // The store compares this with the row it locks, so a concurrent change
        // to the binding cannot be overwritten on this authorization.
        expected_replaced = Some(Some(gateway_core::McpCredentialBindingPin {
            mcp_server_id: existing.mcp_server_id,
            owner_scope_key: existing.owner_scope_key.clone(),
            uses_secret_ref: existing.secret_ref.is_some(),
        }));
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
            // visibility the binding was authorized against. Pinned to the URL
            // and auth settings too, so a retarget or a new token endpoint that
            // lands first refuses the write, rather than store a credential for
            // a destination it was not checked against, or OAuth tokens the
            // change's own revoke has already passed over.
            expected_server: Some(McpServerOwnershipGuard::of(&server).with_auth_settings(&server)),
            expected_replaced,
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

pub(super) async fn find_credential_binding(
    service: &McpCredentialService<AnyStore>,
    credential_binding_id: Uuid,
) -> Result<Option<RedactedMcpCredentialBinding>, AppError> {
    Ok(service.binding(credential_binding_id).await?)
}
