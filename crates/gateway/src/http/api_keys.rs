use std::collections::{BTreeSet, HashMap};

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use gateway_core::{
    ApiKeyOwnerKind, ApiKeyRecord, GatewayError, GatewayModel, ModelRepository, NewApiKeyRecord,
    TeamRecord, UserStatus,
};
use gateway_service::hash_gateway_key_secret;
use gateway_store::{AnyStore, GatewayStore};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    error::AppError,
    identity::{Envelope, envelope, format_timestamp},
    state::AppState,
};

#[derive(Debug, Serialize)]
pub(crate) struct AdminApiKeysPayload {
    items: Vec<AdminApiKeyView>,
    users: Vec<AdminApiKeyUserOwnerView>,
    teams: Vec<AdminApiKeyTeamOwnerView>,
    models: Vec<AdminApiKeyModelView>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminApiKeyView {
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

#[derive(Debug, Serialize)]
pub(crate) struct AdminApiKeyUserOwnerView {
    id: String,
    name: String,
    email: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminApiKeyTeamOwnerView {
    id: String,
    name: String,
    key: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminApiKeyModelView {
    id: String,
    key: String,
    description: Option<String>,
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateApiKeyRequest {
    name: String,
    owner_kind: String,
    owner_user_id: Option<String>,
    owner_team_id: Option<String>,
    model_keys: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CreateApiKeyResponse {
    api_key: AdminApiKeyView,
    raw_key: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct RevokeApiKeyResponse {
    api_key: AdminApiKeyView,
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<AdminApiKeysPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    Ok(Json(envelope(load_admin_api_keys_payload(&state.store).await?)))
}

pub async fn create_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<Json<Envelope<CreateApiKeyResponse>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let users = state.store.list_identity_users().await?;
    let teams = state.store.list_active_teams().await?;
    let models = state.store.list_models().await?;

    let name = request.name.trim();
    if name.is_empty() {
        return Err(AppError(GatewayError::InvalidRequest(
            "api key name is required".to_string(),
        )));
    }

    let owner_kind = ApiKeyOwnerKind::from_db(request.owner_kind.trim()).ok_or_else(|| {
        AppError(GatewayError::InvalidRequest(
            "owner_kind must be `user` or `team`".to_string(),
        ))
    })?;
    let (owner_user_id, owner_team_id) =
        validate_owner(&request, owner_kind, &users, &teams).await?;
    let granted_models = select_granted_models(&request.model_keys, &models)?;

    let public_id = Uuid::new_v4().simple().to_string();
    let secret = format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let raw_key = format!("gwk_{public_id}.{secret}");
    let secret_hash = hash_gateway_key_secret(&secret)
        .map_err(|error| GatewayError::Internal(error.to_string()))?;
    let now = OffsetDateTime::now_utc();

    let new_api_key = NewApiKeyRecord {
        name: name.to_string(),
        public_id,
        secret_hash,
        owner_kind,
        owner_user_id,
        owner_team_id,
        created_at: now,
    };
    let api_key = state
        .store
        .create_api_key(&new_api_key)
        .await?;
    let model_ids = granted_models.iter().map(|model| model.id).collect::<Vec<_>>();
    state
        .store
        .replace_api_key_model_grants(api_key.id, &model_ids)
        .await?;

    let granted_models = state.store.list_models_for_api_key(api_key.id).await?;
    let api_key = build_admin_api_key_view(&api_key, &users, &teams, &granted_models)?;

    Ok(Json(envelope(CreateApiKeyResponse { api_key, raw_key })))
}

pub async fn revoke_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(api_key_id): Path<String>,
) -> Result<Json<Envelope<RevokeApiKeyResponse>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let api_key_id = parse_uuid(&api_key_id, "api_key_id")?;
    let users = state.store.list_identity_users().await?;
    let teams = state.store.list_active_teams().await?;

    let existing = state
        .store
        .get_api_key_by_id(api_key_id)
        .await?
        .ok_or_else(|| gateway_core::StoreError::NotFound(format!("api key `{api_key_id}`")))?;

    let _ = state
        .store
        .revoke_api_key(api_key_id, OffsetDateTime::now_utc())
        .await?;
    let api_key = state
        .store
        .get_api_key_by_id(api_key_id)
        .await?
        .unwrap_or(existing);
    let granted_models = state.store.list_models_for_api_key(api_key.id).await?;
    let api_key = build_admin_api_key_view(&api_key, &users, &teams, &granted_models)?;

    Ok(Json(envelope(RevokeApiKeyResponse { api_key })))
}

async fn load_admin_api_keys_payload(store: &AnyStore) -> Result<AdminApiKeysPayload, AppError> {
    let api_keys = store.list_api_keys().await?;
    let users = store.list_identity_users().await?;
    let teams = store.list_active_teams().await?;
    let models = store.list_models().await?;

    let mut items = Vec::with_capacity(api_keys.len());
    for api_key in api_keys {
        let granted_models = store.list_models_for_api_key(api_key.id).await?;
        items.push(build_admin_api_key_view(
            &api_key,
            &users,
            &teams,
            &granted_models,
        )?);
    }

    Ok(AdminApiKeysPayload {
        items,
        users: users
            .into_iter()
            .filter(|user| user.user.status == UserStatus::Active)
            .map(|user| AdminApiKeyUserOwnerView {
                id: user.user.user_id.to_string(),
                name: user.user.name,
                email: user.user.email,
            })
            .collect(),
        teams: teams
            .iter()
            .map(|team| AdminApiKeyTeamOwnerView {
                id: team.team_id.to_string(),
                name: team.team_name.clone(),
                key: team.team_key.clone(),
            })
            .collect(),
        models: models
            .into_iter()
            .map(|model| AdminApiKeyModelView {
                id: model.id.to_string(),
                key: model.model_key,
                description: model.description,
                tags: model.tags,
            })
            .collect(),
    })
}

fn build_admin_api_key_view(
    api_key: &ApiKeyRecord,
    users: &[gateway_core::IdentityUserRecord],
    teams: &[TeamRecord],
    granted_models: &[GatewayModel],
) -> Result<AdminApiKeyView, AppError> {
    let user_map = users
        .iter()
        .map(|user| (user.user.user_id, user))
        .collect::<HashMap<_, _>>();
    let team_map = teams
        .iter()
        .map(|team| (team.team_id, team))
        .collect::<HashMap<_, _>>();

    let (owner_id, owner_name, owner_email, owner_team_key) = match api_key.owner_kind {
        ApiKeyOwnerKind::User => {
            let owner_id = api_key.owner_user_id.ok_or_else(|| {
                AppError(GatewayError::Internal(format!(
                    "api key `{}` is missing owner_user_id",
                    api_key.id
                )))
            })?;
            let owner = user_map.get(&owner_id).ok_or_else(|| {
                AppError(GatewayError::Internal(format!(
                    "api key `{}` references missing user `{owner_id}`",
                    api_key.id
                )))
            })?;
            (
                owner_id.to_string(),
                owner.user.name.clone(),
                Some(owner.user.email.clone()),
                None,
            )
        }
        ApiKeyOwnerKind::Team => {
            let owner_id = api_key.owner_team_id.ok_or_else(|| {
                AppError(GatewayError::Internal(format!(
                    "api key `{}` is missing owner_team_id",
                    api_key.id
                )))
            })?;
            let owner = team_map.get(&owner_id).ok_or_else(|| {
                AppError(GatewayError::Internal(format!(
                    "api key `{}` references missing team `{owner_id}`",
                    api_key.id
                )))
            })?;
            (
                owner_id.to_string(),
                owner.team_name.clone(),
                None,
                Some(owner.team_key.clone()),
            )
        }
    };

    Ok(AdminApiKeyView {
        id: api_key.id.to_string(),
        name: api_key.name.clone(),
        prefix: format!("gwk_{}", api_key.public_id),
        status: api_key.status.as_str().to_string(),
        owner_kind: api_key.owner_kind.as_str().to_string(),
        owner_id,
        owner_name,
        owner_email,
        owner_team_key,
        model_keys: granted_models
            .iter()
            .map(|model| model.model_key.clone())
            .collect(),
        created_at: format_timestamp(api_key.created_at),
        last_used_at: api_key.last_used_at.map(format_timestamp),
        revoked_at: api_key.revoked_at.map(format_timestamp),
    })
}

async fn validate_owner(
    request: &CreateApiKeyRequest,
    owner_kind: ApiKeyOwnerKind,
    users: &[gateway_core::IdentityUserRecord],
    teams: &[TeamRecord],
) -> Result<(Option<Uuid>, Option<Uuid>), AppError> {
    match owner_kind {
        ApiKeyOwnerKind::User => {
            if request.owner_team_id.is_some() {
                return Err(AppError(GatewayError::InvalidRequest(
                    "user-owned api keys cannot include owner_team_id".to_string(),
                )));
            }
            let user_id = request
                .owner_user_id
                .as_deref()
                .ok_or_else(|| {
                    AppError(GatewayError::InvalidRequest(
                        "owner_user_id is required for user-owned api keys".to_string(),
                    ))
                })
                .and_then(|value| parse_uuid(value, "owner_user_id"))?;
            let user = users
                .iter()
                .find(|user| user.user.user_id == user_id)
                .ok_or_else(|| gateway_core::StoreError::NotFound(format!("user `{user_id}`")))?;
            if user.user.status != UserStatus::Active {
                return Err(AppError(GatewayError::InvalidRequest(
                    "user-owned api keys require an active user".to_string(),
                )));
            }
            Ok((Some(user_id), None))
        }
        ApiKeyOwnerKind::Team => {
            if request.owner_user_id.is_some() {
                return Err(AppError(GatewayError::InvalidRequest(
                    "team-owned api keys cannot include owner_user_id".to_string(),
                )));
            }
            let team_id = request
                .owner_team_id
                .as_deref()
                .ok_or_else(|| {
                    AppError(GatewayError::InvalidRequest(
                        "owner_team_id is required for team-owned api keys".to_string(),
                    ))
                })
                .and_then(|value| parse_uuid(value, "owner_team_id"))?;
            let team = teams
                .iter()
                .find(|team| team.team_id == team_id)
                .ok_or_else(|| gateway_core::StoreError::NotFound(format!("team `{team_id}`")))?;
            if team.status != "active" {
                return Err(AppError(GatewayError::InvalidRequest(
                    "team-owned api keys require an active team".to_string(),
                )));
            }
            Ok((None, Some(team_id)))
        }
    }
}

fn select_granted_models(
    raw_model_keys: &[String],
    models: &[GatewayModel],
) -> Result<Vec<GatewayModel>, AppError> {
    let mut seen = BTreeSet::new();
    let model_map = models
        .iter()
        .map(|model| (model.model_key.as_str(), model))
        .collect::<HashMap<_, _>>();

    let mut selected = Vec::new();
    for raw_model_key in raw_model_keys {
        let model_key = raw_model_key.trim();
        if model_key.is_empty() || !seen.insert(model_key.to_string()) {
            continue;
        }
        let model = model_map.get(model_key).ok_or_else(|| {
            AppError(GatewayError::InvalidRequest(format!(
                "unknown model_key `{model_key}`"
            )))
        })?;
        selected.push((*model).clone());
    }

    if selected.is_empty() {
        return Err(AppError(GatewayError::InvalidRequest(
            "at least one model_key is required".to_string(),
        )));
    }

    Ok(selected)
}

fn parse_uuid(raw: &str, field_name: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| {
        AppError(GatewayError::InvalidRequest(format!(
            "{field_name} must be a valid uuid"
        )))
    })
}
