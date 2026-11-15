use gateway_core::{
    ATTRIBUTION_MAX_HASHES_PER_LOOKUP, AttributionCommitSession, AttributionOwner,
    AttributionPullRequestCommit, AttributionSessionEventKind, AttributionSessionEventWrite,
};
use time::{Duration, OffsetDateTime};

use super::commits::{COMMIT_COLUMNS, COMMIT_SESSION_COLUMNS, JOINED_COMMIT_SESSION_COLUMNS};
use super::pull_requests::{
    JOINED_PULL_REQUEST_COMMIT_COLUMNS, PULL_REQUEST_COLUMNS, PULL_REQUEST_COMMIT_COLUMNS,
};
use super::*;

fn column_count(columns: &str) -> usize {
    columns.split(',').count()
}

#[test]
fn column_counts_match_their_lists() {
    assert_eq!(column_count(COMMIT_COLUMNS), COMMIT_COLUMN_COUNT);
    assert_eq!(
        column_count(PULL_REQUEST_COLUMNS),
        PULL_REQUEST_COLUMN_COUNT
    );
    assert_eq!(
        column_count(COMMIT_SESSION_COLUMNS),
        column_count(JOINED_COMMIT_SESSION_COLUMNS)
    );
    assert_eq!(
        column_count(PULL_REQUEST_COMMIT_COLUMNS),
        column_count(JOINED_PULL_REQUEST_COMMIT_COLUMNS)
    );
}

fn start() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("time")
}

fn event(
    kind: AttributionSessionEventKind,
    observed_at: OffsetDateTime,
    head_sha: &str,
) -> AttributionSessionEventWrite {
    AttributionSessionEventWrite {
        owner: AttributionOwner {
            ownership_scope_key: "user:one".to_string(),
            owner_user_id: None,
            owner_service_account_id: None,
        },
        harness_key: "claude_code".to_string(),
        session_hash: "hash".to_string(),
        session_hashes: vec!["hash".to_string()],
        event: kind,
        repository: Some("github.com/acme/app".to_string()),
        branch: None,
        head_sha: Some(head_sha.to_string()),
        harness_version: None,
        observed_at,
        received_at: observed_at,
    }
}

#[test]
fn session_events_keep_the_earliest_start_and_the_latest_end() {
    let first = event(AttributionSessionEventKind::Start, start(), "aaa");
    let stored = session_from_event(&first).expect("first event");
    assert_eq!(
        merge_session_event(&stored, &first).expect("merge"),
        None,
        "a replayed event changes nothing"
    );
    let later_start = event(
        AttributionSessionEventKind::Start,
        start() + Duration::minutes(1),
        "bbb",
    );
    let merged = merge_session_event(&stored, &later_start)
        .expect("merge")
        .expect("a later observation moves last_observed_at");
    assert_eq!(merged.started_at, Some(start()));
    assert_eq!(merged.start_head_sha.as_deref(), Some("aaa"));

    let end = event(
        AttributionSessionEventKind::End,
        start() + Duration::hours(1),
        "ccc",
    );
    let ended = merge_session_event(&stored, &end)
        .expect("merge")
        .expect("an end changes the session");
    assert_eq!(ended.ended_at, Some(start() + Duration::hours(1)));
    assert_eq!(ended.end_head_sha.as_deref(), Some("ccc"));
    assert_eq!(ended.last_observed_at, start() + Duration::hours(1));
    let earlier_end = event(
        AttributionSessionEventKind::End,
        start() + Duration::minutes(30),
        "ddd",
    );
    assert_eq!(
        merge_session_event(&ended, &earlier_end).expect("merge"),
        None
    );
}

#[test]
fn session_descriptions_prefer_the_newest_observation() {
    let stored = session_from_event(&event(AttributionSessionEventKind::Start, start(), "a"))
        .expect("first event");
    let newer = AttributionSessionEventWrite {
        repository: Some("github.com/acme/renamed".to_string()),
        branch: Some("main".to_string()),
        ..event(
            AttributionSessionEventKind::End,
            start() + Duration::hours(1),
            "b",
        )
    };
    let merged = merge_session_event(&stored, &newer)
        .expect("merge")
        .expect("changed");
    assert_eq!(
        merged.repository.as_deref(),
        Some("github.com/acme/renamed")
    );
    assert_eq!(merged.branch.as_deref(), Some("main"));

    let older = AttributionSessionEventWrite {
        repository: Some("github.com/acme/old".to_string()),
        harness_version: Some("1.0.0".to_string()),
        ..event(
            AttributionSessionEventKind::Start,
            start() - Duration::minutes(5),
            "c",
        )
    };
    let merged = merge_session_event(&merged, &older)
        .expect("merge")
        .expect("changed");
    assert_eq!(
        merged.repository.as_deref(),
        Some("github.com/acme/renamed")
    );
    assert_eq!(merged.harness_version.as_deref(), Some("1.0.0"));
    assert_eq!(merged.started_at, Some(start() - Duration::minutes(5)));
    assert_eq!(merged.last_observed_at, start() + Duration::hours(1));
}

#[test]
fn session_times_compare_at_stored_precision() {
    let stored = session_from_event(&event(AttributionSessionEventKind::Start, start(), "a"))
        .expect("first event");
    let replay = event(
        AttributionSessionEventKind::Start,
        start() + Duration::microseconds(400),
        "a",
    );
    assert_eq!(merge_session_event(&stored, &replay).expect("merge"), None);
}

#[test]
fn session_hashes_accumulate_across_events() {
    let first = event(AttributionSessionEventKind::Start, start(), "a");
    let with = |hashes: &[&str]| AttributionSessionEventWrite {
        session_hashes: hashes.iter().map(|hash| (*hash).to_string()).collect(),
        ..first.clone()
    };
    let stored = session_from_event(&with(&["b", "a", "b"])).expect("first event");
    assert_eq!(stored.session_hashes, ["a", "b"]);
    assert_eq!(
        merge_session_event(&stored, &with(&["a"])).expect("merge"),
        None,
        "known hashes change nothing"
    );
    let merged = merge_session_event(&stored, &with(&["c"]))
        .expect("merge")
        .expect("a new hash changes the session");
    assert_eq!(merged.session_hashes, ["a", "b", "c"]);
}

#[test]
fn hash_lookups_are_bounded() {
    assert!(
        list_request_facts(Dialect::Libsql, &[], start())
            .expect("empty lookup")
            .is_none()
    );
    let hashes = vec!["secret-hash".to_string(); ATTRIBUTION_MAX_HASHES_PER_LOOKUP + 1];
    let error = list_session_commit_times(Dialect::Postgres, &hashes, start())
        .expect_err("too many hashes");
    assert!(!error.to_string().contains("secret-hash"));
}

#[test]
fn pull_request_commits_keep_the_first_position_of_a_repeated_sha() {
    let commit = |sha: &str| AttributionPullRequestCommit {
        commit_sha: sha.to_string(),
        subject: None,
        authored_at: None,
        author_user_id: None,
        author_email_hash: None,
    };
    let statements = insert_pull_request_commits(
        Dialect::Postgres,
        "pr",
        &[commit("a"), commit("b"), commit("a")],
    )
    .expect("statements");
    assert_eq!(statements.len(), 1);
    let binds = &statements[0].binds;
    assert_eq!(binds.len(), 14);
    assert_eq!(binds[1], AttributionBind::Int(Some(0)));
    assert_eq!(binds[2], AttributionBind::Text(Some("a".to_string())));
    assert_eq!(binds[8], AttributionBind::Int(Some(1)));
    assert_eq!(binds[9], AttributionBind::Text(Some("b".to_string())));
    assert!(statements[0].sql.contains("($8, $9, $10"));
}

#[test]
fn commit_sessions_insert_before_their_distinct_hashes() {
    let session = |key: &str, hashes: &[&str]| AttributionCommitSession {
        note_session_key: key.to_string(),
        agent_tool: "pi".to_string(),
        agent_model: None,
        harness_key: None,
        session_hashes: hashes.iter().map(|hash| (*hash).to_string()).collect(),
        ai_lines: 1,
        human_author_user_id: None,
        human_author_hash: None,
    };
    let statements = insert_commit_sessions(
        Dialect::Libsql,
        "commit",
        &[session("s_1", &["b", "a", "b"]), session("s_2", &[])],
    );
    assert_eq!(statements.len(), 2);
    assert!(
        statements[0]
            .sql
            .starts_with("INSERT INTO attribution_commit_sessions ")
    );
    assert!(
        statements[1]
            .sql
            .starts_with("INSERT INTO attribution_commit_session_hashes ")
    );
    assert_eq!(
        statements[1].binds,
        [
            text("commit"),
            text("s_1"),
            text("a"),
            text("commit"),
            text("s_1"),
            text("b"),
        ]
    );
}

#[test]
fn report_filters_bind_only_what_is_in_use() {
    let query = AttributionReportQuery {
        from: start() + Duration::microseconds(1),
        to: start() + Duration::days(1),
        user_id: None,
        repository: Some("github.com/acme/app".to_string()),
        limit: 5_000,
    };
    let statement = list_agent_sessions(Dialect::Libsql, &query).expect("statement");
    assert_eq!(
        statement.binds,
        vec![
            AttributionBind::Int(Some(1_800_000_000_001)),
            AttributionBind::Int(Some(1_800_086_400_000)),
            AttributionBind::Text(Some("github.com/acme/app".to_string())),
            AttributionBind::Int(Some(1_000)),
        ]
    );
    assert!(statement.sql.contains(
        "((started_at >= ?1 AND started_at <= ?2) OR (ended_at >= ?1 AND ended_at <= ?2)) \
         AND repository = ?3"
    ));
    assert!(!statement.sql.contains("owner_user_id ="));
}
