use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use gateway_core::GatewayError;
use gateway_store::GatewayStore;
use uuid::Uuid;

use crate::http::{
    admin_auth::require_platform_admin,
    admin_contract::{
        AdminProviderCredentialStatusView, Envelope, IdentityActionStatus,
        UpsertProviderCredentialRequest, envelope, format_timestamp,
    },
    error::AppError,
    state::AppState,
};

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
    Json(input): Json<UpsertProviderCredentialRequest>,
) -> Result<Json<Envelope<AdminProviderCredentialStatusView>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    ensure_copilot_user_provider(&state, &provider_key)?;
    if state.store.get_identity_user(user_id).await?.is_none() {
        return Err(AppError(GatewayError::InvalidRequest(format!(
            "user `{user_id}` does not exist"
        ))));
    }
    let status = gateway_service::ProviderCredentialService::new(state.store.clone())
        .upsert(&provider_key, user_id, &input.token)
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
) -> Result<Json<Envelope<IdentityActionStatus>>, AppError> {
    require_platform_admin(&state, &headers).await?;
    let deleted = gateway_service::ProviderCredentialService::new(state.store.clone())
        .delete(&provider_key, user_id)
        .await?;
    Ok(Json(envelope(IdentityActionStatus {
        status: if deleted { "deleted" } else { "not_found" },
    })))
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
