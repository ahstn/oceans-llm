use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, header::AUTHORIZATION},
};
use gateway_core::{
    AuthError, GatewayError, GlobalRole, IdentityRepository, MembershipRole, ReviewAgentProvider,
    ReviewAgentRepository, ReviewAgentRepositoryRecord, ReviewAgentRepositoryStatus,
    ReviewAgentRunRecord, ReviewAgentRunStatus, ReviewAgentSettings, StoreError, UserStatus,
};
use gateway_service::{
    ActionConfigResolveInput, ActionPullRequestInput, ActionRepositoryIdentity,
    ActionRunCompleteInput, ActionRunFailInput, ActionRunHeartbeatInput, ActionRunStartInput,
    CreateReviewAgentRepositoryInput, ReviewAgentConfigOverrides, ReviewAgentService,
    UpdateReviewAgentRepositoryInput, WorkflowRenderInput,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::http::{
    admin_auth::require_authenticated_session,
    admin_contract::{Envelope, envelope, format_timestamp},
    error::AppError,
    state::AppState,
};

#[derive(Debug, Deserialize, IntoParams)]
pub struct ReviewAgentListQuery {
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct ReviewAgentRunsQuery {
    pr_number: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReviewAgentRepositoriesPayload {
    items: Vec<ReviewAgentRepositoryView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReviewAgentRepositoryPayload {
    repository: ReviewAgentRepositoryView,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReviewAgentRunsPayload {
    items: Vec<ReviewAgentRunView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReviewAgentWorkflowPayload {
    file_name: String,
    yaml: String,
    action_ref: String,
    oceans_url: String,
    api_key_secret_name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReviewAgentRepositoryView {
    id: String,
    provider: String,
    external_repository_id: Option<String>,
    owner: String,
    name: String,
    full_name: String,
    service_account_id: String,
    status: String,
    settings: ReviewAgentSettingsView,
    settings_json: Value,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ReviewAgentSettingsView {
    inline_review_enabled: bool,
    pr_summary_enabled: bool,
    diagrams_enabled: bool,
    linked_issue_detection_enabled: bool,
    linked_issue_assessment_enabled: bool,
    default_model_key: Option<String>,
    max_inline_comments: Option<i64>,
    request_changes_on_high_severity: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ReviewAgentRunView {
    id: String,
    repository_id: String,
    pull_request_id: Option<String>,
    head_sha: Option<String>,
    github_run_id: Option<String>,
    github_run_attempt: Option<i64>,
    status: String,
    started_at: Option<String>,
    heartbeat_at: Option<String>,
    finished_at: Option<String>,
    duration_ms: Option<i64>,
    files_changed: Option<i64>,
    additions: Option<i64>,
    deletions: Option<i64>,
    changed_loc: Option<i64>,
    inline_comments_created: Option<i64>,
    inline_comments_updated: Option<i64>,
    inline_comments_skipped: Option<i64>,
    inline_comments_failed: Option<i64>,
    stale_comments_deleted: Option<i64>,
    managed_comment_id: Option<String>,
    managed_comment_action: Option<String>,
    managed_comment_status: Option<String>,
    review_event_status: Option<String>,
    summary_status: Option<String>,
    diagram_status: Option<String>,
    linked_issue_count: Option<i64>,
    linked_issue_status: Option<String>,
    model_execution_mode: Option<String>,
    provider_key: Option<String>,
    model_key: Option<String>,
    effective_config_json: Value,
    degraded_features_json: Option<Value>,
    error_summary: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateReviewAgentRepositoryRequest {
    provider: String,
    external_repository_id: Option<String>,
    owner: String,
    name: String,
    full_name: String,
    service_account_id: String,
    settings: Option<ReviewAgentSettingsView>,
    #[schema(additional_properties = true)]
    settings_json: Option<Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateReviewAgentRepositoryRequest {
    external_repository_id: Option<String>,
    owner: String,
    name: String,
    full_name: String,
    service_account_id: String,
    status: String,
    settings: ReviewAgentSettingsView,
    #[schema(additional_properties = true)]
    settings_json: Option<Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct WorkflowRenderRequest {
    action_ref: Option<String>,
    oceans_url: Option<String>,
    api_key_secret_name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRepositoryIdentityRequest {
    provider: String,
    external_repository_id: Option<String>,
    owner: String,
    name: String,
    full_name: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionPullRequestRequest {
    provider_pr_id: Option<String>,
    pr_number: i64,
    title: Option<String>,
    author_login: Option<String>,
    head_sha: Option<String>,
    base_sha: Option<String>,
    head_repository_full_name: String,
    base_repository_full_name: String,
    is_draft: bool,
}

#[derive(Debug, Default, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionConfigOverridesRequest {
    model_id: Option<String>,
    model_execution_mode: Option<String>,
    provider_key: Option<String>,
    inline_review_enabled: Option<bool>,
    pr_summary_enabled: Option<bool>,
    diagrams_enabled: Option<bool>,
    linked_issue_detection_enabled: Option<bool>,
    linked_issue_assessment_enabled: Option<bool>,
    max_inline_comments: Option<i64>,
    request_changes_on_high_severity: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionConfigResolveRequest {
    event_name: String,
    repository: ActionRepositoryIdentityRequest,
    pull_request: ActionPullRequestRequest,
    #[serde(default)]
    overrides: ActionConfigOverridesRequest,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ActionConfigResolveResponse {
    repository: ReviewAgentRepositoryView,
    pull_request_id: String,
    effective_config: Value,
    overrides_applied: Value,
    overrides_rejected: Value,
    reporting: Value,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRunStartRequest {
    event_name: String,
    repository: ActionRepositoryIdentityRequest,
    pull_request: ActionPullRequestRequest,
    github_run_id: Option<String>,
    github_run_attempt: Option<i64>,
    model_execution_mode: Option<String>,
    provider_key: Option<String>,
    model_key: Option<String>,
    effective_config_json: Value,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRunHeartbeatRequest {
    status: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRunMetricsRequest {
    status: Option<String>,
    duration_ms: Option<i64>,
    files_changed: Option<i64>,
    additions: Option<i64>,
    deletions: Option<i64>,
    changed_loc: Option<i64>,
    inline_comments_created: Option<i64>,
    inline_comments_updated: Option<i64>,
    inline_comments_skipped: Option<i64>,
    inline_comments_failed: Option<i64>,
    stale_comments_deleted: Option<i64>,
    managed_comment_id: Option<String>,
    managed_comment_action: Option<String>,
    managed_comment_status: Option<String>,
    review_event_status: Option<String>,
    summary_status: Option<String>,
    diagram_status: Option<String>,
    linked_issue_count: Option<i64>,
    linked_issue_status: Option<String>,
    degraded_features_json: Option<Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRunFailRequest {
    error_summary: String,
    metrics: ActionRunMetricsRequest,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ActionRunResponse {
    run: ReviewAgentRunView,
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/review-agent/repositories",
    params(ReviewAgentListQuery),
    responses((status = 200, body = Envelope<ReviewAgentRepositoriesPayload>)),
    security(("session_cookie" = []))
)]
pub async fn list_review_agent_repositories(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ReviewAgentListQuery>,
) -> Result<Json<Envelope<ReviewAgentRepositoriesPayload>>, AppError> {
    let scope = require_review_agent_admin_scope(&state, &headers).await?;
    let service = review_agent_service(&state);
    let status = query
        .status
        .as_deref()
        .map(parse_repository_status)
        .transpose()?;
    let items = match scope {
        ReviewAgentAdminScope::Platform => {
            service
                .list_repositories(status, query.limit.unwrap_or(50), query.offset.unwrap_or(0))
                .await?
        }
        ReviewAgentAdminScope::Team(team_id) => {
            service
                .list_repositories_for_team(
                    team_id,
                    status,
                    query.limit.unwrap_or(50),
                    query.offset.unwrap_or(0),
                )
                .await?
        }
    };
    Ok(Json(envelope(ReviewAgentRepositoriesPayload {
        items: items.into_iter().map(map_repository).collect(),
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/review-agent/repositories",
    request_body = CreateReviewAgentRepositoryRequest,
    responses((status = 200, body = Envelope<ReviewAgentRepositoryPayload>)),
    security(("session_cookie" = []))
)]
pub async fn create_review_agent_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateReviewAgentRepositoryRequest>,
) -> Result<Json<Envelope<ReviewAgentRepositoryPayload>>, AppError> {
    let scope = require_review_agent_admin_scope(&state, &headers).await?;
    let service_account_id = parse_uuid(&request.service_account_id, "service_account_id")?;
    authorize_service_account_scope(&state, &scope, service_account_id).await?;
    let settings = request
        .settings
        .map(settings_from_view)
        .transpose()?
        .unwrap_or_default();
    let repository = review_agent_service(&state)
        .create_repository(CreateReviewAgentRepositoryInput {
            provider: parse_provider(&request.provider)?,
            external_repository_id: request.external_repository_id,
            owner: request.owner,
            name: request.name,
            full_name: request.full_name,
            service_account_id,
            settings,
            settings_json: validate_settings_json(request.settings_json)?,
        })
        .await?;
    Ok(Json(envelope(ReviewAgentRepositoryPayload {
        repository: map_repository(repository),
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/review-agent/repositories/{repository_id}",
    params(("repository_id" = String, Path, description = "Review Agent repository ID")),
    responses((status = 200, body = Envelope<ReviewAgentRepositoryPayload>)),
    security(("session_cookie" = []))
)]
pub async fn get_review_agent_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(repository_id): Path<String>,
) -> Result<Json<Envelope<ReviewAgentRepositoryPayload>>, AppError> {
    let scope = require_review_agent_admin_scope(&state, &headers).await?;
    let repository =
        require_visible_repository(&state, &scope, parse_uuid(&repository_id, "repository_id")?)
            .await?;
    Ok(Json(envelope(ReviewAgentRepositoryPayload {
        repository: map_repository(repository),
    })))
}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/review-agent/repositories/{repository_id}",
    request_body = UpdateReviewAgentRepositoryRequest,
    params(("repository_id" = String, Path, description = "Review Agent repository ID")),
    responses((status = 200, body = Envelope<ReviewAgentRepositoryPayload>)),
    security(("session_cookie" = []))
)]
pub async fn update_review_agent_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(repository_id): Path<String>,
    Json(request): Json<UpdateReviewAgentRepositoryRequest>,
) -> Result<Json<Envelope<ReviewAgentRepositoryPayload>>, AppError> {
    let scope = require_review_agent_admin_scope(&state, &headers).await?;
    let repository_id = parse_uuid(&repository_id, "repository_id")?;
    require_visible_repository(&state, &scope, repository_id).await?;
    let service_account_id = parse_uuid(&request.service_account_id, "service_account_id")?;
    authorize_service_account_scope(&state, &scope, service_account_id).await?;
    let settings = settings_from_view(request.settings)?;
    let repository = review_agent_service(&state)
        .update_repository(UpdateReviewAgentRepositoryInput {
            repository_id,
            external_repository_id: request.external_repository_id,
            owner: request.owner,
            name: request.name,
            full_name: request.full_name,
            service_account_id,
            status: parse_repository_status(&request.status)?,
            settings,
            settings_json: validate_settings_json(request.settings_json)?,
        })
        .await?;
    Ok(Json(envelope(ReviewAgentRepositoryPayload {
        repository: map_repository(repository),
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/review-agent/repositories/{repository_id}/disable",
    params(("repository_id" = String, Path, description = "Review Agent repository ID")),
    responses((status = 200, body = Envelope<ReviewAgentRepositoryPayload>)),
    security(("session_cookie" = []))
)]
pub async fn disable_review_agent_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(repository_id): Path<String>,
) -> Result<Json<Envelope<ReviewAgentRepositoryPayload>>, AppError> {
    set_repository_status_handler(
        state,
        headers,
        repository_id,
        ReviewAgentRepositoryStatus::Disabled,
    )
    .await
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/review-agent/repositories/{repository_id}/reactivate",
    params(("repository_id" = String, Path, description = "Review Agent repository ID")),
    responses((status = 200, body = Envelope<ReviewAgentRepositoryPayload>)),
    security(("session_cookie" = []))
)]
pub async fn reactivate_review_agent_repository(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(repository_id): Path<String>,
) -> Result<Json<Envelope<ReviewAgentRepositoryPayload>>, AppError> {
    set_repository_status_handler(
        state,
        headers,
        repository_id,
        ReviewAgentRepositoryStatus::Active,
    )
    .await
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/review-agent/repositories/{repository_id}/runs",
    params(("repository_id" = String, Path, description = "Review Agent repository ID"), ReviewAgentRunsQuery),
    responses((status = 200, body = Envelope<ReviewAgentRunsPayload>)),
    security(("session_cookie" = []))
)]
pub async fn list_review_agent_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(repository_id): Path<String>,
    Query(query): Query<ReviewAgentRunsQuery>,
) -> Result<Json<Envelope<ReviewAgentRunsPayload>>, AppError> {
    let scope = require_review_agent_admin_scope(&state, &headers).await?;
    let repository_id = parse_uuid(&repository_id, "repository_id")?;
    require_visible_repository(&state, &scope, repository_id).await?;
    let runs = review_agent_service(&state)
        .list_runs(
            repository_id,
            query.pr_number,
            query.limit.unwrap_or(50),
            query.offset.unwrap_or(0),
        )
        .await?;
    Ok(Json(envelope(ReviewAgentRunsPayload {
        items: runs.into_iter().map(map_run).collect(),
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/review-agent/repositories/{repository_id}/workflow",
    request_body = WorkflowRenderRequest,
    params(("repository_id" = String, Path, description = "Review Agent repository ID")),
    responses((status = 200, body = Envelope<ReviewAgentWorkflowPayload>)),
    security(("session_cookie" = []))
)]
pub async fn render_review_agent_workflow(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(repository_id): Path<String>,
    Json(request): Json<WorkflowRenderRequest>,
) -> Result<Json<Envelope<ReviewAgentWorkflowPayload>>, AppError> {
    let scope = require_review_agent_admin_scope(&state, &headers).await?;
    let repository_id = parse_uuid(&repository_id, "repository_id")?;
    require_visible_repository(&state, &scope, repository_id).await?;
    let rendered = review_agent_service(&state)
        .render_workflow(
            repository_id,
            WorkflowRenderInput {
                action_ref: request.action_ref,
                oceans_url: request.oceans_url,
                api_key_secret_name: request.api_key_secret_name,
            },
        )
        .await?;
    Ok(Json(envelope(ReviewAgentWorkflowPayload {
        file_name: rendered.file_name,
        yaml: rendered.yaml,
        action_ref: rendered.action_ref,
        oceans_url: rendered.oceans_url,
        api_key_secret_name: rendered.api_key_secret_name,
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/review-agent/action/config/resolve",
    request_body = ActionConfigResolveRequest,
    responses((status = 200, body = Envelope<ActionConfigResolveResponse>)),
    security(("gateway_api_key" = []))
)]
pub async fn resolve_review_agent_action_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ActionConfigResolveRequest>,
) -> Result<Json<Envelope<ActionConfigResolveResponse>>, AppError> {
    let auth = authenticate_action(&state, &headers).await?;
    let output = review_agent_service(&state)
        .resolve_config(&auth, map_config_resolve_request(request)?)
        .await?;
    Ok(Json(envelope(ActionConfigResolveResponse {
        repository: map_repository(output.repository),
        pull_request_id: output.pull_request.pull_request_id.to_string(),
        effective_config: serde_json::to_value(output.effective_config)
            .map_err(|error| GatewayError::InvalidRequest(error.to_string()))?,
        overrides_applied: serde_json::to_value(output.overrides_applied)
            .map_err(|error| GatewayError::InvalidRequest(error.to_string()))?,
        overrides_rejected: serde_json::to_value(output.overrides_rejected)
            .map_err(|error| GatewayError::InvalidRequest(error.to_string()))?,
        reporting: serde_json::to_value(output.reporting)
            .map_err(|error| GatewayError::InvalidRequest(error.to_string()))?,
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/review-agent/action/runs",
    request_body = ActionRunStartRequest,
    responses((status = 200, body = Envelope<ActionRunResponse>)),
    security(("gateway_api_key" = []))
)]
pub async fn start_review_agent_action_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ActionRunStartRequest>,
) -> Result<Json<Envelope<ActionRunResponse>>, AppError> {
    let auth = authenticate_action(&state, &headers).await?;
    let run = review_agent_service(&state)
        .start_run(&auth, map_run_start_request(request)?)
        .await?;
    Ok(Json(envelope(ActionRunResponse { run: map_run(run) })))
}

#[utoipa::path(
    post,
    path = "/api/v1/review-agent/action/runs/{run_id}/heartbeat",
    request_body = ActionRunHeartbeatRequest,
    params(("run_id" = String, Path, description = "Review Agent run ID")),
    responses((status = 200, body = Envelope<ActionRunResponse>)),
    security(("gateway_api_key" = []))
)]
pub async fn heartbeat_review_agent_action_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(request): Json<ActionRunHeartbeatRequest>,
) -> Result<Json<Envelope<ActionRunResponse>>, AppError> {
    let auth = authenticate_action(&state, &headers).await?;
    let run = review_agent_service(&state)
        .heartbeat_run(
            &auth,
            parse_uuid(&run_id, "run_id")?,
            ActionRunHeartbeatInput {
                status: request
                    .status
                    .as_deref()
                    .map(parse_run_status)
                    .transpose()?,
            },
        )
        .await?;
    Ok(Json(envelope(ActionRunResponse { run: map_run(run) })))
}

#[utoipa::path(
    post,
    path = "/api/v1/review-agent/action/runs/{run_id}/complete",
    request_body = ActionRunMetricsRequest,
    params(("run_id" = String, Path, description = "Review Agent run ID")),
    responses((status = 200, body = Envelope<ActionRunResponse>)),
    security(("gateway_api_key" = []))
)]
pub async fn complete_review_agent_action_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(request): Json<ActionRunMetricsRequest>,
) -> Result<Json<Envelope<ActionRunResponse>>, AppError> {
    let auth = authenticate_action(&state, &headers).await?;
    let run = review_agent_service(&state)
        .complete_run(
            &auth,
            parse_uuid(&run_id, "run_id")?,
            map_complete_request(request)?,
        )
        .await?;
    Ok(Json(envelope(ActionRunResponse { run: map_run(run) })))
}

#[utoipa::path(
    post,
    path = "/api/v1/review-agent/action/runs/{run_id}/fail",
    request_body = ActionRunFailRequest,
    params(("run_id" = String, Path, description = "Review Agent run ID")),
    responses((status = 200, body = Envelope<ActionRunResponse>)),
    security(("gateway_api_key" = []))
)]
pub async fn fail_review_agent_action_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(request): Json<ActionRunFailRequest>,
) -> Result<Json<Envelope<ActionRunResponse>>, AppError> {
    let auth = authenticate_action(&state, &headers).await?;
    let run = review_agent_service(&state)
        .fail_run(
            &auth,
            parse_uuid(&run_id, "run_id")?,
            ActionRunFailInput {
                error_summary: request.error_summary,
                metrics: map_complete_request(request.metrics)?,
            },
        )
        .await?;
    Ok(Json(envelope(ActionRunResponse { run: map_run(run) })))
}

async fn set_repository_status_handler(
    state: AppState,
    headers: HeaderMap,
    repository_id: String,
    status: ReviewAgentRepositoryStatus,
) -> Result<Json<Envelope<ReviewAgentRepositoryPayload>>, AppError> {
    let scope = require_review_agent_admin_scope(&state, &headers).await?;
    let repository_id = parse_uuid(&repository_id, "repository_id")?;
    require_visible_repository(&state, &scope, repository_id).await?;
    let repository = review_agent_service(&state)
        .set_repository_status(repository_id, status)
        .await?;
    Ok(Json(envelope(ReviewAgentRepositoryPayload {
        repository: map_repository(repository),
    })))
}

fn review_agent_service(state: &AppState) -> ReviewAgentService<gateway_store::AnyStore> {
    ReviewAgentService::new(
        state.store.clone(),
        (*state.client_config_gateway_base_url).clone(),
    )
}

async fn authenticate_action(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<gateway_core::AuthenticatedApiKey, AppError> {
    let authorization = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    state
        .service
        .authenticate(authorization)
        .await
        .map_err(Into::into)
}

enum ReviewAgentAdminScope {
    Platform,
    Team(Uuid),
}

async fn require_review_agent_admin_scope(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<ReviewAgentAdminScope, AppError> {
    let actor = require_authenticated_session(state, headers).await?;
    if actor.status != UserStatus::Active {
        return Err(insufficient_privileges());
    }
    if actor.global_role == GlobalRole::PlatformAdmin {
        return Ok(ReviewAgentAdminScope::Platform);
    }
    let membership = state
        .store
        .get_team_membership_for_user(actor.user_id)
        .await?
        .ok_or_else(insufficient_privileges)?;
    if !matches!(
        membership.role,
        MembershipRole::Owner | MembershipRole::Admin
    ) {
        return Err(insufficient_privileges());
    }
    Ok(ReviewAgentAdminScope::Team(membership.team_id))
}

async fn authorize_service_account_scope(
    state: &AppState,
    scope: &ReviewAgentAdminScope,
    service_account_id: Uuid,
) -> Result<(), AppError> {
    let ReviewAgentAdminScope::Team(team_id) = scope else {
        return Ok(());
    };
    let service_account = state
        .store
        .get_service_account_by_id(service_account_id)
        .await?
        .ok_or_else(insufficient_privileges)?;
    if service_account.team_id != *team_id {
        return Err(insufficient_privileges());
    }
    Ok(())
}

async fn require_visible_repository(
    state: &AppState,
    scope: &ReviewAgentAdminScope,
    repository_id: Uuid,
) -> Result<ReviewAgentRepositoryRecord, AppError> {
    let repository = state
        .store
        .get_review_agent_repository(repository_id)
        .await?
        .ok_or_else(|| {
            StoreError::NotFound(format!("review agent repository `{repository_id}`"))
        })?;
    authorize_service_account_scope(state, scope, repository.service_account_id).await?;
    Ok(repository)
}

fn map_config_resolve_request(
    request: ActionConfigResolveRequest,
) -> Result<ActionConfigResolveInput, AppError> {
    Ok(ActionConfigResolveInput {
        event_name: request.event_name,
        repository: map_action_repository(request.repository)?,
        pull_request: map_action_pull_request(request.pull_request),
        overrides: map_overrides(request.overrides),
    })
}

fn map_run_start_request(request: ActionRunStartRequest) -> Result<ActionRunStartInput, AppError> {
    Ok(ActionRunStartInput {
        event_name: request.event_name,
        repository: map_action_repository(request.repository)?,
        pull_request: map_action_pull_request(request.pull_request),
        github_run_id: request.github_run_id,
        github_run_attempt: request.github_run_attempt,
        model_execution_mode: request.model_execution_mode,
        provider_key: request.provider_key,
        model_key: request.model_key,
        effective_config_json: sanitize_effective_config_json(request.effective_config_json)?,
    })
}

fn map_action_repository(
    request: ActionRepositoryIdentityRequest,
) -> Result<ActionRepositoryIdentity, AppError> {
    Ok(ActionRepositoryIdentity {
        provider: parse_provider(&request.provider)?,
        external_repository_id: request.external_repository_id,
        owner: request.owner,
        name: request.name,
        full_name: request.full_name,
    })
}

fn map_action_pull_request(request: ActionPullRequestRequest) -> ActionPullRequestInput {
    ActionPullRequestInput {
        provider_pr_id: request.provider_pr_id,
        pr_number: request.pr_number,
        title: request.title,
        author_login: request.author_login,
        head_sha: request.head_sha,
        base_sha: request.base_sha,
        head_repository_full_name: request.head_repository_full_name,
        base_repository_full_name: request.base_repository_full_name,
        is_draft: request.is_draft,
    }
}

fn map_overrides(request: ActionConfigOverridesRequest) -> ReviewAgentConfigOverrides {
    ReviewAgentConfigOverrides {
        model_id: request.model_id,
        model_execution_mode: request.model_execution_mode,
        provider_key: request.provider_key,
        inline_review_enabled: request.inline_review_enabled,
        pr_summary_enabled: request.pr_summary_enabled,
        diagrams_enabled: request.diagrams_enabled,
        linked_issue_detection_enabled: request.linked_issue_detection_enabled,
        linked_issue_assessment_enabled: request.linked_issue_assessment_enabled,
        max_inline_comments: request.max_inline_comments,
        request_changes_on_high_severity: request.request_changes_on_high_severity,
    }
}

fn map_complete_request(
    request: ActionRunMetricsRequest,
) -> Result<ActionRunCompleteInput, AppError> {
    Ok(ActionRunCompleteInput {
        status: request
            .status
            .as_deref()
            .map(parse_run_status)
            .transpose()?,
        duration_ms: request.duration_ms,
        files_changed: request.files_changed,
        additions: request.additions,
        deletions: request.deletions,
        changed_loc: request.changed_loc,
        inline_comments_created: request.inline_comments_created,
        inline_comments_updated: request.inline_comments_updated,
        inline_comments_skipped: request.inline_comments_skipped,
        inline_comments_failed: request.inline_comments_failed,
        stale_comments_deleted: request.stale_comments_deleted,
        managed_comment_id: request.managed_comment_id,
        managed_comment_action: request.managed_comment_action,
        managed_comment_status: request.managed_comment_status,
        review_event_status: request.review_event_status,
        summary_status: request.summary_status,
        diagram_status: request.diagram_status,
        linked_issue_count: request.linked_issue_count,
        linked_issue_status: request.linked_issue_status,
        degraded_features_json: request
            .degraded_features_json
            .map(sanitize_degraded_features_json)
            .transpose()?,
    })
}

fn sanitize_effective_config_json(value: Value) -> Result<Value, AppError> {
    let Value::Object(map) = value else {
        return invalid_json_blob("effective_config_json must be an object");
    };

    let mut sanitized = Map::new();
    for (key, value) in map {
        let clean = match key.as_str() {
            "model_id" | "model_execution_mode" | "provider_key" => {
                optional_string_value(&key, value, 200)?
            }
            "oceans_base_url" => optional_string_value(&key, value, 2048)?,
            "inline_review_enabled"
            | "pr_summary_enabled"
            | "diagrams_enabled"
            | "linked_issue_detection_enabled"
            | "linked_issue_assessment_enabled"
            | "request_changes_on_high_severity" => optional_bool_value(&key, value)?,
            "max_inline_comments" => optional_non_negative_i64_value(&key, value)?,
            _ => {
                return invalid_json_blob(&format!(
                    "effective_config_json contains unsupported field `{key}`"
                ));
            }
        };
        sanitized.insert(key, clean);
    }
    Ok(Value::Object(sanitized))
}

fn sanitize_degraded_features_json(value: Value) -> Result<Value, AppError> {
    let Value::Array(items) = value else {
        return invalid_json_blob("degraded_features_json must be an array of strings");
    };
    if items.len() > 20 {
        return invalid_json_blob("degraded_features_json cannot contain more than 20 items");
    }
    let mut sanitized = Vec::with_capacity(items.len());
    for item in items {
        let Value::String(feature) = item else {
            return invalid_json_blob("degraded_features_json must contain only strings");
        };
        if feature.len() > 100 || feature.chars().any(char::is_control) {
            return invalid_json_blob(
                "degraded_features_json entries must be single-line strings up to 100 bytes",
            );
        }
        sanitized.push(Value::String(feature));
    }
    Ok(Value::Array(sanitized))
}

fn validate_settings_json(value: Option<Value>) -> Result<Value, AppError> {
    let Some(value) = value else {
        return Ok(json!({}));
    };
    let Value::Object(map) = &value else {
        return invalid_json_blob("settings_json must be an object");
    };
    const TYPED_SETTINGS: &[&str] = &[
        "inline_review_enabled",
        "pr_summary_enabled",
        "diagrams_enabled",
        "linked_issue_detection_enabled",
        "linked_issue_assessment_enabled",
        "default_model_key",
        "max_inline_comments",
        "request_changes_on_high_severity",
    ];
    if let Some(key) = TYPED_SETTINGS.iter().find(|key| map.contains_key(**key)) {
        return invalid_json_blob(&format!(
            "settings_json cannot override typed setting `{key}`"
        ));
    }
    Ok(value)
}

fn optional_string_value(field: &str, value: Value, max_len: usize) -> Result<Value, AppError> {
    match value {
        Value::Null => Ok(Value::Null),
        Value::String(text) if text.len() <= max_len && !text.chars().any(char::is_control) => {
            Ok(Value::String(text))
        }
        _ => invalid_json_blob(&format!(
            "{field} must be a bounded single-line string or null"
        )),
    }
}

fn optional_bool_value(field: &str, value: Value) -> Result<Value, AppError> {
    match value {
        Value::Null | Value::Bool(_) => Ok(value),
        _ => invalid_json_blob(&format!("{field} must be a boolean or null")),
    }
}

fn optional_non_negative_i64_value(field: &str, value: Value) -> Result<Value, AppError> {
    match value {
        Value::Null => Ok(Value::Null),
        Value::Number(number) if number.as_i64().is_some_and(|value| value >= 0) => {
            Ok(Value::Number(number))
        }
        _ => invalid_json_blob(&format!("{field} must be a non-negative integer or null")),
    }
}

fn invalid_json_blob<T>(message: &str) -> Result<T, AppError> {
    Err(AppError(GatewayError::UnprocessableEntity(
        message.to_string(),
    )))
}

fn parse_uuid(raw: &str, field: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|error| {
        AppError(GatewayError::InvalidRequest(format!(
            "{field} must be a UUID: {error}"
        )))
    })
}

fn parse_provider(raw: &str) -> Result<ReviewAgentProvider, AppError> {
    ReviewAgentProvider::from_db(raw).ok_or_else(|| {
        AppError(GatewayError::UnprocessableEntity(format!(
            "unsupported review agent provider `{raw}`"
        )))
    })
}

fn parse_repository_status(raw: &str) -> Result<ReviewAgentRepositoryStatus, AppError> {
    ReviewAgentRepositoryStatus::from_db(raw).ok_or_else(|| {
        AppError(GatewayError::UnprocessableEntity(format!(
            "unsupported review agent repository status `{raw}`"
        )))
    })
}

fn parse_run_status(raw: &str) -> Result<ReviewAgentRunStatus, AppError> {
    ReviewAgentRunStatus::from_db(raw).ok_or_else(|| {
        AppError(GatewayError::UnprocessableEntity(format!(
            "unsupported review agent run status `{raw}`"
        )))
    })
}

fn settings_from_view(view: ReviewAgentSettingsView) -> Result<ReviewAgentSettings, AppError> {
    if view.max_inline_comments.is_some_and(|value| value < 0) {
        return Err(AppError(GatewayError::UnprocessableEntity(
            "max_inline_comments must be non-negative".to_string(),
        )));
    }
    Ok(ReviewAgentSettings {
        inline_review_enabled: view.inline_review_enabled,
        pr_summary_enabled: view.pr_summary_enabled,
        diagrams_enabled: view.diagrams_enabled,
        linked_issue_detection_enabled: view.linked_issue_detection_enabled,
        linked_issue_assessment_enabled: view.linked_issue_assessment_enabled,
        default_model_key: view.default_model_key,
        max_inline_comments: view.max_inline_comments,
        request_changes_on_high_severity: view.request_changes_on_high_severity,
    })
}

fn settings_to_view(settings: ReviewAgentSettings) -> ReviewAgentSettingsView {
    ReviewAgentSettingsView {
        inline_review_enabled: settings.inline_review_enabled,
        pr_summary_enabled: settings.pr_summary_enabled,
        diagrams_enabled: settings.diagrams_enabled,
        linked_issue_detection_enabled: settings.linked_issue_detection_enabled,
        linked_issue_assessment_enabled: settings.linked_issue_assessment_enabled,
        default_model_key: settings.default_model_key,
        max_inline_comments: settings.max_inline_comments,
        request_changes_on_high_severity: settings.request_changes_on_high_severity,
    }
}

fn map_repository(record: ReviewAgentRepositoryRecord) -> ReviewAgentRepositoryView {
    ReviewAgentRepositoryView {
        id: record.repository_id.to_string(),
        provider: record.provider.as_str().to_string(),
        external_repository_id: record.external_repository_id,
        owner: record.owner,
        name: record.name,
        full_name: record.full_name,
        service_account_id: record.service_account_id.to_string(),
        status: record.status.as_str().to_string(),
        settings: settings_to_view(record.settings),
        settings_json: record.settings_json,
        created_at: format_timestamp(record.created_at),
        updated_at: format_timestamp(record.updated_at),
    }
}

fn map_run(record: ReviewAgentRunRecord) -> ReviewAgentRunView {
    ReviewAgentRunView {
        id: record.run_id.to_string(),
        repository_id: record.repository_id.to_string(),
        pull_request_id: record.pull_request_id.map(|value| value.to_string()),
        head_sha: record.head_sha,
        github_run_id: record.github_run_id,
        github_run_attempt: record.github_run_attempt,
        status: record.status.as_str().to_string(),
        started_at: record.started_at.map(format_timestamp),
        heartbeat_at: record.heartbeat_at.map(format_timestamp),
        finished_at: record.finished_at.map(format_timestamp),
        duration_ms: record.duration_ms,
        files_changed: record.files_changed,
        additions: record.additions,
        deletions: record.deletions,
        changed_loc: record.changed_loc,
        inline_comments_created: record.inline_comments_created,
        inline_comments_updated: record.inline_comments_updated,
        inline_comments_skipped: record.inline_comments_skipped,
        inline_comments_failed: record.inline_comments_failed,
        stale_comments_deleted: record.stale_comments_deleted,
        managed_comment_id: record.managed_comment_id,
        managed_comment_action: record.managed_comment_action,
        managed_comment_status: record.managed_comment_status,
        review_event_status: record.review_event_status,
        summary_status: record.summary_status,
        diagram_status: record.diagram_status,
        linked_issue_count: record.linked_issue_count,
        linked_issue_status: record.linked_issue_status,
        model_execution_mode: record.model_execution_mode,
        provider_key: record.provider_key,
        model_key: record.model_key,
        effective_config_json: record.effective_config_json,
        degraded_features_json: record.degraded_features_json,
        error_summary: record.error_summary,
        created_at: format_timestamp(record.created_at),
        updated_at: format_timestamp(record.updated_at),
    }
}

fn insufficient_privileges() -> AppError {
    AppError(GatewayError::Auth(AuthError::InsufficientPrivileges))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_config_request_rejects_unknown_raw_fields() {
        let request = serde_json::json!({
            "event_name": "pull_request",
            "repository": {
                "provider": "github",
                "owner": "octo",
                "name": "repo",
                "full_name": "octo/repo",
                "raw_diff": "must not be accepted"
            },
            "pull_request": {
                "pr_number": 42,
                "head_repository_full_name": "octo/repo",
                "base_repository_full_name": "octo/repo",
                "is_draft": false
            },
            "overrides": {}
        });

        let error = serde_json::from_value::<ActionConfigResolveRequest>(request)
            .expect_err("unknown fields should be rejected");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn action_run_metrics_rejects_prompt_or_transcript_fields() {
        let request = serde_json::json!({
            "duration_ms": 10,
            "prompt": "must not be accepted",
            "model_transcript": "must not be accepted"
        });

        let error = serde_json::from_value::<ActionRunMetricsRequest>(request)
            .expect_err("unknown metric fields should be rejected");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn run_start_rejects_unbounded_effective_config_json() {
        assert!(
            sanitize_effective_config_json(json!({
                "model_id": "fast",
                "inline_review_enabled": true,
                "max_inline_comments": 10
            }))
            .is_ok()
        );
        assert!(
            sanitize_effective_config_json(json!({
                "model_id": "fast",
                "raw_diff": "do not persist"
            }))
            .is_err()
        );
        assert!(sanitize_effective_config_json(json!({"max_inline_comments": -1})).is_err());
    }

    #[test]
    fn run_metrics_rejects_unbounded_degraded_features_json() {
        assert!(sanitize_degraded_features_json(json!(["summary", "diagrams"])).is_ok());
        assert!(sanitize_degraded_features_json(json!({"raw": "object"})).is_err());
        assert!(sanitize_degraded_features_json(json!(["line\nbreak"])).is_err());
    }

    #[test]
    fn admin_settings_reject_negative_limits_and_shadow_json() {
        let settings = ReviewAgentSettingsView {
            inline_review_enabled: true,
            pr_summary_enabled: true,
            diagrams_enabled: false,
            linked_issue_detection_enabled: true,
            linked_issue_assessment_enabled: false,
            default_model_key: None,
            max_inline_comments: Some(-1),
            request_changes_on_high_severity: false,
        };
        assert!(settings_from_view(settings).is_err());
        assert!(validate_settings_json(Some(json!({"notes": "enabled"}))).is_ok());
        assert!(validate_settings_json(Some(json!({"max_inline_comments": 3}))).is_err());
    }

    #[test]
    fn invalid_provider_and_status_are_422_validation_errors() {
        assert_eq!(
            parse_provider("gitlab").unwrap_err().0.http_status_code(),
            422
        );
        assert_eq!(
            parse_repository_status("paused")
                .unwrap_err()
                .0
                .http_status_code(),
            422
        );
        assert_eq!(
            parse_run_status("waiting")
                .unwrap_err()
                .0
                .http_status_code(),
            422
        );
    }
}
