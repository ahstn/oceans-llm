use gateway_core::{AuthMode, IdentityUserRecord, MembershipRole, UserStatus};
use gateway_store::{AnyStore, GatewayStore};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::http::{
    error::AppError,
    identity::{
        AdminIdentityUserView, AdminOnboardingActionView, AdminTeamAdminView,
        AdminTeamAssignableUserView, AdminTeamManagementView, AdminTeamMemberView, invitation_url,
        oidc_sign_in_url, format_timestamp,
    },
};

pub(crate) async fn build_admin_identity_user_view(
    store: &AnyStore,
    secret: &str,
    origin: &str,
    now: OffsetDateTime,
    user: IdentityUserRecord,
) -> Result<AdminIdentityUserView, AppError> {
    let onboarding = match user.user.auth_mode {
        AuthMode::Password if user.user.status == UserStatus::Invited => {
            let active_invitation = store
                .find_active_password_invitation_for_user(user.user.user_id, now)
                .await?;
            Some(AdminOnboardingActionView::PasswordInvite {
                invite_url: active_invitation
                    .as_ref()
                    .map(|invitation| invitation_url(origin, invitation, secret)),
                expires_at: active_invitation
                    .as_ref()
                    .map(|invitation| format_timestamp(invitation.expires_at)),
                can_resend: true,
            })
        }
        AuthMode::Oidc => user
            .oidc_provider_key
            .as_deref()
            .map(|provider_key| AdminOnboardingActionView::OidcSignIn {
                sign_in_url: oidc_sign_in_url(origin, provider_key, &user.user.email),
                provider_key: provider_key.to_string(),
                provider_label: provider_key.to_string(),
            }),
        _ => None,
    };

    Ok(AdminIdentityUserView {
        id: user.user.user_id.to_string(),
        name: user.user.name,
        email: user.user.email,
        auth_mode: user.user.auth_mode.as_str().to_string(),
        global_role: user.user.global_role.as_str().to_string(),
        team_id: user.team_id.map(|value| value.to_string()),
        team_name: user.team_name,
        team_role: user.membership_role.map(|value| value.as_str().to_string()),
        status: format_user_status(user.user.status),
        onboarding,
    })
}

pub(crate) fn build_assignable_user_views(
    users: &[IdentityUserRecord],
) -> Vec<AdminTeamAssignableUserView> {
    let mut views: Vec<_> = users
        .iter()
        .map(|user| AdminTeamAssignableUserView {
            id: user.user.user_id.to_string(),
            name: user.user.name.clone(),
            email: user.user.email.clone(),
            status: format_user_status(user.user.status),
            team_id: user.team_id.map(|value| value.to_string()),
            team_name: user.team_name.clone(),
            team_role: user.membership_role.map(|value| value.as_str().to_string()),
        })
        .collect();
    views.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.email.cmp(&right.email))
    });
    views
}

pub(crate) fn build_admin_team_views(
    teams: &[gateway_core::TeamRecord],
    users: &[IdentityUserRecord],
) -> Vec<AdminTeamManagementView> {
    teams
        .iter()
        .map(|team| {
            let mut admins: Vec<_> = users
                .iter()
                .filter(|user| user.team_id == Some(team.team_id))
                .filter(|user| user.membership_role == Some(MembershipRole::Admin))
                .map(|user| AdminTeamAdminView {
                    id: user.user.user_id.to_string(),
                    name: user.user.name.clone(),
                    email: user.user.email.clone(),
                    status: format_user_status(user.user.status),
                })
                .collect();
            let mut members: Vec<_> = users
                .iter()
                .filter(|user| user.team_id == Some(team.team_id))
                .map(|user| AdminTeamMemberView {
                    id: user.user.user_id.to_string(),
                    name: user.user.name.clone(),
                    email: user.user.email.clone(),
                    status: format_user_status(user.user.status),
                    role: user
                        .membership_role
                        .map(|value| value.as_str().to_string())
                        .unwrap_or_else(|| "member".to_string()),
                })
                .collect();
            admins.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.email.cmp(&right.email))
            });
            members.sort_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.email.cmp(&right.email))
            });

            let member_count = users
                .iter()
                .filter(|user| user.team_id == Some(team.team_id))
                .count();

            AdminTeamManagementView {
                id: team.team_id.to_string(),
                name: team.team_name.clone(),
                key: team.team_key.clone(),
                status: team.status.clone(),
                member_count,
                admins,
                members,
            }
        })
        .collect()
}

pub(crate) async fn reload_team_view(
    store: &AnyStore,
    team_id: Uuid,
) -> Result<AdminTeamManagementView, AppError> {
    let teams = store.list_teams().await?;
    let users = store.list_identity_users().await?;
    build_admin_team_views(&teams, &users)
        .into_iter()
        .find(|team| team.id == team_id.to_string())
        .ok_or_else(|| {
            AppError(gateway_core::GatewayError::Store(
                gateway_core::StoreError::NotFound("team missing".to_string()),
            ))
        })
}

pub(crate) async fn reload_identity_user(
    store: &AnyStore,
    user_id: Uuid,
) -> Result<IdentityUserRecord, AppError> {
    store.get_identity_user(user_id).await?.ok_or_else(|| {
        AppError(gateway_core::GatewayError::Store(
            gateway_core::StoreError::NotFound("user missing".to_string()),
        ))
    })
}

pub(crate) fn format_user_status(status: UserStatus) -> String {
    status.as_str().to_string()
}
