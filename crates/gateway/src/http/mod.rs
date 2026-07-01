pub mod admin_auth;
pub mod admin_contract;
mod anthropic_stream;
pub mod api_keys;
pub mod error;
mod focus_export;
pub mod handlers;
pub mod identity;
pub mod identity_lifecycle;
pub mod identity_views;
pub mod mcp_gateway;
pub mod mcp_registry;
pub mod models;
pub mod observability;
pub mod request_tags;
pub mod review_agent;
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

use self::{
    api_keys::*, handlers::*, identity::*, mcp_gateway::*, mcp_registry::*, models::*,
    observability::*, review_agent::*, spend::*, state::AppState,
};

pub fn build_router(state: AppState, admin_ui: AdminUiConfig) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");

    let api_router = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/api/v1/health", get(api_health))
        .route(
            "/api/v1/admin/api-keys",
            get(list_api_keys).post(create_api_key),
        )
        .route("/api/v1/admin/api-keys/{api_key_id}", patch(update_api_key))
        .route(
            "/api/v1/admin/api-keys/{api_key_id}/revoke",
            post(revoke_api_key),
        )
        .route("/api/v1/admin/models", get(list_models))
        .route(
            "/api/v1/admin/models/client-configs",
            post(generate_model_client_configs),
        )
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
            "/api/v1/admin/identity/service-accounts",
            get(list_identity_service_accounts).post(create_identity_service_account),
        )
        .route(
            "/api/v1/admin/identity/service-accounts/{service_account_id}",
            patch(update_identity_service_account).delete(disable_identity_service_account),
        )
        .route(
            "/api/v1/admin/identity/users/{user_id}/password-invite",
            post(regenerate_password_invite),
        )
        .route("/api/v1/admin/spend/report", get(get_spend_report))
        .route(
            "/api/v1/admin/review-agent/repositories",
            get(list_review_agent_repositories).post(create_review_agent_repository),
        )
        .route(
            "/api/v1/admin/review-agent/repositories/{repository_id}",
            get(get_review_agent_repository).patch(update_review_agent_repository),
        )
        .route(
            "/api/v1/admin/review-agent/repositories/{repository_id}/disable",
            post(disable_review_agent_repository),
        )
        .route(
            "/api/v1/admin/review-agent/repositories/{repository_id}/reactivate",
            post(reactivate_review_agent_repository),
        )
        .route(
            "/api/v1/admin/review-agent/repositories/{repository_id}/runs",
            get(list_review_agent_runs),
        )
        .route(
            "/api/v1/admin/review-agent/repositories/{repository_id}/workflow",
            post(render_review_agent_workflow),
        )
        .route(
            "/api/v1/review-agent/action/config/resolve",
            post(resolve_review_agent_action_config),
        )
        .route(
            "/api/v1/review-agent/action/runs",
            post(start_review_agent_action_run),
        )
        .route(
            "/api/v1/review-agent/action/runs/{run_id}/heartbeat",
            post(heartbeat_review_agent_action_run),
        )
        .route(
            "/api/v1/review-agent/action/runs/{run_id}/complete",
            post(complete_review_agent_action_run),
        )
        .route(
            "/api/v1/review-agent/action/runs/{run_id}/fail",
            post(fail_review_agent_action_run),
        )
        .route("/api/v1/admin/spend/focus.csv", get(get_admin_focus_export))
        .route("/api/v1/me/spend/focus.csv", get(get_my_focus_export))
        .route(
            "/api/v1/admin/spend/budgets",
            get(list_spend_budgets).put(upsert_budget),
        )
        .route(
            "/api/v1/admin/spend/budgets/deactivate",
            post(deactivate_budget),
        )
        .route(
            "/api/v1/admin/spend/budget-alerts",
            get(list_budget_alert_history),
        )
        .route(
            "/api/v1/admin/observability/leaderboard",
            get(get_usage_leaderboard),
        )
        .route(
            "/api/v1/admin/observability/harness-usage",
            get(get_harness_usage),
        )
        .route(
            "/api/v1/admin/observability/request-logs",
            get(list_request_logs),
        )
        .route(
            "/api/v1/admin/observability/request-logs/{request_log_id}",
            get(get_request_log_detail),
        )
        .route(
            "/api/v1/admin/observability/mcp-invocations",
            get(list_mcp_tool_invocations),
        )
        .route(
            "/api/v1/admin/observability/mcp-invocations/{mcp_tool_invocation_id}",
            get(get_mcp_tool_invocation_detail),
        )
        .route(
            "/api/v1/admin/mcp/recommended-servers",
            get(list_recommended_mcp_servers),
        )
        .route(
            "/api/v1/admin/mcp/servers",
            get(list_mcp_servers).post(create_mcp_server),
        )
        .route(
            "/api/v1/admin/mcp/servers/{server_id}",
            patch(update_mcp_server),
        )
        .route(
            "/api/v1/admin/mcp/servers/{server_id}/disable",
            post(disable_mcp_server),
        )
        .route(
            "/api/v1/admin/mcp/servers/{server_id}/tools",
            get(list_mcp_server_tools),
        )
        .route(
            "/api/v1/admin/mcp/servers/{server_id}/discovery-refresh",
            post(refresh_mcp_server_discovery),
        )
        .route(
            "/api/v1/admin/mcp/toolsets",
            get(list_mcp_toolsets).post(create_mcp_toolset),
        )
        .route(
            "/api/v1/admin/mcp/toolsets/{toolset_id}",
            patch(update_mcp_toolset),
        )
        .route(
            "/api/v1/admin/mcp/toolsets/{toolset_id}/disable",
            post(disable_mcp_toolset),
        )
        .route(
            "/api/v1/admin/mcp/toolsets/{toolset_id}/tools",
            axum::routing::put(replace_mcp_toolset_tools),
        )
        .route(
            "/api/v1/admin/mcp/grants",
            get(list_mcp_grants)
                .put(upsert_mcp_grant)
                .delete(revoke_mcp_grant),
        )
        .route(
            "/api/v1/admin/mcp/credential-bindings",
            get(list_mcp_credential_bindings).put(upsert_mcp_credential_binding),
        )
        .route(
            "/api/v1/admin/mcp/credential-bindings/{credential_binding_id}",
            axum::routing::delete(revoke_mcp_credential_binding),
        )
        .route(
            "/api/v1/admin/mcp/effective-access",
            get(preview_mcp_effective_access),
        )
        .route("/api/v1/auth/session", get(get_auth_session))
        .route("/api/v1/auth/login/password", post(login_with_password))
        .route("/api/v1/auth/logout", post(logout_current_session))
        .route("/api/v1/auth/password/change", post(change_password))
        .route(
            "/api/v1/auth/invitations/{token}",
            get(validate_password_invitation),
        )
        .route(
            "/api/v1/auth/invitations/{token}/password",
            post(complete_password_invitation),
        )
        .route(
            "/api/v1/auth/oidc/providers",
            get(list_public_oidc_providers),
        )
        .route("/api/v1/auth/oidc/start", get(oidc_start))
        .route("/api/v1/auth/oidc/callback", get(oidc_callback))
        .route(
            "/api/v1/auth/oauth/providers",
            get(list_public_oauth_providers),
        )
        .route("/api/v1/auth/oauth/start", get(oauth_start))
        .route(
            "/api/v1/auth/oauth/callback/github",
            get(oauth_callback_github),
        )
        .route("/v1/models", get(v1_models))
        .route("/v1/messages", post(v1_messages))
        .route("/messages", post(v1_messages))
        .route("/v1/chat/completions", post(v1_chat_completions))
        .route("/v1/responses", post(v1_responses))
        .route("/v1/embeddings", post(v1_embeddings))
        .route(
            "/mcp",
            post(mcp_aggregate_streamable_http)
                .get(mcp_aggregate_streamable_http)
                .delete(mcp_aggregate_streamable_http),
        )
        .route(
            "/mcp/{server_key}",
            post(mcp_streamable_http_proxy)
                .get(mcp_streamable_http_proxy)
                .delete(mcp_streamable_http_proxy),
        )
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
                    uri = %request.uri().path(),
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
