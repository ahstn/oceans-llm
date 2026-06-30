use async_trait::async_trait;
use gateway_core::{
    NewReviewAgentRepositoryRecord, NewReviewAgentRunRecord, ReviewAgentProvider,
    ReviewAgentPullRequestRecord, ReviewAgentRepository, ReviewAgentRepositoryRecord,
    ReviewAgentRepositoryStatus, ReviewAgentRunRecord, StoreError,
    UpdateReviewAgentRepositoryRecord, UpdateReviewAgentRunRecord,
    UpsertReviewAgentPullRequestRecord,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::store::AnyStore;

#[async_trait]
impl ReviewAgentRepository for AnyStore {
    async fn list_review_agent_repositories(
        &self,
        status: Option<ReviewAgentRepositoryStatus>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ReviewAgentRepositoryRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .list_review_agent_repositories(status, limit, offset)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .list_review_agent_repositories(status, limit, offset)
                    .await
            }
        }
    }

    async fn get_review_agent_repository(
        &self,
        repository_id: Uuid,
    ) -> Result<Option<ReviewAgentRepositoryRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.get_review_agent_repository(repository_id).await,
            Self::Postgres(store) => store.get_review_agent_repository(repository_id).await,
        }
    }

    async fn get_review_agent_repository_by_identity(
        &self,
        provider: ReviewAgentProvider,
        external_repository_id: Option<&str>,
        owner: &str,
        name: &str,
    ) -> Result<Option<ReviewAgentRepositoryRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .get_review_agent_repository_by_identity(
                        provider,
                        external_repository_id,
                        owner,
                        name,
                    )
                    .await
            }
            Self::Postgres(store) => {
                store
                    .get_review_agent_repository_by_identity(
                        provider,
                        external_repository_id,
                        owner,
                        name,
                    )
                    .await
            }
        }
    }

    async fn create_review_agent_repository(
        &self,
        input: &NewReviewAgentRepositoryRecord,
    ) -> Result<ReviewAgentRepositoryRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.create_review_agent_repository(input).await,
            Self::Postgres(store) => store.create_review_agent_repository(input).await,
        }
    }

    async fn update_review_agent_repository(
        &self,
        input: &UpdateReviewAgentRepositoryRecord,
    ) -> Result<ReviewAgentRepositoryRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.update_review_agent_repository(input).await,
            Self::Postgres(store) => store.update_review_agent_repository(input).await,
        }
    }

    async fn set_review_agent_repository_status(
        &self,
        repository_id: Uuid,
        status: ReviewAgentRepositoryStatus,
        updated_at: OffsetDateTime,
    ) -> Result<ReviewAgentRepositoryRecord, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .set_review_agent_repository_status(repository_id, status, updated_at)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .set_review_agent_repository_status(repository_id, status, updated_at)
                    .await
            }
        }
    }

    async fn upsert_review_agent_pull_request(
        &self,
        input: &UpsertReviewAgentPullRequestRecord,
    ) -> Result<ReviewAgentPullRequestRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.upsert_review_agent_pull_request(input).await,
            Self::Postgres(store) => store.upsert_review_agent_pull_request(input).await,
        }
    }

    async fn get_review_agent_pull_request(
        &self,
        repository_id: Uuid,
        pr_number: i64,
    ) -> Result<Option<ReviewAgentPullRequestRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .get_review_agent_pull_request(repository_id, pr_number)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .get_review_agent_pull_request(repository_id, pr_number)
                    .await
            }
        }
    }

    async fn start_review_agent_run(
        &self,
        input: &NewReviewAgentRunRecord,
    ) -> Result<ReviewAgentRunRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.start_review_agent_run(input).await,
            Self::Postgres(store) => store.start_review_agent_run(input).await,
        }
    }

    async fn get_review_agent_run(
        &self,
        run_id: Uuid,
    ) -> Result<Option<ReviewAgentRunRecord>, StoreError> {
        match self {
            Self::Libsql(store) => store.get_review_agent_run(run_id).await,
            Self::Postgres(store) => store.get_review_agent_run(run_id).await,
        }
    }

    async fn get_review_agent_run_by_github_attempt(
        &self,
        repository_id: Uuid,
        github_run_id: &str,
        github_run_attempt: i64,
    ) -> Result<Option<ReviewAgentRunRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .get_review_agent_run_by_github_attempt(
                        repository_id,
                        github_run_id,
                        github_run_attempt,
                    )
                    .await
            }
            Self::Postgres(store) => {
                store
                    .get_review_agent_run_by_github_attempt(
                        repository_id,
                        github_run_id,
                        github_run_attempt,
                    )
                    .await
            }
        }
    }

    async fn update_review_agent_run(
        &self,
        input: &UpdateReviewAgentRunRecord,
    ) -> Result<ReviewAgentRunRecord, StoreError> {
        match self {
            Self::Libsql(store) => store.update_review_agent_run(input).await,
            Self::Postgres(store) => store.update_review_agent_run(input).await,
        }
    }

    async fn list_review_agent_runs_for_repository(
        &self,
        repository_id: Uuid,
        pr_number: Option<i64>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ReviewAgentRunRecord>, StoreError> {
        match self {
            Self::Libsql(store) => {
                store
                    .list_review_agent_runs_for_repository(repository_id, pr_number, limit, offset)
                    .await
            }
            Self::Postgres(store) => {
                store
                    .list_review_agent_runs_for_repository(repository_id, pr_number, limit, offset)
                    .await
            }
        }
    }
}
