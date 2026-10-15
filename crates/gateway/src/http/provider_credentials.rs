use axum::{
    Json,
    extract::{FromRequestParts, Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode, header::AUTHORIZATION, request::Parts},
};
use gateway_core::{
    AuditActor, AuditActorKind, AuthError, GatewayError, IdentityRepository, UserStatus,
};
use gateway_service::{
    AuditAction, AuditEvent, ProviderCredentialService, ProviderCredentialStatus,
    admin_audit::actions,
};
use gateway_store::GatewayStore;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{
        AdminProviderCredentialStatusView, Envelope, IdentityActionStatus,
        UpsertProviderCredentialRequest, envelope, format_timestamp,
    },
    audit::AuditRequestMeta,
    error::AppError,
    state::AppState,
};

pub(super) const PROVIDER_CREDENTIAL_BODY_LIMIT: usize = 8 * 1024;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetMyProviderCredentialRequest {
    pub token: String,
}

#[derive(Serialize, ToSchema)]
pub struct SelfProviderCredentialView {
    pub provider_key: String,
    pub configured: bool,
    pub updated_at: Option<String>,
    pub last_used_at: Option<String>,
}

impl From<ProviderCredentialStatus> for SelfProviderCredentialView {
    fn from(status: ProviderCredentialStatus) -> Self {
        Self {
            provider_key: status.provider_key,
            configured: status.configured,
            updated_at: status.updated_at.map(format_timestamp),
            last_used_at: status.last_used_at.map(format_timestamp),
        }
    }
}

// Authenticate before buffering a token, and never borrow a cookie's identity.
pub struct ProviderCredentialUser(Uuid, AuditActor);

impl FromRequestParts<AppState> for ProviderCredentialUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let authorization = parts
            .headers
            .get(AUTHORIZATION)
            .map(|value| {
                value
                    .to_str()
                    .map_err(|_| AuthError::InvalidAuthorizationHeader)
            })
            .transpose()?;
        let key = state.service.authenticate(authorization).await?;
        if !key.is_user_owned() {
            return Err(AuthError::InsufficientPrivileges.into());
        }
        let user_id = key.owner_user_id.ok_or(AuthError::ApiKeyOwnerInvalid)?;
        let user = state
            .store
            .get_user_by_id(user_id)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        if user.status != UserStatus::Active {
            return Err(AuthError::InsufficientPrivileges.into());
        }
        let actor = AuditActor {
            kind: AuditActorKind::ApiKey,
            user_id: Some(user_id),
            api_key_id: Some(key.id),
            label: Some(user.email),
        };
        Ok(Self(user_id, actor))
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/me/provider-credentials",
    responses(
        (status = 200, body = [SelfProviderCredentialView]),
        (status = 401, description = "Missing or invalid gateway API key"),
        (status = 403, description = "An active user-owned gateway API key is required")
    ),
    security(("gateway_api_key" = []))
)]
pub async fn list_my_provider_credentials(
    State(state): State<AppState>,
    ProviderCredentialUser(user_id, _): ProviderCredentialUser,
) -> Result<Json<Vec<SelfProviderCredentialView>>, AppError> {
    let service = ProviderCredentialService::new(state.store.clone());
    let mut credentials = Vec::with_capacity(state.copilot_user_provider_keys.len());
    for provider_key in state.copilot_user_provider_keys.iter() {
        credentials.push(service.status(provider_key, user_id).await?.into());
    }
    Ok(Json(credentials))
}

#[utoipa::path(
    put,
    path = "/api/v1/me/provider-credentials/{provider_key}",
    request_body = SetMyProviderCredentialRequest,
    responses(
        (status = 200, body = SelfProviderCredentialView),
        (status = 400, description = "Invalid token or provider is not configured for GitHub user authentication"),
        (status = 401, description = "Missing or invalid gateway API key"),
        (status = 403, description = "An active user-owned gateway API key is required"),
        (status = 413, description = "Request body exceeds 8 KiB")
    ),
    security(("gateway_api_key" = []))
)]
pub async fn set_my_provider_credential(
    State(state): State<AppState>,
    ProviderCredentialUser(user_id, actor): ProviderCredentialUser,
    meta: AuditRequestMeta,
    Path(provider_key): Path<String>,
    input: Result<Json<SetMyProviderCredentialRequest>, JsonRejection>,
) -> Result<Json<SelfProviderCredentialView>, AppError> {
    // Serde errors can quote submitted values. Keep secret input out of errors.
    let Json(input) = input.map_err(|error| {
        if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
            GatewayError::PayloadTooLarge {
                limit_bytes: PROVIDER_CREDENTIAL_BODY_LIMIT,
            }
        } else {
            GatewayError::InvalidRequest(
                "request must be JSON containing only a token string".to_string(),
            )
        }
    })?;
    ensure_copilot_user_provider(&state, &provider_key)?;
    let service = ProviderCredentialService::new(state.store.clone());
    let status = state
        .audit()
        .audited(
            &meta.context(actor),
            service.upsert(&provider_key, user_id, &input.token),
            |_| provider_credential_event(actions::PROVIDER_CREDENTIAL_SET, user_id, &provider_key),
        )
        .await?;
    Ok(Json(status.into()))
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/identity/users/{user_id}/provider-credentials/{provider_key}",
    tag = "crate::http::identity",
    request_body = UpsertProviderCredentialRequest,
    responses((status = 200, body = Envelope<AdminProviderCredentialStatusView>)),
    security(("session_cookie" = []))
)]
pub async fn upsert_identity_user_provider_credential(
    State(state): State<AppState>,
    Path((user_id, provider_key)): Path<(Uuid, String)>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
    Json(input): Json<UpsertProviderCredentialRequest>,
) -> Result<Json<Envelope<AdminProviderCredentialStatusView>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    ensure_copilot_user_provider(&state, &provider_key)?;
    if state.store.get_identity_user(user_id).await?.is_none() {
        return Err(AppError(GatewayError::InvalidRequest(format!(
            "user `{user_id}` does not exist"
        ))));
    }
    let service = ProviderCredentialService::new(state.store.clone());
    let status = state
        .audit()
        .audited(
            &meta.user_context(&actor),
            service.upsert(&provider_key, user_id, &input.token),
            |_| provider_credential_event(actions::PROVIDER_CREDENTIAL_SET, user_id, &provider_key),
        )
        .await?;
    Ok(Json(envelope(AdminProviderCredentialStatusView {
        user_id: user_id.to_string(),
        configured: true,
        updated_at: status.updated_at.map(format_timestamp),
        last_used_at: status.last_used_at.map(format_timestamp),
    })))
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/identity/users/{user_id}/provider-credentials/{provider_key}",
    tag = "crate::http::identity",
    responses((status = 200, body = Envelope<IdentityActionStatus>)),
    security(("session_cookie" = []))
)]
pub async fn delete_identity_user_provider_credential(
    State(state): State<AppState>,
    Path((user_id, provider_key)): Path<(Uuid, String)>,
    headers: HeaderMap,
    meta: AuditRequestMeta,
) -> Result<Json<Envelope<IdentityActionStatus>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let service = ProviderCredentialService::new(state.store.clone());
    let deleted = state
        .audit()
        .audited_change(
            &meta.user_context(&actor),
            service.delete(&provider_key, user_id),
            || {
                provider_credential_event(
                    actions::PROVIDER_CREDENTIAL_DELETED,
                    user_id,
                    &provider_key,
                )
            },
        )
        .await?;
    Ok(Json(envelope(IdentityActionStatus {
        status: if deleted { "deleted" } else { "not_found" },
    })))
}

/// The token itself is never part of the entry; the target names whose credential changed.
fn provider_credential_event(action: AuditAction, user_id: Uuid, provider_key: &str) -> AuditEvent {
    AuditEvent::new(action)
        .target(format!("{user_id}/{provider_key}"))
        .label(provider_key.to_string())
        .meta("user_id", user_id)
        .meta("provider_key", provider_key)
}

fn ensure_copilot_user_provider(state: &AppState, provider_key: &str) -> Result<(), AppError> {
    if state
        .copilot_user_provider_keys
        .iter()
        .any(|key| key == provider_key)
    {
        Ok(())
    } else {
        Err(AppError(GatewayError::InvalidRequest(format!(
            "provider `{provider_key}` is not configured for GitHub user authentication"
        ))))
    }
}

#[cfg(test)]
mod tests;
