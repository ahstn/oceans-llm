use axum::http::HeaderMap;
use gateway_core::{
    AuthError, GatewayError, GlobalRole, IdentityRepository, MembershipRole, UserRecord, UserStatus,
};

use crate::http::{error::AppError, identity::resolve_session_user, state::AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdminDataScope {
    Platform,
    Team(uuid::Uuid),
}

pub(crate) async fn require_admin_data_scope(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AdminDataScope, AppError> {
    let current_user = require_authenticated_session(state, headers).await?;
    if current_user.status != UserStatus::Active {
        return Err(insufficient_privileges());
    }
    if current_user.global_role == GlobalRole::PlatformAdmin {
        return Ok(AdminDataScope::Platform);
    }
    let membership = state
        .store
        .get_team_membership_for_user(current_user.user_id)
        .await?
        .ok_or_else(insufficient_privileges)?;
    if !matches!(
        membership.role,
        MembershipRole::Owner | MembershipRole::Admin
    ) {
        return Err(insufficient_privileges());
    }
    Ok(AdminDataScope::Team(membership.team_id))
}

pub(crate) async fn require_agent_analysis_scope(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AdminDataScope, AppError> {
    let scope = require_admin_data_scope(state, headers).await?;
    if agent_analysis_scope_enabled(state.agent_analysis, scope) {
        Ok(scope)
    } else {
        Err(insufficient_privileges())
    }
}

fn agent_analysis_scope_enabled(
    capabilities: crate::http::state::AgentAnalysisRuntimeCapabilities,
    scope: AdminDataScope,
) -> bool {
    match scope {
        AdminDataScope::Platform => {
            capabilities.shadow_diagnostics_visible || capabilities.calibrated_score_visible
        }
        AdminDataScope::Team(_) => {
            capabilities.team_admin_analytics_enabled && capabilities.calibrated_score_visible
        }
    }
}

fn insufficient_privileges() -> AppError {
    AppError(GatewayError::Auth(AuthError::InsufficientPrivileges))
}

pub(crate) async fn require_platform_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<UserRecord, AppError> {
    let current_user = require_active_session(state, headers).await?;

    if current_user.global_role != GlobalRole::PlatformAdmin {
        return Err(AppError(GatewayError::Auth(
            AuthError::InsufficientPrivileges,
        )));
    }

    Ok(current_user)
}

pub(crate) async fn require_active_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<UserRecord, AppError> {
    let current_user = require_authenticated_session(state, headers).await?;
    if current_user.status != UserStatus::Active {
        return Err(AppError(GatewayError::Auth(
            AuthError::InsufficientPrivileges,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::state::AgentAnalysisRuntimeCapabilities;

    fn capabilities() -> AgentAnalysisRuntimeCapabilities {
        AgentAnalysisRuntimeCapabilities {
            passive_analysis_enabled: true,
            shadow_diagnostics_visible: false,
            calibrated_score_visible: false,
            team_admin_analytics_enabled: false,
        }
    }

    #[test]
    fn shadow_access_is_platform_only() {
        let mut flags = capabilities();
        flags.shadow_diagnostics_visible = true;

        assert!(agent_analysis_scope_enabled(
            flags,
            AdminDataScope::Platform
        ));
        assert!(!agent_analysis_scope_enabled(
            flags,
            AdminDataScope::Team(uuid::Uuid::nil())
        ));
    }

    #[test]
    fn team_access_requires_calibrated_and_team_flags() {
        let mut flags = capabilities();
        flags.team_admin_analytics_enabled = true;
        assert!(!agent_analysis_scope_enabled(
            flags,
            AdminDataScope::Team(uuid::Uuid::nil())
        ));

        flags.calibrated_score_visible = true;
        assert!(agent_analysis_scope_enabled(
            flags,
            AdminDataScope::Team(uuid::Uuid::nil())
        ));
    }
}
