use super::*;
use crate::shared::{parse_uuid, serialize_json, unix_to_datetime};

const REPOSITORY_COLUMNS: &str = "repository_id, provider, external_repository_id, owner, name, full_name, service_account_id, status, inline_review_enabled, pr_summary_enabled, diagrams_enabled, linked_issue_detection_enabled, linked_issue_assessment_enabled, default_model_key, max_inline_comments, request_changes_on_high_severity, settings_json, created_at, updated_at";
const PULL_REQUEST_COLUMNS: &str = "pull_request_id, repository_id, provider_pr_id, pr_number, title, author_login, state, head_sha, base_sha, head_repository_full_name, base_repository_full_name, is_draft, created_at, updated_at";
const RUN_COLUMNS: &str = "run_id, repository_id, pull_request_id, head_sha, github_run_id, github_run_attempt, status, started_at, heartbeat_at, finished_at, duration_ms, files_changed, additions, deletions, changed_loc, inline_comments_created, inline_comments_updated, inline_comments_skipped, inline_comments_failed, stale_comments_deleted, managed_comment_id, managed_comment_action, managed_comment_status, review_event_status, summary_status, diagram_status, linked_issue_count, linked_issue_status, model_execution_mode, provider_key, model_key, effective_config_json, degraded_features_json, error_summary, created_at, updated_at";
const ALIASED_RUN_COLUMNS: &str = "r.run_id, r.repository_id, r.pull_request_id, r.head_sha, r.github_run_id, r.github_run_attempt, r.status, r.started_at, r.heartbeat_at, r.finished_at, r.duration_ms, r.files_changed, r.additions, r.deletions, r.changed_loc, r.inline_comments_created, r.inline_comments_updated, r.inline_comments_skipped, r.inline_comments_failed, r.stale_comments_deleted, r.managed_comment_id, r.managed_comment_action, r.managed_comment_status, r.review_event_status, r.summary_status, r.diagram_status, r.linked_issue_count, r.linked_issue_status, r.model_execution_mode, r.provider_key, r.model_key, r.effective_config_json, r.degraded_features_json, r.error_summary, r.created_at, r.updated_at";

fn bool_from_i64(value: i64) -> bool {
    value == 1
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn decode_json_value(raw: &str) -> Result<serde_json::Value, StoreError> {
    serde_json::from_str(raw).map_err(|error| StoreError::Serialization(error.to_string()))
}

fn decode_optional_json_value(
    raw: Option<String>,
) -> Result<Option<serde_json::Value>, StoreError> {
    raw.as_deref().map(decode_json_value).transpose()
}

fn decode_repository(row: &libsql::Row) -> Result<ReviewAgentRepositoryRecord, StoreError> {
    let repository_id: String = row.get(0).map_err(to_query_error)?;
    let provider: String = row.get(1).map_err(to_query_error)?;
    let service_account_id: String = row.get(6).map_err(to_query_error)?;
    let status: String = row.get(7).map_err(to_query_error)?;
    let settings_json: String = row.get(16).map_err(to_query_error)?;
    let created_at: i64 = row.get(17).map_err(to_query_error)?;
    let updated_at: i64 = row.get(18).map_err(to_query_error)?;

    Ok(ReviewAgentRepositoryRecord {
        repository_id: parse_uuid(&repository_id)?,
        provider: ReviewAgentProvider::from_db(&provider).ok_or_else(|| {
            StoreError::Serialization(format!("invalid review agent provider `{provider}`"))
        })?,
        external_repository_id: row.get(2).map_err(to_query_error)?,
        owner: row.get(3).map_err(to_query_error)?,
        name: row.get(4).map_err(to_query_error)?,
        full_name: row.get(5).map_err(to_query_error)?,
        service_account_id: parse_uuid(&service_account_id)?,
        status: ReviewAgentRepositoryStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid review agent repository status `{status}`"))
        })?,
        settings: ReviewAgentSettings {
            inline_review_enabled: bool_from_i64(row.get(8).map_err(to_query_error)?),
            pr_summary_enabled: bool_from_i64(row.get(9).map_err(to_query_error)?),
            diagrams_enabled: bool_from_i64(row.get(10).map_err(to_query_error)?),
            linked_issue_detection_enabled: bool_from_i64(row.get(11).map_err(to_query_error)?),
            linked_issue_assessment_enabled: bool_from_i64(row.get(12).map_err(to_query_error)?),
            default_model_key: row.get(13).map_err(to_query_error)?,
            max_inline_comments: row.get(14).map_err(to_query_error)?,
            request_changes_on_high_severity: bool_from_i64(row.get(15).map_err(to_query_error)?),
        },
        settings_json: decode_json_value(&settings_json)?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_pull_request(row: &libsql::Row) -> Result<ReviewAgentPullRequestRecord, StoreError> {
    let pull_request_id: String = row.get(0).map_err(to_query_error)?;
    let repository_id: String = row.get(1).map_err(to_query_error)?;
    let state: String = row.get(6).map_err(to_query_error)?;
    let is_draft: i64 = row.get(11).map_err(to_query_error)?;
    let created_at: i64 = row.get(12).map_err(to_query_error)?;
    let updated_at: i64 = row.get(13).map_err(to_query_error)?;

    Ok(ReviewAgentPullRequestRecord {
        pull_request_id: parse_uuid(&pull_request_id)?,
        repository_id: parse_uuid(&repository_id)?,
        provider_pr_id: row.get(2).map_err(to_query_error)?,
        pr_number: row.get(3).map_err(to_query_error)?,
        title: row.get(4).map_err(to_query_error)?,
        author_login: row.get(5).map_err(to_query_error)?,
        state: ReviewAgentPullRequestState::from_db(&state).ok_or_else(|| {
            StoreError::Serialization(format!("invalid review agent PR state `{state}`"))
        })?,
        head_sha: row.get(7).map_err(to_query_error)?,
        base_sha: row.get(8).map_err(to_query_error)?,
        head_repository_full_name: row.get(9).map_err(to_query_error)?,
        base_repository_full_name: row.get(10).map_err(to_query_error)?,
        is_draft: bool_from_i64(is_draft),
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

fn decode_run(row: &libsql::Row) -> Result<ReviewAgentRunRecord, StoreError> {
    let run_id: String = row.get(0).map_err(to_query_error)?;
    let repository_id: String = row.get(1).map_err(to_query_error)?;
    let pull_request_id: Option<String> = row.get(2).map_err(to_query_error)?;
    let status: String = row.get(6).map_err(to_query_error)?;
    let started_at: Option<i64> = row.get(7).map_err(to_query_error)?;
    let heartbeat_at: Option<i64> = row.get(8).map_err(to_query_error)?;
    let finished_at: Option<i64> = row.get(9).map_err(to_query_error)?;
    let effective_config_json: String = row.get(31).map_err(to_query_error)?;
    let degraded_features_json: Option<String> = row.get(32).map_err(to_query_error)?;
    let created_at: i64 = row.get(34).map_err(to_query_error)?;
    let updated_at: i64 = row.get(35).map_err(to_query_error)?;

    Ok(ReviewAgentRunRecord {
        run_id: parse_uuid(&run_id)?,
        repository_id: parse_uuid(&repository_id)?,
        pull_request_id: pull_request_id.as_deref().map(parse_uuid).transpose()?,
        head_sha: row.get(3).map_err(to_query_error)?,
        github_run_id: row.get(4).map_err(to_query_error)?,
        github_run_attempt: row.get(5).map_err(to_query_error)?,
        status: ReviewAgentRunStatus::from_db(&status).ok_or_else(|| {
            StoreError::Serialization(format!("invalid review agent run status `{status}`"))
        })?,
        started_at: started_at.map(unix_to_datetime).transpose()?,
        heartbeat_at: heartbeat_at.map(unix_to_datetime).transpose()?,
        finished_at: finished_at.map(unix_to_datetime).transpose()?,
        duration_ms: row.get(10).map_err(to_query_error)?,
        files_changed: row.get(11).map_err(to_query_error)?,
        additions: row.get(12).map_err(to_query_error)?,
        deletions: row.get(13).map_err(to_query_error)?,
        changed_loc: row.get(14).map_err(to_query_error)?,
        inline_comments_created: row.get(15).map_err(to_query_error)?,
        inline_comments_updated: row.get(16).map_err(to_query_error)?,
        inline_comments_skipped: row.get(17).map_err(to_query_error)?,
        inline_comments_failed: row.get(18).map_err(to_query_error)?,
        stale_comments_deleted: row.get(19).map_err(to_query_error)?,
        managed_comment_id: row.get(20).map_err(to_query_error)?,
        managed_comment_action: row.get(21).map_err(to_query_error)?,
        managed_comment_status: row.get(22).map_err(to_query_error)?,
        review_event_status: row.get(23).map_err(to_query_error)?,
        summary_status: row.get(24).map_err(to_query_error)?,
        diagram_status: row.get(25).map_err(to_query_error)?,
        linked_issue_count: row.get(26).map_err(to_query_error)?,
        linked_issue_status: row.get(27).map_err(to_query_error)?,
        model_execution_mode: row.get(28).map_err(to_query_error)?,
        provider_key: row.get(29).map_err(to_query_error)?,
        model_key: row.get(30).map_err(to_query_error)?,
        effective_config_json: decode_json_value(&effective_config_json)?,
        degraded_features_json: decode_optional_json_value(degraded_features_json)?,
        error_summary: row.get(33).map_err(to_query_error)?,
        created_at: unix_to_datetime(created_at)?,
        updated_at: unix_to_datetime(updated_at)?,
    })
}

async fn load_repository(
    connection: &libsql::Connection,
    repository_id: Uuid,
) -> Result<ReviewAgentRepositoryRecord, StoreError> {
    let sql = format!(
        "SELECT {REPOSITORY_COLUMNS} FROM review_agent_repositories WHERE repository_id = ?1"
    );
    let mut rows = connection
        .query(&sql, [repository_id.to_string()])
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .map(|row| decode_repository(&row))
        .transpose()?
        .ok_or_else(|| {
            StoreError::NotFound(format!(
                "review agent repository `{repository_id}` was not found"
            ))
        })
}

async fn load_run(
    connection: &libsql::Connection,
    run_id: Uuid,
) -> Result<ReviewAgentRunRecord, StoreError> {
    let sql = format!("SELECT {RUN_COLUMNS} FROM review_agent_runs WHERE run_id = ?1");
    let mut rows = connection
        .query(&sql, [run_id.to_string()])
        .await
        .map_err(to_query_error)?;
    rows.next()
        .await
        .map_err(to_query_error)?
        .map(|row| decode_run(&row))
        .transpose()?
        .ok_or_else(|| StoreError::NotFound(format!("review agent run `{run_id}` was not found")))
}

#[async_trait]
impl ReviewAgentRepository for LibsqlStore {
    async fn list_review_agent_repositories(
        &self,
        status: Option<ReviewAgentRepositoryStatus>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ReviewAgentRepositoryRecord>, StoreError> {
        let sql = format!(
            "SELECT {REPOSITORY_COLUMNS} FROM review_agent_repositories WHERE (?1 IS NULL OR status = ?1) ORDER BY updated_at DESC, full_name ASC LIMIT ?2 OFFSET ?3"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![
                    status.map(|value| value.as_str().to_string()),
                    limit,
                    offset
                ],
            )
            .await
            .map_err(to_query_error)?;
        let mut repositories = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            repositories.push(decode_repository(&row)?);
        }
        Ok(repositories)
    }

    async fn get_review_agent_repository(
        &self,
        repository_id: Uuid,
    ) -> Result<Option<ReviewAgentRepositoryRecord>, StoreError> {
        let sql = format!(
            "SELECT {REPOSITORY_COLUMNS} FROM review_agent_repositories WHERE repository_id = ?1"
        );
        let mut rows = self
            .connection
            .query(&sql, [repository_id.to_string()])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_repository(&row))
            .transpose()
    }

    async fn get_review_agent_repository_by_identity(
        &self,
        provider: ReviewAgentProvider,
        external_repository_id: Option<&str>,
        owner: &str,
        name: &str,
    ) -> Result<Option<ReviewAgentRepositoryRecord>, StoreError> {
        let sql = format!(
            "SELECT {REPOSITORY_COLUMNS} FROM review_agent_repositories WHERE provider = ?1 AND ((?2 IS NOT NULL AND external_repository_id = ?2) OR (?2 IS NULL AND owner = ?3 AND name = ?4)) ORDER BY status = 'active' DESC, updated_at DESC LIMIT 1"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![provider.as_str(), external_repository_id, owner, name,],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_repository(&row))
            .transpose()
    }

    async fn create_review_agent_repository(
        &self,
        input: &NewReviewAgentRepositoryRecord,
    ) -> Result<ReviewAgentRepositoryRecord, StoreError> {
        let repository_id = Uuid::new_v4();
        let settings_json = serialize_json(&input.settings_json)?;
        self.connection
            .execute(
                r#"
                INSERT INTO review_agent_repositories (
                    repository_id, provider, external_repository_id, owner, name, full_name,
                    service_account_id, status, inline_review_enabled, pr_summary_enabled,
                    diagrams_enabled, linked_issue_detection_enabled,
                    linked_issue_assessment_enabled, default_model_key, max_inline_comments,
                    request_changes_on_high_severity, settings_json, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17)
                "#,
                libsql::params![
                    repository_id.to_string(),
                    input.provider.as_str(),
                    input.external_repository_id.as_deref(),
                    input.owner.as_str(),
                    input.name.as_str(),
                    input.full_name.as_str(),
                    input.service_account_id.to_string(),
                    bool_to_i64(input.settings.inline_review_enabled),
                    bool_to_i64(input.settings.pr_summary_enabled),
                    bool_to_i64(input.settings.diagrams_enabled),
                    bool_to_i64(input.settings.linked_issue_detection_enabled),
                    bool_to_i64(input.settings.linked_issue_assessment_enabled),
                    input.settings.default_model_key.as_deref(),
                    input.settings.max_inline_comments,
                    bool_to_i64(input.settings.request_changes_on_high_severity),
                    settings_json,
                    input.created_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        load_repository(&self.connection, repository_id).await
    }

    async fn update_review_agent_repository(
        &self,
        input: &UpdateReviewAgentRepositoryRecord,
    ) -> Result<ReviewAgentRepositoryRecord, StoreError> {
        let settings_json = serialize_json(&input.settings_json)?;
        let rows = self
            .connection
            .execute(
                r#"
                UPDATE review_agent_repositories
                SET external_repository_id = ?1, owner = ?2, name = ?3, full_name = ?4,
                    service_account_id = ?5, status = ?6, inline_review_enabled = ?7,
                    pr_summary_enabled = ?8, diagrams_enabled = ?9,
                    linked_issue_detection_enabled = ?10,
                    linked_issue_assessment_enabled = ?11, default_model_key = ?12,
                    max_inline_comments = ?13, request_changes_on_high_severity = ?14,
                    settings_json = ?15, updated_at = ?16
                WHERE repository_id = ?17
                "#,
                libsql::params![
                    input.external_repository_id.as_deref(),
                    input.owner.as_str(),
                    input.name.as_str(),
                    input.full_name.as_str(),
                    input.service_account_id.to_string(),
                    input.status.as_str(),
                    bool_to_i64(input.settings.inline_review_enabled),
                    bool_to_i64(input.settings.pr_summary_enabled),
                    bool_to_i64(input.settings.diagrams_enabled),
                    bool_to_i64(input.settings.linked_issue_detection_enabled),
                    bool_to_i64(input.settings.linked_issue_assessment_enabled),
                    input.settings.default_model_key.as_deref(),
                    input.settings.max_inline_comments,
                    bool_to_i64(input.settings.request_changes_on_high_severity),
                    settings_json,
                    input.updated_at.unix_timestamp(),
                    input.repository_id.to_string(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        if rows == 0 {
            return Err(StoreError::NotFound(format!(
                "review agent repository `{}` was not found",
                input.repository_id
            )));
        }
        load_repository(&self.connection, input.repository_id).await
    }

    async fn set_review_agent_repository_status(
        &self,
        repository_id: Uuid,
        status: ReviewAgentRepositoryStatus,
        updated_at: OffsetDateTime,
    ) -> Result<ReviewAgentRepositoryRecord, StoreError> {
        let rows = self
            .connection
            .execute(
                "UPDATE review_agent_repositories SET status = ?1, updated_at = ?2 WHERE repository_id = ?3",
                libsql::params![status.as_str(), updated_at.unix_timestamp(), repository_id.to_string()],
            )
            .await
            .map_err(to_write_error)?;
        if rows == 0 {
            return Err(StoreError::NotFound(format!(
                "review agent repository `{repository_id}` was not found"
            )));
        }
        load_repository(&self.connection, repository_id).await
    }

    async fn upsert_review_agent_pull_request(
        &self,
        input: &UpsertReviewAgentPullRequestRecord,
    ) -> Result<ReviewAgentPullRequestRecord, StoreError> {
        let pull_request_id = self
            .get_review_agent_pull_request(input.repository_id, input.pr_number)
            .await?
            .map(|record| record.pull_request_id)
            .unwrap_or_else(Uuid::new_v4);
        self.connection
            .execute(
                r#"
                INSERT INTO review_agent_pull_requests (
                    pull_request_id, repository_id, provider_pr_id, pr_number, title,
                    author_login, state, head_sha, base_sha, head_repository_full_name,
                    base_repository_full_name, is_draft, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)
                ON CONFLICT(repository_id, pr_number) DO UPDATE SET
                    provider_pr_id = excluded.provider_pr_id,
                    title = excluded.title,
                    author_login = excluded.author_login,
                    state = excluded.state,
                    head_sha = excluded.head_sha,
                    base_sha = excluded.base_sha,
                    head_repository_full_name = excluded.head_repository_full_name,
                    base_repository_full_name = excluded.base_repository_full_name,
                    is_draft = excluded.is_draft,
                    updated_at = excluded.updated_at
                "#,
                libsql::params![
                    pull_request_id.to_string(),
                    input.repository_id.to_string(),
                    input.provider_pr_id.as_deref(),
                    input.pr_number,
                    input.title.as_deref(),
                    input.author_login.as_deref(),
                    input.state.as_str(),
                    input.head_sha.as_deref(),
                    input.base_sha.as_deref(),
                    input.head_repository_full_name.as_deref(),
                    input.base_repository_full_name.as_deref(),
                    bool_to_i64(input.is_draft),
                    input.updated_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        self.get_review_agent_pull_request(input.repository_id, input.pr_number)
            .await?
            .ok_or_else(|| {
                StoreError::NotFound(format!(
                    "review agent pull request `{}` missing after upsert",
                    input.pr_number
                ))
            })
    }

    async fn get_review_agent_pull_request(
        &self,
        repository_id: Uuid,
        pr_number: i64,
    ) -> Result<Option<ReviewAgentPullRequestRecord>, StoreError> {
        let sql = format!(
            "SELECT {PULL_REQUEST_COLUMNS} FROM review_agent_pull_requests WHERE repository_id = ?1 AND pr_number = ?2"
        );
        let mut rows = self
            .connection
            .query(&sql, libsql::params![repository_id.to_string(), pr_number])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_pull_request(&row))
            .transpose()
    }

    async fn start_review_agent_run(
        &self,
        input: &NewReviewAgentRunRecord,
    ) -> Result<ReviewAgentRunRecord, StoreError> {
        if let (Some(github_run_id), Some(github_run_attempt)) =
            (&input.github_run_id, input.github_run_attempt)
            && let Some(existing) = self
                .get_review_agent_run_by_github_attempt(
                    input.repository_id,
                    github_run_id,
                    github_run_attempt,
                )
                .await?
        {
            return Ok(existing);
        }

        let run_id = Uuid::new_v4();
        let effective_config_json = serialize_json(&input.effective_config_json)?;
        self.connection
            .execute(
                r#"
                INSERT INTO review_agent_runs (
                    run_id, repository_id, pull_request_id, head_sha, github_run_id,
                    github_run_attempt, status, started_at, model_execution_mode, provider_key,
                    model_key, effective_config_json, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)
                "#,
                libsql::params![
                    run_id.to_string(),
                    input.repository_id.to_string(),
                    input.pull_request_id.map(|value| value.to_string()),
                    input.head_sha.as_deref(),
                    input.github_run_id.as_deref(),
                    input.github_run_attempt,
                    input.status.as_str(),
                    input.started_at.map(|value| value.unix_timestamp()),
                    input.model_execution_mode.as_deref(),
                    input.provider_key.as_deref(),
                    input.model_key.as_deref(),
                    effective_config_json,
                    input.created_at.unix_timestamp(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        load_run(&self.connection, run_id).await
    }

    async fn get_review_agent_run(
        &self,
        run_id: Uuid,
    ) -> Result<Option<ReviewAgentRunRecord>, StoreError> {
        let sql = format!("SELECT {RUN_COLUMNS} FROM review_agent_runs WHERE run_id = ?1");
        let mut rows = self
            .connection
            .query(&sql, [run_id.to_string()])
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_run(&row))
            .transpose()
    }

    async fn get_review_agent_run_by_github_attempt(
        &self,
        repository_id: Uuid,
        github_run_id: &str,
        github_run_attempt: i64,
    ) -> Result<Option<ReviewAgentRunRecord>, StoreError> {
        let sql = format!(
            "SELECT {RUN_COLUMNS} FROM review_agent_runs WHERE repository_id = ?1 AND github_run_id = ?2 AND github_run_attempt = ?3"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![repository_id.to_string(), github_run_id, github_run_attempt],
            )
            .await
            .map_err(to_query_error)?;
        rows.next()
            .await
            .map_err(to_query_error)?
            .map(|row| decode_run(&row))
            .transpose()
    }

    async fn update_review_agent_run(
        &self,
        input: &UpdateReviewAgentRunRecord,
    ) -> Result<ReviewAgentRunRecord, StoreError> {
        let degraded_features_json = input
            .degraded_features_json
            .as_ref()
            .map(serialize_json)
            .transpose()?;
        let rows = self
            .connection
            .execute(
                r#"
                UPDATE review_agent_runs
                SET status = ?1, heartbeat_at = ?2, finished_at = ?3, duration_ms = ?4,
                    files_changed = ?5, additions = ?6, deletions = ?7, changed_loc = ?8,
                    inline_comments_created = ?9, inline_comments_updated = ?10,
                    inline_comments_skipped = ?11, inline_comments_failed = ?12,
                    stale_comments_deleted = ?13, managed_comment_id = ?14,
                    managed_comment_action = ?15, managed_comment_status = ?16,
                    review_event_status = ?17, summary_status = ?18, diagram_status = ?19,
                    linked_issue_count = ?20, linked_issue_status = ?21,
                    degraded_features_json = ?22, error_summary = ?23, updated_at = ?24
                WHERE run_id = ?25
                "#,
                libsql::params![
                    input.status.as_str(),
                    input.heartbeat_at.map(|value| value.unix_timestamp()),
                    input.finished_at.map(|value| value.unix_timestamp()),
                    input.duration_ms,
                    input.files_changed,
                    input.additions,
                    input.deletions,
                    input.changed_loc,
                    input.inline_comments_created,
                    input.inline_comments_updated,
                    input.inline_comments_skipped,
                    input.inline_comments_failed,
                    input.stale_comments_deleted,
                    input.managed_comment_id.as_deref(),
                    input.managed_comment_action.as_deref(),
                    input.managed_comment_status.as_deref(),
                    input.review_event_status.as_deref(),
                    input.summary_status.as_deref(),
                    input.diagram_status.as_deref(),
                    input.linked_issue_count,
                    input.linked_issue_status.as_deref(),
                    degraded_features_json,
                    input.error_summary.as_deref(),
                    input.updated_at.unix_timestamp(),
                    input.run_id.to_string(),
                ],
            )
            .await
            .map_err(to_write_error)?;
        if rows == 0 {
            return Err(StoreError::NotFound(format!(
                "review agent run `{}` was not found",
                input.run_id
            )));
        }
        load_run(&self.connection, input.run_id).await
    }

    async fn list_review_agent_runs_for_repository(
        &self,
        repository_id: Uuid,
        pr_number: Option<i64>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ReviewAgentRunRecord>, StoreError> {
        let sql = format!(
            "SELECT {ALIASED_RUN_COLUMNS} FROM review_agent_runs r LEFT JOIN review_agent_pull_requests pr ON pr.pull_request_id = r.pull_request_id WHERE r.repository_id = ?1 AND (?2 IS NULL OR pr.pr_number = ?2) ORDER BY r.created_at DESC LIMIT ?3 OFFSET ?4"
        );
        let mut rows = self
            .connection
            .query(
                &sql,
                libsql::params![repository_id.to_string(), pr_number, limit, offset],
            )
            .await
            .map_err(to_query_error)?;
        let mut runs = Vec::new();
        while let Some(row) = rows.next().await.map_err(to_query_error)? {
            runs.push(decode_run(&row)?);
        }
        Ok(runs)
    }
}
