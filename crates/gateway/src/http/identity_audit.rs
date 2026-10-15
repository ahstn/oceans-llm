//! Audit events for identity mutations: users, teams, service accounts and sessions.
//!
//! Summaries are built field by field so that invite URLs, sign-in links and
//! credentials in the response views never reach an audit entry.

use gateway_core::{IdentityUserRecord, MembershipRole, ServiceAccountRecord, UserRecord};
use gateway_service::{AuditEvent, admin_audit::targets};
use serde_json::{Value, json};

use crate::http::admin_contract::AdminTeamManagementView;

pub(crate) fn service_account_event(action: &str, account: &ServiceAccountRecord) -> AuditEvent {
    AuditEvent::new(action, targets::SERVICE_ACCOUNT)
        .target(account.service_account_id)
        .label(account.service_account_name.clone())
        .meta("team_id", account.team_id)
}

pub(crate) fn team_event(action: &str, team: &AdminTeamManagementView) -> AuditEvent {
    AuditEvent::new(action, targets::TEAM)
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

pub(crate) fn user_event(action: &str, user: &UserRecord) -> AuditEvent {
    AuditEvent::new(action, targets::USER)
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
