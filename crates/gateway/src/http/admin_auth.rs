use axum::http::HeaderMap;
use gateway_core::{
    AuthError, GatewayError, GlobalRole, IdentityRepository, MembershipRole, UserRecord, UserStatus,
};
use gateway_service::McpAdminScope;

use crate::config::{AdminPage, AdminPermissionGroup};
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

/// Spend Controls is open to platform admins and, when the `team_admins`
/// permission group includes the page, to owners/admins of a team.
/// Returns the actor so writes can record who made them.
pub(crate) async fn require_spend_controls_scope(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(UserRecord, AdminDataScope), AppError> {
    let current_user = require_active_session(state, headers).await?;
    if current_user.global_role == GlobalRole::PlatformAdmin {
        return Ok((current_user, AdminDataScope::Platform));
    }
    let membership = state
        .store
        .get_team_membership_for_user(current_user.user_id)
        .await?
        .ok_or_else(insufficient_privileges)?;
    let team_admin = matches!(
        membership.role,
        MembershipRole::Owner | MembershipRole::Admin
    );
    let page_granted = state
        .admin_permissions
        .for_group(AdminPermissionGroup::TeamAdmins)
        .pages
        .contains(&AdminPage::SpendControls);
    if !(team_admin && page_granted) {
        return Err(insufficient_privileges());
    }
    Ok((current_user, AdminDataScope::Team(membership.team_id)))
}

/// MCP administration is open to platform admins and, when the `team_admins`
/// permission group includes the `mcp` page, to owners/admins of a team. The
/// scope limits a team admin to their own team's servers; see
/// `gateway_service::McpOwnership`.
pub(crate) async fn require_mcp_admin_scope(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(UserRecord, McpAdminScope), AppError> {
    let current_user = require_active_session(state, headers).await?;
    let membership = state
        .store
        .get_team_membership_for_user(current_user.user_id)
        .await?;
    if current_user.global_role == GlobalRole::PlatformAdmin {
        let scope = McpAdminScope::platform_admin(membership.as_ref());
        return Ok((current_user, scope));
    }
    let page_granted = state
        .admin_permissions
        .for_group(AdminPermissionGroup::TeamAdmins)
        .pages
        .contains(&AdminPage::Mcp);
    let scope = membership
        .as_ref()
        .and_then(McpAdminScope::team_admin)
        .filter(|_| page_granted)
        .ok_or_else(insufficient_privileges)?;
    Ok((current_user, scope))
}

/// Who is reading the MCP area. Admins see what [`McpAdminScope`] allows; every
/// other user with the `mcp` page sees, read-only, the servers their own grants
/// let them connect to.
#[derive(Debug, Clone, Copy)]
pub(crate) enum McpViewer {
    Admin(McpAdminScope),
    Member {
        user_id: uuid::Uuid,
        team_id: Option<uuid::Uuid>,
    },
}

/// For the MCP read routes: an admin scope when the caller has one, otherwise a
/// member view when the `users` group includes the `mcp` page.
pub(crate) async fn require_mcp_viewer(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<McpViewer, AppError> {
    match require_mcp_admin_scope(state, headers).await {
        Ok((_, scope)) => return Ok(McpViewer::Admin(scope)),
        Err(AppError(GatewayError::Auth(AuthError::InsufficientPrivileges))) => {}
        Err(error) => return Err(error),
    }
    let current_user = require_active_session(state, headers).await?;
    let page_granted = state
        .admin_permissions
        .for_group(AdminPermissionGroup::Users)
        .pages
        .contains(&AdminPage::Mcp);
    if !page_granted {
        return Err(insufficient_privileges());
    }
    let team_id = state
        .store
        .get_team_membership_for_user(current_user.user_id)
        .await?
        .map(|membership| membership.team_id);
    Ok(McpViewer::Member {
        user_id: current_user.user_id,
        team_id,
    })
}

/// Like [`require_mcp_admin_scope`], for MCP routes only platform admins use,
/// such as tool sets. The scope is kept for the audit entry.
pub(crate) async fn require_mcp_platform_scope(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(UserRecord, McpAdminScope), AppError> {
    let (actor, scope) = require_mcp_admin_scope(state, headers).await?;
    if scope.is_platform_admin() {
        Ok((actor, scope))
    } else {
        Err(insufficient_privileges())
    }
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
    capabilities: crate::config::AgentAnalysisRuntimeCapabilities,
    scope: AdminDataScope,
) -> bool {
    match scope {
        AdminDataScope::Platform => capabilities.access_for(true, false).allowed,
        AdminDataScope::Team(_) => capabilities.access_for(false, true).allowed,
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
    use crate::config::AgentAnalysisRuntimeCapabilities;

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
