//! Harness session statements: the start/end merge, each session's hashes and the reads keyed
//! by session hash.

use std::collections::BTreeSet;

use gateway_core::{
    ATTRIBUTION_MAX_REQUEST_FACTS, AttributionAgentSessionRecord, AttributionReportQuery,
    AttributionSessionEventKind, AttributionSessionEventWrite, StoreError,
};
use time::OffsetDateTime;

use super::{
    AttributionBind, AttributionStatement, Binds, Dialect, INSERT_CHUNK_ROWS, check_hash_lookup,
    int, millis, optional_id, optional_millis, optional_text, report_conditions, report_limit,
    stored_time, text,
};
use crate::shared::datetime_to_unix_millis;

/// Agent session columns in the order the decoders read them. A read follows them with the
/// session's hashes; see [`selected_agent_session`].
const AGENT_SESSION_COLUMNS: &str = "ownership_scope_key, owner_user_id, \
    owner_service_account_id, harness_key, session_hash, repository, branch, start_head_sha, \
    end_head_sha, harness_version, started_at, ended_at, last_observed_at";

/// The columns a session read selects from `attribution_agent_sessions a`: the session
/// columns, then its hashes.
fn selected_agent_session(dialect: Dialect) -> String {
    let hashes = dialect.hash_list(
        "attribution_agent_session_hashes",
        "h.ownership_scope_key = a.ownership_scope_key AND h.harness_key = a.harness_key \
         AND h.agent_session_hash = a.session_hash",
    );
    format!("{AGENT_SESSION_COLUMNS}, {hashes}")
}

/// Ascending and distinct.
fn distinct_hashes<'a>(hashes: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    hashes
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .cloned()
        .collect()
}

/// The session row a first event for its key creates.
pub(crate) fn session_from_event(
    write: &AttributionSessionEventWrite,
) -> Result<AttributionAgentSessionRecord, StoreError> {
    let observed_at = stored_time(write.observed_at)?;
    let (started_at, start_head_sha, ended_at, end_head_sha) = match write.event {
        AttributionSessionEventKind::Start => {
            (Some(observed_at), write.head_sha.clone(), None, None)
        }
        AttributionSessionEventKind::End => (None, None, Some(observed_at), write.head_sha.clone()),
    };
    Ok(AttributionAgentSessionRecord {
        owner: write.owner.clone(),
        harness_key: write.harness_key.clone(),
        session_hash: write.session_hash.clone(),
        session_hashes: distinct_hashes(&write.session_hashes),
        repository: write.repository.clone(),
        branch: write.branch.clone(),
        start_head_sha,
        end_head_sha,
        harness_version: write.harness_version.clone(),
        started_at,
        ended_at,
        last_observed_at: observed_at,
    })
}

/// `stored` with `write` applied, or `None` when the event changes nothing. A start keeps the
/// earliest start and an end the latest end, each with its head. Repository, branch and
/// harness version take the newest observation's values, falling back to older ones where
/// the newest left them out. The hashes are the union of both.
pub(crate) fn merge_session_event(
    stored: &AttributionAgentSessionRecord,
    write: &AttributionSessionEventWrite,
) -> Result<Option<AttributionAgentSessionRecord>, StoreError> {
    let observed_at = stored_time(write.observed_at)?;
    let mut merged = stored.clone();
    match write.event {
        AttributionSessionEventKind::Start => {
            if stored
                .started_at
                .is_none_or(|started_at| observed_at < started_at)
            {
                merged.started_at = Some(observed_at);
                merged.start_head_sha = write.head_sha.clone();
            }
        }
        AttributionSessionEventKind::End => {
            if stored
                .ended_at
                .is_none_or(|ended_at| observed_at > ended_at)
            {
                merged.ended_at = Some(observed_at);
                merged.end_head_sha = write.head_sha.clone();
            }
        }
    }
    let incoming_is_newer = observed_at > stored.last_observed_at;
    let pick = |stored: &Option<String>, incoming: &Option<String>| {
        let (preferred, fallback) = if incoming_is_newer {
            (incoming, stored)
        } else {
            (stored, incoming)
        };
        preferred.clone().or_else(|| fallback.clone())
    };
    merged.repository = pick(&stored.repository, &write.repository);
    merged.branch = pick(&stored.branch, &write.branch);
    merged.harness_version = pick(&stored.harness_version, &write.harness_version);
    merged.last_observed_at = stored.last_observed_at.max(observed_at);
    merged.session_hashes =
        distinct_hashes(stored.session_hashes.iter().chain(&write.session_hashes));
    Ok((merged != *stored).then_some(merged))
}

/// Inserts a session row. Does nothing when its key is already stored.
pub(crate) fn insert_agent_session(
    dialect: Dialect,
    session: &AttributionAgentSessionRecord,
    received_at: OffsetDateTime,
) -> Result<AttributionStatement, StoreError> {
    let received_at = datetime_to_unix_millis(received_at)?;
    let mut binds = Binds::new(dialect);
    let row = binds.row([
        text(&session.owner.ownership_scope_key),
        optional_id(session.owner.owner_user_id),
        optional_id(session.owner.owner_service_account_id),
        text(&session.harness_key),
        text(&session.session_hash),
        optional_text(session.repository.as_deref()),
        optional_text(session.branch.as_deref()),
        optional_text(session.start_head_sha.as_deref()),
        optional_text(session.end_head_sha.as_deref()),
        optional_text(session.harness_version.as_deref()),
        optional_millis(session.started_at)?,
        optional_millis(session.ended_at)?,
        millis(session.last_observed_at)?,
        int(received_at),
        int(received_at),
    ]);
    Ok(binds.finish(format!(
        "INSERT INTO attribution_agent_sessions ({AGENT_SESSION_COLUMNS}, created_at, updated_at) \
         VALUES {row} ON CONFLICT (ownership_scope_key, harness_key, session_hash) DO NOTHING"
    )))
}

/// Adds `session`'s hashes to its stored ones, one statement per chunk.
pub(crate) fn insert_agent_session_hashes(
    dialect: Dialect,
    session: &AttributionAgentSessionRecord,
) -> Vec<AttributionStatement> {
    session
        .session_hashes
        .chunks(INSERT_CHUNK_ROWS)
        .map(|chunk| {
            let mut binds = Binds::new(dialect);
            let rows = chunk
                .iter()
                .map(|session_hash| {
                    binds.row([
                        text(&session.owner.ownership_scope_key),
                        text(&session.harness_key),
                        text(&session.session_hash),
                        text(session_hash),
                    ])
                })
                .collect::<Vec<_>>()
                .join(", ");
            binds.finish(format!(
                "INSERT INTO attribution_agent_session_hashes \
                 (ownership_scope_key, harness_key, agent_session_hash, session_hash) \
                 VALUES {rows} ON CONFLICT \
                 (ownership_scope_key, harness_key, agent_session_hash, session_hash) DO NOTHING"
            ))
        })
        .collect()
}

/// The stored session `write` reports on, locked until the transaction ends.
pub(crate) fn select_agent_session(
    dialect: Dialect,
    write: &AttributionSessionEventWrite,
) -> AttributionStatement {
    let mut binds = Binds::new(dialect);
    let scope = binds.push(text(&write.owner.ownership_scope_key));
    let harness_key = binds.push(text(&write.harness_key));
    let session_hash = binds.push(text(&write.session_hash));
    binds.finish(format!(
        "SELECT {columns} FROM attribution_agent_sessions a \
         WHERE ownership_scope_key = {scope} AND harness_key = {harness_key} \
         AND session_hash = {session_hash}{lock}",
        columns = selected_agent_session(dialect),
        lock = dialect.row_lock()
    ))
}

/// Stores a merged session over its row.
pub(crate) fn update_agent_session(
    dialect: Dialect,
    session: &AttributionAgentSessionRecord,
    received_at: OffsetDateTime,
) -> Result<AttributionStatement, StoreError> {
    let mut binds = Binds::new(dialect);
    let set = binds.assignments(vec![
        ("repository", optional_text(session.repository.as_deref())),
        ("branch", optional_text(session.branch.as_deref())),
        (
            "start_head_sha",
            optional_text(session.start_head_sha.as_deref()),
        ),
        (
            "end_head_sha",
            optional_text(session.end_head_sha.as_deref()),
        ),
        (
            "harness_version",
            optional_text(session.harness_version.as_deref()),
        ),
        ("started_at", optional_millis(session.started_at)?),
        ("ended_at", optional_millis(session.ended_at)?),
        ("last_observed_at", millis(session.last_observed_at)?),
        ("updated_at", millis(received_at)?),
    ]);
    let scope = binds.push(text(&session.owner.ownership_scope_key));
    let harness_key = binds.push(text(&session.harness_key));
    let session_hash = binds.push(text(&session.session_hash));
    Ok(binds.finish(format!(
        "UPDATE attribution_agent_sessions SET {set} WHERE ownership_scope_key = {scope} \
         AND harness_key = {harness_key} AND session_hash = {session_hash}"
    )))
}

/// Sessions that started or ended in range, most recently observed first, with their hashes.
pub(crate) fn list_agent_sessions(
    dialect: Dialect,
    query: &AttributionReportQuery,
) -> Result<AttributionStatement, StoreError> {
    let mut binds = Binds::new(dialect);
    let conditions = report_conditions(
        &mut binds,
        query,
        &["started_at", "ended_at"],
        &["owner_user_id"],
    )?;
    let limit = report_limit(&mut binds, query);
    Ok(binds.finish(format!(
        "SELECT {columns} FROM attribution_agent_sessions a WHERE {conditions} \
         ORDER BY last_observed_at DESC, harness_key, session_hash LIMIT {limit}",
        columns = selected_agent_session(dialect)
    )))
}

/// Gateway requests linked to the hashes at or before `until`, oldest first, as
/// `(ownership_scope_key, session_hash, request_id, occurred_at, model_key, cost_10000,
/// pricing_status)`. Usage joins on the request and the session's scope, so usage recorded
/// after the request was linked still counts. `None` when there are no hashes to look up.
pub(crate) fn list_request_facts(
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
    let limit = binds.push(int(i64::from(ATTRIBUTION_MAX_REQUEST_FACTS)));
    Ok(Some(binds.finish(format!(
        "SELECT s.ownership_scope_key, r.normalized_session_id, r.request_id, r.occurred_at, \
         COALESCE(l.resolved_model_key, l.model_key), \
         CASE WHEN u.pricing_status IN ('priced', 'legacy_estimated') \
         THEN u.computed_cost_10000 END, \
         u.pricing_status \
         FROM agent_session_requests r \
         JOIN agent_sessions s ON s.agent_session_id = r.agent_session_id \
         LEFT JOIN usage_cost_events u \
         ON u.request_id = r.request_id AND u.ownership_scope_key = s.ownership_scope_key \
         LEFT JOIN request_logs l ON l.request_log_id = r.request_log_id \
         WHERE {} AND r.occurred_at <= {until} \
         ORDER BY r.occurred_at, r.request_id LIMIT {limit}",
        dialect.any_of("r.normalized_session_id", &hashes)
    ))))
}
