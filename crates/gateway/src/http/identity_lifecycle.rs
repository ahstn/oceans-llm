use gateway_core::{AuthMode, GatewayError, GlobalRole, MembershipRole, UserRecord, UserStatus};

pub(crate) fn ensure_manageable_user(user: &UserRecord) -> Result<(), GatewayError> {
    if user.user_id.to_string() == gateway_core::SYSTEM_BOOTSTRAP_ADMIN_USER_ID {
        return Err(GatewayError::InvalidRequest(
            "bootstrap admin is managed outside normal user lifecycle".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_assignable_membership_role(
    role: MembershipRole,
) -> Result<MembershipRole, GatewayError> {
    match role {
        MembershipRole::Owner => Err(GatewayError::InvalidRequest(
            "owner memberships cannot be created, removed, or transferred in this workflow"
                .to_string(),
        )),
        MembershipRole::Admin | MembershipRole::Member => Ok(role),
    }
}

pub(crate) fn ensure_mutable_membership(
    role: Option<MembershipRole>,
) -> Result<(), GatewayError> {
    if role == Some(MembershipRole::Owner) {
        return Err(GatewayError::InvalidRequest(
            "owner memberships cannot be created, removed, or transferred in this workflow"
                .to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_auth_mode_edit_allowed(
    user: &UserRecord,
    next_auth_mode: AuthMode,
) -> Result<(), GatewayError> {
    if user.auth_mode != next_auth_mode && user.status != UserStatus::Invited {
        return Err(GatewayError::InvalidRequest(
            "auth mode can only change while the user is invited".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_not_self_demoting(
    actor: &UserRecord,
    target: &UserRecord,
    next_role: GlobalRole,
) -> Result<(), GatewayError> {
    if actor.user_id == target.user_id
        && target.global_role == GlobalRole::PlatformAdmin
        && next_role != GlobalRole::PlatformAdmin
    {
        return Err(GatewayError::InvalidRequest(
            "you cannot demote your own platform admin account".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_not_self_deactivating(
    actor: &UserRecord,
    target: &UserRecord,
) -> Result<(), GatewayError> {
    if actor.user_id == target.user_id {
        return Err(GatewayError::InvalidRequest(
            "you cannot deactivate your own account".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_reset_onboarding_allowed(user: &UserRecord) -> Result<(), GatewayError> {
    match user.status {
        UserStatus::Invited | UserStatus::Disabled => Ok(()),
        UserStatus::Active => Err(GatewayError::InvalidRequest(
            "onboarding can only be reset for invited or disabled users".to_string(),
        )),
    }
}

pub(crate) fn ensure_deactivation_allowed(user: &UserRecord) -> Result<(), GatewayError> {
    if user.status == UserStatus::Disabled {
        return Err(GatewayError::InvalidRequest(
            "user is already disabled".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_reactivation_allowed(user: &UserRecord) -> Result<(), GatewayError> {
    if user.status != UserStatus::Disabled {
        return Err(GatewayError::InvalidRequest(
            "only disabled users can be reactivated".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn reactivation_status(auth_mode: AuthMode, has_auth_proof: bool) -> UserStatus {
    match auth_mode {
        AuthMode::Password | AuthMode::Oidc if has_auth_proof => UserStatus::Active,
        AuthMode::Password | AuthMode::Oidc => UserStatus::Invited,
        AuthMode::Oauth => UserStatus::Invited,
    }
}
