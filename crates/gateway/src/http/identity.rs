use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, header::SET_COOKIE},
    response::{IntoResponse, Redirect, Response},
};
use gateway_core::{
    AuthError, AuthMode, GatewayError, GlobalRole, IdentityRepository, IdentityUserRecord,
    MembershipRole, OidcProviderRecord, PasswordInvitationRecord, UserRecord,
};
use gateway_store::LibsqlStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use url::form_urlencoded;
use uuid::Uuid;

use crate::http::{error::AppError, state::AppState};

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
    id: String,
    name: String,
    email: String,
    auth_mode: String,
    global_role: String,
    team_id: Option<String>,
    team_name: Option<String>,
    team_role: Option<String>,
    status: String,
    onboarding: Option<AdminOnboardingActionView>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminTeamView {
    id: String,
    name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AdminOidcProviderView {
    id: String,
    key: String,
    label: String,
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

    let membership = match (request.team_id.as_deref(), request.team_role.as_deref()) {
        (Some(team_id), Some(role)) => Some((parse_uuid(team_id)?, parse_membership_role(role)?)),
        (None, None) => None,
        _ => {
            return Err(AppError(GatewayError::InvalidRequest(
                "team_id and team_role must either both be present or both be absent".to_string(),
            )));
        }
    };

    let oidc_provider = match auth_mode {
        AuthMode::Oidc => {
            let provider_key = request.oidc_provider_key.as_deref().ok_or_else(|| {
                AppError(GatewayError::InvalidRequest(
                    "oidc_provider_key is required for oidc users".to_string(),
                ))
            })?;
            Some(load_enabled_oidc_provider(&state.store, provider_key).await?)
        }
        _ => {
            if request.oidc_provider_key.is_some() {
                return Err(AppError(GatewayError::InvalidRequest(
                    "oidc_provider_key is only valid for oidc users".to_string(),
                )));
            }
            None
        }
    };

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
            "invited",
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

    let identity_user = reload_identity_user(&state.store, user.user_id).await?;
    let view = build_admin_identity_user_view(
        &state.store,
        &state.identity_token_secret,
        &origin,
        created_at,
        identity_user,
    )
    .await?;

    let response = match oidc_provider {
        Some(provider) => CreateUserResponse::OidcSignIn {
            user: view,
            sign_in_url: oidc_sign_in_url(&origin, &provider, &user.email),
            provider_label: provider.provider_key,
        },
        None => {
            let invitation = create_password_invite(
                &state.store,
                &state.identity_token_secret,
                &origin,
                user.user_id,
            )
            .await?;
            CreateUserResponse::PasswordInvite {
                user: view,
                invite_url: invitation.url,
                expires_at: invitation.expires_at,
            }
        }
    };

    Ok(Json(envelope(response)))
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
    if user.status != "invited" {
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
        .update_user_status(invitation.user_id, "active", now)
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
        if user.status == "disabled" {
            return Err(AppError(GatewayError::InvalidRequest(
                "disabled users cannot sign in".to_string(),
            )));
        }
        if user.status == "invited" {
            state
                .store
                .update_user_status(user.user_id, "active", now)
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

        if user.status == "disabled" {
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
            .update_user_status(user.user_id, "active", now)
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

fn envelope<T>(data: T) -> Envelope<T> {
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

async fn require_platform_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<UserRecord, AppError> {
    let current_user = if let Some(user) = resolve_session_user(state, headers).await? {
        user
    } else {
        state.store.ensure_bootstrap_admin_user().await?
    };

    if current_user.global_role != GlobalRole::PlatformAdmin {
        return Err(AppError(GatewayError::Auth(
            AuthError::InsufficientPrivileges,
        )));
    }
    if current_user.status != "active" {
        return Err(AppError(GatewayError::InvalidRequest(
            "only active admins can manage users".to_string(),
        )));
    }

    Ok(current_user)
}

async fn resolve_session_user(
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

    state
        .store
        .touch_user_session(session.session_id, now)
        .await?;
    let user = state.store.get_user_by_id(session.user_id).await?;
    Ok(user)
}

async fn build_admin_identity_user_view(
    store: &LibsqlStore,
    secret: &str,
    origin: &str,
    now: OffsetDateTime,
    user: IdentityUserRecord,
) -> Result<AdminIdentityUserView, AppError> {
    let onboarding = match user.user.auth_mode {
        AuthMode::Password if user.user.status == "invited" => {
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
        AuthMode::Oidc => {
            let provider_key = user.oidc_provider_key.clone().unwrap_or_default();
            if provider_key.is_empty() {
                None
            } else {
                let provider = OidcProviderRecord {
                    oidc_provider_id: user.oidc_provider_id.clone().unwrap_or_default(),
                    provider_key: provider_key.clone(),
                    provider_type: "generic_oidc".to_string(),
                    issuer_url: String::new(),
                    client_id: String::new(),
                    scopes: Vec::new(),
                    enabled: true,
                    created_at: now,
                    updated_at: now,
                };
                Some(AdminOnboardingActionView::OidcSignIn {
                    sign_in_url: oidc_sign_in_url(origin, &provider, &user.user.email),
                    provider_key: provider_key.clone(),
                    provider_label: provider_key,
                })
            }
        }
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
        status: user.user.status,
        onboarding,
    })
}

async fn reload_identity_user(
    store: &LibsqlStore,
    user_id: Uuid,
) -> Result<IdentityUserRecord, AppError> {
    let user = store
        .list_identity_users()
        .await?
        .into_iter()
        .find(|record| record.user.user_id == user_id)
        .ok_or_else(|| {
            AppError(GatewayError::Store(gateway_core::StoreError::NotFound(
                "user missing".to_string(),
            )))
        })?;
    Ok(user)
}

struct GeneratedInvite {
    url: String,
    expires_at: String,
}

async fn create_password_invite(
    store: &LibsqlStore,
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
    store: &LibsqlStore,
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

fn oidc_sign_in_url(origin: &str, provider: &OidcProviderRecord, email: &str) -> String {
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("provider_key", &provider.provider_key)
        .append_pair("login_hint", email)
        .append_pair("redirect_to", "/admin/account-ready?mode=oidc")
        .finish();
    format!("{origin}/api/v1/auth/oidc/start?{query}")
}

fn invitation_url(origin: &str, invitation: &PasswordInvitationRecord, secret: &str) -> String {
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

fn format_timestamp(value: OffsetDateTime) -> String {
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
