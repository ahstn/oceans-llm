//! Git AI attribution on PostgreSQL. Statements and write rules come from
//! [`crate::attribution`]; this module runs them in transactions and decodes the rows. A write
//! inserts first, so concurrent writers of one key serialize on the row or its unique index.

use async_trait::async_trait;
use gateway_core::{
    AttributionAgentSessionRecord, AttributionApplyOutcome, AttributionCommitRecord,
    AttributionCommitSession, AttributionCommitWrite, AttributionFreshness,
    AttributionPullRequestCommit, AttributionPullRequestRecord, AttributionPullRequestWrite,
    AttributionReportQuery, AttributionRepository, AttributionRequestFact,
    AttributionSessionCommitTime, AttributionSessionEventWrite, StoreError,
};
use sqlx::postgres::{PgArguments, PgRow};
use sqlx::query::Query;
use sqlx::{PgConnection, Postgres, Row};
use time::OffsetDateTime;

use super::{PostgresStore, to_query_error, to_write_error};
use crate::attribution::{
    self, AttributionBind, AttributionStatement, Dialect, optional_time, optional_uuid, owner,
};
use crate::shared::{parse_uuid, unix_millis_to_datetime};

const DIALECT: Dialect = Dialect::Postgres;
/// First session column of a commit report row.
const COMMIT_SESSION_OFFSET: usize = attribution::COMMIT_COLUMN_COUNT;
/// First commit column of a pull request report row.
const PULL_REQUEST_COMMIT_OFFSET: usize = attribution::PULL_REQUEST_COLUMN_COUNT;

fn bound(sql: &str, binds: Vec<AttributionBind>) -> Query<'_, Postgres, PgArguments> {
    binds
        .into_iter()
        .fold(sqlx::query(sql), |query, bind| match bind {
            AttributionBind::Int(value) => query.bind(value),
            AttributionBind::Text(value) => query.bind(value),
            AttributionBind::Bool(value) => query.bind(value),
            AttributionBind::TextList(values) => query.bind(values),
        })
}

async fn execute(
    connection: &mut PgConnection,
    statement: AttributionStatement,
) -> Result<u64, StoreError> {
    Ok(bound(&statement.sql, statement.binds)
        .execute(connection)
        .await
        .map_err(to_write_error)?
        .rows_affected())
}

async fn execute_all(
    connection: &mut PgConnection,
    statements: Vec<AttributionStatement>,
) -> Result<u64, StoreError> {
    let mut changed = 0;
    for statement in statements {
        changed += execute(connection, statement).await?;
    }
    Ok(changed)
}

async fn fetch_all(
    connection: &mut PgConnection,
    statement: AttributionStatement,
) -> Result<Vec<PgRow>, StoreError> {
    bound(&statement.sql, statement.binds)
        .fetch_all(connection)
        .await
        .map_err(to_query_error)
}

/// The row a locking read finds for a key the transaction's insert conflicted with.
async fn fetch_conflicting(
    connection: &mut PgConnection,
    statement: AttributionStatement,
) -> Result<PgRow, StoreError> {
    bound(&statement.sql, statement.binds)
        .fetch_optional(connection)
        .await
        .map_err(to_query_error)?
        .ok_or_else(attribution::conflicting_row_missing)
}

fn get<'r, T>(row: &'r PgRow, index: usize) -> Result<T, StoreError>
where
    T: sqlx::Decode<'r, Postgres> + sqlx::Type<Postgres>,
{
    row.try_get(index).map_err(to_query_error)
}

fn uuid_at(row: &PgRow, index: usize) -> Result<uuid::Uuid, StoreError> {
    parse_uuid(&get::<String>(row, index)?)
}

fn time_at(row: &PgRow, index: usize) -> Result<OffsetDateTime, StoreError> {
    unix_millis_to_datetime(get(row, index)?)
}

fn decode_commit(row: &PgRow) -> Result<AttributionCommitRecord, StoreError> {
    Ok(AttributionCommitRecord {
        attribution_commit_id: uuid_at(row, 0)?,
        owner: owner(get(row, 1)?, get(row, 2)?, get(row, 3)?)?,
        repository: get(row, 4)?,
        commit_sha: get(row, 5)?,
        branch: get(row, 6)?,
        is_default_branch: get(row, 7)?,
        author_user_id: optional_uuid(get(row, 8)?)?,
        author_email_hash: get(row, 9)?,
        schema_version: get(row, 10)?,
        git_ai_version: get(row, 11)?,
        ai_lines: get(row, 12)?,
        human_lines: get(row, 13)?,
        freshness: AttributionFreshness {
            installation_id: get(row, 14)?,
            sequence: get(row, 15)?,
            observed_at: time_at(row, 16)?,
            late_sequenced: get(row, 17)?,
        },
        first_observed_at: time_at(row, 18)?,
        sessions: Vec::new(),
    })
}

/// The session columns of a commit report row; `None` for a commit without sessions.
fn decode_commit_session(row: &PgRow) -> Result<Option<AttributionCommitSession>, StoreError> {
    let at = COMMIT_SESSION_OFFSET;
    let Some(note_session_key) = get(row, at)? else {
        return Ok(None);
    };
    Ok(Some(AttributionCommitSession {
        note_session_key,
        agent_tool: get(row, at + 1)?,
        agent_model: get(row, at + 2)?,
        harness_key: get(row, at + 3)?,
        ai_lines: get(row, at + 4)?,
        human_author_user_id: optional_uuid(get(row, at + 5)?)?,
        human_author_hash: get(row, at + 6)?,
        session_hashes: get::<Option<Vec<String>>>(row, at + 7)?.unwrap_or_default(),
    }))
}

fn decode_agent_session(row: &PgRow) -> Result<AttributionAgentSessionRecord, StoreError> {
    Ok(AttributionAgentSessionRecord {
        owner: owner(get(row, 0)?, get(row, 1)?, get(row, 2)?)?,
        harness_key: get(row, 3)?,
        session_hash: get(row, 4)?,
        repository: get(row, 5)?,
        branch: get(row, 6)?,
        start_head_sha: get(row, 7)?,
        end_head_sha: get(row, 8)?,
        harness_version: get(row, 9)?,
        started_at: optional_time(get(row, 10)?)?,
        ended_at: optional_time(get(row, 11)?)?,
        last_observed_at: time_at(row, 12)?,
        session_hashes: get::<Option<Vec<String>>>(row, 13)?.unwrap_or_default(),
    })
}

fn decode_pull_request(row: &PgRow) -> Result<AttributionPullRequestRecord, StoreError> {
    Ok(AttributionPullRequestRecord {
        attribution_pull_request_id: uuid_at(row, 0)?,
        owner: owner(get(row, 1)?, get(row, 2)?, get(row, 3)?)?,
        repository: get(row, 4)?,
        number: get(row, 5)?,
        title: get(row, 6)?,
        url: get(row, 7)?,
        base_ref: get(row, 8)?,
        head_ref: get(row, 9)?,
        merge_commit_sha: get(row, 10)?,
        merged_at: time_at(row, 11)?,
        author_user_id: optional_uuid(get(row, 12)?)?,
        author_email_hash: get(row, 13)?,
        commits: Vec::new(),
        observed_at: time_at(row, 14)?,
    })
}

/// The commit columns of a pull request report row; `None` for an empty commit list.
fn decode_pull_request_commit(
    row: &PgRow,
) -> Result<Option<AttributionPullRequestCommit>, StoreError> {
    let at = PULL_REQUEST_COMMIT_OFFSET;
    let Some(commit_sha) = get(row, at)? else {
        return Ok(None);
    };
    Ok(Some(AttributionPullRequestCommit {
        commit_sha,
        subject: get(row, at + 1)?,
        authored_at: optional_time(get(row, at + 2)?)?,
        author_user_id: optional_uuid(get(row, at + 3)?)?,
        author_email_hash: get(row, at + 4)?,
    }))
}

fn decode_commit_rows(rows: &[PgRow]) -> Result<Vec<AttributionCommitRecord>, StoreError> {
    let mut commits: Vec<AttributionCommitRecord> = Vec::new();
    for row in rows {
        let commit_id = uuid_at(row, 0)?;
        if commits
            .last()
            .is_none_or(|last| last.attribution_commit_id != commit_id)
        {
            commits.push(decode_commit(row)?);
        }
        if let (Some(session), Some(commit)) = (decode_commit_session(row)?, commits.last_mut()) {
            commit.sessions.push(session);
        }
    }
    Ok(commits)
}

fn decode_pull_request_rows(
    rows: &[PgRow],
) -> Result<Vec<AttributionPullRequestRecord>, StoreError> {
    let mut pull_requests: Vec<AttributionPullRequestRecord> = Vec::new();
    for row in rows {
        let pull_request_id = uuid_at(row, 0)?;
        if pull_requests
            .last()
            .is_none_or(|last| last.attribution_pull_request_id != pull_request_id)
        {
            pull_requests.push(decode_pull_request(row)?);
        }
        if let (Some(commit), Some(pull_request)) =
            (decode_pull_request_commit(row)?, pull_requests.last_mut())
        {
            pull_request.commits.push(commit);
        }
    }
    Ok(pull_requests)
}

impl PostgresStore {
    async fn fetch_report(
        &self,
        statement: AttributionStatement,
    ) -> Result<Vec<PgRow>, StoreError> {
        let mut connection = self.pool.acquire().await.map_err(to_query_error)?;
        fetch_all(&mut connection, statement).await
    }
}

#[async_trait]
impl AttributionRepository for PostgresStore {
    async fn apply_attribution_commit(
        &self,
        write: &AttributionCommitWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let insert = attribution::insert_commit(DIALECT, write)?;
        let commit_id = if execute(&mut transaction, insert).await? > 0 {
            write.attribution_commit_id.to_string()
        } else {
            let row = fetch_conflicting(
                &mut transaction,
                attribution::select_commit_freshness(DIALECT, write),
            )
            .await?;
            let commit_id: String = get(&row, 0)?;
            let stored = AttributionFreshness {
                installation_id: get(&row, 1)?,
                sequence: get(&row, 2)?,
                observed_at: time_at(&row, 3)?,
                late_sequenced: get(&row, 4)?,
            };
            let outcome = attribution::commit_outcome(write, &stored)?;
            if outcome != AttributionApplyOutcome::Applied {
                transaction.rollback().await.map_err(to_query_error)?;
                return Ok(outcome);
            }
            let update = attribution::update_commit(DIALECT, &commit_id, write)?;
            execute(&mut transaction, update).await?;
            let delete = attribution::delete_commit_sessions(DIALECT, &commit_id);
            execute_all(&mut transaction, delete).await?;
            commit_id
        };
        let sessions = attribution::insert_commit_sessions(DIALECT, &commit_id, &write.sessions);
        execute_all(&mut transaction, sessions).await?;
        transaction.commit().await.map_err(to_write_error)?;
        Ok(AttributionApplyOutcome::Applied)
    }

    async fn apply_attribution_session_event(
        &self,
        write: &AttributionSessionEventWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        let created = attribution::session_from_event(write)?;
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let insert = attribution::insert_agent_session(DIALECT, &created, write.received_at)?;
        let session = if execute(&mut transaction, insert).await? > 0 {
            created
        } else {
            let row = fetch_conflicting(
                &mut transaction,
                attribution::select_agent_session(DIALECT, write),
            )
            .await?;
            let stored = decode_agent_session(&row)?;
            let Some(merged) = attribution::merge_session_event(&stored, write)? else {
                transaction.rollback().await.map_err(to_query_error)?;
                return Ok(AttributionApplyOutcome::Duplicate);
            };
            let update = attribution::update_agent_session(DIALECT, &merged, write.received_at)?;
            execute(&mut transaction, update).await?;
            merged
        };
        let hashes = attribution::insert_agent_session_hashes(DIALECT, &session);
        execute_all(&mut transaction, hashes).await?;
        transaction.commit().await.map_err(to_write_error)?;
        Ok(AttributionApplyOutcome::Applied)
    }

    async fn apply_attribution_pull_request(
        &self,
        write: &AttributionPullRequestWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let insert = attribution::insert_pull_request(DIALECT, write)?;
        let pull_request_id = if execute(&mut transaction, insert).await? > 0 {
            write.attribution_pull_request_id.to_string()
        } else {
            let row = fetch_conflicting(
                &mut transaction,
                attribution::select_pull_request_observed_at(DIALECT, write),
            )
            .await?;
            let pull_request_id: String = get(&row, 0)?;
            let outcome = attribution::pull_request_outcome(write, get(&row, 1)?)?;
            if outcome != AttributionApplyOutcome::Applied {
                transaction.rollback().await.map_err(to_query_error)?;
                return Ok(outcome);
            }
            let update = attribution::update_pull_request(DIALECT, &pull_request_id, write)?;
            execute(&mut transaction, update).await?;
            let delete = attribution::delete_pull_request_commits(DIALECT, &pull_request_id);
            execute(&mut transaction, delete).await?;
            pull_request_id
        };
        let commits =
            attribution::insert_pull_request_commits(DIALECT, &pull_request_id, &write.commits)?;
        execute_all(&mut transaction, commits).await?;
        transaction.commit().await.map_err(to_write_error)?;
        Ok(AttributionApplyOutcome::Applied)
    }

    async fn list_attribution_commits(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionCommitRecord>, StoreError> {
        let rows = self
            .fetch_report(attribution::list_commits(DIALECT, query)?)
            .await?;
        decode_commit_rows(&rows)
    }

    async fn list_attribution_pull_requests(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionPullRequestRecord>, StoreError> {
        let rows = self
            .fetch_report(attribution::list_pull_requests(DIALECT, query)?)
            .await?;
        decode_pull_request_rows(&rows)
    }

    async fn list_attribution_agent_sessions(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionAgentSessionRecord>, StoreError> {
        let rows = self
            .fetch_report(attribution::list_agent_sessions(DIALECT, query)?)
            .await?;
        rows.iter().map(decode_agent_session).collect()
    }

    async fn list_attribution_session_commit_times(
        &self,
        session_hashes: &[String],
        until: OffsetDateTime,
    ) -> Result<Vec<AttributionSessionCommitTime>, StoreError> {
        let Some(statement) =
            attribution::list_session_commit_times(DIALECT, session_hashes, until)?
        else {
            return Ok(Vec::new());
        };
        let rows = self.fetch_report(statement).await?;
        rows.iter()
            .map(|row| {
                Ok(AttributionSessionCommitTime {
                    ownership_scope_key: get(row, 0)?,
                    session_hash: get(row, 1)?,
                    attribution_commit_id: uuid_at(row, 2)?,
                    committed_at: time_at(row, 3)?,
                })
            })
            .collect()
    }

    async fn list_attribution_request_facts(
        &self,
        session_hashes: &[String],
        until: OffsetDateTime,
    ) -> Result<Vec<AttributionRequestFact>, StoreError> {
        let Some(statement) = attribution::list_request_facts(DIALECT, session_hashes, until)?
        else {
            return Ok(Vec::new());
        };
        let rows = self.fetch_report(statement).await?;
        rows.iter()
            .map(|row| {
                Ok(AttributionRequestFact {
                    ownership_scope_key: get(row, 0)?,
                    session_hash: get(row, 1)?,
                    request_id: get(row, 2)?,
                    occurred_at: time_at(row, 3)?,
                    model_key: get(row, 4)?,
                    cost_10000: get(row, 5)?,
                    pricing_status: get(row, 6)?,
                })
            })
            .collect()
    }

    async fn delete_attribution_for_owner(
        &self,
        ownership_scope_key: &str,
    ) -> Result<u64, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(to_query_error)?;
        let deleted = execute_all(
            &mut transaction,
            attribution::delete_for_owner(DIALECT, ownership_scope_key),
        )
        .await?;
        transaction.commit().await.map_err(to_write_error)?;
        Ok(deleted)
    }
}
