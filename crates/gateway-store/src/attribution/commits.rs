//! Commit note statements: replacement-key upsert, session and session hash rows and the
//! commit reads.

use std::collections::BTreeSet;

use gateway_core::{
    AttributionApplyOutcome, AttributionCommitSession, AttributionCommitWrite,
    AttributionFreshness, AttributionReportQuery, StoreError,
};
use time::OffsetDateTime;

use super::{
    AttributionBind, AttributionStatement, Binds, Dialect, INSERT_CHUNK_ROWS, check_hash_lookup,
    id, int, millis, optional_id, optional_text, report_conditions, report_limit, stored_time,
    text,
};
use crate::shared::datetime_to_unix_millis;

/// Commit columns in the order the decoders read them.
pub(super) const COMMIT_COLUMNS: &str = "attribution_commit_id, ownership_scope_key, \
    owner_user_id, owner_service_account_id, repository, commit_sha, branch, is_default_branch, \
    author_user_id, author_email_hash, schema_version, git_ai_version, ai_lines, human_lines, \
    installation_id, sequence, observed_at, late_sequenced, first_observed_at";
/// Number of [`COMMIT_COLUMNS`]; in a commit report row the session columns follow them.
pub(crate) const COMMIT_COLUMN_COUNT: usize = 19;
pub(super) const COMMIT_SESSION_COLUMNS: &str = "note_session_key, agent_tool, agent_model, \
    harness_key, ai_lines, human_author_user_id, human_author_hash";
/// [`COMMIT_SESSION_COLUMNS`] of the session joined as `s`. A commit report row follows them
/// with the session's hashes.
pub(super) const JOINED_COMMIT_SESSION_COLUMNS: &str = "s.note_session_key, s.agent_tool, \
    s.agent_model, s.harness_key, s.ai_lines, s.human_author_user_id, s.human_author_hash";

/// Inserts a new note's commit row. Does nothing when its replacement key is already stored.
pub(crate) fn insert_commit(
    dialect: Dialect,
    write: &AttributionCommitWrite,
) -> Result<AttributionStatement, StoreError> {
    let observed_at = datetime_to_unix_millis(write.freshness.observed_at)?;
    let received_at = datetime_to_unix_millis(write.received_at)?;
    let mut binds = Binds::new(dialect);
    let row = binds.row([
        id(write.attribution_commit_id),
        text(&write.owner.ownership_scope_key),
        optional_id(write.owner.owner_user_id),
        optional_id(write.owner.owner_service_account_id),
        text(&write.repository),
        text(&write.commit_sha),
        optional_text(write.branch.as_deref()),
        AttributionBind::Bool(write.is_default_branch),
        optional_id(write.author_user_id),
        optional_text(write.author_email_hash.as_deref()),
        text(&write.schema_version),
        optional_text(write.git_ai_version.as_deref()),
        int(write.ai_lines),
        int(write.human_lines),
        text(&write.freshness.installation_id),
        int(write.freshness.sequence),
        int(observed_at),
        AttributionBind::Bool(write.freshness.late_sequenced),
        int(observed_at),
        int(received_at),
        int(received_at),
    ]);
    Ok(binds.finish(format!(
        "INSERT INTO attribution_commits ({COMMIT_COLUMNS}, created_at, updated_at) VALUES {row} \
         ON CONFLICT (ownership_scope_key, repository, commit_sha) DO NOTHING"
    )))
}

/// The id and revision stored under `write`'s replacement key, as `(attribution_commit_id,
/// installation_id, sequence, observed_at, late_sequenced)`, locked until the transaction ends.
pub(crate) fn select_commit_freshness(
    dialect: Dialect,
    write: &AttributionCommitWrite,
) -> AttributionStatement {
    let mut binds = Binds::new(dialect);
    let scope = binds.push(text(&write.owner.ownership_scope_key));
    let repository = binds.push(text(&write.repository));
    let commit_sha = binds.push(text(&write.commit_sha));
    binds.finish(format!(
        "SELECT attribution_commit_id, installation_id, sequence, observed_at, late_sequenced \
         FROM attribution_commits WHERE ownership_scope_key = {scope} \
         AND repository = {repository} AND commit_sha = {commit_sha}{lock}",
        lock = dialect.row_lock()
    ))
}

/// Whether `write` replaces the stored revision, compared at stored precision.
pub(crate) fn commit_outcome(
    write: &AttributionCommitWrite,
    stored: &AttributionFreshness,
) -> Result<AttributionApplyOutcome, StoreError> {
    let incoming = AttributionFreshness {
        observed_at: stored_time(write.freshness.observed_at)?,
        ..write.freshness.clone()
    };
    Ok(incoming.decide_against(stored))
}

/// Replaces a stored commit's note, keeping its id, `first_observed_at` and `created_at`.
pub(crate) fn update_commit(
    dialect: Dialect,
    attribution_commit_id: &str,
    write: &AttributionCommitWrite,
) -> Result<AttributionStatement, StoreError> {
    let mut binds = Binds::new(dialect);
    let set = binds.assignments(vec![
        ("branch", optional_text(write.branch.as_deref())),
        (
            "is_default_branch",
            AttributionBind::Bool(write.is_default_branch),
        ),
        ("author_user_id", optional_id(write.author_user_id)),
        (
            "author_email_hash",
            optional_text(write.author_email_hash.as_deref()),
        ),
        ("schema_version", text(&write.schema_version)),
        (
            "git_ai_version",
            optional_text(write.git_ai_version.as_deref()),
        ),
        ("ai_lines", int(write.ai_lines)),
        ("human_lines", int(write.human_lines)),
        ("installation_id", text(&write.freshness.installation_id)),
        ("sequence", int(write.freshness.sequence)),
        ("observed_at", millis(write.freshness.observed_at)?),
        (
            "late_sequenced",
            AttributionBind::Bool(write.freshness.late_sequenced),
        ),
        ("updated_at", millis(write.received_at)?),
    ]);
    let commit = binds.push(text(attribution_commit_id));
    Ok(binds.finish(format!(
        "UPDATE attribution_commits SET {set} WHERE attribution_commit_id = {commit}"
    )))
}

/// Deletes a commit's session rows, their hashes first.
pub(crate) fn delete_commit_sessions(
    dialect: Dialect,
    attribution_commit_id: &str,
) -> Vec<AttributionStatement> {
    [
        "attribution_commit_session_hashes",
        "attribution_commit_sessions",
    ]
    .into_iter()
    .map(|table| {
        let mut binds = Binds::new(dialect);
        let commit = binds.push(text(attribution_commit_id));
        binds.finish(format!(
            "DELETE FROM {table} WHERE attribution_commit_id = {commit}"
        ))
    })
    .collect()
}

/// Inserts a commit's session rows and then their hashes, one statement per chunk. A repeated
/// `note_session_key` fails the insert; a hash repeated within a session is stored once.
pub(crate) fn insert_commit_sessions(
    dialect: Dialect,
    attribution_commit_id: &str,
    sessions: &[AttributionCommitSession],
) -> Vec<AttributionStatement> {
    let hashes = sessions
        .iter()
        .flat_map(|session| {
            let distinct = session.session_hashes.iter().collect::<BTreeSet<_>>();
            distinct
                .into_iter()
                .map(move |hash| (session.note_session_key.as_str(), hash.as_str()))
        })
        .collect::<Vec<_>>();
    sessions
        .chunks(INSERT_CHUNK_ROWS)
        .map(|chunk| insert_session_rows(dialect, attribution_commit_id, chunk))
        .chain(
            hashes
                .chunks(INSERT_CHUNK_ROWS)
                .map(|chunk| insert_hash_rows(dialect, attribution_commit_id, chunk)),
        )
        .collect()
}

fn insert_session_rows(
    dialect: Dialect,
    attribution_commit_id: &str,
    sessions: &[AttributionCommitSession],
) -> AttributionStatement {
    let mut binds = Binds::new(dialect);
    let rows = sessions
        .iter()
        .map(|session| {
            binds.row([
                text(attribution_commit_id),
                text(&session.note_session_key),
                text(&session.agent_tool),
                optional_text(session.agent_model.as_deref()),
                optional_text(session.harness_key.as_deref()),
                int(session.ai_lines),
                optional_id(session.human_author_user_id),
                optional_text(session.human_author_hash.as_deref()),
            ])
        })
        .collect::<Vec<_>>()
        .join(", ");
    binds.finish(format!(
        "INSERT INTO attribution_commit_sessions \
         (attribution_commit_id, {COMMIT_SESSION_COLUMNS}) VALUES {rows}"
    ))
}

/// Inserts `(note_session_key, session_hash)` rows for one commit.
fn insert_hash_rows(
    dialect: Dialect,
    attribution_commit_id: &str,
    hashes: &[(&str, &str)],
) -> AttributionStatement {
    let mut binds = Binds::new(dialect);
    let rows = hashes
        .iter()
        .map(|(note_session_key, session_hash)| {
            binds.row([
                text(attribution_commit_id),
                text(note_session_key),
                text(session_hash),
            ])
        })
        .collect::<Vec<_>>()
        .join(", ");
    binds.finish(format!(
        "INSERT INTO attribution_commit_session_hashes \
         (attribution_commit_id, note_session_key, session_hash) VALUES {rows}"
    ))
}

/// A page of commits, newest first, each followed by its sessions: one row per session with
/// its hashes in the last column, or one row with NULL session columns for a commit without
/// sessions.
pub(crate) fn list_commits(
    dialect: Dialect,
    query: &AttributionReportQuery,
) -> Result<AttributionStatement, StoreError> {
    let mut binds = Binds::new(dialect);
    let conditions = report_conditions(
        &mut binds,
        query,
        &["first_observed_at"],
        &["owner_user_id", "author_user_id"],
    )?;
    let limit = report_limit(&mut binds, query);
    Ok(binds.finish(format!(
        "WITH page AS (SELECT {COMMIT_COLUMNS} FROM attribution_commits WHERE {conditions} \
         ORDER BY first_observed_at DESC, attribution_commit_id DESC LIMIT {limit}) \
         SELECT page.*, {JOINED_COMMIT_SESSION_COLUMNS}, {hashes} FROM page \
         LEFT JOIN attribution_commit_sessions s \
         ON s.attribution_commit_id = page.attribution_commit_id \
         ORDER BY page.first_observed_at DESC, page.attribution_commit_id DESC, \
         s.note_session_key",
        hashes = dialect.hash_list(
            "attribution_commit_session_hashes",
            "h.attribution_commit_id = s.attribution_commit_id \
             AND h.note_session_key = s.note_session_key"
        )
    )))
}

/// Commit times at or before `until` for commit sessions with any of the hashes, as
/// `(ownership_scope_key, session_hash, attribution_commit_id, committed_at)`: one row per
/// commit and matched hash. `None` when there are no hashes to look up.
pub(crate) fn list_session_commit_times(
    dialect: Dialect,
    session_hashes: &[String],
    until: OffsetDateTime,
) -> Result<Option<AttributionStatement>, StoreError> {
    if !check_hash_lookup(session_hashes)? {
        return Ok(None);
    }
    let mut binds = Binds::new(dialect);
    let hashes = binds.push(AttributionBind::TextList(session_hashes.to_vec()));
    let until = binds.push(millis(until)?);
    Ok(Some(binds.finish(format!(
        "SELECT DISTINCT c.ownership_scope_key, h.session_hash, c.attribution_commit_id, \
         c.first_observed_at FROM attribution_commit_session_hashes h \
         JOIN attribution_commits c ON c.attribution_commit_id = h.attribution_commit_id \
         WHERE {} AND c.first_observed_at <= {until} \
         ORDER BY c.first_observed_at, c.attribution_commit_id, h.session_hash",
        dialect.any_of("h.session_hash", &hashes)
    ))))
}
