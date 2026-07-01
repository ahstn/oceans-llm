use std::sync::Arc;

use gateway_core::{
    AuthError, AuthenticatedApiKey, GatewayError, IdentityRepository, ModelRepository,
    NewReviewAgentRepositoryRecord, NewReviewAgentRunRecord, ReviewAgentProvider,
    ReviewAgentPullRequestRecord, ReviewAgentPullRequestState, ReviewAgentRepository,
    ReviewAgentRepositoryRecord, ReviewAgentRepositoryStatus, ReviewAgentRunRecord,
    ReviewAgentRunStatus, ReviewAgentSettings, StoreError, UpdateReviewAgentRepositoryRecord,
    UpdateReviewAgentRunRecord, UpsertReviewAgentPullRequestRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone)]
pub struct ReviewAgentService<S> {
    store: Arc<S>,
    oceans_base_url: Option<String>,
}

impl<S> ReviewAgentService<S>
where
    S: ReviewAgentRepository + IdentityRepository + ModelRepository + Send + Sync,
{
    #[must_use]
    pub fn new(store: Arc<S>, oceans_base_url: Option<String>) -> Self {
        Self {
            store,
            oceans_base_url,
        }
    }

    pub async fn list_repositories(
        &self,
        status: Option<ReviewAgentRepositoryStatus>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ReviewAgentRepositoryRecord>, GatewayError> {
        self.store
            .list_review_agent_repositories(status, limit.clamp(1, 100), offset.max(0))
            .await
            .map_err(Into::into)
    }

    pub async fn list_repositories_for_team(
        &self,
        team_id: Uuid,
        status: Option<ReviewAgentRepositoryStatus>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ReviewAgentRepositoryRecord>, GatewayError> {
        self.store
            .list_review_agent_repositories_for_team(
                team_id,
                status,
                limit.clamp(1, 100),
                offset.max(0),
            )
            .await
            .map_err(Into::into)
    }

    pub async fn create_repository(
        &self,
        input: CreateReviewAgentRepositoryInput,
    ) -> Result<ReviewAgentRepositoryRecord, GatewayError> {
        self.validate_service_account(input.service_account_id)
            .await?;
        self.store
            .create_review_agent_repository(&NewReviewAgentRepositoryRecord {
                provider: input.provider,
                external_repository_id: input.external_repository_id,
                owner: input.owner,
                name: input.name,
                full_name: input.full_name,
                service_account_id: input.service_account_id,
                settings: input.settings,
                settings_json: input.settings_json,
                created_at: OffsetDateTime::now_utc(),
            })
            .await
            .map_err(Into::into)
    }

    pub async fn update_repository(
        &self,
        input: UpdateReviewAgentRepositoryInput,
    ) -> Result<ReviewAgentRepositoryRecord, GatewayError> {
        self.validate_service_account(input.service_account_id)
            .await?;
        self.store
            .update_review_agent_repository(&UpdateReviewAgentRepositoryRecord {
                repository_id: input.repository_id,
                external_repository_id: input.external_repository_id,
                owner: input.owner,
                name: input.name,
                full_name: input.full_name,
                service_account_id: input.service_account_id,
                status: input.status,
                settings: input.settings,
                settings_json: input.settings_json,
                updated_at: OffsetDateTime::now_utc(),
            })
            .await
            .map_err(Into::into)
    }

    pub async fn set_repository_status(
        &self,
        repository_id: Uuid,
        status: ReviewAgentRepositoryStatus,
    ) -> Result<ReviewAgentRepositoryRecord, GatewayError> {
        self.store
            .set_review_agent_repository_status(repository_id, status, OffsetDateTime::now_utc())
            .await
            .map_err(Into::into)
    }

    pub async fn list_runs(
        &self,
        repository_id: Uuid,
        pr_number: Option<i64>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ReviewAgentRunRecord>, GatewayError> {
        self.store
            .list_review_agent_runs_for_repository(
                repository_id,
                pr_number,
                limit.clamp(1, 100),
                offset.max(0),
            )
            .await
            .map_err(Into::into)
    }

    pub async fn render_workflow(
        &self,
        repository_id: Uuid,
        input: WorkflowRenderInput,
    ) -> Result<RenderedWorkflow, GatewayError> {
        let repository = self
            .store
            .get_review_agent_repository(repository_id)
            .await?
            .ok_or_else(|| StoreError::NotFound(format!("repository `{repository_id}`")))?;

        let action_ref = input
            .action_ref
            .unwrap_or_else(|| "main".to_string())
            .trim()
            .to_string();
        validate_action_ref(&action_ref)?;
        let oceans_url = input
            .oceans_url
            .or_else(|| self.oceans_base_url.clone())
            .unwrap_or_else(|| "https://oceans.example.com".to_string());
        validate_workflow_url(&oceans_url)?;
        let api_key_secret_name = input
            .api_key_secret_name
            .unwrap_or_else(|| "OCEANS_REVIEW_AGENT_API_KEY".to_string());
        validate_secret_name(&api_key_secret_name)?;

        Ok(RenderedWorkflow {
            file_name: "oceans-review-agent.yml".to_string(),
            yaml: render_workflow_yaml(
                &repository.full_name,
                &action_ref,
                &oceans_url,
                &api_key_secret_name,
            ),
            action_ref,
            oceans_url,
            api_key_secret_name,
        })
    }

    pub async fn resolve_config(
        &self,
        auth: &AuthenticatedApiKey,
        input: ActionConfigResolveInput,
    ) -> Result<ActionConfigResolveOutput, GatewayError> {
        let service_account_id = require_service_account_auth(auth)?;
        let repository = self.resolve_action_repository(&input.repository).await?;
        self.require_action_repo_access(&repository, service_account_id)?;
        validate_repository_active(&repository)?;
        validate_pull_request_safety(&input.event_name, &input.pull_request)?;

        let pull_request = self
            .store
            .upsert_review_agent_pull_request(&pull_request_upsert(
                repository.repository_id,
                &input.pull_request,
            ))
            .await?;

        let (effective_config, overrides_applied, overrides_rejected) = resolve_effective_config(
            &repository.settings,
            &input.overrides,
            self.oceans_base_url.clone(),
        )
        .await?;

        Ok(ActionConfigResolveOutput {
            repository,
            pull_request,
            effective_config,
            overrides_applied,
            overrides_rejected,
            reporting: ActionReportingHints {
                run_start_url: "/api/v1/review-agent/action/runs".to_string(),
            },
        })
    }

    pub async fn start_run(
        &self,
        auth: &AuthenticatedApiKey,
        input: ActionRunStartInput,
    ) -> Result<ReviewAgentRunRecord, GatewayError> {
        let service_account_id = require_service_account_auth(auth)?;
        let repository = self.resolve_action_repository(&input.repository).await?;
        self.require_action_repo_access(&repository, service_account_id)?;
        validate_repository_active(&repository)?;
        validate_pull_request_safety(&input.event_name, &input.pull_request)?;

        let pull_request = self
            .store
            .upsert_review_agent_pull_request(&pull_request_upsert(
                repository.repository_id,
                &input.pull_request,
            ))
            .await?;
        let now = OffsetDateTime::now_utc();
        self.store
            .start_review_agent_run(&NewReviewAgentRunRecord {
                repository_id: repository.repository_id,
                pull_request_id: Some(pull_request.pull_request_id),
                head_sha: input.pull_request.head_sha,
                github_run_id: input.github_run_id,
                github_run_attempt: input.github_run_attempt,
                status: ReviewAgentRunStatus::InProgress,
                started_at: Some(now),
                model_execution_mode: input.model_execution_mode,
                provider_key: input.provider_key,
                model_key: input.model_key,
                effective_config_json: input.effective_config_json,
                created_at: now,
            })
            .await
            .map_err(Into::into)
    }

    pub async fn heartbeat_run(
        &self,
        auth: &AuthenticatedApiKey,
        run_id: Uuid,
        input: ActionRunHeartbeatInput,
    ) -> Result<ReviewAgentRunRecord, GatewayError> {
        let service_account_id = require_service_account_auth(auth)?;
        let run = self
            .require_action_run_access(run_id, service_account_id)
            .await?;
        let status = input.status.unwrap_or(run.status);
        validate_heartbeat_status(status)?;
        self.store
            .update_review_agent_run(&UpdateReviewAgentRunRecord {
                run_id: run.run_id,
                status,
                heartbeat_at: Some(OffsetDateTime::now_utc()),
                finished_at: run.finished_at,
                duration_ms: run.duration_ms,
                files_changed: run.files_changed,
                additions: run.additions,
                deletions: run.deletions,
                changed_loc: run.changed_loc,
                inline_comments_created: run.inline_comments_created,
                inline_comments_updated: run.inline_comments_updated,
                inline_comments_skipped: run.inline_comments_skipped,
                inline_comments_failed: run.inline_comments_failed,
                stale_comments_deleted: run.stale_comments_deleted,
                managed_comment_id: run.managed_comment_id,
                managed_comment_action: run.managed_comment_action,
                managed_comment_status: run.managed_comment_status,
                review_event_status: run.review_event_status,
                summary_status: run.summary_status,
                diagram_status: run.diagram_status,
                linked_issue_count: run.linked_issue_count,
                linked_issue_status: run.linked_issue_status,
                degraded_features_json: run.degraded_features_json,
                error_summary: run.error_summary,
                updated_at: OffsetDateTime::now_utc(),
            })
            .await
            .map_err(Into::into)
    }

    pub async fn complete_run(
        &self,
        auth: &AuthenticatedApiKey,
        run_id: Uuid,
        input: ActionRunCompleteInput,
    ) -> Result<ReviewAgentRunRecord, GatewayError> {
        let service_account_id = require_service_account_auth(auth)?;
        self.require_action_run_access(run_id, service_account_id)
            .await?;
        let status = input.status.unwrap_or(ReviewAgentRunStatus::Succeeded);
        validate_complete_status(status)?;
        let now = OffsetDateTime::now_utc();
        self.store
            .update_review_agent_run(&UpdateReviewAgentRunRecord {
                run_id,
                status,
                heartbeat_at: Some(now),
                finished_at: Some(now),
                duration_ms: input.duration_ms,
                files_changed: input.files_changed,
                additions: input.additions,
                deletions: input.deletions,
                changed_loc: input.changed_loc,
                inline_comments_created: input.inline_comments_created,
                inline_comments_updated: input.inline_comments_updated,
                inline_comments_skipped: input.inline_comments_skipped,
                inline_comments_failed: input.inline_comments_failed,
                stale_comments_deleted: input.stale_comments_deleted,
                managed_comment_id: input.managed_comment_id,
                managed_comment_action: input.managed_comment_action,
                managed_comment_status: input.managed_comment_status,
                review_event_status: input.review_event_status,
                summary_status: input.summary_status,
                diagram_status: input.diagram_status,
                linked_issue_count: input.linked_issue_count,
                linked_issue_status: input.linked_issue_status,
                degraded_features_json: input.degraded_features_json,
                error_summary: None,
                updated_at: now,
            })
            .await
            .map_err(Into::into)
    }

    pub async fn fail_run(
        &self,
        auth: &AuthenticatedApiKey,
        run_id: Uuid,
        input: ActionRunFailInput,
    ) -> Result<ReviewAgentRunRecord, GatewayError> {
        let service_account_id = require_service_account_auth(auth)?;
        self.require_action_run_access(run_id, service_account_id)
            .await?;
        let status = input.metrics.status.unwrap_or(ReviewAgentRunStatus::Failed);
        validate_fail_status(status)?;
        let now = OffsetDateTime::now_utc();
        self.store
            .update_review_agent_run(&UpdateReviewAgentRunRecord {
                run_id,
                status,
                heartbeat_at: Some(now),
                finished_at: Some(now),
                duration_ms: input.metrics.duration_ms,
                files_changed: input.metrics.files_changed,
                additions: input.metrics.additions,
                deletions: input.metrics.deletions,
                changed_loc: input.metrics.changed_loc,
                inline_comments_created: input.metrics.inline_comments_created,
                inline_comments_updated: input.metrics.inline_comments_updated,
                inline_comments_skipped: input.metrics.inline_comments_skipped,
                inline_comments_failed: input.metrics.inline_comments_failed,
                stale_comments_deleted: input.metrics.stale_comments_deleted,
                managed_comment_id: input.metrics.managed_comment_id,
                managed_comment_action: input.metrics.managed_comment_action,
                managed_comment_status: input.metrics.managed_comment_status,
                review_event_status: input.metrics.review_event_status,
                summary_status: input.metrics.summary_status,
                diagram_status: input.metrics.diagram_status,
                linked_issue_count: input.metrics.linked_issue_count,
                linked_issue_status: input.metrics.linked_issue_status,
                degraded_features_json: input.metrics.degraded_features_json,
                error_summary: Some(sanitize_error_summary(&input.error_summary)),
                updated_at: now,
            })
            .await
            .map_err(Into::into)
    }

    async fn validate_service_account(&self, service_account_id: Uuid) -> Result<(), GatewayError> {
        let service_account = self
            .store
            .get_service_account_by_id(service_account_id)
            .await?
            .ok_or_else(|| {
                StoreError::NotFound(format!("service account `{service_account_id}`"))
            })?;
        if service_account.status.as_str() != "active" {
            return Err(GatewayError::UnprocessableEntity(
                "review agent repositories require an active service account".to_string(),
            ));
        }
        Ok(())
    }

    async fn resolve_action_repository(
        &self,
        identity: &ActionRepositoryIdentity,
    ) -> Result<ReviewAgentRepositoryRecord, GatewayError> {
        self.store
            .get_review_agent_repository_by_identity(
                identity.provider,
                identity.external_repository_id.as_deref(),
                &identity.owner,
                &identity.name,
            )
            .await?
            .ok_or_else(|| {
                StoreError::NotFound(format!(
                    "review agent repository `{}/{}` was not found",
                    identity.owner, identity.name
                ))
                .into()
            })
            .and_then(|repository| {
                if repository.full_name != identity.full_name {
                    Err(GatewayError::Auth(AuthError::InsufficientPrivileges))
                } else {
                    Ok(repository)
                }
            })
    }

    fn require_action_repo_access(
        &self,
        repository: &ReviewAgentRepositoryRecord,
        service_account_id: Uuid,
    ) -> Result<(), GatewayError> {
        if repository.service_account_id != service_account_id {
            return Err(GatewayError::Auth(AuthError::InsufficientPrivileges));
        }
        Ok(())
    }

    async fn require_action_run_access(
        &self,
        run_id: Uuid,
        service_account_id: Uuid,
    ) -> Result<ReviewAgentRunRecord, GatewayError> {
        let run = self
            .store
            .get_review_agent_run(run_id)
            .await?
            .ok_or_else(|| StoreError::NotFound(format!("review agent run `{run_id}`")))?;
        let repository = self
            .store
            .get_review_agent_repository(run.repository_id)
            .await?
            .ok_or_else(|| {
                StoreError::NotFound(format!("review agent repository `{}`", run.repository_id))
            })?;
        self.require_action_repo_access(&repository, service_account_id)?;
        Ok(run)
    }
}

#[derive(Debug, Clone)]
pub struct CreateReviewAgentRepositoryInput {
    pub provider: ReviewAgentProvider,
    pub external_repository_id: Option<String>,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub service_account_id: Uuid,
    pub settings: ReviewAgentSettings,
    pub settings_json: Value,
}

#[derive(Debug, Clone)]
pub struct UpdateReviewAgentRepositoryInput {
    pub repository_id: Uuid,
    pub external_repository_id: Option<String>,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub service_account_id: Uuid,
    pub status: ReviewAgentRepositoryStatus,
    pub settings: ReviewAgentSettings,
    pub settings_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRepositoryIdentity {
    pub provider: ReviewAgentProvider,
    pub external_repository_id: Option<String>,
    pub owner: String,
    pub name: String,
    pub full_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPullRequestInput {
    pub provider_pr_id: Option<String>,
    pub pr_number: i64,
    pub title: Option<String>,
    pub author_login: Option<String>,
    pub head_sha: Option<String>,
    pub base_sha: Option<String>,
    pub head_repository_full_name: String,
    pub base_repository_full_name: String,
    pub is_draft: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewAgentConfigOverrides {
    pub model_id: Option<String>,
    pub model_execution_mode: Option<String>,
    pub provider_key: Option<String>,
    pub inline_review_enabled: Option<bool>,
    pub pr_summary_enabled: Option<bool>,
    pub diagrams_enabled: Option<bool>,
    pub linked_issue_detection_enabled: Option<bool>,
    pub linked_issue_assessment_enabled: Option<bool>,
    pub max_inline_comments: Option<i64>,
    pub request_changes_on_high_severity: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionConfigResolveInput {
    pub event_name: String,
    pub repository: ActionRepositoryIdentity,
    pub pull_request: ActionPullRequestInput,
    pub overrides: ReviewAgentConfigOverrides,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionConfigResolveOutput {
    pub repository: ReviewAgentRepositoryRecord,
    pub pull_request: ReviewAgentPullRequestRecord,
    pub effective_config: EffectiveReviewAgentConfig,
    pub overrides_applied: Vec<OverrideDiagnostic>,
    pub overrides_rejected: Vec<OverrideDiagnostic>,
    pub reporting: ActionReportingHints,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveReviewAgentConfig {
    pub model_id: Option<String>,
    pub model_execution_mode: String,
    pub provider_key: Option<String>,
    pub oceans_base_url: Option<String>,
    pub inline_review_enabled: bool,
    pub pr_summary_enabled: bool,
    pub diagrams_enabled: bool,
    pub linked_issue_detection_enabled: bool,
    pub linked_issue_assessment_enabled: bool,
    pub max_inline_comments: Option<i64>,
    pub request_changes_on_high_severity: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideDiagnostic {
    pub field: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionReportingHints {
    pub run_start_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRunStartInput {
    pub event_name: String,
    pub repository: ActionRepositoryIdentity,
    pub pull_request: ActionPullRequestInput,
    pub github_run_id: Option<String>,
    pub github_run_attempt: Option<i64>,
    pub model_execution_mode: Option<String>,
    pub provider_key: Option<String>,
    pub model_key: Option<String>,
    pub effective_config_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRunHeartbeatInput {
    pub status: Option<ReviewAgentRunStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRunCompleteInput {
    pub status: Option<ReviewAgentRunStatus>,
    pub duration_ms: Option<i64>,
    pub files_changed: Option<i64>,
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
    pub changed_loc: Option<i64>,
    pub inline_comments_created: Option<i64>,
    pub inline_comments_updated: Option<i64>,
    pub inline_comments_skipped: Option<i64>,
    pub inline_comments_failed: Option<i64>,
    pub stale_comments_deleted: Option<i64>,
    pub managed_comment_id: Option<String>,
    pub managed_comment_action: Option<String>,
    pub managed_comment_status: Option<String>,
    pub review_event_status: Option<String>,
    pub summary_status: Option<String>,
    pub diagram_status: Option<String>,
    pub linked_issue_count: Option<i64>,
    pub linked_issue_status: Option<String>,
    pub degraded_features_json: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRunFailInput {
    pub error_summary: String,
    pub metrics: ActionRunCompleteInput,
}

#[derive(Debug, Clone)]
pub struct WorkflowRenderInput {
    pub action_ref: Option<String>,
    pub oceans_url: Option<String>,
    pub api_key_secret_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderedWorkflow {
    pub file_name: String,
    pub yaml: String,
    pub action_ref: String,
    pub oceans_url: String,
    pub api_key_secret_name: String,
}

fn require_service_account_auth(auth: &AuthenticatedApiKey) -> Result<Uuid, GatewayError> {
    auth.owner_service_account_id
        .filter(|_| auth.is_service_account_owned())
        .ok_or(GatewayError::Auth(AuthError::InsufficientPrivileges))
}

fn validate_repository_active(
    repository: &ReviewAgentRepositoryRecord,
) -> Result<(), GatewayError> {
    if repository.status != ReviewAgentRepositoryStatus::Active {
        return Err(StoreError::Conflict(format!(
            "review agent repository `{}` is not active",
            repository.repository_id
        ))
        .into());
    }
    Ok(())
}

fn validate_pull_request_safety(
    event_name: &str,
    pull_request: &ActionPullRequestInput,
) -> Result<(), GatewayError> {
    if event_name != "pull_request" {
        return Err(GatewayError::UnprocessableEntity(
            "review agent only supports pull_request events".to_string(),
        ));
    }
    if pull_request.is_draft {
        return Err(GatewayError::UnprocessableEntity(
            "draft pull requests are skipped by the review agent".to_string(),
        ));
    }
    if pull_request.head_repository_full_name != pull_request.base_repository_full_name {
        return Err(GatewayError::UnprocessableEntity(
            "fork pull requests are not supported by the review agent".to_string(),
        ));
    }
    if pull_request
        .head_sha
        .as_deref()
        .unwrap_or_default()
        .is_empty()
    {
        return Err(GatewayError::UnprocessableEntity(
            "pull request head_sha is required".to_string(),
        ));
    }
    Ok(())
}

async fn resolve_effective_config(
    defaults: &ReviewAgentSettings,
    overrides: &ReviewAgentConfigOverrides,
    oceans_base_url: Option<String>,
) -> Result<
    (
        EffectiveReviewAgentConfig,
        Vec<OverrideDiagnostic>,
        Vec<OverrideDiagnostic>,
    ),
    GatewayError,
> {
    let mut applied = Vec::new();
    let mut rejected = Vec::new();
    let model_execution_mode = overrides
        .model_execution_mode
        .clone()
        .unwrap_or_else(|| "oceans".to_string());
    if !matches!(model_execution_mode.as_str(), "oceans" | "direct") {
        rejected.push(OverrideDiagnostic {
            field: "model_execution_mode".to_string(),
            reason: "supported values are oceans or direct".to_string(),
        });
        return Err(GatewayError::UnprocessableEntity(
            "unsupported review agent model execution mode".to_string(),
        ));
    }
    if overrides.model_execution_mode.is_some() {
        applied.push(applied_override("model_execution_mode"));
    }
    let model_id = overrides
        .model_id
        .clone()
        .or_else(|| defaults.default_model_key.clone());
    if overrides.model_id.is_some() {
        applied.push(applied_override("model_id"));
    }
    if overrides.provider_key.is_some() {
        applied.push(applied_override("provider_key"));
    }
    if overrides.max_inline_comments.is_some() {
        if overrides.max_inline_comments.is_some_and(|value| value < 0) {
            rejected.push(OverrideDiagnostic {
                field: "max_inline_comments".to_string(),
                reason: "value must be non-negative".to_string(),
            });
            return Err(GatewayError::UnprocessableEntity(
                "max_inline_comments must be non-negative".to_string(),
            ));
        }
        applied.push(applied_override("max_inline_comments"));
    }

    let config = EffectiveReviewAgentConfig {
        model_id,
        model_execution_mode: model_execution_mode.clone(),
        provider_key: overrides.provider_key.clone(),
        oceans_base_url: if model_execution_mode == "oceans" {
            oceans_base_url
        } else {
            None
        },
        inline_review_enabled: override_bool(
            "inline_review_enabled",
            defaults.inline_review_enabled,
            overrides.inline_review_enabled,
            &mut applied,
        ),
        pr_summary_enabled: override_bool(
            "pr_summary_enabled",
            defaults.pr_summary_enabled,
            overrides.pr_summary_enabled,
            &mut applied,
        ),
        diagrams_enabled: override_bool(
            "diagrams_enabled",
            defaults.diagrams_enabled,
            overrides.diagrams_enabled,
            &mut applied,
        ),
        linked_issue_detection_enabled: override_bool(
            "linked_issue_detection_enabled",
            defaults.linked_issue_detection_enabled,
            overrides.linked_issue_detection_enabled,
            &mut applied,
        ),
        linked_issue_assessment_enabled: override_bool(
            "linked_issue_assessment_enabled",
            defaults.linked_issue_assessment_enabled,
            overrides.linked_issue_assessment_enabled,
            &mut applied,
        ),
        max_inline_comments: overrides
            .max_inline_comments
            .or(defaults.max_inline_comments),
        request_changes_on_high_severity: override_bool(
            "request_changes_on_high_severity",
            defaults.request_changes_on_high_severity,
            overrides.request_changes_on_high_severity,
            &mut applied,
        ),
    };

    Ok((config, applied, rejected))
}

fn override_bool(
    field: &str,
    default_value: bool,
    override_value: Option<bool>,
    applied: &mut Vec<OverrideDiagnostic>,
) -> bool {
    if let Some(value) = override_value {
        applied.push(applied_override(field));
        value
    } else {
        default_value
    }
}

fn applied_override(field: &str) -> OverrideDiagnostic {
    OverrideDiagnostic {
        field: field.to_string(),
        reason: "action input override applied".to_string(),
    }
}

fn pull_request_upsert(
    repository_id: Uuid,
    pull_request: &ActionPullRequestInput,
) -> UpsertReviewAgentPullRequestRecord {
    UpsertReviewAgentPullRequestRecord {
        repository_id,
        provider_pr_id: pull_request.provider_pr_id.clone(),
        pr_number: pull_request.pr_number,
        title: pull_request.title.clone(),
        author_login: pull_request.author_login.clone(),
        state: ReviewAgentPullRequestState::Open,
        head_sha: pull_request.head_sha.clone(),
        base_sha: pull_request.base_sha.clone(),
        head_repository_full_name: Some(pull_request.head_repository_full_name.clone()),
        base_repository_full_name: Some(pull_request.base_repository_full_name.clone()),
        is_draft: pull_request.is_draft,
        updated_at: OffsetDateTime::now_utc(),
    }
}

fn sanitize_error_summary(summary: &str) -> String {
    summary
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(500)
        .collect()
}

fn validate_action_ref(value: &str) -> Result<(), GatewayError> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/'))
    {
        return Err(GatewayError::UnprocessableEntity(
            "action_ref must be a non-empty Git ref containing only letters, numbers, '.', '_', '-' or '/'".to_string(),
        ));
    }
    Ok(())
}

fn validate_workflow_url(value: &str) -> Result<(), GatewayError> {
    let parsed = url::Url::parse(value).map_err(|error| {
        GatewayError::UnprocessableEntity(format!("oceans_url must be a valid URL: {error}"))
    })?;
    if !matches!(parsed.scheme(), "http" | "https")
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace() || ch == '\'')
    {
        return Err(GatewayError::UnprocessableEntity(
            "oceans_url must be a single-line http(s) URL".to_string(),
        ));
    }
    Ok(())
}

fn validate_secret_name(value: &str) -> Result<(), GatewayError> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(GatewayError::UnprocessableEntity(
            "api_key_secret_name cannot be empty".to_string(),
        ));
    };
    if !(first.is_ascii_alphabetic() || first == '_')
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(GatewayError::UnprocessableEntity(
            "api_key_secret_name must contain only letters, numbers and underscores, and cannot start with a number".to_string(),
        ));
    }
    Ok(())
}

fn validate_heartbeat_status(status: ReviewAgentRunStatus) -> Result<(), GatewayError> {
    if matches!(
        status,
        ReviewAgentRunStatus::Queued | ReviewAgentRunStatus::InProgress
    ) {
        return Ok(());
    }
    Err(GatewayError::UnprocessableEntity(
        "heartbeat status must be queued or in_progress".to_string(),
    ))
}

fn validate_complete_status(status: ReviewAgentRunStatus) -> Result<(), GatewayError> {
    if matches!(
        status,
        ReviewAgentRunStatus::Succeeded
            | ReviewAgentRunStatus::Skipped
            | ReviewAgentRunStatus::Cancelled
    ) {
        return Ok(());
    }
    Err(GatewayError::UnprocessableEntity(
        "complete status must be succeeded, skipped or cancelled".to_string(),
    ))
}

fn validate_fail_status(status: ReviewAgentRunStatus) -> Result<(), GatewayError> {
    if status == ReviewAgentRunStatus::Failed {
        return Ok(());
    }
    Err(GatewayError::UnprocessableEntity(
        "fail status must be failed".to_string(),
    ))
}

fn render_workflow_yaml(
    repo_full_name: &str,
    action_ref: &str,
    oceans_url: &str,
    api_key_secret_name: &str,
) -> String {
    format!(
        r#"name: Oceans Review Agent

# Repository: {repo_full_name}

on:
  pull_request:
    types: [opened, synchronize, reopened, ready_for_review]

permissions:
  contents: read
  pull-requests: write
  issues: write

concurrency:
  group: oceans-review-agent-${{{{ github.repository }}}}-${{{{ github.event.pull_request.number }}}}
  cancel-in-progress: true

jobs:
  review:
    if: ${{{{ github.event.pull_request.head.repo.full_name == github.repository && github.event.pull_request.draft == false }}}}
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          ref: ${{{{ github.event.pull_request.head.sha }}}}
          fetch-depth: 0
      - uses: ahstn/oceans-llm/actions/review-agent@{action_ref}
        with:
          oceans-url: {oceans_url}
          oceans-api-key: ${{{{ secrets.{api_key_secret_name} }}}}
          github-token: ${{{{ github.token }}}}
          # model-id: fast
          # diagrams: "false"
          # linked-issue-assessment: "false"
"#,
        action_ref = action_ref,
        oceans_url = oceans_url,
        api_key_secret_name = api_key_secret_name,
        repo_full_name = repo_full_name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull_request() -> ActionPullRequestInput {
        ActionPullRequestInput {
            provider_pr_id: Some("123".to_string()),
            pr_number: 7,
            title: Some("Review me".to_string()),
            author_login: Some("octocat".to_string()),
            head_sha: Some("abc123".to_string()),
            base_sha: Some("def456".to_string()),
            head_repository_full_name: "ahstn/oceans-llm".to_string(),
            base_repository_full_name: "ahstn/oceans-llm".to_string(),
            is_draft: false,
        }
    }

    #[test]
    fn workflow_yaml_uses_safe_pull_request_contract() {
        let yaml = render_workflow_yaml(
            "ahstn/oceans-llm",
            "main",
            "https://oceans.example.com",
            "OCEANS_REVIEW_AGENT_API_KEY",
        );

        assert!(yaml.contains("pull_request:"));
        assert!(!yaml.contains("pull_request_target"));
        assert!(
            yaml.contains("github.event.pull_request.head.repo.full_name == github.repository")
        );
        assert!(yaml.contains("github.event.pull_request.draft == false"));
        assert!(yaml.contains("permissions:"));
        assert!(yaml.contains("pull-requests: write"));
        assert!(yaml.contains("issues: write"));
        assert!(yaml.contains("actions/checkout@v4"));
        assert!(yaml.contains("ref: ${{ github.event.pull_request.head.sha }}"));
        assert!(yaml.contains("ahstn/oceans-llm/actions/review-agent@main"));
        assert!(yaml.contains("${{ secrets.OCEANS_REVIEW_AGENT_API_KEY }}"));
    }

    #[test]
    fn workflow_inputs_reject_yaml_injection_shapes() {
        assert!(validate_action_ref("feature/review-agent").is_ok());
        assert!(validate_action_ref("main\npermissions: write-all").is_err());
        assert!(validate_workflow_url("https://oceans.example.com").is_ok());
        assert!(validate_workflow_url("https://oceans.example.com'\nfoo: bar").is_err());
        assert!(validate_secret_name("OCEANS_REVIEW_AGENT_API_KEY").is_ok());
        assert!(validate_secret_name("OCEANS-REVIEW-AGENT-API-KEY").is_err());
    }

    #[test]
    fn lifecycle_statuses_are_endpoint_specific() {
        assert!(validate_heartbeat_status(ReviewAgentRunStatus::InProgress).is_ok());
        assert!(validate_heartbeat_status(ReviewAgentRunStatus::Succeeded).is_err());
        assert!(validate_complete_status(ReviewAgentRunStatus::Succeeded).is_ok());
        assert!(validate_complete_status(ReviewAgentRunStatus::InProgress).is_err());
        assert!(validate_fail_status(ReviewAgentRunStatus::Failed).is_ok());
        assert!(validate_fail_status(ReviewAgentRunStatus::Succeeded).is_err());
    }

    #[test]
    fn pull_request_safety_rejects_unsupported_events_forks_and_drafts() {
        let valid = pull_request();
        assert!(validate_pull_request_safety("pull_request", &valid).is_ok());

        assert!(validate_pull_request_safety("push", &valid).is_err());

        let mut fork = valid.clone();
        fork.head_repository_full_name = "someone/oceans-llm".to_string();
        assert!(validate_pull_request_safety("pull_request", &fork).is_err());

        let mut draft = valid;
        draft.is_draft = true;
        assert!(validate_pull_request_safety("pull_request", &draft).is_err());
    }

    #[tokio::test]
    async fn effective_config_applies_action_overrides_without_mutating_defaults() {
        let defaults = ReviewAgentSettings {
            default_model_key: Some("fast".to_string()),
            max_inline_comments: Some(20),
            ..ReviewAgentSettings::default()
        };
        let overrides = ReviewAgentConfigOverrides {
            model_id: Some("deep".to_string()),
            model_execution_mode: Some("direct".to_string()),
            inline_review_enabled: Some(false),
            max_inline_comments: Some(5),
            ..ReviewAgentConfigOverrides::default()
        };

        let (config, applied, rejected) = resolve_effective_config(
            &defaults,
            &overrides,
            Some("https://oceans.local".to_string()),
        )
        .await
        .expect("effective config");

        assert_eq!(config.model_id.as_deref(), Some("deep"));
        assert_eq!(config.model_execution_mode, "direct");
        assert_eq!(config.oceans_base_url, None);
        assert!(!config.inline_review_enabled);
        assert_eq!(config.max_inline_comments, Some(5));
        assert!(applied.iter().any(|item| item.field == "model_id"));
        assert!(
            applied
                .iter()
                .any(|item| item.field == "inline_review_enabled")
        );
        assert!(rejected.is_empty());
        assert_eq!(defaults.default_model_key.as_deref(), Some("fast"));
    }

    #[tokio::test]
    async fn effective_config_rejects_invalid_model_mode_and_negative_comment_limit() {
        let defaults = ReviewAgentSettings::default();
        let invalid_mode = ReviewAgentConfigOverrides {
            model_execution_mode: Some("shell".to_string()),
            ..ReviewAgentConfigOverrides::default()
        };
        assert!(
            resolve_effective_config(&defaults, &invalid_mode, None)
                .await
                .is_err()
        );

        let invalid_limit = ReviewAgentConfigOverrides {
            max_inline_comments: Some(-1),
            ..ReviewAgentConfigOverrides::default()
        };
        assert!(
            resolve_effective_config(&defaults, &invalid_limit, None)
                .await
                .is_err()
        );
    }

    #[test]
    fn error_summary_is_single_line_and_bounded() {
        let long = format!("{}\nsecret second line", "x".repeat(800));
        let sanitized = sanitize_error_summary(&long);
        assert_eq!(sanitized.len(), 500);
        assert!(!sanitized.contains("secret second line"));
    }
}
