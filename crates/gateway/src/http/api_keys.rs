use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use gateway_core::GatewayError;
use gateway_service::{
    AdminApiKeyModelOption, AdminApiKeyService, AdminApiKeySummary, AdminApiKeyTeamOwner,
    AdminApiKeyUserOwner, AdminApiKeysPayload as ServiceAdminApiKeysPayload,
    CreateAdminApiKeyInput, CreateAdminApiKeyResult, UpdateAdminApiKeyInput,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{Envelope, envelope, format_timestamp},
    error::AppError,
    state::AppState,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminApiKeysPayload {
    items: Vec<AdminApiKeyView>,
    users: Vec<AdminApiKeyUserOwnerView>,
    teams: Vec<AdminApiKeyTeamOwnerView>,
    models: Vec<AdminApiKeyModelView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminApiKeyView {
    id: String,
    name: String,
    prefix: String,
    status: String,
    owner_kind: String,
    owner_id: String,
    owner_name: String,
    owner_email: Option<String>,
    owner_team_key: Option<String>,
    model_keys: Vec<String>,
    created_at: String,
    last_used_at: Option<String>,
    revoked_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminApiKeyUserOwnerView {
    id: String,
    name: String,
    email: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminApiKeyTeamOwnerView {
    id: String,
    name: String,
    key: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminApiKeyModelView {
    id: String,
    key: String,
    description: Option<String>,
    tags: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateApiKeyRequest {
    name: String,
    owner_kind: String,
    owner_user_id: Option<String>,
    owner_team_id: Option<String>,
    model_keys: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateApiKeyResponse {
    api_key: AdminApiKeyView,
    raw_key: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateApiKeyRequest {
    model_keys: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateApiKeyResponse {
    api_key: AdminApiKeyView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RevokeApiKeyResponse {
    api_key: AdminApiKeyView,
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/api-keys",
    responses((status = 200, body = Envelope<AdminApiKeysPayload>)),
    security(("session_cookie" = []))
)]
pub async fn list_api_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<AdminApiKeysPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let service = AdminApiKeyService::new(state.store.clone());
    let payload = service.list_api_keys().await?;
    Ok(Json(envelope(map_payload(payload))))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/api-keys",
    request_body = CreateApiKeyRequest,
    responses((status = 200, body = Envelope<CreateApiKeyResponse>)),
    security(("session_cookie" = []))
)]
pub async fn create_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<Json<Envelope<CreateApiKeyResponse>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let service = AdminApiKeyService::new(state.store.clone());
    let result = service
        .create_api_key(CreateAdminApiKeyInput {
            name: request.name,
            owner_kind: request.owner_kind,
            owner_user_id: request.owner_user_id,
            owner_team_id: request.owner_team_id,
            model_keys: request.model_keys,
        })
        .await?;

    Ok(Json(envelope(map_create_result(result))))
}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/api-keys/{api_key_id}",
    request_body = UpdateApiKeyRequest,
    params(("api_key_id" = String, Path, description = "API key identifier")),
    responses((status = 200, body = Envelope<UpdateApiKeyResponse>)),
    security(("session_cookie" = []))
)]
pub async fn update_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(api_key_id): Path<String>,
    Json(request): Json<UpdateApiKeyRequest>,
) -> Result<Json<Envelope<UpdateApiKeyResponse>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let service = AdminApiKeyService::new(state.store.clone());
    let api_key = service
        .update_api_key(
            parse_uuid(&api_key_id, "api_key_id")?,
            UpdateAdminApiKeyInput {
                model_keys: request.model_keys,
            },
        )
        .await?;

    Ok(Json(envelope(UpdateApiKeyResponse {
        api_key: map_api_key_summary(api_key),
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/api-keys/{api_key_id}/revoke",
    params(("api_key_id" = String, Path, description = "API key identifier")),
    responses((status = 200, body = Envelope<RevokeApiKeyResponse>)),
    security(("session_cookie" = []))
)]
pub async fn revoke_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(api_key_id): Path<String>,
) -> Result<Json<Envelope<RevokeApiKeyResponse>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let service = AdminApiKeyService::new(state.store.clone());
    let api_key = service
        .revoke_api_key(parse_uuid(&api_key_id, "api_key_id")?)
        .await?;

    Ok(Json(envelope(RevokeApiKeyResponse {
        api_key: map_api_key_summary(api_key),
    })))
}

fn map_payload(payload: ServiceAdminApiKeysPayload) -> AdminApiKeysPayload {
    AdminApiKeysPayload {
        items: payload.items.into_iter().map(map_api_key_summary).collect(),
        users: payload.users.into_iter().map(map_user_owner).collect(),
        teams: payload.teams.into_iter().map(map_team_owner).collect(),
        models: payload.models.into_iter().map(map_model_option).collect(),
    }
}

fn map_create_result(result: CreateAdminApiKeyResult) -> CreateApiKeyResponse {
    CreateApiKeyResponse {
        api_key: map_api_key_summary(result.api_key),
        raw_key: result.raw_key,
    }
}

fn map_api_key_summary(api_key: AdminApiKeySummary) -> AdminApiKeyView {
    AdminApiKeyView {
        id: api_key.id.to_string(),
        name: api_key.name,
        prefix: api_key.prefix,
        status: api_key.status.as_str().to_string(),
        owner_kind: api_key.owner_kind.as_str().to_string(),
        owner_id: api_key.owner_id.to_string(),
        owner_name: api_key.owner_name,
        owner_email: api_key.owner_email,
        owner_team_key: api_key.owner_team_key,
        model_keys: api_key.model_keys,
        created_at: format_timestamp(api_key.created_at),
        last_used_at: api_key.last_used_at.map(format_timestamp),
        revoked_at: api_key.revoked_at.map(format_timestamp),
    }
}

fn map_user_owner(user: AdminApiKeyUserOwner) -> AdminApiKeyUserOwnerView {
    AdminApiKeyUserOwnerView {
        id: user.id.to_string(),
        name: user.name,
        email: user.email,
    }
}

fn map_team_owner(team: AdminApiKeyTeamOwner) -> AdminApiKeyTeamOwnerView {
    AdminApiKeyTeamOwnerView {
        id: team.id.to_string(),
        name: team.name,
        key: team.key,
    }
}

fn map_model_option(model: AdminApiKeyModelOption) -> AdminApiKeyModelView {
    AdminApiKeyModelView {
        id: model.id.to_string(),
        key: model.key,
        description: model.description,
        tags: model.tags,
    }
}

fn parse_uuid(raw: &str, field_name: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| {
        AppError(GatewayError::InvalidRequest(format!(
            "{field_name} must be a valid uuid"
        )))
    })
}
