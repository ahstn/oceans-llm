use std::collections::BTreeMap;

use gateway_core::{
    AuthMode, IdentityUserRecord, MembershipRole, OidcProviderRecord, SeedTeam, SeedUser,
    StoreError, TeamRecord, UserStatus,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::GatewayStore;

pub(crate) fn model_uuid(model_key: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("model:{model_key}").as_bytes(),
    )
}

pub(crate) fn route_uuid(
    model_key: &str,
    provider_key: &str,
    upstream_model: &str,
    priority: i32,
    route_index: usize,
) -> Uuid {
    let key = format!("route:{model_key}:{provider_key}:{upstream_model}:{priority}:{route_index}");
    Uuid::new_v5(&Uuid::NAMESPACE_OID, key.as_bytes())
}

pub(crate) fn api_key_uuid(public_id: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("api_key:{public_id}").as_bytes(),
    )
}

pub(crate) async fn reconcile_seed_teams<S>(
    store: &S,
    teams: &[SeedTeam],
    now: OffsetDateTime,
) -> Result<BTreeMap<String, TeamRecord>, StoreError>
where
    S: GatewayStore + ?Sized,
{
    let mut records = BTreeMap::new();

    for team in teams {
        let mut record = match store.get_team_by_key(&team.team_key).await? {
            Some(existing) => existing,
            None => store.create_team(&team.team_key, &team.team_name).await?,
        };

        if record.team_name != team.team_name {
            store
                .update_team_name(record.team_id, &team.team_name, now)
                .await?;
            record.team_name = team.team_name.clone();
            record.updated_at = now;
        }

        match &team.budget {
            Some(budget) => {
                store
                    .upsert_active_budget_for_team(
                        record.team_id,
                        budget.cadence,
                        budget.amount_usd,
                        budget.hard_limit,
                        &budget.timezone,
                        now,
                    )
                    .await?;
            }
            None => {
                store
                    .deactivate_active_budget_for_team(record.team_id, now)
                    .await?;
            }
        }

        records.insert(team.team_key.clone(), record);
    }

    Ok(records)
}

pub(crate) async fn reconcile_seed_users<S>(
    store: &S,
    teams_by_key: &BTreeMap<String, TeamRecord>,
    users: &[SeedUser],
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    for user in users {
        reconcile_seed_user(store, teams_by_key, user, now).await?;
    }

    Ok(())
}

async fn reconcile_seed_user<S>(
    store: &S,
    teams_by_key: &BTreeMap<String, TeamRecord>,
    seed_user: &SeedUser,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let oidc_provider = resolve_seed_oidc_provider(store, seed_user).await?;

    let existing_user = match store
        .get_user_by_email_normalized(&seed_user.email_normalized)
        .await?
    {
        Some(existing) => existing,
        None => {
            store
                .create_identity_user(
                    &seed_user.name,
                    &seed_user.email,
                    &seed_user.email_normalized,
                    seed_user.global_role,
                    seed_user.auth_mode,
                    UserStatus::Invited,
                )
                .await?
        }
    };

    store
        .seed_update_identity_user_profile(
            existing_user.user_id,
            &seed_user.name,
            &seed_user.email,
            &seed_user.email_normalized,
            seed_user.request_logging_enabled,
            now,
        )
        .await?;

    if existing_user.auth_mode != seed_user.auth_mode && existing_user.status != UserStatus::Invited
    {
        return Err(StoreError::Conflict(
            "auth mode can only change while the user is invited".to_string(),
        ));
    }

    if existing_user.global_role != seed_user.global_role
        || existing_user.auth_mode != seed_user.auth_mode
    {
        store
            .update_identity_user(
                existing_user.user_id,
                seed_user.global_role,
                seed_user.auth_mode,
                now,
            )
            .await?;
    }

    let mut identity_user = load_identity_user(store, existing_user.user_id).await?;
    sync_seed_user_auth_mode(
        store,
        &identity_user,
        seed_user.auth_mode,
        oidc_provider.as_ref(),
        now,
    )
    .await?;

    identity_user = load_identity_user(store, existing_user.user_id).await?;
    sync_seed_user_membership(store, &identity_user, teams_by_key, seed_user, now).await?;

    match &seed_user.budget {
        Some(budget) => {
            store
                .upsert_active_budget_for_user(
                    existing_user.user_id,
                    budget.cadence,
                    budget.amount_usd,
                    budget.hard_limit,
                    &budget.timezone,
                    now,
                )
                .await?;
        }
        None => {
            store
                .deactivate_active_budget_for_user(existing_user.user_id, now)
                .await?;
        }
    }

    Ok(())
}

async fn load_identity_user<S>(store: &S, user_id: Uuid) -> Result<IdentityUserRecord, StoreError>
where
    S: GatewayStore + ?Sized,
{
    store
        .get_identity_user(user_id)
        .await?
        .ok_or_else(|| StoreError::NotFound(format!("identity user `{user_id}` not found")))
}

async fn resolve_seed_oidc_provider<S>(
    store: &S,
    seed_user: &SeedUser,
) -> Result<Option<OidcProviderRecord>, StoreError>
where
    S: GatewayStore + ?Sized,
{
    match seed_user.auth_mode {
        AuthMode::Oidc => {
            let provider_key = seed_user.oidc_provider_key.as_deref().ok_or_else(|| {
                StoreError::Conflict(
                    "oidc_provider_key is required for oidc users".to_string(),
                )
            })?;
            Ok(Some(
                store
                    .get_enabled_oidc_provider_by_key(provider_key)
                    .await?
                    .ok_or_else(|| {
                        StoreError::NotFound(format!(
                            "oidc provider `{provider_key}` is not enabled"
                        ))
                    })?,
            ))
        }
        AuthMode::Password => Ok(None),
        AuthMode::Oauth => Err(StoreError::Conflict(
            "users config does not support auth_mode `oauth`".to_string(),
        )),
    }
}

async fn sync_seed_user_auth_mode<S>(
    store: &S,
    user: &IdentityUserRecord,
    next_auth_mode: AuthMode,
    oidc_provider: Option<&OidcProviderRecord>,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    if user.user.auth_mode == AuthMode::Password && next_auth_mode != AuthMode::Password {
        store.delete_user_password_auth(user.user.user_id).await?;
        store
            .revoke_password_invitations_for_user(user.user.user_id, now)
            .await?;
    }

    if let Some(current_provider_id) = user.oidc_provider_id.as_deref() {
        let next_provider_id = oidc_provider.map(|provider| provider.oidc_provider_id.as_str());
        if next_auth_mode != AuthMode::Oidc || next_provider_id != Some(current_provider_id) {
            store
                .delete_user_oidc_auth(user.user.user_id, current_provider_id)
                .await?;
        }
    }

    match next_auth_mode {
        AuthMode::Password => {
            store.clear_user_oidc_link(user.user.user_id).await?;
        }
        AuthMode::Oidc => {
            let provider = oidc_provider.ok_or_else(|| {
                StoreError::Conflict("oidc provider configuration is required".to_string())
            })?;
            store
                .set_user_oidc_link(user.user.user_id, &provider.oidc_provider_id, now)
                .await?;
        }
        AuthMode::Oauth => {
            return Err(StoreError::Conflict(
                "users config does not support auth_mode `oauth`".to_string(),
            ));
        }
    }

    Ok(())
}

async fn sync_seed_user_membership<S>(
    store: &S,
    user: &IdentityUserRecord,
    teams_by_key: &BTreeMap<String, TeamRecord>,
    seed_user: &SeedUser,
    now: OffsetDateTime,
) -> Result<(), StoreError>
where
    S: GatewayStore + ?Sized,
{
    let requested_membership = requested_seed_membership(teams_by_key, seed_user)?;
    if current_membership(user) == requested_membership {
        return Ok(());
    }
    if user.membership_role == Some(MembershipRole::Owner) {
        return Err(StoreError::Conflict(
            "owner memberships cannot be created, removed, or transferred in this workflow"
                .to_string(),
        ));
    }

    match (user.team_id, requested_membership) {
        (None, None) => Ok(()),
        (None, Some((team_id, role))) => {
            store
                .assign_team_membership(user.user.user_id, team_id, role)
                .await
        }
        (Some(team_id), None) => {
            store.remove_team_membership(team_id, user.user.user_id).await?;
            Ok(())
        }
        (Some(current_team_id), Some((next_team_id, next_role)))
            if current_team_id == next_team_id =>
        {
            if user.membership_role != Some(next_role) {
                store
                    .update_team_membership_role(current_team_id, user.user.user_id, next_role, now)
                    .await?;
            }
            Ok(())
        }
        (Some(current_team_id), Some((next_team_id, next_role))) => {
            store
                .transfer_team_membership(
                    user.user.user_id,
                    current_team_id,
                    next_team_id,
                    next_role,
                    now,
                )
                .await
        }
    }
}

fn requested_seed_membership(
    teams_by_key: &BTreeMap<String, TeamRecord>,
    seed_user: &SeedUser,
) -> Result<Option<(Uuid, MembershipRole)>, StoreError> {
    let Some(membership) = seed_user.membership.as_ref() else {
        return Ok(None);
    };
    if membership.role == MembershipRole::Owner {
        return Err(StoreError::Conflict(
            "owner memberships cannot be created, removed, or transferred in this workflow"
                .to_string(),
        ));
    }
    let team = teams_by_key.get(&membership.team_key).ok_or_else(|| {
        StoreError::NotFound(format!(
            "seed user `{}` references unknown team `{}`",
            seed_user.email, membership.team_key
        ))
    })?;
    Ok(Some((team.team_id, membership.role)))
}

fn current_membership(user: &IdentityUserRecord) -> Option<(Uuid, MembershipRole)> {
    match (user.team_id, user.membership_role) {
        (Some(team_id), Some(role)) => Some((team_id, role)),
        _ => None,
    }
}
