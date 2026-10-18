//! Editing an API key's capability scopes and MCP server list.
//! See `docs/adr/2026-10-17-scoped-api-keys.md`.

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use gateway_core::ApiKeyAccess;
use gateway_service::{AdminApiKeyService, UpdateAdminApiKeyScopesInput, admin_audit::actions};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{
    AdminApiKeyView, api_key_event, authorize_existing_api_key, map_api_key_summary, parse_uuid,
    partial_api_key_event, require_api_key_action_scope,
};
use crate::{
    config::AdminAction,
    http::{
        admin_contract::{Envelope, envelope},
        audit::AuditRequestMeta,
        error::AppError,
        state::AppState,
    },
};

/// Both fields are required, so leaving one out can't widen a key by
/// accident. `null` restores full access or every server.
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateApiKeyScopesRequest {
    #[serde(deserialize_with = "Option::deserialize")]
    #[schema(required = true)]
    scopes: Option<Vec<String>>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schema(required = true)]
    mcp_server_keys: Option<Vec<String>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateApiKeyScopesResponse {
    api_key: AdminApiKeyView,
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/api-keys/{api_key_id}/scopes",
    request_body = UpdateApiKeyScopesRequest,
    params(("api_key_id" = String, Path, description = "API key identifier")),
    responses((status = 200, body = Envelope<UpdateApiKeyScopesResponse>)),
    security(("session_cookie" = []))
)]
pub async fn update_api_key_scopes(
    State(state): State<AppState>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Path(api_key_id): Path<String>,
    Json(request): Json<UpdateApiKeyScopesRequest>,
) -> Result<Json<Envelope<UpdateApiKeyScopesResponse>>, AppError> {
    let (actor, scope) =
        require_api_key_action_scope(&state, &headers, AdminAction::UpdateApiKey).await?;
    let api_key_id = parse_uuid(&api_key_id, "api_key_id")?;
    authorize_existing_api_key(&state, &scope, api_key_id).await?;

    let service = AdminApiKeyService::new(state.store.clone());
    let requested = serde_json::json!({
        "scopes": request.scopes,
        "mcp_server_keys": request.mcp_server_keys,
    });
    let progress = gateway_service::AuditProgress::default();
    let update = service.update_api_key_scopes(
        api_key_id,
        UpdateAdminApiKeyScopesInput {
            scopes: request.scopes,
            mcp_server_keys: request.mcp_server_keys,
            viewer_team: super::api_key_viewer_team(&state, &scope).await?,
        },
        &progress,
    );
    let result = state
        .audit()
        .audited_steps(
            &meta.user_context(&actor),
            &progress,
            update,
            // The store reports the access it replaced, read under the write lock, and
            // writes nothing when the key already has this access, which records nothing.
            |result| {
                result.replaced_access.as_ref().map(|previous| {
                    api_key_event(actions::API_KEY_SCOPES_UPDATED, &result.api_key)
                        .before(access_summary(previous))
                        .after(access_summary(&result.api_key.access))
                })
            },
            |progress, error| {
                partial_api_key_event(actions::API_KEY_SCOPES_UPDATED, progress, error)
                    .meta("requested", &requested)
            },
        )
        .await?;

    Ok(Json(envelope(UpdateApiKeyScopesResponse {
        api_key: map_api_key_summary(result.api_key),
    })))
}

fn access_summary(access: &ApiKeyAccess) -> serde_json::Value {
    serde_json::json!({
        "scopes": access.scopes,
        "mcp_server_keys": access.mcp_server_keys,
    })
}
