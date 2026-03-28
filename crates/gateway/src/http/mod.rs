pub mod admin_auth;
pub mod error;
pub mod handlers;
pub mod identity;
pub mod identity_lifecycle;
pub mod identity_views;
pub mod observability;
pub mod request_tags;
pub mod spend;
pub mod state;

use admin_ui::{AdminUiConfig, mount_admin_ui};
use axum::{
    Router,
    routing::{delete, get, patch, post},
};
use http::HeaderName;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

use self::{handlers::*, identity::*, observability::*, spend::*, state::AppState};

pub fn build_router(state: AppState, admin_ui: AdminUiConfig) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");

    let api_router = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/v1/health", get(api_health))
        .route(
            "/api/v1/admin/identity/users",
            get(list_identity_users).post(create_identity_user),
        )
        .route(
            "/api/v1/admin/identity/users/{user_id}",
            patch(update_identity_user),
        )
        .route(
            "/api/v1/admin/identity/users/{user_id}/deactivate",
            post(deactivate_identity_user),
        )
        .route(
            "/api/v1/admin/identity/users/{user_id}/reactivate",
            post(reactivate_identity_user),
        )
        .route(
            "/api/v1/admin/identity/users/{user_id}/reset-onboarding",
            post(reset_identity_user_onboarding),
        )
        .route(
            "/api/v1/admin/identity/teams",
            get(list_identity_teams).post(create_identity_team),
        )
        .route(
            "/api/v1/admin/identity/teams/{team_id}",
            axum::routing::patch(update_identity_team),
        )
        .route(
            "/api/v1/admin/identity/teams/{team_id}/members",
            post(add_identity_team_members),
        )
        .route(
            "/api/v1/admin/identity/teams/{team_id}/members/{user_id}",
            delete(remove_identity_team_member),
        )
        .route(
            "/api/v1/admin/identity/teams/{team_id}/members/{user_id}/transfer",
            post(transfer_identity_team_member),
        )
        .route(
            "/api/v1/admin/identity/users/{user_id}/password-invite",
            post(regenerate_password_invite),
        )
        .route("/api/v1/admin/spend/report", get(get_spend_report))
        .route("/api/v1/admin/spend/budgets", get(list_spend_budgets))
        .route(
            "/api/v1/admin/spend/budget-alerts",
            get(list_budget_alert_history),
        )
        .route(
            "/api/v1/admin/spend/budgets/users/{user_id}",
            axum::routing::put(upsert_user_budget).delete(deactivate_user_budget),
        )
        .route(
            "/api/v1/admin/spend/budgets/teams/{team_id}",
            axum::routing::put(upsert_team_budget).delete(deactivate_team_budget),
        )
        .route(
            "/api/v1/admin/observability/request-logs",
            get(list_request_logs),
        )
        .route(
            "/api/v1/admin/observability/request-logs/{request_log_id}",
            get(get_request_log_detail),
        )
        .route("/api/v1/auth/session", get(get_auth_session))
        .route("/api/v1/auth/login/password", post(login_with_password))
        .route("/api/v1/auth/password/change", post(change_password))
        .route(
            "/api/v1/auth/invitations/{token}",
            get(validate_password_invitation),
        )
        .route(
            "/api/v1/auth/invitations/{token}/password",
            post(complete_password_invitation),
        )
        .route("/api/v1/auth/oidc/start", get(oidc_start))
        .route("/api/v1/auth/oidc/callback", get(oidc_callback))
        .route("/v1/models", get(v1_models))
        .route("/v1/chat/completions", post(v1_chat_completions))
        .route("/v1/embeddings", post(v1_embeddings))
        .with_state(state)
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &http::Request<_>| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("missing");

                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    request_id = %request_id,
                    http.route = tracing::field::Empty,
                    requested_model = tracing::field::Empty,
                    resolved_model = tracing::field::Empty,
                    provider = tracing::field::Empty,
                    stream = tracing::field::Empty,
                    ownership_kind = tracing::field::Empty
                )
            }),
        )
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid));

    mount_admin_ui(api_router, admin_ui)
}
