//! Audit events for identity mutations: users, teams, service accounts and sessions.
//!
//! Summaries are built field by field so that invite URLs, sign-in links and
//! credentials in the response views never reach an audit entry.

use gateway_core::{IdentityUserRecord, MembershipRole, ServiceAccountRecord, UserRecord};
use gateway_service::{AuditAction, AuditEvent, AuditProgress};
use serde_json::{Value, json};

use crate::http::{admin_contract::AdminTeamManagementView, error::AppError};

pub(crate) fn service_account_event(
    action: AuditAction,
    account: &ServiceAccountRecord,
) -> AuditEvent {
    AuditEvent::new(action)
        .target(account.service_account_id)
        .label(account.service_account_name.clone())
        .meta("team_id", account.team_id)
}

pub(crate) fn team_event(action: AuditAction, team: &AdminTeamManagementView) -> AuditEvent {
    AuditEvent::new(action)
        .target(&team.id)
        .label(team.name.clone())
}

pub(crate) fn team_summary(team: &AdminTeamManagementView) -> Value {
    json!({
        "name": team.name,
        "status": team.status,
        "tags": team.tags,
        "admin_user_ids": team.admins.iter().map(|admin| &admin.id).collect::<Vec<_>>(),
    })
}

pub(crate) fn user_event(action: AuditAction, user: &UserRecord) -> AuditEvent {
    AuditEvent::new(action)
        .target(user.user_id)
        .label(user.email.clone())
}

/// The audited fields of a user. Credentials and onboarding links are left out.
pub(crate) fn user_summary(identity: &IdentityUserRecord) -> Value {
    let user = &identity.user;
    json!({
        "name": user.name,
        "email": user.email,
        "auth_mode": user.auth_mode.as_str(),
        "global_role": user.global_role.as_str(),
        "status": user.status.as_str(),
        "request_logging_enabled": user.request_logging_enabled,
        "team_id": identity.team_id,
        "team_role": identity.membership_role.map(MembershipRole::as_str),
        "oidc_provider_key": identity.oidc_provider_key,
        "oauth_provider_key": identity.oauth_provider_key,
        "tags": user.tags,
    })
}

/// The entry for a multi-step change that failed after some of its writes landed.
/// The boundary adds `outcome: partial`; this adds the failure's error code.
pub(crate) fn partial_event(
    action: AuditAction,
    progress: &AuditProgress,
    error: &AppError,
) -> AuditEvent {
    let event = AuditEvent::new(action).meta("error", error.0.error_code());
    match progress.target_id() {
        Some(target_id) => event.target(target_id),
        None => event,
    }
}
