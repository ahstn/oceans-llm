use axum::http::HeaderMap;
use gateway_core::{AuthError, GatewayError, GlobalRole, UserRecord};

use crate::http::{error::AppError, identity::resolve_session_user, state::AppState};

pub(crate) async fn require_platform_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<UserRecord, AppError> {
    let current_user = require_authenticated_session(state, headers).await?;

    if current_user.global_role != GlobalRole::PlatformAdmin {
        return Err(AppError(GatewayError::Auth(
            AuthError::InsufficientPrivileges,
        )));
    }
    if current_user.status != "active" {
        return Err(AppError(GatewayError::InvalidRequest(
            "only active admins can access admin endpoints".to_string(),
        )));
    }

    Ok(current_user)
}

pub(crate) async fn require_authenticated_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<UserRecord, AppError> {
    resolve_session_user(state, headers)
        .await?
        .ok_or(AppError(GatewayError::Auth(AuthError::SessionRequired)))
}
