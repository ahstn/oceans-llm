//! Git AI attribution storage on both backends
//! (ADR `2026-11-01-git-ai-attribution-via-oceans-hooks`).

use gateway_core::{
    ATTRIBUTION_MAX_HASHES_PER_LOOKUP, AgentSessionRecord, AgentSessionRequestLinkRecord,
    ApiKeyAccess, ApiKeyModelGrantMode, ApiKeyOwnerKind, AttributionAgentSessionRecord,
    AttributionApplyOutcome, AttributionCommitRecord, AttributionCommitSession,
    AttributionCommitWrite, AttributionFreshness, AttributionOwner, AttributionPullRequestCommit,
    AttributionPullRequestRecord, AttributionPullRequestWrite, AttributionReportQuery,
    AttributionRequestFact, AttributionSessionCommitTime, AttributionSessionEventKind,
    AttributionSessionEventWrite, AuthMode, Confidence, GlobalRole, NewApiKeyRecord,
    RequestLogRecord, RequestTags, RequestToolCardinality, SessionLifecycleState, StoreError,
    UsagePricingStatus, UserStatus,
};
use serde_json::Map;
use serial_test::serial;
use tempfile::{TempDir, tempdir};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::tests::{
    PostgresTestDatabase, build_usage_ledger_record, create_postgres_test_database,
    drop_postgres_test_database,
};
use crate::{
    GatewayStore, LibsqlStore, PostgresStore, StoreConnectionOptions, run_migrations,
    run_migrations_with_options,
};

const REPOSITORY: &str = "github.com/acme/app";

macro_rules! on_both_backends {
    ($($libsql:ident, $postgres:ident => $scenario:ident;)*) => {
        $(
            #[tokio::test]
            #[serial]
            async fn $libsql() {
                let (_directory, store) = libsql_store().await;
                $scenario(&store).await;
            }

            #[tokio::test]
            #[serial]
            async fn $postgres() {
                let Some(database) = create_postgres_test_database().await else {
                    eprintln!("skipping postgres attribution test: TEST_POSTGRES_URL is not set");
                    return;
                };
                let store = postgres_store(&database).await;
                $scenario(&store).await;
                drop(store);
                drop_postgres_test_database(&database).await;
            }
        )*
    };
}

on_both_backends! {
    libsql_attribution_commits_replace_by_freshness,
    postgres_attribution_commits_replace_by_freshness => commits_replace_by_freshness;
    libsql_attribution_pull_requests_replace_by_observation,
    postgres_attribution_pull_requests_replace_by_observation =>
        pull_requests_replace_by_observation;
    libsql_attribution_agent_sessions_merge_events,
    postgres_attribution_agent_sessions_merge_events => agent_sessions_merge_events;
    libsql_attribution_session_lookups,
    postgres_attribution_session_lookups => session_lookups;
    libsql_attribution_reports_filter_and_owner_deletion,
    postgres_attribution_reports_filter_and_owner_deletion => reports_filter_and_owner_deletion;
}

async fn libsql_store() -> (TempDir, LibsqlStore) {
    let directory = tempdir().expect("tempdir");
    let path = directory.path().join("gateway.db");
    run_migrations(&path).await.expect("migrations");
    let store = LibsqlStore::new_local(path.to_str().expect("database path"))
        .await
        .expect("store");
    (directory, store)
}

async fn postgres_store(database: &PostgresTestDatabase) -> PostgresStore {
    run_migrations_with_options(&StoreConnectionOptions::Postgres {
        url: database.database_url.clone(),
        max_connections: 4,
    })
    .await
    .expect("migrations");
    PostgresStore::connect(&database.database_url, 4)
        .await
        .expect("store")
}

fn base() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("base time")
}

fn report(from: OffsetDateTime, to: OffsetDateTime) -> AttributionReportQuery {
    AttributionReportQuery {
        from,
        to,
        user_id: None,
        repository: None,
        limit: 100,
    }
}

fn day() -> AttributionReportQuery {
    report(base() - Duration::days(1), base() + Duration::days(1))
}

async fn user_owner<S: GatewayStore>(store: &S, name: &str) -> AttributionOwner {
    let email = format!("{name}@example.com");
    let user = store
        .create_identity_user(
            name,
            &email,
            &email,
            GlobalRole::User,
            AuthMode::Password,
            UserStatus::Active,
        )
        .await
        .expect("user");
    AttributionOwner {
        ownership_scope_key: format!("user:{}", user.user_id),
        owner_user_id: Some(user.user_id),
        owner_service_account_id: None,
    }
}

fn freshness(
    installation_id: &str,
    sequence: i64,
    observed_at: OffsetDateTime,
) -> AttributionFreshness {
    AttributionFreshness {
        installation_id: installation_id.to_string(),
        sequence,
        observed_at,
        late_sequenced: false,
    }
}

fn session(note_session_key: &str, session_hashes: &[&str]) -> AttributionCommitSession {
    AttributionCommitSession {
        note_session_key: note_session_key.to_string(),
        agent_tool: "pi".to_string(),
        agent_model: Some("claude-opus".to_string()),
        harness_key: Some("oh_my_pi".to_string()),
        session_hashes: session_hashes
            .iter()
            .map(|hash| (*hash).to_string())
            .collect(),
        ai_lines: 12,
        human_author_user_id: None,
        human_author_hash: Some("sha256:human".to_string()),
    }
}

fn commit(
    owner: &AttributionOwner,
    repository: &str,
    commit_sha: &str,
    observed_at: OffsetDateTime,
    sessions: Vec<AttributionCommitSession>,
) -> AttributionCommitWrite {
    AttributionCommitWrite {
        attribution_commit_id: Uuid::new_v4(),
        owner: owner.clone(),
        repository: repository.to_string(),
        commit_sha: commit_sha.to_string(),
        branch: Some("main".to_string()),
        is_default_branch: true,
        author_user_id: owner.owner_user_id,
        author_email_hash: Some("sha256:email".to_string()),
        schema_version: "authorship/3.0.0".to_string(),
        git_ai_version: Some("1.2.3".to_string()),
        ai_lines: 20,
        human_lines: 5,
        freshness: freshness("installation-a", 1, observed_at),
        sessions,
        received_at: observed_at,
    }
}

/// The record `write` reads back as once stored under `attribution_commit_id`.
fn stored_commit(
    write: &AttributionCommitWrite,
    attribution_commit_id: Uuid,
    first_observed_at: OffsetDateTime,
) -> AttributionCommitRecord {
    AttributionCommitRecord {
        attribution_commit_id,
        owner: write.owner.clone(),
        repository: write.repository.clone(),
        commit_sha: write.commit_sha.clone(),
        branch: write.branch.clone(),
        is_default_branch: write.is_default_branch,
        author_user_id: write.author_user_id,
        author_email_hash: write.author_email_hash.clone(),
        schema_version: write.schema_version.clone(),
        git_ai_version: write.git_ai_version.clone(),
        ai_lines: write.ai_lines,
        human_lines: write.human_lines,
        freshness: write.freshness.clone(),
        first_observed_at,
        sessions: write.sessions.clone(),
    }
}

async fn commit_shas<S: GatewayStore>(store: &S, query: AttributionReportQuery) -> Vec<String> {
    store
        .list_attribution_commits(&query)
        .await
        .expect("commit report")
        .into_iter()
        .map(|commit| commit.commit_sha)
        .collect()
}

/// The commits whose sessions carry any of `hashes`, with the matched hash, as stored by
/// `base() + 1 day`.
async fn commits_matching<S: GatewayStore>(store: &S, hashes: &[&str]) -> Vec<(Uuid, String)> {
    let hashes = hashes
        .iter()
        .map(|hash| (*hash).to_string())
        .collect::<Vec<_>>();
    store
        .list_attribution_session_commit_times(&hashes, base() + Duration::days(1))
        .await
        .expect("commit times")
        .into_iter()
        .map(|time| (time.attribution_commit_id, time.session_hash))
        .collect()
}

fn pull_request(
    owner: &AttributionOwner,
    number: i64,
    merged_at: OffsetDateTime,
    commit_shas: &[&str],
) -> AttributionPullRequestWrite {
    AttributionPullRequestWrite {
        attribution_pull_request_id: Uuid::new_v4(),
        owner: owner.clone(),
        repository: REPOSITORY.to_string(),
        number,
        title: "Add attribution".to_string(),
        url: Some(format!("https://github.com/acme/app/pull/{number}")),
        base_ref: Some("main".to_string()),
        head_ref: Some("feature".to_string()),
        merge_commit_sha: Some("merge-sha".to_string()),
        merged_at,
        author_user_id: owner.owner_user_id,
        author_email_hash: Some("sha256:email".to_string()),
        commits: commit_shas
            .iter()
            .map(|sha| AttributionPullRequestCommit {
                commit_sha: (*sha).to_string(),
                subject: Some(format!("commit {sha}")),
                authored_at: Some(merged_at - Duration::hours(1)),
                author_user_id: None,
                author_email_hash: Some("sha256:email".to_string()),
            })
            .collect(),
        observed_at: merged_at,
        received_at: merged_at,
    }
}

fn stored_pull_request(
    write: &AttributionPullRequestWrite,
    attribution_pull_request_id: Uuid,
    commits: Vec<AttributionPullRequestCommit>,
) -> AttributionPullRequestRecord {
    AttributionPullRequestRecord {
        attribution_pull_request_id,
        owner: write.owner.clone(),
        repository: write.repository.clone(),
        number: write.number,
        title: write.title.clone(),
        url: write.url.clone(),
        base_ref: write.base_ref.clone(),
        head_ref: write.head_ref.clone(),
        merge_commit_sha: write.merge_commit_sha.clone(),
        merged_at: write.merged_at,
        author_user_id: write.author_user_id,
        author_email_hash: write.author_email_hash.clone(),
        commits,
        observed_at: write.observed_at,
    }
}

fn session_event(
    owner: &AttributionOwner,
    event: AttributionSessionEventKind,
    observed_at: OffsetDateTime,
    head_sha: &str,
) -> AttributionSessionEventWrite {
    AttributionSessionEventWrite {
        owner: owner.clone(),
        harness_key: "claude_code".to_string(),
        session_hash: "hash-session".to_string(),
        session_hashes: vec!["hash-session".to_string(), "hash-oceans".to_string()],
        event,
        repository: Some(REPOSITORY.to_string()),
        branch: Some("main".to_string()),
        head_sha: Some(head_sha.to_string()),
        harness_version: Some("2.1.0".to_string()),
        observed_at,
        received_at: observed_at,
    }
}

async fn commits_replace_by_freshness<S: GatewayStore>(store: &S) {
    let alice = user_owner(store, "alice").await;
    let first = commit(
        &alice,
        REPOSITORY,
        "sha-1",
        base(),
        vec![
            session("s_1", &["hash-1"]),
            session("s_2", &["hash-2", "alt-2", "hash-2"]),
            session("s_9", &[]),
        ],
    );
    let apply = |write: AttributionCommitWrite| async move {
        store
            .apply_attribution_commit(&write)
            .await
            .expect("apply commit")
    };
    assert_eq!(apply(first.clone()).await, AttributionApplyOutcome::Applied);
    assert_eq!(
        apply(first.clone()).await,
        AttributionApplyOutcome::Duplicate
    );
    let stored_hashes = store
        .list_attribution_commits(&day())
        .await
        .expect("report")
        .remove(0)
        .sessions
        .into_iter()
        .map(|session| session.session_hashes)
        .collect::<Vec<_>>();
    assert_eq!(
        stored_hashes,
        [
            vec!["hash-1".to_string()],
            vec!["alt-2".to_string(), "hash-2".to_string()],
            Vec::new(),
        ],
        "hashes read back ascending and distinct"
    );

    let replacement = AttributionCommitWrite {
        attribution_commit_id: Uuid::new_v4(),
        branch: None,
        is_default_branch: false,
        ai_lines: 40,
        freshness: freshness("installation-a", 2, base() + Duration::minutes(5)),
        sessions: vec![session("s_1", &["hash-1b", "hash-3"])],
        ..first.clone()
    };
    assert_eq!(
        apply(replacement.clone()).await,
        AttributionApplyOutcome::Applied
    );
    assert_eq!(
        commits_matching(store, &["hash-1", "hash-2", "alt-2", "hash-3"]).await,
        vec![(first.attribution_commit_id, "hash-3".to_string())],
        "the replaced note's hashes are gone"
    );
    let older = AttributionCommitWrite {
        freshness: freshness("installation-a", 1, base() + Duration::hours(1)),
        ..first.clone()
    };
    assert_eq!(apply(older).await, AttributionApplyOutcome::Stale);
    // Another installation observed within the stored millisecond is not newer.
    let same_millisecond = AttributionCommitWrite {
        freshness: freshness(
            "installation-b",
            9,
            base() + Duration::minutes(5) + Duration::microseconds(400),
        ),
        ..first.clone()
    };
    assert_eq!(
        apply(same_millisecond).await,
        AttributionApplyOutcome::Stale
    );

    // The same commit reported under another owner is a separate row.
    let bob = user_owner(store, "bob").await;
    let bobs = commit(
        &bob,
        REPOSITORY,
        "sha-1",
        base() + Duration::minutes(1),
        Vec::new(),
    );
    assert_eq!(apply(bobs.clone()).await, AttributionApplyOutcome::Applied);

    assert_eq!(
        store
            .list_attribution_commits(&day())
            .await
            .expect("report"),
        vec![
            stored_commit(
                &bobs,
                bobs.attribution_commit_id,
                bobs.freshness.observed_at
            ),
            stored_commit(&replacement, first.attribution_commit_id, base()),
        ]
    );
}

async fn pull_requests_replace_by_observation<S: GatewayStore>(store: &S) {
    let alice = user_owner(store, "alice").await;
    let apply = |write: AttributionPullRequestWrite| async move {
        store
            .apply_attribution_pull_request(&write)
            .await
            .expect("apply pull request")
    };
    let first = pull_request(&alice, 7, base(), &["c-2", "c-1", "c-2"]);
    assert_eq!(apply(first.clone()).await, AttributionApplyOutcome::Applied);
    assert_eq!(
        apply(first.clone()).await,
        AttributionApplyOutcome::Duplicate
    );
    assert_eq!(
        store
            .list_attribution_pull_requests(&day())
            .await
            .expect("report"),
        vec![stored_pull_request(
            &first,
            first.attribution_pull_request_id,
            first.commits[..2].to_vec()
        )]
    );

    let replacement = AttributionPullRequestWrite {
        attribution_pull_request_id: Uuid::new_v4(),
        title: "Add attribution hooks".to_string(),
        commits: Vec::new(),
        observed_at: base() + Duration::minutes(1),
        ..first.clone()
    };
    assert_eq!(
        apply(replacement.clone()).await,
        AttributionApplyOutcome::Applied
    );
    let older = AttributionPullRequestWrite {
        title: "Stale title".to_string(),
        observed_at: base() - Duration::minutes(1),
        ..first.clone()
    };
    assert_eq!(apply(older).await, AttributionApplyOutcome::Stale);
    assert_eq!(
        store
            .list_attribution_pull_requests(&day())
            .await
            .expect("report"),
        vec![stored_pull_request(
            &replacement,
            first.attribution_pull_request_id,
            Vec::new()
        )]
    );
}

async fn agent_sessions_merge_events<S: GatewayStore>(store: &S) {
    let alice = user_owner(store, "alice").await;
    let apply = |write: AttributionSessionEventWrite| async move {
        store
            .apply_attribution_session_event(&write)
            .await
            .expect("apply session event")
    };
    let start = session_event(&alice, AttributionSessionEventKind::Start, base(), "aaa");
    assert_eq!(apply(start.clone()).await, AttributionApplyOutcome::Applied);
    assert_eq!(apply(start).await, AttributionApplyOutcome::Duplicate);
    let end_at = base() + Duration::hours(1);
    let end = session_event(&alice, AttributionSessionEventKind::End, end_at, "bbb");
    assert_eq!(apply(end).await, AttributionApplyOutcome::Applied);
    let earlier_end = session_event(
        &alice,
        AttributionSessionEventKind::End,
        base() + Duration::minutes(30),
        "ccc",
    );
    assert_eq!(apply(earlier_end).await, AttributionApplyOutcome::Duplicate);
    // A later gateway hashing under one more namespace widens the stored list.
    let widened = AttributionSessionEventWrite {
        session_hashes: vec!["hash-codex".to_string()],
        ..session_event(&alice, AttributionSessionEventKind::Start, base(), "aaa")
    };
    assert_eq!(
        apply(widened.clone()).await,
        AttributionApplyOutcome::Applied
    );
    assert_eq!(apply(widened).await, AttributionApplyOutcome::Duplicate);

    let expected = AttributionAgentSessionRecord {
        owner: alice,
        harness_key: "claude_code".to_string(),
        session_hash: "hash-session".to_string(),
        session_hashes: vec![
            "hash-codex".to_string(),
            "hash-oceans".to_string(),
            "hash-session".to_string(),
        ],
        repository: Some(REPOSITORY.to_string()),
        branch: Some("main".to_string()),
        start_head_sha: Some("aaa".to_string()),
        end_head_sha: Some("bbb".to_string()),
        harness_version: Some("2.1.0".to_string()),
        started_at: Some(base()),
        ended_at: Some(end_at),
        last_observed_at: end_at,
    };
    let list = |query: AttributionReportQuery| async move {
        store
            .list_attribution_agent_sessions(&query)
            .await
            .expect("session report")
    };
    assert_eq!(list(day()).await, vec![expected.clone()]);
    let around_end = report(base() + Duration::minutes(50), base() + Duration::hours(2));
    assert_eq!(list(around_end).await, vec![expected]);
    let between = report(
        base() + Duration::minutes(10),
        base() + Duration::minutes(50),
    );
    assert!(list(between).await.is_empty());
}

async fn session_lookups<S: GatewayStore>(store: &S) {
    let alice = user_owner(store, "alice").await;
    // One agent session under two note keys, and a session without an id.
    let early = commit(
        &alice,
        REPOSITORY,
        "sha-early",
        base(),
        vec![
            session("s_1", &["alt-a", "hash-a"]),
            session("abc1234", &["hash-a"]),
            session("s_2", &[]),
        ],
    );
    let late = commit(
        &alice,
        REPOSITORY,
        "sha-late",
        base() + Duration::hours(2),
        vec![session("s_1", &["hash-a"])],
    );
    for write in [&early, &late] {
        store.apply_attribution_commit(write).await.expect("commit");
    }
    let until = base() + Duration::hours(1);
    let hashes = [
        "alt-a".to_string(),
        "hash-a".to_string(),
        "hash-unused".to_string(),
    ];
    let time = |session_hash: &str| AttributionSessionCommitTime {
        ownership_scope_key: alice.ownership_scope_key.clone(),
        session_hash: session_hash.to_string(),
        attribution_commit_id: early.attribution_commit_id,
        committed_at: base(),
    };
    assert_eq!(
        store
            .list_attribution_session_commit_times(&hashes, until)
            .await
            .expect("commit times"),
        vec![time("alt-a"), time("hash-a")]
    );
    assert!(
        store
            .list_attribution_session_commit_times(&[], until)
            .await
            .expect("no hashes")
            .is_empty()
    );
    let too_many = vec!["hash".to_string(); ATTRIBUTION_MAX_HASHES_PER_LOOKUP + 1];
    assert!(matches!(
        store.list_attribution_request_facts(&too_many, until).await,
        Err(StoreError::Unexpected(_))
    ));
    request_facts_join_usage(store, &alice).await;
}

async fn open_agent_session<S: GatewayStore>(
    store: &S,
    owner: &AttributionOwner,
) -> AgentSessionRecord {
    let api_key = store
        .create_api_key(&NewApiKeyRecord {
            name: "attribution".to_string(),
            public_id: format!("attribution-{}", Uuid::new_v4()),
            secret_hash: "hash".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::All,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: owner.owner_user_id,
            owner_team_id: None,
            owner_service_account_id: None,
            access: ApiKeyAccess::default(),
            created_at: base(),
        })
        .await
        .expect("api key");
    let session = AgentSessionRecord {
        agent_session_id: Uuid::new_v4(),
        agent_session_source_id: None,
        ownership_scope_key: owner.ownership_scope_key.clone(),
        api_key_id: api_key.id,
        user_id: owner.owner_user_id,
        team_id: None,
        service_account_id: None,
        actor_user_id: None,
        harness_key: "claude_code".to_string(),
        requested_model_key: "requested-model".to_string(),
        operation: "responses".to_string(),
        caller_class: "user".to_string(),
        request_tags: serde_json::json!({}),
        boundary_group_key: "sha256:boundary".to_string(),
        boundary_policy_version: agent_session_analysis::SESSION_BOUNDARY_POLICY_VERSION
            .to_string(),
        lifecycle: SessionLifecycleState::Open,
        boundary_confidence: Confidence::High,
        started_at: base(),
        ended_at: None,
        input_watermark_at: base(),
        finalized_reason: None,
        created_at: base(),
        updated_at: base(),
    };
    assert!(
        store
            .insert_agent_session_if_absent(&session)
            .await
            .expect("agent session")
    );
    session
}

async fn link_request<S: GatewayStore>(
    store: &S,
    session: &AgentSessionRecord,
    request_id: &str,
    session_hash: &str,
    occurred_at: OffsetDateTime,
) {
    let request_log_id = Uuid::new_v4();
    let log = RequestLogRecord {
        request_log_id,
        request_id: request_id.to_string(),
        api_key_id: session.api_key_id,
        user_id: session.user_id,
        team_id: None,
        service_account_id: None,
        model_key: "requested-model".to_string(),
        resolved_model_key: "resolved-model".to_string(),
        provider_key: "openai-prod".to_string(),
        status_code: Some(200),
        latency_ms: Some(10),
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        error_code: None,
        has_payload: false,
        request_payload_truncated: false,
        response_payload_truncated: false,
        request_tags: RequestTags::default(),
        tool_cardinality: RequestToolCardinality::default(),
        user_agent_raw: None,
        agent_harness_key: "claude_code".to_string(),
        agent_harness_label: "Claude Code".to_string(),
        metadata: Map::new(),
        occurred_at,
    };
    store
        .insert_request_log(&log, None)
        .await
        .expect("request log");
    let link = AgentSessionRequestLinkRecord {
        agent_session_id: session.agent_session_id,
        request_id: request_id.to_string(),
        request_log_id: Some(request_log_id),
        usage_event_id: None,
        ordinal: 0,
        execution_id: None,
        parent_execution_id: None,
        normalized_session_id: Some(session_hash.to_string()),
        correlation_confidence: Confidence::High,
        limitation_codes: Vec::new(),
        occurred_at,
        completed_at: None,
        terminal_success: None,
    };
    store
        .append_agent_session_request(&link)
        .await
        .expect("request link");
}

async fn request_facts_join_usage<S: GatewayStore>(store: &S, owner: &AttributionOwner) {
    let session = open_agent_session(store, owner).await;
    for (request_id, session_hash, offset) in [
        ("req-priced", "hash-a", 0),
        ("req-unpriced", "hash-a", 1),
        ("req-late", "hash-a", 7_200),
        ("req-other-session", "hash-b", 2),
    ] {
        let occurred_at = base() + Duration::seconds(offset);
        link_request(store, &session, request_id, session_hash, occurred_at).await;
    }
    // Usage recorded after the request was linked still counts.
    for (request_id, status, cost_10000) in [
        ("req-priced", UsagePricingStatus::Priced, 1_234),
        ("req-unpriced", UsagePricingStatus::Unpriced, 0),
    ] {
        let usage = build_usage_ledger_record(
            request_id,
            owner.ownership_scope_key.clone(),
            session.api_key_id,
            owner.owner_user_id,
            None,
            None,
            None,
            "gpt-test",
            status,
            cost_10000,
            base(),
        );
        store
            .insert_usage_ledger_if_absent(&usage)
            .await
            .expect("usage");
    }
    let fact = |request_id: &str, offset: i64, cost_10000: Option<i64>, status: &str| {
        AttributionRequestFact {
            ownership_scope_key: owner.ownership_scope_key.clone(),
            session_hash: "hash-a".to_string(),
            request_id: request_id.to_string(),
            occurred_at: base() + Duration::seconds(offset),
            model_key: Some("resolved-model".to_string()),
            cost_10000,
            pricing_status: Some(status.to_string()),
        }
    };
    assert_eq!(
        store
            .list_attribution_request_facts(&["hash-a".to_string()], base() + Duration::hours(1))
            .await
            .expect("request facts"),
        vec![
            fact("req-priced", 0, Some(1_234), "priced"),
            fact("req-unpriced", 1, None, "unpriced"),
        ]
    );
}

async fn reports_filter_and_owner_deletion<S: GatewayStore>(store: &S) {
    let alice = user_owner(store, "alice").await;
    let bob = user_owner(store, "bob").await;
    let alices = commit(
        &alice,
        REPOSITORY,
        "sha-1",
        base(),
        vec![
            session("s_1", &["hash-1", "hash-1b"]),
            session("s_2", &["hash-2"]),
        ],
    );
    let alices_api = commit(
        &alice,
        "github.com/acme/api",
        "sha-2",
        base() + Duration::minutes(1),
        Vec::new(),
    );
    let bobs_authored_by_alice = AttributionCommitWrite {
        author_user_id: alice.owner_user_id,
        ..commit(
            &bob,
            REPOSITORY,
            "sha-3",
            base() + Duration::minutes(2),
            vec![session("s_1", &["hash-3"])],
        )
    };
    for write in [&alices, &alices_api, &bobs_authored_by_alice] {
        store.apply_attribution_commit(write).await.expect("commit");
    }
    let by_user = |user: &AttributionOwner| AttributionReportQuery {
        user_id: user.owner_user_id,
        ..day()
    };
    assert_eq!(commit_shas(store, by_user(&bob)).await, vec!["sha-3"]);
    assert_eq!(
        commit_shas(store, by_user(&alice)).await,
        vec!["sha-3", "sha-2", "sha-1"]
    );
    let in_repository = AttributionReportQuery {
        repository: Some(REPOSITORY.to_string()),
        ..day()
    };
    assert_eq!(
        commit_shas(store, in_repository).await,
        vec!["sha-3", "sha-1"]
    );
    let limited = AttributionReportQuery { limit: 2, ..day() };
    assert_eq!(commit_shas(store, limited).await, vec!["sha-3", "sha-2"]);
    // A `from` inside a millisecond starts at the next stored millisecond.
    let after_sha_2 = report(
        base() + Duration::minutes(1) + Duration::microseconds(1),
        base() + Duration::minutes(2),
    );
    assert_eq!(commit_shas(store, after_sha_2).await, vec!["sha-3"]);

    let alices_pull_request = pull_request(&alice, 1, base(), &["sha-1", "sha-2"]);
    store
        .apply_attribution_pull_request(&alices_pull_request)
        .await
        .expect("pull request");
    let start = session_event(&alice, AttributionSessionEventKind::Start, base(), "aaa");
    store
        .apply_attribution_session_event(&start)
        .await
        .expect("session event");
    // Two commits with two sessions and three hashes, a pull request with two commits, and a
    // session with two hashes.
    let scope = alice.ownership_scope_key.as_str();
    assert_eq!(
        store
            .delete_attribution_for_owner(scope)
            .await
            .expect("delete"),
        13
    );
    assert_eq!(
        store
            .delete_attribution_for_owner(scope)
            .await
            .expect("repeat"),
        0
    );
    assert_eq!(commit_shas(store, day()).await, vec!["sha-3"]);
    assert_eq!(
        commits_matching(store, &["hash-1", "hash-2", "hash-3"]).await,
        vec![(
            bobs_authored_by_alice.attribution_commit_id,
            "hash-3".to_string()
        )]
    );
    assert!(
        store
            .list_attribution_pull_requests(&day())
            .await
            .expect("pull requests")
            .is_empty()
    );
    assert!(
        store
            .list_attribution_agent_sessions(&day())
            .await
            .expect("sessions")
            .is_empty()
    );
}
