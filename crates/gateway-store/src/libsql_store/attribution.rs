//! Git AI attribution on libsql. Statements and write rules come from [`crate::attribution`];
//! this module runs them in `BEGIN IMMEDIATE` transactions and decodes the rows.

use async_trait::async_trait;
use gateway_core::{
    AttributionAgentSessionRecord, AttributionApplyOutcome, AttributionCommitRecord,
    AttributionCommitSession, AttributionCommitWrite, AttributionFreshness,
    AttributionPullRequestCommit, AttributionPullRequestRecord, AttributionPullRequestWrite,
    AttributionReportQuery, AttributionRepository, AttributionRequestFact,
    AttributionSessionCommitTime, AttributionSessionEventWrite, StoreError,
};
use time::OffsetDateTime;

use super::{LibsqlStore, to_query_error, to_write_error};
use crate::attribution::{
    self, AttributionBind, AttributionStatement, Dialect, optional_time, optional_uuid, owner,
};
use crate::shared::{parse_uuid, unix_millis_to_datetime};

const DIALECT: Dialect = Dialect::Libsql;
/// First session column of a commit report row.
const COMMIT_SESSION_OFFSET: i32 = attribution::COMMIT_COLUMN_COUNT as i32;
/// First commit column of a pull request report row.
const PULL_REQUEST_COMMIT_OFFSET: i32 = attribution::PULL_REQUEST_COLUMN_COUNT as i32;

fn values(binds: Vec<AttributionBind>) -> Result<Vec<libsql::Value>, StoreError> {
    binds
        .into_iter()
        .map(|bind| {
            Ok(match bind {
                AttributionBind::Int(Some(value)) => libsql::Value::Integer(value),
                AttributionBind::Text(Some(value)) => libsql::Value::Text(value),
                AttributionBind::Int(None) | AttributionBind::Text(None) => libsql::Value::Null,
                AttributionBind::Bool(value) => libsql::Value::Integer(i64::from(value)),
                AttributionBind::TextList(values) => libsql::Value::Text(
                    serde_json::to_string(&values)
                        .map_err(|error| StoreError::Serialization(error.to_string()))?,
                ),
            })
        })
        .collect()
}

async fn execute(
    connection: &libsql::Connection,
    statement: AttributionStatement,
) -> Result<u64, StoreError> {
    connection
        .execute(&statement.sql, values(statement.binds)?)
        .await
        .map_err(to_write_error)
}

async fn query(
    connection: &libsql::Connection,
    statement: AttributionStatement,
) -> Result<libsql::Rows, StoreError> {
    connection
        .query(&statement.sql, values(statement.binds)?)
        .await
        .map_err(to_query_error)
}

async fn next_row(rows: &mut libsql::Rows) -> Result<Option<libsql::Row>, StoreError> {
    rows.next().await.map_err(to_query_error)
}

fn string(row: &libsql::Row, index: i32) -> Result<String, StoreError> {
    row.get(index).map_err(to_query_error)
}

fn optional_string(row: &libsql::Row, index: i32) -> Result<Option<String>, StoreError> {
    row.get(index).map_err(to_query_error)
}

fn integer(row: &libsql::Row, index: i32) -> Result<i64, StoreError> {
    row.get(index).map_err(to_query_error)
}

fn optional_integer(row: &libsql::Row, index: i32) -> Result<Option<i64>, StoreError> {
    row.get(index).map_err(to_query_error)
}

fn time_at(row: &libsql::Row, index: i32) -> Result<OffsetDateTime, StoreError> {
    unix_millis_to_datetime(integer(row, index)?)
}

fn flag(row: &libsql::Row, index: i32) -> Result<bool, StoreError> {
    Ok(integer(row, index)? == 1)
}

fn decode_commit(row: &libsql::Row) -> Result<AttributionCommitRecord, StoreError> {
    Ok(AttributionCommitRecord {
        attribution_commit_id: parse_uuid(&string(row, 0)?)?,
        owner: owner(
            string(row, 1)?,
            optional_string(row, 2)?,
            optional_string(row, 3)?,
        )?,
        repository: string(row, 4)?,
        commit_sha: string(row, 5)?,
        branch: optional_string(row, 6)?,
        is_default_branch: flag(row, 7)?,
        author_user_id: optional_uuid(optional_string(row, 8)?)?,
        author_email_hash: optional_string(row, 9)?,
        schema_version: string(row, 10)?,
        git_ai_version: optional_string(row, 11)?,
        ai_lines: integer(row, 12)?,
        human_lines: integer(row, 13)?,
        freshness: AttributionFreshness {
            installation_id: string(row, 14)?,
            sequence: integer(row, 15)?,
            observed_at: time_at(row, 16)?,
            late_sequenced: flag(row, 17)?,
        },
        first_observed_at: time_at(row, 18)?,
        sessions: Vec::new(),
    })
}

/// The session columns of a commit report row; `None` for a commit without sessions.
fn decode_commit_session(
    row: &libsql::Row,
) -> Result<Option<AttributionCommitSession>, StoreError> {
    let at = COMMIT_SESSION_OFFSET;
    let Some(note_session_key) = optional_string(row, at)? else {
        return Ok(None);
    };
    Ok(Some(AttributionCommitSession {
        note_session_key,
        agent_tool: string(row, at + 1)?,
        agent_model: optional_string(row, at + 2)?,
        harness_key: optional_string(row, at + 3)?,
        ai_lines: integer(row, at + 4)?,
        human_author_user_id: optional_uuid(optional_string(row, at + 5)?)?,
        human_author_hash: optional_string(row, at + 6)?,
        session_hashes: session_hashes(&string(row, at + 7)?)?,
    }))
}

/// A session's `json_group_array` of hashes, ascending.
fn session_hashes(raw: &str) -> Result<Vec<String>, StoreError> {
    let mut hashes: Vec<String> =
        serde_json::from_str(raw).map_err(|error| StoreError::Serialization(error.to_string()))?;
    hashes.sort_unstable();
    Ok(hashes)
}

fn decode_agent_session(row: &libsql::Row) -> Result<AttributionAgentSessionRecord, StoreError> {
    Ok(AttributionAgentSessionRecord {
        owner: owner(
            string(row, 0)?,
            optional_string(row, 1)?,
            optional_string(row, 2)?,
        )?,
        harness_key: string(row, 3)?,
        session_hash: string(row, 4)?,
        repository: optional_string(row, 5)?,
        branch: optional_string(row, 6)?,
        start_head_sha: optional_string(row, 7)?,
        end_head_sha: optional_string(row, 8)?,
        harness_version: optional_string(row, 9)?,
        started_at: optional_time(optional_integer(row, 10)?)?,
        ended_at: optional_time(optional_integer(row, 11)?)?,
        last_observed_at: time_at(row, 12)?,
        session_hashes: session_hashes(&string(row, 13)?)?,
    })
}

fn decode_pull_request(row: &libsql::Row) -> Result<AttributionPullRequestRecord, StoreError> {
    Ok(AttributionPullRequestRecord {
        attribution_pull_request_id: parse_uuid(&string(row, 0)?)?,
        owner: owner(
            string(row, 1)?,
            optional_string(row, 2)?,
            optional_string(row, 3)?,
        )?,
        repository: string(row, 4)?,
        number: integer(row, 5)?,
        title: string(row, 6)?,
        url: optional_string(row, 7)?,
        base_ref: optional_string(row, 8)?,
        head_ref: optional_string(row, 9)?,
        merge_commit_sha: optional_string(row, 10)?,
        merged_at: time_at(row, 11)?,
        author_user_id: optional_uuid(optional_string(row, 12)?)?,
        author_email_hash: optional_string(row, 13)?,
        commits: Vec::new(),
        observed_at: time_at(row, 14)?,
    })
}

/// The commit columns of a pull request report row; `None` for an empty commit list.
fn decode_pull_request_commit(
    row: &libsql::Row,
) -> Result<Option<AttributionPullRequestCommit>, StoreError> {
    let at = PULL_REQUEST_COMMIT_OFFSET;
    let Some(commit_sha) = optional_string(row, at)? else {
        return Ok(None);
    };
    Ok(Some(AttributionPullRequestCommit {
        commit_sha,
        subject: optional_string(row, at + 1)?,
        authored_at: optional_time(optional_integer(row, at + 2)?)?,
        author_user_id: optional_uuid(optional_string(row, at + 3)?)?,
        author_email_hash: optional_string(row, at + 4)?,
    }))
}

/// The id and revision of the commit whose key `write`'s insert conflicted with.
async fn stored_commit(
    connection: &libsql::Connection,
    write: &AttributionCommitWrite,
) -> Result<(String, AttributionFreshness), StoreError> {
    let mut rows = query(
        connection,
        attribution::select_commit_freshness(DIALECT, write),
    )
    .await?;
    let row = next_row(&mut rows)
        .await?
        .ok_or_else(attribution::conflicting_row_missing)?;
    Ok((
        string(&row, 0)?,
        AttributionFreshness {
            installation_id: string(&row, 1)?,
            sequence: integer(&row, 2)?,
            observed_at: time_at(&row, 3)?,
            late_sequenced: flag(&row, 4)?,
        },
    ))
}

async fn stored_agent_session(
    connection: &libsql::Connection,
    write: &AttributionSessionEventWrite,
) -> Result<AttributionAgentSessionRecord, StoreError> {
    let mut rows = query(
        connection,
        attribution::select_agent_session(DIALECT, write),
    )
    .await?;
    let row = next_row(&mut rows)
        .await?
        .ok_or_else(attribution::conflicting_row_missing)?;
    decode_agent_session(&row)
}

/// The id and `observed_at` of the pull request whose key `write`'s insert conflicted with.
async fn stored_pull_request(
    connection: &libsql::Connection,
    write: &AttributionPullRequestWrite,
) -> Result<(String, i64), StoreError> {
    let mut rows = query(
        connection,
        attribution::select_pull_request_observed_at(DIALECT, write),
    )
    .await?;
    let row = next_row(&mut rows)
        .await?
        .ok_or_else(attribution::conflicting_row_missing)?;
    Ok((string(&row, 0)?, integer(&row, 1)?))
}

async fn execute_all(
    connection: &libsql::Connection,
    statements: Vec<AttributionStatement>,
) -> Result<u64, StoreError> {
    let mut changed = 0;
    for statement in statements {
        changed += execute(connection, statement).await?;
    }
    Ok(changed)
}

async fn rollback(transaction: libsql::Transaction) -> Result<(), StoreError> {
    transaction.rollback().await.map_err(to_query_error)
}

async fn commit(transaction: libsql::Transaction) -> Result<(), StoreError> {
    transaction.commit().await.map_err(to_write_error)
}

#[async_trait]
impl AttributionRepository for LibsqlStore {
    async fn apply_attribution_commit(
        &self,
        write: &AttributionCommitWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        let transaction = self.begin_transaction().await?;
        let commit_id =
            if execute(&transaction, attribution::insert_commit(DIALECT, write)?).await? > 0 {
                write.attribution_commit_id.to_string()
            } else {
                let (commit_id, stored) = stored_commit(&transaction, write).await?;
                let outcome = attribution::commit_outcome(write, &stored)?;
                if outcome != AttributionApplyOutcome::Applied {
                    rollback(transaction).await?;
                    return Ok(outcome);
                }
                execute(
                    &transaction,
                    attribution::update_commit(DIALECT, &commit_id, write)?,
                )
                .await?;
                execute_all(
                    &transaction,
                    attribution::delete_commit_sessions(DIALECT, &commit_id),
                )
                .await?;
                commit_id
            };
        execute_all(
            &transaction,
            attribution::insert_commit_sessions(DIALECT, &commit_id, &write.sessions),
        )
        .await?;
        commit(transaction).await?;
        Ok(AttributionApplyOutcome::Applied)
    }

    async fn apply_attribution_session_event(
        &self,
        write: &AttributionSessionEventWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        let created = attribution::session_from_event(write)?;
        let transaction = self.begin_transaction().await?;
        let insert = attribution::insert_agent_session(DIALECT, &created, write.received_at)?;
        let session = if execute(&transaction, insert).await? > 0 {
            created
        } else {
            let stored = stored_agent_session(&transaction, write).await?;
            let Some(merged) = attribution::merge_session_event(&stored, write)? else {
                rollback(transaction).await?;
                return Ok(AttributionApplyOutcome::Duplicate);
            };
            execute(
                &transaction,
                attribution::update_agent_session(DIALECT, &merged, write.received_at)?,
            )
            .await?;
            merged
        };
        execute_all(
            &transaction,
            attribution::insert_agent_session_hashes(DIALECT, &session),
        )
        .await?;
        commit(transaction).await?;
        Ok(AttributionApplyOutcome::Applied)
    }

    async fn apply_attribution_pull_request(
        &self,
        write: &AttributionPullRequestWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        let transaction = self.begin_transaction().await?;
        let insert = attribution::insert_pull_request(DIALECT, write)?;
        let pull_request_id = if execute(&transaction, insert).await? > 0 {
            write.attribution_pull_request_id.to_string()
        } else {
            let (pull_request_id, stored_observed_at) =
                stored_pull_request(&transaction, write).await?;
            let outcome = attribution::pull_request_outcome(write, stored_observed_at)?;
            if outcome != AttributionApplyOutcome::Applied {
                rollback(transaction).await?;
                return Ok(outcome);
            }
            execute(
                &transaction,
                attribution::update_pull_request(DIALECT, &pull_request_id, write)?,
            )
            .await?;
            execute(
                &transaction,
                attribution::delete_pull_request_commits(DIALECT, &pull_request_id),
            )
            .await?;
            pull_request_id
        };
        execute_all(
            &transaction,
            attribution::insert_pull_request_commits(DIALECT, &pull_request_id, &write.commits)?,
        )
        .await?;
        commit(transaction).await?;
        Ok(AttributionApplyOutcome::Applied)
    }

    async fn list_attribution_commits(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionCommitRecord>, StoreError> {
        let mut rows =
            self::query(&self.connection, attribution::list_commits(DIALECT, query)?).await?;
        let mut commits: Vec<AttributionCommitRecord> = Vec::new();
        while let Some(row) = next_row(&mut rows).await? {
            let commit_id = parse_uuid(&string(&row, 0)?)?;
            if commits
                .last()
                .is_none_or(|last| last.attribution_commit_id != commit_id)
            {
                commits.push(decode_commit(&row)?);
            }
            if let (Some(session), Some(commit)) =
                (decode_commit_session(&row)?, commits.last_mut())
            {
                commit.sessions.push(session);
            }
        }
        Ok(commits)
    }

    async fn list_attribution_pull_requests(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionPullRequestRecord>, StoreError> {
        let mut rows = self::query(
            &self.connection,
            attribution::list_pull_requests(DIALECT, query)?,
        )
        .await?;
        let mut pull_requests: Vec<AttributionPullRequestRecord> = Vec::new();
        while let Some(row) = next_row(&mut rows).await? {
            let pull_request_id = parse_uuid(&string(&row, 0)?)?;
            if pull_requests
                .last()
                .is_none_or(|last| last.attribution_pull_request_id != pull_request_id)
            {
                pull_requests.push(decode_pull_request(&row)?);
            }
            if let (Some(commit), Some(pull_request)) =
                (decode_pull_request_commit(&row)?, pull_requests.last_mut())
            {
                pull_request.commits.push(commit);
            }
        }
        Ok(pull_requests)
    }

    async fn list_attribution_agent_sessions(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionAgentSessionRecord>, StoreError> {
        let mut rows = self::query(
            &self.connection,
            attribution::list_agent_sessions(DIALECT, query)?,
        )
        .await?;
        let mut sessions = Vec::new();
        while let Some(row) = next_row(&mut rows).await? {
            sessions.push(decode_agent_session(&row)?);
        }
        Ok(sessions)
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
        let mut rows = self::query(&self.connection, statement).await?;
        let mut times = Vec::new();
        while let Some(row) = next_row(&mut rows).await? {
            times.push(AttributionSessionCommitTime {
                ownership_scope_key: string(&row, 0)?,
                session_hash: string(&row, 1)?,
                attribution_commit_id: parse_uuid(&string(&row, 2)?)?,
                committed_at: time_at(&row, 3)?,
            });
        }
        Ok(times)
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
        let mut rows = self::query(&self.connection, statement).await?;
        let mut facts = Vec::new();
        while let Some(row) = next_row(&mut rows).await? {
            facts.push(AttributionRequestFact {
                ownership_scope_key: string(&row, 0)?,
                session_hash: string(&row, 1)?,
                request_id: string(&row, 2)?,
                occurred_at: time_at(&row, 3)?,
                model_key: optional_string(&row, 4)?,
                cost_10000: optional_integer(&row, 5)?,
                pricing_status: optional_string(&row, 6)?,
            });
        }
        Ok(facts)
    }

    async fn delete_attribution_for_owner(
        &self,
        ownership_scope_key: &str,
    ) -> Result<u64, StoreError> {
        let transaction = self.begin_transaction().await?;
        let deleted = execute_all(
            &transaction,
            attribution::delete_for_owner(DIALECT, ownership_scope_key),
        )
        .await?;
        commit(transaction).await?;
        Ok(deleted)
    }
}
