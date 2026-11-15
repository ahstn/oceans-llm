//! Backend-neutral Git AI attribution statements and write rules, shared by the libsql and
//! PostgreSQL stores (ADR `2026-11-01-git-ai-attribution-via-oceans-hooks`).
//!
//! Each builder renders one statement for a [`Dialect`] with its binds in stored form, so both
//! backends run the same SQL. The backends own their transactions, binding and row decoding.
//! Times are stored as unix milliseconds.

mod commits;
mod pull_requests;
mod sessions;
#[cfg(test)]
mod tests;

use gateway_core::{
    ATTRIBUTION_MAX_HASHES_PER_LOOKUP, ATTRIBUTION_MAX_REPORT_ROWS, AttributionOwner,
    AttributionReportQuery, StoreError,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::shared::{
    datetime_to_unix_millis, lower_bound_millis, parse_uuid, unix_millis_to_datetime,
};

pub(crate) use commits::{
    COMMIT_COLUMN_COUNT, commit_outcome, delete_commit_sessions, insert_commit,
    insert_commit_sessions, list_commits, list_session_commit_times, select_commit_freshness,
    update_commit,
};
pub(crate) use pull_requests::{
    PULL_REQUEST_COLUMN_COUNT, delete_pull_request_commits, insert_pull_request,
    insert_pull_request_commits, list_pull_requests, pull_request_outcome,
    select_pull_request_observed_at, update_pull_request,
};
pub(crate) use sessions::{
    insert_agent_session, insert_agent_session_hashes, list_agent_sessions, list_request_facts,
    merge_session_event, select_agent_session, session_from_event, update_agent_session,
};

/// Rows per multi-row insert. At no more than ten binds a row, a chunk stays far inside both
/// backends' bind limits.
const INSERT_CHUNK_ROWS: usize = 500;

/// The SQL that differs between the two backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    Libsql,
    Postgres,
}

impl Dialect {
    fn marker(self, index: usize) -> String {
        match self {
            Self::Libsql => format!("?{index}"),
            Self::Postgres => format!("${index}"),
        }
    }

    /// Lock for a read that decides a write in the same transaction. A libsql write
    /// transaction already holds the database write lock from `BEGIN IMMEDIATE`.
    fn row_lock(self) -> &'static str {
        match self {
            Self::Libsql => "",
            Self::Postgres => " FOR UPDATE",
        }
    }

    /// `column` equals one of the strings bound as [`AttributionBind::TextList`] at `marker`.
    fn any_of(self, column: &str, marker: &str) -> String {
        match self {
            Self::Libsql => format!("{column} IN (SELECT value FROM json_each({marker}))"),
            Self::Postgres => format!("{column} = ANY({marker})"),
        }
    }

    /// The `session_hash` values of the `table` rows, aliased `h`, that match `correlation`: a
    /// JSON array for libsql, a sorted `TEXT[]` for PostgreSQL (NULL when none match).
    fn hash_list(self, table: &str, correlation: &str) -> String {
        let aggregate = match self {
            Self::Libsql => "json_group_array(h.session_hash)",
            Self::Postgres => "array_agg(h.session_hash ORDER BY h.session_hash)",
        };
        format!("(SELECT {aggregate} FROM {table} h WHERE {correlation})")
    }
}

/// One value an attribution statement binds, in its stored form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttributionBind {
    Int(Option<i64>),
    Text(Option<String>),
    Bool(bool),
    /// Matched by [`Dialect::any_of`]: a JSON array for libsql, `TEXT[]` for PostgreSQL.
    TextList(Vec<String>),
}

/// A statement rendered for one dialect, with its binds in marker order.
#[derive(Debug)]
pub(crate) struct AttributionStatement {
    pub sql: String,
    pub binds: Vec<AttributionBind>,
}

/// Collects a statement's binds and hands out their markers.
struct Binds {
    dialect: Dialect,
    values: Vec<AttributionBind>,
}

impl Binds {
    fn new(dialect: Dialect) -> Self {
        Self {
            dialect,
            values: Vec::new(),
        }
    }

    fn push(&mut self, value: AttributionBind) -> String {
        self.values.push(value);
        self.dialect.marker(self.values.len())
    }

    /// A parenthesised `VALUES` row binding `values`.
    fn row(&mut self, values: impl IntoIterator<Item = AttributionBind>) -> String {
        let markers = values
            .into_iter()
            .map(|value| self.push(value))
            .collect::<Vec<_>>();
        format!("({})", markers.join(", "))
    }

    /// A `SET` clause binding each column's value.
    fn assignments(&mut self, columns: Vec<(&str, AttributionBind)>) -> String {
        columns
            .into_iter()
            .map(|(column, value)| format!("{column} = {}", self.push(value)))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn finish(self, sql: String) -> AttributionStatement {
        AttributionStatement {
            sql,
            binds: self.values,
        }
    }
}

fn text(value: &str) -> AttributionBind {
    AttributionBind::Text(Some(value.to_string()))
}

fn optional_text(value: Option<&str>) -> AttributionBind {
    AttributionBind::Text(value.map(str::to_string))
}

fn id(value: Uuid) -> AttributionBind {
    AttributionBind::Text(Some(value.to_string()))
}

fn optional_id(value: Option<Uuid>) -> AttributionBind {
    AttributionBind::Text(value.map(|value| value.to_string()))
}

fn int(value: i64) -> AttributionBind {
    AttributionBind::Int(Some(value))
}

fn millis(value: OffsetDateTime) -> Result<AttributionBind, StoreError> {
    Ok(int(datetime_to_unix_millis(value)?))
}

fn optional_millis(value: Option<OffsetDateTime>) -> Result<AttributionBind, StoreError> {
    Ok(AttributionBind::Int(
        value.map(datetime_to_unix_millis).transpose()?,
    ))
}

/// `value` at the millisecond precision it is stored with.
fn stored_time(value: OffsetDateTime) -> Result<OffsetDateTime, StoreError> {
    unix_millis_to_datetime(datetime_to_unix_millis(value)?)
}

/// Decodes stored ownership columns.
pub(crate) fn owner(
    ownership_scope_key: String,
    owner_user_id: Option<String>,
    owner_service_account_id: Option<String>,
) -> Result<AttributionOwner, StoreError> {
    Ok(AttributionOwner {
        ownership_scope_key,
        owner_user_id: optional_uuid(owner_user_id)?,
        owner_service_account_id: optional_uuid(owner_service_account_id)?,
    })
}

pub(crate) fn optional_uuid(raw: Option<String>) -> Result<Option<Uuid>, StoreError> {
    raw.as_deref().map(parse_uuid).transpose()
}

pub(crate) fn optional_time(raw: Option<i64>) -> Result<Option<OffsetDateTime>, StoreError> {
    raw.map(unix_millis_to_datetime).transpose()
}

/// A write's insert found its key taken, but the row was gone when read back: another
/// transaction deleted it in between. Retrying the write stores it.
pub(crate) fn conflicting_row_missing() -> StoreError {
    StoreError::Conflict("attribution row was deleted concurrently; retry the write".to_string())
}

/// Refuses lookups over the per-call hash limit. Returns whether there is anything to look up.
fn check_hash_lookup(session_hashes: &[String]) -> Result<bool, StoreError> {
    if session_hashes.len() > ATTRIBUTION_MAX_HASHES_PER_LOOKUP {
        return Err(StoreError::Unexpected(format!(
            "attribution lookups accept at most {ATTRIBUTION_MAX_HASHES_PER_LOOKUP} session hashes"
        )));
    }
    Ok(!session_hashes.is_empty())
}

/// The report filters for rows with any of `time_columns` in range and the user in any of
/// `user_columns`. Only the filters in use are bound, so each backend can pick the matching
/// index.
fn report_conditions(
    binds: &mut Binds,
    query: &AttributionReportQuery,
    time_columns: &[&str],
    user_columns: &[&str],
) -> Result<String, StoreError> {
    let from = binds.push(int(lower_bound_millis(query.from)?));
    let to = binds.push(millis(query.to)?);
    let in_range = time_columns
        .iter()
        .map(|column| format!("({column} >= {from} AND {column} <= {to})"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut conditions = vec![format!("({in_range})")];
    if let Some(user_id) = query.user_id {
        let user = binds.push(id(user_id));
        let matches = user_columns
            .iter()
            .map(|column| format!("{column} = {user}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        conditions.push(format!("({matches})"));
    }
    if let Some(repository) = query.repository.as_deref() {
        conditions.push(format!("repository = {}", binds.push(text(repository))));
    }
    Ok(conditions.join(" AND "))
}

fn report_limit(binds: &mut Binds, query: &AttributionReportQuery) -> String {
    binds.push(int(i64::from(query.limit.min(ATTRIBUTION_MAX_REPORT_ROWS))))
}

/// Deletes an ownership scope's attribution rows, children before their parents, so every
/// deleted row is counted.
pub(crate) fn delete_for_owner(
    dialect: Dialect,
    ownership_scope_key: &str,
) -> Vec<AttributionStatement> {
    [
        "DELETE FROM attribution_commit_session_hashes WHERE attribution_commit_id IN \
         (SELECT attribution_commit_id FROM attribution_commits WHERE ownership_scope_key = {scope})",
        "DELETE FROM attribution_commit_sessions WHERE attribution_commit_id IN \
         (SELECT attribution_commit_id FROM attribution_commits WHERE ownership_scope_key = {scope})",
        "DELETE FROM attribution_commits WHERE ownership_scope_key = {scope}",
        "DELETE FROM attribution_pull_request_commits WHERE attribution_pull_request_id IN \
         (SELECT attribution_pull_request_id FROM attribution_pull_requests \
         WHERE ownership_scope_key = {scope})",
        "DELETE FROM attribution_pull_requests WHERE ownership_scope_key = {scope}",
        "DELETE FROM attribution_agent_session_hashes WHERE ownership_scope_key = {scope}",
        "DELETE FROM attribution_agent_sessions WHERE ownership_scope_key = {scope}",
    ]
    .into_iter()
    .map(|template| {
        let mut binds = Binds::new(dialect);
        let scope = binds.push(text(ownership_scope_key));
        binds.finish(template.replace("{scope}", &scope))
    })
    .collect()
}
