//! Explicit request-to-journal associations, with the same revision rules as commit notes.

use gateway_core::{
    AttributionApplyOutcome, AttributionFreshness, AttributionRequestSessionWrite, StoreError,
};

use super::{
    AttributionBind, AttributionStatement, Binds, Dialect, id, int, millis, optional_id,
    stored_time, text,
};

pub(crate) fn insert_request_session(
    dialect: Dialect,
    write: &AttributionRequestSessionWrite,
) -> Result<AttributionStatement, StoreError> {
    let mut binds = Binds::new(dialect);
    let row = binds.row([
        text(&write.owner.ownership_scope_key),
        optional_id(write.owner.owner_user_id),
        optional_id(write.owner.owner_service_account_id),
        id(write.request_id),
        text(&write.session_hash),
        text(&write.freshness.installation_id),
        int(write.freshness.sequence),
        millis(write.freshness.observed_at)?,
        AttributionBind::Bool(write.freshness.late_sequenced),
        millis(write.received_at)?,
        millis(write.received_at)?,
    ]);
    Ok(binds.finish(format!(
        "INSERT INTO attribution_request_sessions (ownership_scope_key, owner_user_id, \
         owner_service_account_id, request_id, session_hash, installation_id, sequence, \
         observed_at, late_sequenced, created_at, updated_at) VALUES {row} \
         ON CONFLICT (ownership_scope_key, request_id) DO NOTHING"
    )))
}

/// The stored revision, locked until the transaction ends.
pub(crate) fn select_request_session_freshness(
    dialect: Dialect,
    write: &AttributionRequestSessionWrite,
) -> AttributionStatement {
    let mut binds = Binds::new(dialect);
    let scope = binds.push(text(&write.owner.ownership_scope_key));
    let request_id = binds.push(id(write.request_id));
    binds.finish(format!(
        "SELECT installation_id, sequence, observed_at, late_sequenced \
         FROM attribution_request_sessions WHERE ownership_scope_key = {scope} \
         AND request_id = {request_id}{lock}",
        lock = dialect.row_lock()
    ))
}

pub(crate) fn request_session_outcome(
    write: &AttributionRequestSessionWrite,
    stored: &AttributionFreshness,
) -> Result<AttributionApplyOutcome, StoreError> {
    let incoming = AttributionFreshness {
        observed_at: stored_time(write.freshness.observed_at)?,
        ..write.freshness.clone()
    };
    Ok(incoming.decide_against(stored))
}

pub(crate) fn update_request_session(
    dialect: Dialect,
    write: &AttributionRequestSessionWrite,
) -> Result<AttributionStatement, StoreError> {
    let mut binds = Binds::new(dialect);
    let set = binds.assignments(vec![
        ("session_hash", text(&write.session_hash)),
        ("installation_id", text(&write.freshness.installation_id)),
        ("sequence", int(write.freshness.sequence)),
        ("observed_at", millis(write.freshness.observed_at)?),
        (
            "late_sequenced",
            AttributionBind::Bool(write.freshness.late_sequenced),
        ),
        ("updated_at", millis(write.received_at)?),
    ]);
    let scope = binds.push(text(&write.owner.ownership_scope_key));
    let request_id = binds.push(id(write.request_id));
    Ok(binds.finish(format!(
        "UPDATE attribution_request_sessions SET {set} \
         WHERE ownership_scope_key = {scope} AND request_id = {request_id}"
    )))
}
