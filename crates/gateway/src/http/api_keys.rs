use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use gateway_core::{
    AdminApiKeyRepository, ApiKeyOwnerKind, AuthError, GatewayError, GlobalRole,
    IdentityRepository, MembershipRole, UserStatus,
};
use gateway_service::{
    AdminApiKeyModelOption, AdminApiKeyService, AdminApiKeyServiceAccountOwner, AdminApiKeySummary,
    AdminApiKeyUserOwner, AdminApiKeysPayload as ServiceAdminApiKeysPayload,
    CreateAdminApiKeyInput, CreateAdminApiKeyResult, UpdateAdminApiKeyInput,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::{
    admin_auth::require_authenticated_session,
    admin_contract::{Envelope, envelope, format_timestamp},
    error::AppError,
    state::AppState,
};

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminApiKeysPayload {
    items: Vec<AdminApiKeyView>,
    users: Vec<AdminApiKeyUserOwnerView>,
    service_accounts: Vec<AdminApiKeyServiceAccountOwnerView>,
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
    owner_service_account_key: Option<String>,
    owner_service_account_team_id: Option<String>,
    owner_service_account_team_key: Option<String>,
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
pub struct AdminApiKeyServiceAccountOwnerView {
    id: String,
    name: String,
    key: String,
    team_id: String,
    team_key: String,
    team_name: String,
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
    owner_service_account_id: Option<String>,
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
    let scope = require_api_key_admin_scope(&state, &headers).await?;

    let service = AdminApiKeyService::new(state.store.clone());
    let payload = service.list_api_keys().await?;
    Ok(Json(envelope(map_payload_for_scope(payload, &scope))))
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
    let scope = require_api_key_admin_scope(&state, &headers).await?;
    authorize_create_api_key(&state, &scope, &request).await?;

    let service = AdminApiKeyService::new(state.store.clone());
    let result = service
        .create_api_key(CreateAdminApiKeyInput {
            name: request.name,
            owner_kind: request.owner_kind,
            owner_user_id: request.owner_user_id,
            owner_team_id: request.owner_team_id,
            owner_service_account_id: request.owner_service_account_id,
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
    let scope = require_api_key_admin_scope(&state, &headers).await?;
    let api_key_id = parse_uuid(&api_key_id, "api_key_id")?;
    authorize_existing_api_key(&state, &scope, api_key_id).await?;

    let service = AdminApiKeyService::new(state.store.clone());
    let api_key = service
        .update_api_key(
            api_key_id,
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
    let scope = require_api_key_admin_scope(&state, &headers).await?;
    let api_key_id = parse_uuid(&api_key_id, "api_key_id")?;
    authorize_existing_api_key(&state, &scope, api_key_id).await?;

    let service = AdminApiKeyService::new(state.store.clone());
    let api_key = service.revoke_api_key(api_key_id).await?;

    Ok(Json(envelope(RevokeApiKeyResponse {
        api_key: map_api_key_summary(api_key),
    })))
}

enum ApiKeyAdminScope {
    Platform,
    Team(Uuid),
}

async fn require_api_key_admin_scope(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<ApiKeyAdminScope, AppError> {
    let actor = require_authenticated_session(state, headers).await?;
    if actor.status != UserStatus::Active {
        return Err(insufficient_privileges());
    }
    if actor.global_role == GlobalRole::PlatformAdmin {
        return Ok(ApiKeyAdminScope::Platform);
    }

    let membership = state
        .store
        .get_team_membership_for_user(actor.user_id)
        .await?
        .ok_or_else(insufficient_privileges)?;
    if !matches!(
        membership.role,
        MembershipRole::Owner | MembershipRole::Admin
    ) {
        return Err(insufficient_privileges());
    }

    Ok(ApiKeyAdminScope::Team(membership.team_id))
}

async fn authorize_create_api_key(
    state: &AppState,
    scope: &ApiKeyAdminScope,
    request: &CreateApiKeyRequest,
) -> Result<(), AppError> {
    let ApiKeyAdminScope::Team(team_id) = scope else {
        return Ok(());
    };
    if request.owner_kind.trim() != "service_account" {
        return Err(insufficient_privileges());
    }
    let Some(raw_service_account_id) = request.owner_service_account_id.as_deref() else {
        return Err(insufficient_privileges());
    };
    let service_account_id = parse_uuid(raw_service_account_id, "owner_service_account_id")?;
    let service_account = state
        .store
        .get_service_account_by_id(service_account_id)
        .await?
        .ok_or_else(insufficient_privileges)?;
    if service_account.team_id != *team_id {
        return Err(insufficient_privileges());
    }
    Ok(())
}

async fn authorize_existing_api_key(
    state: &AppState,
    scope: &ApiKeyAdminScope,
    api_key_id: Uuid,
) -> Result<(), AppError> {
    let ApiKeyAdminScope::Team(team_id) = scope else {
        return Ok(());
    };
    let api_key = state
        .store
        .get_api_key_by_id(api_key_id)
        .await?
        .ok_or_else(|| {
            AppError(GatewayError::InvalidRequest(
                "api key not found".to_string(),
            ))
        })?;
    if api_key.owner_kind != ApiKeyOwnerKind::ServiceAccount
        || api_key.owner_team_id != Some(*team_id)
    {
        return Err(insufficient_privileges());
    }
    Ok(())
}

fn map_payload_for_scope(
    payload: ServiceAdminApiKeysPayload,
    scope: &ApiKeyAdminScope,
) -> AdminApiKeysPayload {
    let mut mapped = map_payload(payload);
    if let ApiKeyAdminScope::Team(team_id) = scope {
        let team_id = team_id.to_string();
        mapped
            .items
            .retain(|item| item.owner_service_account_team_id.as_deref() == Some(team_id.as_str()));
        mapped
            .service_accounts
            .retain(|service_account| service_account.team_id == team_id);
        mapped.users.clear();
    }
    mapped
}

fn insufficient_privileges() -> AppError {
    AppError(GatewayError::Auth(AuthError::InsufficientPrivileges))
}

fn map_payload(payload: ServiceAdminApiKeysPayload) -> AdminApiKeysPayload {
    AdminApiKeysPayload {
        items: payload.items.into_iter().map(map_api_key_summary).collect(),
        users: payload.users.into_iter().map(map_user_owner).collect(),
        service_accounts: payload
            .service_accounts
            .into_iter()
            .map(map_service_account_owner)
            .collect(),
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
        owner_service_account_key: api_key.owner_service_account_key,
        owner_service_account_team_id: api_key
            .owner_service_account_team_id
            .map(|value| value.to_string()),
        owner_service_account_team_key: api_key.owner_service_account_team_key,
        model_keys: api_key.model_keys,
        created_at: format_timestamp(api_key.created_at),
        last_used_at: api_key.last_used_at.map(format_timestamp),
        revoked_at: api_key.revoked_at.map(format_timestamp),
    }
}

fn map_service_account_owner(
    service_account: AdminApiKeyServiceAccountOwner,
) -> AdminApiKeyServiceAccountOwnerView {
    AdminApiKeyServiceAccountOwnerView {
        id: service_account.id.to_string(),
        name: service_account.name,
        key: service_account.key,
        team_id: service_account.team_id.to_string(),
        team_key: service_account.team_key,
        team_name: service_account.team_name,
    }
}

fn map_user_owner(user: AdminApiKeyUserOwner) -> AdminApiKeyUserOwnerView {
    AdminApiKeyUserOwnerView {
        id: user.id.to_string(),
        name: user.name,
        email: user.email,
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
