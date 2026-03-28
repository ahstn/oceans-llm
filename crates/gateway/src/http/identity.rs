use std::collections::{BTreeSet, HashMap};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, header::SET_COOKIE},
    response::{IntoResponse, Redirect, Response},
};
use gateway_core::{
    AuthError, AuthMode, GatewayError, GlobalRole, IdentityRepository, IdentityUserRecord,
    MembershipRole, OidcProviderRecord, PasswordInvitationRecord, UserRecord, UserStatus,
};
use gateway_store::{AnyStore, GatewayStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use url::form_urlencoded;
use uuid::Uuid;

use crate::http::{
    admin_auth::{require_authenticated_session, require_platform_admin},
    error::AppError,
    identity_lifecycle::{
        ensure_assignable_membership_role, ensure_auth_mode_edit_allowed,
        ensure_deactivation_allowed, ensure_manageable_user, ensure_mutable_membership,
        ensure_not_self_deactivating, ensure_not_self_demoting, ensure_reactivation_allowed,
        ensure_reset_onboarding_allowed, reactivation_status,
    },
    identity_views::{
        build_admin_identity_user_view, build_admin_team_views, build_assignable_user_views,
        reload_identity_user, reload_team_view,
    },
    state::AppState,
};

const SESSION_COOKIE_NAME: &str = "ogw_session";
const INVITE_TTL_DAYS: i64 = 7;
const SESSION_TTL_DAYS: i64 = 30;

#[derive(Debug, Serialize)]
pub(crate) struct Envelope<T> {
    data: T,
    meta: ResponseMeta,
}

#[derive(Debug, Serialize)]
pub(crate) struct ResponseMeta {
    generated_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminIdentityPayload {
    users: Vec<AdminIdentityUserView>,
    teams: Vec<AdminTeamView>,
    oidc_providers: Vec<AdminOidcProviderView>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminIdentityUserView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) email: String,
    pub(crate) auth_mode: String,
    pub(crate) global_role: String,
    pub(crate) team_id: Option<String>,
    pub(crate) team_name: Option<String>,
    pub(crate) team_role: Option<String>,
    pub(crate) status: String,
    pub(crate) onboarding: Option<AdminOnboardingActionView>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminTeamView {
    id: String,
    name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminTeamsPayload {
    teams: Vec<AdminTeamManagementView>,
    users: Vec<AdminTeamAssignableUserView>,
    oidc_providers: Vec<AdminOidcProviderView>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminTeamManagementView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) key: String,
    pub(crate) status: String,
    pub(crate) member_count: usize,
    pub(crate) admins: Vec<AdminTeamAdminView>,
    pub(crate) members: Vec<AdminTeamMemberView>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminTeamAdminView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) email: String,
    pub(crate) status: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminTeamMemberView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) email: String,
    pub(crate) status: String,
    pub(crate) role: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminTeamAssignableUserView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) email: String,
    pub(crate) status: String,
    pub(crate) team_id: Option<String>,
    pub(crate) team_name: Option<String>,
    pub(crate) team_role: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminOidcProviderView {
    id: String,
    key: String,
    label: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuthSessionUserView {
    id: String,
    name: String,
    email: String,
    global_role: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuthSessionView {
    user: AuthSessionUserView,
    must_change_password: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum AdminOnboardingActionView {
    PasswordInvite {
        invite_url: Option<String>,
        expires_at: Option<String>,
        can_resend: bool,
    },
    OidcSignIn {
        sign_in_url: String,
        provider_key: String,
        provider_label: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub name: String,
    pub email: String,
    pub auth_mode: String,
    pub global_role: String,
    pub team_id: Option<String>,
    pub team_role: Option<String>,
    pub oidc_provider_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
    pub admin_user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTeamRequest {
    pub name: String,
    pub admin_user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddTeamMembersRequest {
    pub user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub global_role: String,
    pub auth_mode: Option<String>,
    pub team_id: Option<String>,
    pub team_role: Option<String>,
    pub oidc_provider_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransferTeamMemberRequest {
    pub destination_team_id: String,
    pub destination_role: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreateUserResponse {
    PasswordInvite {
        user: AdminIdentityUserView,
        invite_url: String,
        expires_at: String,
    },
    OidcSignIn {
        user: AdminIdentityUserView,
        sign_in_url: String,
        provider_label: String,
    },
}

#[derive(Debug, Serialize)]
pub(crate) struct IdentityActionStatus {
    status: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct PasswordInviteResponse {
    user_id: String,
    invite_url: String,
    expires_at: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct InvitationView {
    state: String,
    email: Option<String>,
    name: Option<String>,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteInvitationRequest {
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct PasswordLoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CompleteInvitationResponse {
    status: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct OidcStartQuery {
    pub provider_key: String,
    pub login_hint: String,
    pub redirect_to: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OidcCallbackQuery {
    pub provider_key: String,
    pub email: String,
    pub subject: Option<String>,
    pub redirect_to: Option<String>,
}

pub async fn list_identity_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<AdminIdentityPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let origin = request_origin(&headers);
    let users = state.store.list_identity_users().await?;
    let teams = state.store.list_active_teams().await?;
    let providers = state.store.list_enabled_oidc_providers().await?;

    let now = OffsetDateTime::now_utc();
    let mut user_views = Vec::with_capacity(users.len());
    for user in users {
        user_views.push(
            build_admin_identity_user_view(
                &state.store,
                &state.identity_token_secret,
                &origin,
                now,
                user,
            )
            .await?,
        );
    }

    Ok(Json(envelope(AdminIdentityPayload {
        users: user_views,
        teams: teams
            .into_iter()
            .map(|team| AdminTeamView {
                id: team.team_id.to_string(),
                name: team.team_name,
            })
            .collect(),
        oidc_providers: providers
            .into_iter()
            .map(|provider| AdminOidcProviderView {
                id: provider.oidc_provider_id,
                key: provider.provider_key.clone(),
                label: provider.provider_key,
            })
            .collect(),
    })))
}

pub async fn list_identity_teams(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<AdminTeamsPayload>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let teams = state.store.list_teams().await?;
    let users = state.store.list_identity_users().await?;
    let providers = state.store.list_enabled_oidc_providers().await?;

    Ok(Json(envelope(AdminTeamsPayload {
        teams: build_admin_team_views(&teams, &users),
        users: build_assignable_user_views(&users),
        oidc_providers: providers
            .into_iter()
            .map(|provider| AdminOidcProviderView {
                id: provider.oidc_provider_id,
                key: provider.provider_key.clone(),
                label: provider.provider_key,
            })
            .collect(),
    })))
}

pub async fn create_identity_team(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateTeamRequest>,
) -> Result<Json<Envelope<AdminTeamManagementView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let name = request.name.trim();
    if name.is_empty() {
        return Err(AppError(GatewayError::InvalidRequest(
            "name cannot be empty".to_string(),
        )));
    }

    let users = state.store.list_identity_users().await?;
    let selected_admin_ids = parse_uuid_list(&request.admin_user_ids)?;
    validate_team_admin_assignments(&users, None, &selected_admin_ids)?;

    let team_key = generate_unique_team_key(&state.store, name).await?;
    let team = state.store.create_team(&team_key, name).await?;
    for user_id in selected_admin_ids {
        state
            .store
            .assign_team_membership(user_id, team.team_id, MembershipRole::Admin)
            .await?;
    }

    Ok(Json(envelope(
        reload_team_view(&state.store, team.team_id).await?,
    )))
}

pub async fn update_identity_team(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(request): Json<UpdateTeamRequest>,
) -> Result<Json<Envelope<AdminTeamManagementView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let team_id = parse_uuid(&team_id)?;
    state
        .store
        .get_team_by_id(team_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("team not found".to_string())))?;

    let name = request.name.trim();
    if name.is_empty() {
        return Err(AppError(GatewayError::InvalidRequest(
            "name cannot be empty".to_string(),
        )));
    }

    let users = state.store.list_identity_users().await?;
    let selected_admin_ids = parse_uuid_list(&request.admin_user_ids)?;
    validate_team_admin_assignments(&users, Some(team_id), &selected_admin_ids)?;

    let now = OffsetDateTime::now_utc();
    state.store.update_team_name(team_id, name, now).await?;
    sync_team_admins(&state.store, team_id, &selected_admin_ids, now).await?;

    Ok(Json(envelope(
        reload_team_view(&state.store, team_id).await?,
    )))
}

pub async fn add_identity_team_members(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(request): Json<AddTeamMembersRequest>,
) -> Result<Json<Envelope<AdminTeamManagementView>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let team_id = parse_uuid(&team_id)?;
    state
        .store
        .get_team_by_id(team_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("team not found".to_string())))?;

    let requested_user_ids = parse_uuid_list(&request.user_ids)?;
    let users = state.store.list_identity_users().await?;
    let users_by_id: HashMap<Uuid, IdentityUserRecord> = users
        .into_iter()
        .map(|user| (user.user.user_id, user))
        .collect();

    let mut conflicts = Vec::new();
    let mut assignable_user_ids = Vec::new();
    for user_id in requested_user_ids {
        let Some(user) = users_by_id.get(&user_id) else {
            return Err(AppError(GatewayError::InvalidRequest(format!(
                "user `{user_id}` not found"
            ))));
        };

        match user.team_id {
            Some(existing_team_id) if existing_team_id != team_id => {
                conflicts.push(format!(
                    "{} ({})",
                    user.user.email,
                    user.team_name.as_deref().unwrap_or("another team")
                ));
            }
            Some(_) => {}
            None => assignable_user_ids.push(user_id),
        }
    }

    if !conflicts.is_empty() {
        return Err(AppError(GatewayError::InvalidRequest(format!(
            "users already belong to another team: {}",
            conflicts.join(", ")
        ))));
    }

    for user_id in assignable_user_ids {
        state
            .store
            .assign_team_membership(user_id, team_id, MembershipRole::Member)
            .await?;
    }

    Ok(Json(envelope(
        reload_team_view(&state.store, team_id).await?,
    )))
}

pub async fn get_auth_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<Option<AuthSessionView>>>, AppError> {
    let session = resolve_session_user(&state, &headers)
        .await?
        .map(build_auth_session_view);

    Ok(Json(envelope(session)))
}

pub async fn login_with_password(
    State(state): State<AppState>,
    Json(request): Json<PasswordLoginRequest>,
) -> Result<Response, AppError> {
    let email_normalized = normalize_email(&request.email)?;
    let user = state
        .store
        .get_user_by_email_normalized(&email_normalized)
        .await?
        .ok_or(AppError(GatewayError::Auth(AuthError::InvalidCredentials)))?;
    let password_auth = state
        .store
        .get_user_password_auth(user.user_id)
        .await?
        .ok_or(AppError(GatewayError::Auth(AuthError::InvalidCredentials)))?;

    if user.auth_mode != AuthMode::Password {
        return Err(AppError(GatewayError::Auth(AuthError::InvalidCredentials)));
    }
    if user.global_role != GlobalRole::PlatformAdmin {
        return Err(AppError(GatewayError::Auth(
            AuthError::InsufficientPrivileges,
        )));
    }
    if user.status != UserStatus::Active {
        return Err(AppError(GatewayError::InvalidRequest(
            "only active admins can sign in".to_string(),
        )));
    }
    let password_ok =
        gateway_service::verify_gateway_key_secret(&request.password, &password_auth.password_hash)
            .map_err(|error| AppError(GatewayError::Internal(error.to_string())))?;
    if !password_ok {
        return Err(AppError(GatewayError::Auth(AuthError::InvalidCredentials)));
    }

    let now = OffsetDateTime::now_utc();
    let session_cookie = issue_session_cookie(&state, user.user_id, now).await?;
    let mut response = Json(envelope(build_auth_session_view(user))).into_response();
    response.headers_mut().append(SET_COOKIE, session_cookie);
    Ok(response)
}

pub async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChangePasswordRequest>,
) -> Result<Json<Envelope<AuthSessionView>>, AppError> {
    if request.new_password.len() < 8 {
        return Err(AppError(GatewayError::InvalidRequest(
            "password must be at least 8 characters".to_string(),
        )));
    }

    let user = require_authenticated_session(&state, &headers).await?;
    if user.global_role != GlobalRole::PlatformAdmin {
        return Err(AppError(GatewayError::Auth(
            AuthError::InsufficientPrivileges,
        )));
    }
    if user.auth_mode != AuthMode::Password {
        return Err(AppError(GatewayError::InvalidRequest(
            "password changes are only valid for password users".to_string(),
        )));
    }
    if user.status != UserStatus::Active {
        return Err(AppError(GatewayError::InvalidRequest(
            "only active users can change passwords".to_string(),
        )));
    }
    let password_auth = state
        .store
        .get_user_password_auth(user.user_id)
        .await?
        .ok_or(AppError(GatewayError::Auth(AuthError::InvalidCredentials)))?;
    let current_password_ok = gateway_service::verify_gateway_key_secret(
        &request.current_password,
        &password_auth.password_hash,
    )
    .map_err(|error| AppError(GatewayError::Internal(error.to_string())))?;
    if !current_password_ok {
        return Err(AppError(GatewayError::Auth(AuthError::InvalidCredentials)));
    }

    let now = OffsetDateTime::now_utc();
    let new_password_hash = gateway_service::hash_gateway_key_secret(&request.new_password)
        .map_err(|error| AppError(GatewayError::Internal(error.to_string())))?;
    state
        .store
        .store_user_password(user.user_id, &new_password_hash, now)
        .await?;
    state
        .store
        .update_user_must_change_password(user.user_id, false, now)
        .await?;

    let refreshed_user = state
        .store
        .get_user_by_id(user.user_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("user not found".to_string())))?;

    Ok(Json(envelope(build_auth_session_view(refreshed_user))))
}

pub async fn create_identity_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateUserRequest>,
) -> Result<Json<Envelope<CreateUserResponse>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let origin = request_origin(&headers);
    let name = request.name.trim();
    let email = request.email.trim();
    let email_normalized = normalize_email(email)?;
    let auth_mode = parse_auth_mode(&request.auth_mode)?;
    let global_role = parse_global_role(&request.global_role)?;

    if name.is_empty() {
        return Err(AppError(GatewayError::InvalidRequest(
            "name cannot be empty".to_string(),
        )));
    }

    let membership =
        parse_requested_membership(request.team_id.as_deref(), request.team_role.as_deref())?;
    let oidc_provider = resolve_requested_oidc_provider(
        &state.store,
        auth_mode,
        request.oidc_provider_key.as_deref(),
    )
    .await?;

    if let Some((team_id, _)) = membership {
        state
            .store
            .get_team_by_id(team_id)
            .await?
            .ok_or_else(|| AppError(GatewayError::InvalidRequest("team not found".to_string())))?;
    }

    let user = state
        .store
        .create_identity_user(
            name,
            email,
            &email_normalized,
            global_role,
            auth_mode,
            UserStatus::Invited,
        )
        .await?;
    let created_at = OffsetDateTime::now_utc();

    if let Some((team_id, role)) = membership {
        state
            .store
            .assign_team_membership(user.user_id, team_id, role)
            .await?;
    }

    if let Some(provider) = oidc_provider.as_ref() {
        state
            .store
            .set_user_oidc_link(user.user_id, &provider.oidc_provider_id, created_at)
            .await?;
    }

    let response = build_onboarding_response(
        &state,
        &origin,
        created_at,
        reload_identity_user(&state.store, user.user_id).await?,
    )
    .await?;

    Ok(Json(envelope(response)))
}

pub async fn update_identity_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(request): Json<UpdateUserRequest>,
) -> Result<Json<Envelope<IdentityActionStatus>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let user_id = parse_uuid(&user_id)?;
    let identity_user = load_identity_user_for_mutation(&state, user_id).await?;
    let next_global_role = parse_global_role(&request.global_role)?;
    let next_auth_mode = request
        .auth_mode
        .as_deref()
        .map(parse_auth_mode)
        .transpose()?
        .unwrap_or(identity_user.user.auth_mode);
    let requested_membership =
        parse_requested_membership(request.team_id.as_deref(), request.team_role.as_deref())?;
    let oidc_provider = resolve_requested_oidc_provider(
        &state.store,
        next_auth_mode,
        match next_auth_mode {
            AuthMode::Oidc => request
                .oidc_provider_key
                .as_deref()
                .or(identity_user.oidc_provider_key.as_deref()),
            AuthMode::Password | AuthMode::Oauth => request.oidc_provider_key.as_deref(),
        },
    )
    .await?;

    ensure_not_self_demoting(&actor, &identity_user.user, next_global_role).map_err(AppError)?;
    ensure_auth_mode_edit_allowed(&identity_user.user, next_auth_mode).map_err(AppError)?;
    if membership_update_requested(&identity_user, requested_membership) {
        ensure_mutable_membership(identity_user.membership_role).map_err(AppError)?;
    }
    if let Some((team_id, _)) = requested_membership {
        state
            .store
            .get_team_by_id(team_id)
            .await?
            .ok_or_else(|| AppError(GatewayError::InvalidRequest("team not found".to_string())))?;
    }

    let now = OffsetDateTime::now_utc();
    state
        .store
        .update_identity_user(user_id, next_global_role, next_auth_mode, now)
        .await?;
    sync_identity_user_auth_mode(
        &state.store,
        &identity_user,
        next_auth_mode,
        oidc_provider.as_ref(),
        now,
    )
    .await?;
    sync_identity_user_membership(&state.store, &identity_user, requested_membership, now).await?;

    Ok(Json(envelope(IdentityActionStatus { status: "ok" })))
}

pub async fn deactivate_identity_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<Envelope<IdentityActionStatus>>, AppError> {
    let actor = require_platform_admin(&state, &headers).await?;
    let user_id = parse_uuid(&user_id)?;
    let user = state
        .store
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("user not found".to_string())))?;
    ensure_manageable_user(&user).map_err(AppError)?;
    ensure_not_self_deactivating(&actor, &user).map_err(AppError)?;
    ensure_deactivation_allowed(&user).map_err(AppError)?;
    let now = OffsetDateTime::now_utc();
    state.store.deactivate_identity_user(user_id, now).await?;
    state.store.revoke_user_sessions(user_id, now).await?;
    state
        .store
        .revoke_password_invitations_for_user(user_id, now)
        .await?;

    Ok(Json(envelope(IdentityActionStatus { status: "ok" })))
}

pub async fn reactivate_identity_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<Envelope<IdentityActionStatus>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let user_id = parse_uuid(&user_id)?;
    let identity_user = load_identity_user_for_mutation(&state, user_id).await?;
    ensure_reactivation_allowed(&identity_user.user).map_err(AppError)?;

    let now = OffsetDateTime::now_utc();
    let next_status = reactivation_status(
        identity_user.user.auth_mode,
        user_has_auth_proof(&state.store, &identity_user).await?,
    );
    state
        .store
        .update_user_status(user_id, next_status, now)
        .await?;

    Ok(Json(envelope(IdentityActionStatus { status: "ok" })))
}

pub async fn reset_identity_user_onboarding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<Envelope<CreateUserResponse>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let user_id = parse_uuid(&user_id)?;
    let identity_user = load_identity_user_for_mutation(&state, user_id).await?;
    ensure_reset_onboarding_allowed(&identity_user.user).map_err(AppError)?;
    if identity_user.user.auth_mode == AuthMode::Oidc {
        let provider_key = identity_user.oidc_provider_key.as_deref().ok_or_else(|| {
            AppError(GatewayError::InvalidRequest(
                "oidc users must be linked to a provider before resetting onboarding".to_string(),
            ))
        })?;
        load_enabled_oidc_provider(&state.store, provider_key).await?;
    }

    let now = OffsetDateTime::now_utc();
    match identity_user.user.auth_mode {
        AuthMode::Password => {
            state.store.delete_user_password_auth(user_id).await?;
            state
                .store
                .revoke_password_invitations_for_user(user_id, now)
                .await?;
        }
        AuthMode::Oidc => {
            let provider_id = identity_user.oidc_provider_id.as_deref().ok_or_else(|| {
                AppError(GatewayError::InvalidRequest(
                    "oidc users must be linked to a provider before resetting onboarding"
                        .to_string(),
                ))
            })?;
            state
                .store
                .delete_user_oidc_auth(user_id, provider_id)
                .await?;
        }
        AuthMode::Oauth => {
            return Err(AppError(GatewayError::InvalidRequest(
                "unsupported auth mode".to_string(),
            )));
        }
    }

    state
        .store
        .update_user_status(user_id, UserStatus::Invited, now)
        .await?;
    let origin = request_origin(&headers);
    let identity_user = reload_identity_user(&state.store, user_id).await?;
    let response = build_onboarding_response(&state, &origin, now, identity_user).await?;

    Ok(Json(envelope(response)))
}

pub async fn remove_identity_team_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((team_id, user_id)): Path<(String, String)>,
) -> Result<Json<Envelope<IdentityActionStatus>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let team_id = parse_uuid(&team_id)?;
    let user_id = parse_uuid(&user_id)?;
    let identity_user = load_identity_user_for_mutation(&state, user_id).await?;
    ensure_mutable_membership(identity_user.membership_role).map_err(AppError)?;

    if identity_user.team_id != Some(team_id) {
        return Err(AppError(GatewayError::InvalidRequest(
            "user is not a member of the requested team".to_string(),
        )));
    }

    state.store.remove_team_membership(team_id, user_id).await?;
    Ok(Json(envelope(IdentityActionStatus { status: "ok" })))
}

pub async fn transfer_identity_team_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((team_id, user_id)): Path<(String, String)>,
    Json(request): Json<TransferTeamMemberRequest>,
) -> Result<Json<Envelope<IdentityActionStatus>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let team_id = parse_uuid(&team_id)?;
    let user_id = parse_uuid(&user_id)?;
    let destination_team_id = parse_uuid(&request.destination_team_id)?;
    let destination_role =
        ensure_assignable_membership_role(parse_membership_role(&request.destination_role)?)
            .map_err(AppError)?;
    if destination_team_id == team_id {
        return Err(AppError(GatewayError::InvalidRequest(
            "destination team must differ from the source team".to_string(),
        )));
    }

    state
        .store
        .get_team_by_id(destination_team_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("team not found".to_string())))?;

    let identity_user = load_identity_user_for_mutation(&state, user_id).await?;
    ensure_mutable_membership(identity_user.membership_role).map_err(AppError)?;
    if identity_user.team_id != Some(team_id) {
        return Err(AppError(GatewayError::InvalidRequest(
            "user is not a member of the requested source team".to_string(),
        )));
    }

    state
        .store
        .transfer_team_membership(
            user_id,
            team_id,
            destination_team_id,
            destination_role,
            OffsetDateTime::now_utc(),
        )
        .await?;

    Ok(Json(envelope(IdentityActionStatus { status: "ok" })))
}

pub async fn regenerate_password_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<Envelope<PasswordInviteResponse>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    let user_id = parse_uuid(&user_id)?;
    let user = state
        .store
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("user not found".to_string())))?;

    if user.auth_mode != AuthMode::Password {
        return Err(AppError(GatewayError::InvalidRequest(
            "password invites are only valid for password users".to_string(),
        )));
    }
    if user.status != UserStatus::Invited {
        return Err(AppError(GatewayError::InvalidRequest(
            "only invited users can receive a password invite".to_string(),
        )));
    }

    let origin = request_origin(&headers);
    let invitation = create_password_invite(
        &state.store,
        &state.identity_token_secret,
        &origin,
        user.user_id,
    )
    .await?;

    Ok(Json(envelope(PasswordInviteResponse {
        user_id: user.user_id.to_string(),
        invite_url: invitation.url,
        expires_at: invitation.expires_at,
    })))
}

pub async fn validate_password_invitation(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<Envelope<InvitationView>>, AppError> {
    let state_view = load_invitation_view(&state, &token).await?;
    Ok(Json(envelope(state_view)))
}

pub async fn complete_password_invitation(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Json(request): Json<CompleteInvitationRequest>,
) -> Result<Json<Envelope<CompleteInvitationResponse>>, AppError> {
    if request.password.len() < 8 {
        return Err(AppError(GatewayError::InvalidRequest(
            "password must be at least 8 characters".to_string(),
        )));
    }

    let invitation = load_valid_invitation(&state, &token).await?;
    let password_hash = gateway_service::hash_gateway_key_secret(&request.password)
        .map_err(|error| AppError(GatewayError::Internal(error.to_string())))?;
    let now = OffsetDateTime::now_utc();

    state
        .store
        .store_user_password(invitation.user_id, &password_hash, now)
        .await?;
    state
        .store
        .update_user_status(invitation.user_id, UserStatus::Active, now)
        .await?;
    state
        .store
        .mark_password_invitation_consumed(invitation.invitation_id, now)
        .await?;

    Ok(Json(envelope(CompleteInvitationResponse {
        status: "password_set",
    })))
}

pub async fn oidc_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OidcStartQuery>,
) -> Result<Redirect, AppError> {
    let provider = load_enabled_oidc_provider(&state.store, &query.provider_key).await?;
    let origin = request_origin(&headers);
    let email = normalize_email(&query.login_hint)?;
    let subject = oidc_subject(&provider, &email);
    let redirect_to = query
        .redirect_to
        .unwrap_or_else(|| "/admin/account-ready?mode=oidc".to_string());

    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("provider_key", &provider.provider_key)
        .append_pair("email", &email)
        .append_pair("subject", &subject)
        .append_pair("redirect_to", &redirect_to)
        .finish();
    let callback = format!("{origin}/api/v1/auth/oidc/callback?{query}");

    Ok(Redirect::temporary(&callback))
}

pub async fn oidc_callback(
    State(state): State<AppState>,
    Query(query): Query<OidcCallbackQuery>,
) -> Result<Response, AppError> {
    let provider = load_enabled_oidc_provider(&state.store, &query.provider_key).await?;
    let email = normalize_email(&query.email)?;
    let subject = query
        .subject
        .unwrap_or_else(|| oidc_subject(&provider, &email));
    let now = OffsetDateTime::now_utc();

    let user = if let Some(oidc_auth) = state
        .store
        .get_user_oidc_auth(&provider.oidc_provider_id, &subject)
        .await?
    {
        let user = state
            .store
            .get_user_by_id(oidc_auth.user_id)
            .await?
            .ok_or_else(|| AppError(GatewayError::InvalidRequest("user not found".to_string())))?;
        if user.status == UserStatus::Disabled {
            return Err(AppError(GatewayError::InvalidRequest(
                "disabled users cannot sign in".to_string(),
            )));
        }
        if user.status == UserStatus::Invited {
            state
                .store
                .update_user_status(user.user_id, UserStatus::Active, now)
                .await?;
        }
        user
    } else {
        let user = state
            .store
            .find_invited_oidc_user(&email, &provider.oidc_provider_id)
            .await?
            .ok_or_else(|| {
                AppError(GatewayError::InvalidRequest(
                    "no invited oidc user matches this login".to_string(),
                ))
            })?;

        if user.status == UserStatus::Disabled {
            return Err(AppError(GatewayError::InvalidRequest(
                "disabled users cannot sign in".to_string(),
            )));
        }

        state
            .store
            .create_user_oidc_auth(
                user.user_id,
                &provider.oidc_provider_id,
                &subject,
                Some(email.as_str()),
                now,
            )
            .await?;
        state
            .store
            .update_user_status(user.user_id, UserStatus::Active, now)
            .await?;
        user
    };

    let session_cookie = issue_session_cookie(&state, user.user_id, now).await?;
    let redirect_to = query.redirect_to.unwrap_or_else(|| {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("mode", "oidc")
            .append_pair("email", &user.email)
            .finish();
        format!("/admin/account-ready?{query}")
    });
    let mut response = Redirect::temporary(&redirect_to).into_response();
    response.headers_mut().append(SET_COOKIE, session_cookie);
    Ok(response)
}

pub(crate) fn envelope<T>(data: T) -> Envelope<T> {
    Envelope {
        data,
        meta: ResponseMeta {
            generated_at: format_timestamp(OffsetDateTime::now_utc()),
        },
    }
}

fn request_origin(headers: &HeaderMap) -> String {
    if let Some(origin) = header_value(headers, "x-forwarded-origin") {
        return origin;
    }

    let proto = header_value(headers, "x-forwarded-proto").unwrap_or_else(|| "http".to_string());
    let host = header_value(headers, "host").unwrap_or_else(|| "localhost:8080".to_string());
    format!("{proto}://{host}")
}

pub(crate) async fn resolve_session_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<UserRecord>, AppError> {
    let Some(raw_token) = cookie_value(headers, SESSION_COOKIE_NAME) else {
        return Ok(None);
    };
    let Some(token_id) = parse_signed_token_id(&raw_token) else {
        return Ok(None);
    };
    let Some(session) = state.store.get_user_session(token_id).await? else {
        return Ok(None);
    };
    let now = OffsetDateTime::now_utc();
    if session.revoked_at.is_some()
        || session.expires_at <= now
        || session.token_hash != token_hash(&raw_token)
    {
        return Ok(None);
    }

    let Some(user) = state.store.get_user_by_id(session.user_id).await? else {
        return Ok(None);
    };
    if user.status == UserStatus::Disabled {
        state.store.revoke_user_sessions(user.user_id, now).await?;
        return Ok(None);
    }

    state
        .store
        .touch_user_session(session.session_id, now)
        .await?;
    Ok(Some(user))
}

fn build_auth_session_view(user: UserRecord) -> AuthSessionView {
    AuthSessionView {
        user: AuthSessionUserView {
            id: user.user_id.to_string(),
            name: user.name,
            email: user.email,
            global_role: user.global_role.as_str().to_string(),
        },
        must_change_password: user.must_change_password,
    }
}

async fn generate_unique_team_key(store: &AnyStore, name: &str) -> Result<String, AppError> {
    let base = slugify_team_name(name);
    let mut candidate = base.clone();
    let mut suffix = 2_u32;

    while store.get_team_by_key(&candidate).await?.is_some() {
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }

    Ok(candidate)
}

fn slugify_team_name(name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in name.trim().chars() {
        let lowered = ch.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() {
            slug.push(lowered);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "team".to_string()
    } else {
        slug
    }
}

fn parse_uuid_list(raw_values: &[String]) -> Result<Vec<Uuid>, AppError> {
    let mut seen = BTreeSet::new();
    let mut values = Vec::new();
    for value in raw_values {
        let parsed = parse_uuid(value)?;
        if seen.insert(parsed) {
            values.push(parsed);
        }
    }
    Ok(values)
}

fn validate_team_admin_assignments(
    users: &[IdentityUserRecord],
    team_id: Option<Uuid>,
    selected_admin_ids: &[Uuid],
) -> Result<(), AppError> {
    let users_by_id: HashMap<Uuid, &IdentityUserRecord> =
        users.iter().map(|user| (user.user.user_id, user)).collect();
    let mut conflicts = Vec::new();

    for user_id in selected_admin_ids {
        let Some(user) = users_by_id.get(user_id) else {
            return Err(AppError(GatewayError::InvalidRequest(format!(
                "user `{user_id}` not found"
            ))));
        };

        if let Some(existing_team_id) = user.team_id
            && Some(existing_team_id) != team_id
        {
            conflicts.push(format!(
                "{} ({})",
                user.user.email,
                user.team_name.as_deref().unwrap_or("another team")
            ));
        }
    }

    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(AppError(GatewayError::InvalidRequest(format!(
            "users already belong to another team: {}",
            conflicts.join(", ")
        ))))
    }
}

async fn sync_team_admins(
    store: &AnyStore,
    team_id: Uuid,
    selected_admin_ids: &[Uuid],
    now: OffsetDateTime,
) -> Result<(), AppError> {
    let selected_admin_ids: BTreeSet<_> = selected_admin_ids.iter().copied().collect();
    let memberships = GatewayStore::list_team_memberships(store, team_id).await?;

    for membership in &memberships {
        if membership.role == MembershipRole::Admin
            && !selected_admin_ids.contains(&membership.user_id)
        {
            store
                .update_team_membership_role(
                    team_id,
                    membership.user_id,
                    MembershipRole::Member,
                    now,
                )
                .await?;
        }
    }

    let memberships_by_user: HashMap<Uuid, gateway_core::TeamMembershipRecord> = memberships
        .into_iter()
        .map(|membership| (membership.user_id, membership))
        .collect();

    for user_id in selected_admin_ids {
        match memberships_by_user.get(&user_id) {
            Some(existing)
                if existing.role == MembershipRole::Admin
                    || existing.role == MembershipRole::Owner => {}
            Some(_) => {
                store
                    .update_team_membership_role(team_id, user_id, MembershipRole::Admin, now)
                    .await?;
            }
            None => {
                store
                    .assign_team_membership(user_id, team_id, MembershipRole::Admin)
                    .await?;
            }
        }
    }

    Ok(())
}

fn parse_requested_membership(
    team_id: Option<&str>,
    team_role: Option<&str>,
) -> Result<Option<(Uuid, MembershipRole)>, AppError> {
    match (team_id, team_role) {
        (Some(team_id), Some(role)) => {
            let role = ensure_assignable_membership_role(parse_membership_role(role)?)
                .map_err(AppError)?;
            Ok(Some((parse_uuid(team_id)?, role)))
        }
        (None, None) => Ok(None),
        _ => Err(AppError(GatewayError::InvalidRequest(
            "team_id and team_role must either both be present or both be absent".to_string(),
        ))),
    }
}

async fn resolve_requested_oidc_provider(
    store: &AnyStore,
    auth_mode: AuthMode,
    oidc_provider_key: Option<&str>,
) -> Result<Option<OidcProviderRecord>, AppError> {
    match auth_mode {
        AuthMode::Oidc => {
            let provider_key = oidc_provider_key.ok_or_else(|| {
                AppError(GatewayError::InvalidRequest(
                    "oidc_provider_key is required for oidc users".to_string(),
                ))
            })?;
            Ok(Some(load_enabled_oidc_provider(store, provider_key).await?))
        }
        _ => {
            if oidc_provider_key.is_some() {
                return Err(AppError(GatewayError::InvalidRequest(
                    "oidc_provider_key is only valid for oidc users".to_string(),
                )));
            }
            Ok(None)
        }
    }
}

async fn sync_identity_user_membership(
    store: &AnyStore,
    user: &IdentityUserRecord,
    requested_membership: Option<(Uuid, MembershipRole)>,
    now: OffsetDateTime,
) -> Result<(), AppError> {
    if !membership_update_requested(user, requested_membership) {
        return Ok(());
    }
    ensure_mutable_membership(user.membership_role).map_err(AppError)?;

    match (user.team_id, requested_membership) {
        (None, None) => Ok(()),
        (None, Some((team_id, role))) => {
            store
                .assign_team_membership(user.user.user_id, team_id, role)
                .await?;
            Ok(())
        }
        (Some(team_id), None) => {
            store
                .remove_team_membership(team_id, user.user.user_id)
                .await?;
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
                .await?;
            Ok(())
        }
    }
}

fn membership_update_requested(
    user: &IdentityUserRecord,
    requested_membership: Option<(Uuid, MembershipRole)>,
) -> bool {
    current_membership(user) != requested_membership
}

fn current_membership(user: &IdentityUserRecord) -> Option<(Uuid, MembershipRole)> {
    match (user.team_id, user.membership_role) {
        (Some(team_id), Some(role)) => Some((team_id, role)),
        _ => None,
    }
}

async fn sync_identity_user_auth_mode(
    store: &AnyStore,
    user: &IdentityUserRecord,
    next_auth_mode: AuthMode,
    oidc_provider: Option<&OidcProviderRecord>,
    now: OffsetDateTime,
) -> Result<(), AppError> {
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
                AppError(GatewayError::InvalidRequest(
                    "oidc provider configuration is required".to_string(),
                ))
            })?;
            store
                .set_user_oidc_link(user.user.user_id, &provider.oidc_provider_id, now)
                .await?;
        }
        AuthMode::Oauth => {}
    }

    Ok(())
}

async fn user_has_auth_proof(
    store: &AnyStore,
    user: &IdentityUserRecord,
) -> Result<bool, AppError> {
    match user.user.auth_mode {
        AuthMode::Password => Ok(store
            .get_user_password_auth(user.user.user_id)
            .await?
            .is_some()),
        AuthMode::Oidc => {
            let Some(provider_id) = user.oidc_provider_id.as_deref() else {
                return Ok(false);
            };
            Ok(store
                .get_user_oidc_auth_by_user(user.user.user_id, provider_id)
                .await?
                .is_some())
        }
        AuthMode::Oauth => Ok(false),
    }
}

async fn build_onboarding_response(
    state: &AppState,
    origin: &str,
    now: OffsetDateTime,
    user: IdentityUserRecord,
) -> Result<CreateUserResponse, AppError> {
    match user.user.auth_mode {
        AuthMode::Password => {
            let invitation = create_password_invite(
                &state.store,
                &state.identity_token_secret,
                origin,
                user.user.user_id,
            )
            .await?;
            let view = build_admin_identity_user_view(
                &state.store,
                &state.identity_token_secret,
                origin,
                now,
                reload_identity_user(&state.store, user.user.user_id).await?,
            )
            .await?;
            Ok(CreateUserResponse::PasswordInvite {
                user: view,
                invite_url: invitation.url,
                expires_at: invitation.expires_at,
            })
        }
        AuthMode::Oidc => {
            let provider_key = user.oidc_provider_key.clone().ok_or_else(|| {
                AppError(GatewayError::InvalidRequest(
                    "oidc provider is required for oidc users".to_string(),
                ))
            })?;
            let provider = load_enabled_oidc_provider(&state.store, &provider_key).await?;
            let view = build_admin_identity_user_view(
                &state.store,
                &state.identity_token_secret,
                origin,
                now,
                user.clone(),
            )
            .await?;
            Ok(CreateUserResponse::OidcSignIn {
                user: view,
                sign_in_url: oidc_sign_in_url(origin, &provider.provider_key, &user.user.email),
                provider_label: provider.provider_key,
            })
        }
        AuthMode::Oauth => Err(AppError(GatewayError::InvalidRequest(
            "unsupported auth mode".to_string(),
        ))),
    }
}

async fn load_identity_user_for_mutation(
    state: &AppState,
    user_id: Uuid,
) -> Result<IdentityUserRecord, AppError> {
    let user = state
        .store
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("user not found".to_string())))?;
    ensure_manageable_user(&user).map_err(AppError)?;
    reload_identity_user(&state.store, user_id).await
}

struct GeneratedInvite {
    url: String,
    expires_at: String,
}

async fn create_password_invite(
    store: &AnyStore,
    secret: &str,
    origin: &str,
    user_id: Uuid,
) -> Result<GeneratedInvite, AppError> {
    let now = OffsetDateTime::now_utc();
    store
        .revoke_password_invitations_for_user(user_id, now)
        .await?;

    let invitation_id = Uuid::new_v4();
    let raw_token = signed_token("invite", secret, invitation_id);
    let invitation = store
        .create_password_invitation(
            invitation_id,
            user_id,
            &token_hash(&raw_token),
            now + Duration::days(INVITE_TTL_DAYS),
            now,
        )
        .await?;

    Ok(GeneratedInvite {
        url: invitation_url(origin, &invitation, secret),
        expires_at: format_timestamp(invitation.expires_at),
    })
}

async fn load_invitation_view(state: &AppState, token: &str) -> Result<InvitationView, AppError> {
    let Some(invitation_id) = parse_signed_token_id(token) else {
        return Ok(InvitationView {
            state: "invalid".to_string(),
            email: None,
            name: None,
            expires_at: None,
        });
    };

    let Some(invitation) = state.store.get_password_invitation(invitation_id).await? else {
        return Ok(InvitationView {
            state: "invalid".to_string(),
            email: None,
            name: None,
            expires_at: None,
        });
    };

    if invitation.token_hash != token_hash(token) {
        return Ok(InvitationView {
            state: "invalid".to_string(),
            email: None,
            name: None,
            expires_at: None,
        });
    }

    let user = state
        .store
        .get_user_by_id(invitation.user_id)
        .await?
        .ok_or_else(|| AppError(GatewayError::InvalidRequest("user not found".to_string())))?;
    let now = OffsetDateTime::now_utc();
    let status = if invitation.consumed_at.is_some() {
        "consumed"
    } else if invitation.revoked_at.is_some() {
        "revoked"
    } else if invitation.expires_at <= now {
        "expired"
    } else {
        "valid"
    };

    Ok(InvitationView {
        state: status.to_string(),
        email: Some(user.email),
        name: Some(user.name),
        expires_at: Some(format_timestamp(invitation.expires_at)),
    })
}

async fn load_valid_invitation(
    state: &AppState,
    token: &str,
) -> Result<PasswordInvitationRecord, AppError> {
    let Some(invitation_id) = parse_signed_token_id(token) else {
        return Err(AppError(GatewayError::InvalidRequest(
            "invalid invitation token".to_string(),
        )));
    };
    let invitation = state
        .store
        .get_password_invitation(invitation_id)
        .await?
        .ok_or_else(|| {
            AppError(GatewayError::InvalidRequest(
                "invalid invitation token".to_string(),
            ))
        })?;

    let now = OffsetDateTime::now_utc();
    if invitation.token_hash != token_hash(token)
        || invitation.consumed_at.is_some()
        || invitation.revoked_at.is_some()
        || invitation.expires_at <= now
    {
        return Err(AppError(GatewayError::InvalidRequest(
            "invitation token is no longer valid".to_string(),
        )));
    }

    Ok(invitation)
}

async fn issue_session_cookie(
    state: &AppState,
    user_id: Uuid,
    now: OffsetDateTime,
) -> Result<HeaderValue, AppError> {
    let session_id = Uuid::new_v4();
    let raw_token = signed_token("session", &state.identity_token_secret, session_id);
    let expires_at = now + Duration::days(SESSION_TTL_DAYS);
    state
        .store
        .create_user_session(
            session_id,
            user_id,
            &token_hash(&raw_token),
            expires_at,
            now,
        )
        .await?;

    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE_NAME}={raw_token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        SESSION_TTL_DAYS * 24 * 60 * 60
    ))
    .map_err(|error| AppError(GatewayError::Internal(error.to_string())))
}

async fn load_enabled_oidc_provider(
    store: &AnyStore,
    provider_key: &str,
) -> Result<OidcProviderRecord, AppError> {
    store
        .get_enabled_oidc_provider_by_key(provider_key)
        .await?
        .ok_or_else(|| {
            AppError(GatewayError::InvalidRequest(format!(
                "oidc provider `{provider_key}` is not enabled"
            )))
        })
}

pub(crate) fn oidc_sign_in_url(origin: &str, provider_key: &str, email: &str) -> String {
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("provider_key", provider_key)
        .append_pair("login_hint", email)
        .append_pair("redirect_to", "/admin/account-ready?mode=oidc")
        .finish();
    format!("{origin}/api/v1/auth/oidc/start?{query}")
}

pub(crate) fn invitation_url(
    origin: &str,
    invitation: &PasswordInvitationRecord,
    secret: &str,
) -> String {
    let token = signed_token("invite", secret, invitation.invitation_id);
    format!("{origin}/admin/invite/{token}")
}

fn signed_token(kind: &str, secret: &str, id: Uuid) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b":");
    hasher.update(secret.as_bytes());
    hasher.update(b":");
    hasher.update(id.as_bytes());
    let signature = hex_string(&hasher.finalize());
    format!("{id}.{}", &signature[..32])
}

fn parse_signed_token_id(token: &str) -> Option<Uuid> {
    let (raw_id, _signature) = token.split_once('.')?;
    Uuid::parse_str(raw_id).ok()
}

fn token_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex_string(&hasher.finalize())
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn normalize_email(email: &str) -> Result<String, AppError> {
    let normalized = email.trim().to_ascii_lowercase();
    if normalized.is_empty() || !normalized.contains('@') {
        return Err(AppError(GatewayError::InvalidRequest(
            "email must be a valid email address".to_string(),
        )));
    }
    Ok(normalized)
}

fn parse_auth_mode(raw: &str) -> Result<AuthMode, AppError> {
    match raw {
        "password" => Ok(AuthMode::Password),
        "oidc" => Ok(AuthMode::Oidc),
        _ => Err(AppError(GatewayError::InvalidRequest(format!(
            "unsupported auth_mode `{raw}`"
        )))),
    }
}

fn parse_global_role(raw: &str) -> Result<GlobalRole, AppError> {
    match raw {
        "platform_admin" => Ok(GlobalRole::PlatformAdmin),
        "user" => Ok(GlobalRole::User),
        _ => Err(AppError(GatewayError::InvalidRequest(format!(
            "unsupported global_role `{raw}`"
        )))),
    }
}

fn parse_membership_role(raw: &str) -> Result<MembershipRole, AppError> {
    match raw {
        "owner" => Ok(MembershipRole::Owner),
        "admin" => Ok(MembershipRole::Admin),
        "member" => Ok(MembershipRole::Member),
        _ => Err(AppError(GatewayError::InvalidRequest(format!(
            "unsupported team_role `{raw}`"
        )))),
    }
}

fn parse_uuid(raw: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|error| AppError(GatewayError::InvalidRequest(error.to_string())))
}

fn oidc_subject(provider: &OidcProviderRecord, email: &str) -> String {
    format!("mock:{}:{email}", provider.provider_key)
}

pub(crate) fn format_timestamp(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .unwrap_or_else(|_| value.unix_timestamp().to_string())
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

fn cookie_value(headers: &HeaderMap, key: &str) -> Option<String> {
    let raw_cookie = headers.get("cookie")?.to_str().ok()?;
    raw_cookie.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        if name == key {
            Some(value.to_string())
        } else {
            None
        }
    })
}
