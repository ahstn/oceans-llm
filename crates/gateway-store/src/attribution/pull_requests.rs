//! Merged pull request statements: the observed-at upsert, commit lists and the reads.

use std::cmp::Ordering;
use std::collections::HashSet;

use gateway_core::{
    AttributionApplyOutcome, AttributionPullRequestCommit, AttributionPullRequestWrite,
    AttributionReportQuery, StoreError,
};

use super::{
    AttributionStatement, Binds, Dialect, INSERT_CHUNK_ROWS, id, int, millis, optional_id,
    optional_millis, optional_text, report_conditions, report_limit, text,
};
use crate::shared::datetime_to_unix_millis;

/// Pull request columns in the order the decoders read them.
pub(super) const PULL_REQUEST_COLUMNS: &str = "attribution_pull_request_id, \
    ownership_scope_key, owner_user_id, owner_service_account_id, repository, number, title, \
    url, base_ref, head_ref, merge_commit_sha, merged_at, author_user_id, author_email_hash, \
    observed_at";
/// Number of [`PULL_REQUEST_COLUMNS`]; in a pull request report row the commit columns follow.
pub(crate) const PULL_REQUEST_COLUMN_COUNT: usize = 15;
pub(super) const PULL_REQUEST_COMMIT_COLUMNS: &str =
    "commit_sha, subject, authored_at, author_user_id, author_email_hash";
pub(super) const JOINED_PULL_REQUEST_COMMIT_COLUMNS: &str =
    "c.commit_sha, c.subject, c.authored_at, c.author_user_id, c.author_email_hash";

/// Inserts a new pull request row. Does nothing when its key is already stored.
pub(crate) fn insert_pull_request(
    dialect: Dialect,
    write: &AttributionPullRequestWrite,
) -> Result<AttributionStatement, StoreError> {
    let received_at = datetime_to_unix_millis(write.received_at)?;
    let mut binds = Binds::new(dialect);
    let row = binds.row([
        id(write.attribution_pull_request_id),
        text(&write.owner.ownership_scope_key),
        optional_id(write.owner.owner_user_id),
        optional_id(write.owner.owner_service_account_id),
        text(&write.repository),
        int(write.number),
        text(&write.title),
        optional_text(write.url.as_deref()),
        optional_text(write.base_ref.as_deref()),
        optional_text(write.head_ref.as_deref()),
        optional_text(write.merge_commit_sha.as_deref()),
        millis(write.merged_at)?,
        optional_id(write.author_user_id),
        optional_text(write.author_email_hash.as_deref()),
        millis(write.observed_at)?,
        int(received_at),
        int(received_at),
    ]);
    Ok(binds.finish(format!(
        "INSERT INTO attribution_pull_requests ({PULL_REQUEST_COLUMNS}, created_at, updated_at) \
         VALUES {row} ON CONFLICT (ownership_scope_key, repository, number) DO NOTHING"
    )))
}

/// The id and `observed_at` stored under `write`'s key, locked until the transaction ends.
pub(crate) fn select_pull_request_observed_at(
    dialect: Dialect,
    write: &AttributionPullRequestWrite,
) -> AttributionStatement {
    let mut binds = Binds::new(dialect);
    let scope = binds.push(text(&write.owner.ownership_scope_key));
    let repository = binds.push(text(&write.repository));
    let number = binds.push(int(write.number));
    binds.finish(format!(
        "SELECT attribution_pull_request_id, observed_at FROM attribution_pull_requests \
         WHERE ownership_scope_key = {scope} AND repository = {repository} \
         AND number = {number}{lock}",
        lock = dialect.row_lock()
    ))
}

/// Whether `write` replaces a pull request stored with `stored_observed_at` (unix ms).
pub(crate) fn pull_request_outcome(
    write: &AttributionPullRequestWrite,
    stored_observed_at: i64,
) -> Result<AttributionApplyOutcome, StoreError> {
    Ok(
        match datetime_to_unix_millis(write.observed_at)?.cmp(&stored_observed_at) {
            Ordering::Greater => AttributionApplyOutcome::Applied,
            Ordering::Equal => AttributionApplyOutcome::Duplicate,
            Ordering::Less => AttributionApplyOutcome::Stale,
        },
    )
}

/// Replaces a stored pull request, keeping its id and `created_at`.
pub(crate) fn update_pull_request(
    dialect: Dialect,
    attribution_pull_request_id: &str,
    write: &AttributionPullRequestWrite,
) -> Result<AttributionStatement, StoreError> {
    let mut binds = Binds::new(dialect);
    let set = binds.assignments(vec![
        ("title", text(&write.title)),
        ("url", optional_text(write.url.as_deref())),
        ("base_ref", optional_text(write.base_ref.as_deref())),
        ("head_ref", optional_text(write.head_ref.as_deref())),
        (
            "merge_commit_sha",
            optional_text(write.merge_commit_sha.as_deref()),
        ),
        ("merged_at", millis(write.merged_at)?),
        ("author_user_id", optional_id(write.author_user_id)),
        (
            "author_email_hash",
            optional_text(write.author_email_hash.as_deref()),
        ),
        ("observed_at", millis(write.observed_at)?),
        ("updated_at", millis(write.received_at)?),
    ]);
    let pull_request = binds.push(text(attribution_pull_request_id));
    Ok(binds.finish(format!(
        "UPDATE attribution_pull_requests SET {set} \
         WHERE attribution_pull_request_id = {pull_request}"
    )))
}

pub(crate) fn delete_pull_request_commits(
    dialect: Dialect,
    attribution_pull_request_id: &str,
) -> AttributionStatement {
    let mut binds = Binds::new(dialect);
    let pull_request = binds.push(text(attribution_pull_request_id));
    binds.finish(format!(
        "DELETE FROM attribution_pull_request_commits \
         WHERE attribution_pull_request_id = {pull_request}"
    ))
}

/// Inserts a pull request's commit list in order, one statement per chunk. A SHA listed twice
/// keeps its first position.
pub(crate) fn insert_pull_request_commits(
    dialect: Dialect,
    attribution_pull_request_id: &str,
    commits: &[AttributionPullRequestCommit],
) -> Result<Vec<AttributionStatement>, StoreError> {
    let mut seen = HashSet::new();
    let commits = commits
        .iter()
        .filter(|commit| seen.insert(commit.commit_sha.as_str()))
        .collect::<Vec<_>>();
    let mut statements = Vec::new();
    for (chunk_index, chunk) in commits.chunks(INSERT_CHUNK_ROWS).enumerate() {
        let mut binds = Binds::new(dialect);
        let mut rows = Vec::with_capacity(chunk.len());
        for (index, commit) in chunk.iter().enumerate() {
            let ordinal = i64::try_from(chunk_index * INSERT_CHUNK_ROWS + index)
                .map_err(|error| StoreError::Serialization(error.to_string()))?;
            rows.push(binds.row([
                text(attribution_pull_request_id),
                int(ordinal),
                text(&commit.commit_sha),
                optional_text(commit.subject.as_deref()),
                optional_millis(commit.authored_at)?,
                optional_id(commit.author_user_id),
                optional_text(commit.author_email_hash.as_deref()),
            ]));
        }
        statements.push(binds.finish(format!(
            "INSERT INTO attribution_pull_request_commits \
             (attribution_pull_request_id, ordinal, {PULL_REQUEST_COMMIT_COLUMNS}) VALUES {}",
            rows.join(", ")
        )));
    }
    Ok(statements)
}

/// A page of pull requests, newest merge first, each followed by its commits in list order:
/// one row per commit, or one row with NULL commit columns for an empty list.
pub(crate) fn list_pull_requests(
    dialect: Dialect,
    query: &AttributionReportQuery,
) -> Result<AttributionStatement, StoreError> {
    let mut binds = Binds::new(dialect);
    let conditions = report_conditions(
        &mut binds,
        query,
        &["merged_at"],
        &["owner_user_id", "author_user_id"],
    )?;
    let limit = report_limit(&mut binds, query);
    Ok(binds.finish(format!(
        "WITH page AS (SELECT {PULL_REQUEST_COLUMNS} FROM attribution_pull_requests \
         WHERE {conditions} ORDER BY merged_at DESC, attribution_pull_request_id DESC \
         LIMIT {limit}) \
         SELECT page.*, {JOINED_PULL_REQUEST_COMMIT_COLUMNS} FROM page \
         LEFT JOIN attribution_pull_request_commits c \
         ON c.attribution_pull_request_id = page.attribution_pull_request_id \
         ORDER BY page.merged_at DESC, page.attribution_pull_request_id DESC, c.ordinal"
    )))
}
