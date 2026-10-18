//! Backend-neutral helpers shared by the libsql and PostgreSQL audit log stores.

use gateway_core::{
    AuditActorKind, AuditLogCursor, AuditLogPage, AuditLogQuery, AuditLogRecord, NewAuditLogRecord,
    StoreError,
};
use serde_json::Value;

use crate::shared::datetime_to_unix_millis;

/// Reads the stored API-key actor. Rows written before V61 could hold any text
/// here, so an id that is not a UUID is kept readable in the label instead of
/// failing the whole row.
pub(crate) fn decode_api_key_actor(
    actor_api_key_id: Option<String>,
    actor_label: Option<String>,
) -> (Option<uuid::Uuid>, Option<String>) {
    match actor_api_key_id {
        None => (None, actor_label),
        Some(raw) => match uuid::Uuid::parse_str(&raw) {
            Ok(id) => (Some(id), actor_label),
            Err(_) => (None, actor_label.or(Some(format!("API key {raw}")))),
        },
    }
}

/// Column values for one insert, already converted to their stored form.
pub(crate) struct AuditLogRow {
    pub ts: i64,
    pub actor_kind: &'static str,
    pub actor_user_id: Option<String>,
    pub actor_api_key_id: Option<String>,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub details_json: String,
}

impl AuditLogRow {
    pub(crate) fn from_record(record: &NewAuditLogRecord) -> Result<Self, StoreError> {
        // Only API key actors carry a key reference; the column has a foreign key.
        let actor_api_key_id = match record.actor.kind {
            AuditActorKind::ApiKey => record.actor.api_key_id.map(|id| id.to_string()),
            AuditActorKind::User | AuditActorKind::System => None,
        };
        Ok(Self {
            ts: datetime_to_unix_millis(record.occurred_at)?,
            actor_kind: record.actor.kind.as_str(),
            actor_user_id: record.actor.user_id.map(|id| id.to_string()),
            actor_api_key_id,
            before_json: record.before.as_ref().map(to_json).transpose()?,
            after_json: record.after.as_ref().map(to_json).transpose()?,
            details_json: to_json(&record.metadata)?,
        })
    }
}

/// Rows per multi-row audit insert. Fourteen values each keeps a statement well
/// inside both backends' bind limits.
pub(crate) const AUDIT_INSERT_BATCH: usize = 200;

/// One value an audit insert binds.
pub(crate) enum AuditInsertValue {
    Int(i64),
    Text(Option<String>),
}

impl AuditLogRow {
    /// The values [`insert_many_sql`] binds for `record`, in column order.
    pub(crate) fn insert_values(self, record: &NewAuditLogRecord) -> [AuditInsertValue; 14] {
        use AuditInsertValue::{Int, Text};
        [
            Int(self.ts),
            Text(Some(self.actor_kind.to_string())),
            Text(self.actor_user_id),
            Text(self.actor_api_key_id),
            Text(record.actor.label.clone()),
            Text(Some(record.action.clone())),
            Text(Some(record.target_type.clone())),
            Text(record.target_id.clone()),
            Text(record.target_label.clone()),
            Text(self.before_json),
            Text(self.after_json),
            Text(Some(self.details_json)),
            Text(record.request_ip.clone()),
            Text(record.user_agent.clone()),
        ]
    }
}

/// An insert of `rows` audit entries in one statement. `placeholder` renders
/// the 1-based bind index in the backend's syntax.
pub(crate) fn insert_many_sql(rows: usize, placeholder: impl Fn(usize) -> String) -> String {
    let values = (0..rows)
        .map(|row| {
            let binds = (1..=14)
                .map(|column| placeholder(row * 14 + column))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({binds})")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO audit_logs (ts, actor_kind, actor_user_id, actor_api_key_id, actor_label, action, object_type, object_id, target_label, before_json, after_json, details_json, request_ip, user_agent) VALUES {values}"
    )
}

/// Bind values for a list query. `None` disables that filter.
pub(crate) struct AuditLogFilter {
    pub actor_user_id: Option<String>,
    pub action: Option<String>,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub cursor_ts: Option<i64>,
    pub cursor_id: Option<i64>,
    pub page_size: usize,
    /// One more than the page size, so the store can tell whether another page exists.
    pub fetch_limit: i64,
}

impl AuditLogFilter {
    pub(crate) fn from_query(query: &AuditLogQuery) -> Result<Self, StoreError> {
        let page_size = query.effective_limit();
        Ok(Self {
            actor_user_id: query.actor_user_id.map(|id| id.to_string()),
            action: query.action.clone(),
            target_type: query.target_type.clone(),
            target_id: query.target_id.clone(),
            start_ms: query
                .occurred_at_start
                .map(lower_bound_millis)
                .transpose()?,
            end_ms: query
                .occurred_at_end
                .map(datetime_to_unix_millis)
                .transpose()?,
            cursor_ts: query.cursor.map(|cursor| cursor.occurred_at_ms),
            cursor_id: query.cursor.map(|cursor| cursor.id),
            page_size: page_size as usize,
            fetch_limit: i64::from(page_size) + 1,
        })
    }
}

/// Stored timestamps are whole milliseconds, so an inclusive lower bound with a
/// sub-millisecond fraction rounds up; rounding down would admit earlier entries.
fn lower_bound_millis(value: time::OffsetDateTime) -> Result<i64, StoreError> {
    let millis = datetime_to_unix_millis(value)?;
    if value.unix_timestamp_nanos() % 1_000_000 == 0 {
        Ok(millis)
    } else {
        Ok(millis + 1)
    }
}

/// A bind value for the generated list query.
pub(crate) enum AuditLogBind {
    Text(String),
    Int(i64),
}

/// The list query's `SELECT ... WHERE ... ORDER BY ... LIMIT` with only the
/// filters in use, so each backend can serve it from the matching
/// `(column, ts, id)` index in page order instead of scanning past unused
/// `IS NULL` branches. `placeholder` renders the n-th (1-based) bind marker.
pub(crate) fn list_sql(
    filter: &AuditLogFilter,
    placeholder: impl Fn(usize) -> String,
) -> (String, Vec<AuditLogBind>) {
    let mut conditions = Vec::new();
    let mut binds = Vec::new();
    let mut push = |condition: &str, bind: AuditLogBind, binds: &mut Vec<AuditLogBind>| {
        binds.push(bind);
        conditions.push(condition.replace('?', &placeholder(binds.len())));
    };
    for (column, value) in [
        ("actor_user_id", &filter.actor_user_id),
        ("action", &filter.action),
        ("object_type", &filter.target_type),
        ("object_id", &filter.target_id),
    ] {
        if let Some(value) = value {
            push(
                &format!("{column} = ?"),
                AuditLogBind::Text(value.clone()),
                &mut binds,
            );
        }
    }
    if let Some(start) = filter.start_ms {
        push("ts >= ?", AuditLogBind::Int(start), &mut binds);
    }
    if let Some(end) = filter.end_ms {
        push("ts <= ?", AuditLogBind::Int(end), &mut binds);
    }
    if let (Some(cursor_ts), Some(cursor_id)) = (filter.cursor_ts, filter.cursor_id) {
        binds.push(AuditLogBind::Int(cursor_ts));
        let ts_marker = placeholder(binds.len());
        binds.push(AuditLogBind::Int(cursor_id));
        let id_marker = placeholder(binds.len());
        conditions.push(format!("(ts, id) < ({ts_marker}, {id_marker})"));
    }
    binds.push(AuditLogBind::Int(filter.fetch_limit));
    let limit_marker = placeholder(binds.len());
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    (
        format!(
            "SELECT {AUDIT_LOG_LIST_COLUMNS} FROM audit_logs {where_clause} \
             ORDER BY ts DESC, id DESC LIMIT {limit_marker}"
        ),
        binds,
    )
}

/// The list projection keeps the column order of [`AUDIT_LOG_COLUMNS`] but leaves
/// out the summaries, which can each reach 16 KiB.
pub(crate) const AUDIT_LOG_LIST_COLUMNS: &str = "id, ts, actor_kind, actor_user_id, \
    actor_api_key_id, actor_label, action, object_type, object_id, target_label, \
    NULL AS before_json, NULL AS after_json, '{}' AS details_json, request_ip, user_agent";

pub(crate) const AUDIT_LOG_COLUMNS: &str = "id, ts, actor_kind, actor_user_id, \
    actor_api_key_id, actor_label, action, object_type, object_id, target_label, \
    before_json, after_json, details_json, request_ip, user_agent";

pub(crate) fn finish_page(mut items: Vec<AuditLogRecord>, page_size: usize) -> AuditLogPage {
    let has_more = items.len() > page_size;
    items.truncate(page_size);
    let next_cursor = if has_more {
        items.last().map(|last| AuditLogCursor {
            occurred_at_ms: datetime_to_unix_millis(last.occurred_at).unwrap_or_default(),
            id: last.id,
        })
    } else {
        None
    };
    AuditLogPage { items, next_cursor }
}

fn to_json(value: &Value) -> Result<String, StoreError> {
    serde_json::to_string(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn parse_json(raw: &str) -> Result<Value, StoreError> {
    serde_json::from_str(raw).map_err(|error| StoreError::Serialization(error.to_string()))
}

pub(crate) fn parse_optional_json(raw: Option<&str>) -> Result<Option<Value>, StoreError> {
    raw.map(parse_json).transpose()
}

/// Shared contract test, run against each backend.
#[cfg(test)]
pub(crate) async fn exercise_audit_log_store(store: &impl gateway_core::AuditLogRepository) {
    use gateway_core::{AuditActor, AuditLogQuery};
    use serde_json::json;
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    let admin = Uuid::new_v4();
    let base = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let record = |action: &str, target_type: &str, offset_ms: i64| NewAuditLogRecord {
        occurred_at: base + Duration::milliseconds(offset_ms),
        actor: AuditActor::user(admin, "admin@example.test"),
        action: action.to_string(),
        target_type: target_type.to_string(),
        target_id: Some(format!("{target_type}-{offset_ms}")),
        target_label: Some("Label".to_string()),
        before: Some(json!({ "status": "active" })),
        after: Some(json!({ "status": "disabled" })),
        metadata: json!({ "request_id": "req-1" }),
        request_ip: Some("203.0.113.9".to_string()),
        user_agent: Some("tests".to_string()),
    };
    // Two entries share a millisecond, so the cursor has to break ties on id.
    let inputs = [
        record("team.created", "team", 0),
        record("team.updated", "team", 5),
        record("team.updated", "team", 5),
        record("api_key.revoked", "api_key", 9),
    ];
    let mut ids = Vec::new();
    for input in &inputs {
        ids.push(
            store
                .insert_audit_log(input)
                .await
                .expect("insert audit log"),
        );
    }

    let everything = store
        .list_audit_logs(&AuditLogQuery::default())
        .await
        .expect("list audit logs");
    assert_eq!(everything.next_cursor, None);
    let listed = &everything.items[0];
    assert_eq!(listed.id, ids[3]);
    assert_eq!(listed.before, None, "the list leaves summaries out");
    assert_eq!(listed.metadata, json!({}));
    let newest = store
        .get_audit_log(ids[3])
        .await
        .expect("get audit log")
        .expect("entry exists");
    assert_eq!(newest.id, ids[3]);
    assert_eq!(newest.occurred_at, inputs[3].occurred_at);
    assert_eq!(newest.actor, inputs[3].actor);
    assert_eq!(newest.before, inputs[3].before);
    assert_eq!(newest.after, inputs[3].after);
    assert_eq!(newest.metadata, inputs[3].metadata);
    assert_eq!(newest.request_ip, inputs[3].request_ip);
    assert_eq!(newest.user_agent, inputs[3].user_agent);
    assert_eq!(listed.target_label, newest.target_label);
    assert!(
        store
            .get_audit_log(i64::MAX)
            .await
            .expect("missing")
            .is_none()
    );
    assert_eq!(
        everything
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![ids[3], ids[2], ids[1], ids[0]]
    );

    let mut seen = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list_audit_logs(&AuditLogQuery {
                target_type: Some("team".to_string()),
                cursor,
                limit: 1,
                ..Default::default()
            })
            .await
            .expect("page");
        seen.extend(page.items.iter().map(|item| item.id));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(seen, vec![ids[2], ids[1], ids[0]]);

    let filtered = store
        .list_audit_logs(&AuditLogQuery {
            actor_user_id: Some(admin),
            action: Some("team.updated".to_string()),
            target_id: Some("team-5".to_string()),
            occurred_at_start: Some(base + Duration::milliseconds(5)),
            occurred_at_end: Some(base + Duration::milliseconds(5)),
            ..Default::default()
        })
        .await
        .expect("filtered");
    assert_eq!(filtered.items.len(), 2);

    let nobody = store
        .list_audit_logs(&AuditLogQuery {
            actor_user_id: Some(Uuid::new_v4()),
            ..Default::default()
        })
        .await
        .expect("other actor");
    assert!(nobody.items.is_empty());

    // A batch spanning more than one statement lands whole, with every column.
    let bulk: Vec<_> = (0..AUDIT_INSERT_BATCH + 5)
        .map(|index| NewAuditLogRecord {
            target_id: Some(format!("binding-{index}")),
            target_label: None,
            before: None,
            request_ip: None,
            ..record(
                "mcp_credential_binding.revoked",
                "mcp_credential_binding",
                20,
            )
        })
        .collect();
    store
        .insert_audit_logs(&bulk)
        .await
        .expect("insert audit logs");
    let mut written = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list_audit_logs(&AuditLogQuery {
                action: Some("mcp_credential_binding.revoked".to_string()),
                cursor,
                limit: 100,
                ..AuditLogQuery::default()
            })
            .await
            .expect("list bulk entries");
        written.extend(page.items);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(written.len(), bulk.len());
    let one = store
        .get_audit_log(written[0].id)
        .await
        .expect("get bulk entry")
        .expect("bulk entry exists");
    assert_eq!(one.actor, bulk[0].actor);
    assert_eq!(one.occurred_at, bulk[0].occurred_at);
    assert_eq!(one.target_label, None);
    assert_eq!(one.before, None);
    assert_eq!(one.after, bulk[0].after);
    assert_eq!(one.metadata, bulk[0].metadata);
    assert_eq!(one.request_ip, None);
    assert_eq!(one.user_agent, bulk[0].user_agent);
    store
        .insert_audit_logs(&[])
        .await
        .expect("an empty batch writes nothing");
}

#[cfg(test)]
mod tests {
    use time::{Duration, OffsetDateTime};

    use super::lower_bound_millis;

    #[test]
    fn lower_bounds_round_sub_millisecond_fractions_up() {
        let whole = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        assert_eq!(lower_bound_millis(whole).unwrap(), 1_800_000_000_000);
        assert_eq!(
            lower_bound_millis(whole + Duration::nanoseconds(1)).unwrap(),
            1_800_000_000_001
        );
        assert_eq!(
            lower_bound_millis(whole + Duration::microseconds(1_500)).unwrap(),
            1_800_000_000_002
        );
    }
}
