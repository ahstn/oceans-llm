//! Git AI attribution storage on both backends
//! (ADR `2026-11-01-git-ai-attribution-via-oceans-hooks`).

use gateway_core::{
    ATTRIBUTION_MAX_HASHES_PER_LOOKUP, AgentSessionRecord, AgentSessionRequestLinkRecord,
    ApiKeyAccess, ApiKeyModelGrantMode, ApiKeyOwnerKind, AttributionAgentSessionRecord,
    AttributionApplyOutcome, AttributionCommitRecord, AttributionCommitSession,
    AttributionCommitWrite, AttributionFreshness, AttributionOwner, AttributionPullRequestCommit,
    AttributionPullRequestDetails, AttributionPullRequestLifecycle, AttributionPullRequestRecord,
    AttributionPullRequestState, AttributionPullRequestWrite, AttributionReportQuery,
    AttributionRequestFact, AttributionRequestSessionWrite, AttributionRevertEvidence,
    AttributionRevertLink, AttributionSessionCommitTime, AttributionSessionEventKind,
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
    libsql_attribution_pull_requests_keep_their_lifecycle,
    postgres_attribution_pull_requests_keep_their_lifecycle => pull_requests_keep_their_lifecycle;
    libsql_attribution_agent_sessions_merge_events,
    postgres_attribution_agent_sessions_merge_events => agent_sessions_merge_events;
    libsql_attribution_request_sessions_override_native_identity,
    postgres_attribution_request_sessions_override_native_identity => request_sessions_override_native_identity;
    libsql_attribution_codex_requests_use_exact_execution,
    postgres_attribution_codex_requests_use_exact_execution => codex_requests_use_exact_execution;
    libsql_attribution_codex_turn_ids_keep_native_fallback,
    postgres_attribution_codex_turn_ids_keep_native_fallback => codex_turn_ids_keep_native_fallback;
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
        merged_at: Some(merged_at),
        lifecycle: gateway_core::AttributionPullRequestLifecycle {
            closed_at: Some(merged_at),
            ..Default::default()
        },
        lifecycle_reported: true,
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
        lifecycle: write.lifecycle.clone(),
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

/// An open pull request is stored without a merge time, lists by its opened time, and moves on
/// to merged when re-sent, keeping its activity time when the re-send has none; its details
/// JSON round-trips.
async fn pull_requests_keep_their_lifecycle<S: GatewayStore>(store: &S) {
    let alice = user_owner(store, "alice").await;
    let merged = pull_request(&alice, 9, base(), &["c-1"]);
    let open = AttributionPullRequestWrite {
        merged_at: None,
        merge_commit_sha: None,
        lifecycle: AttributionPullRequestLifecycle {
            state: AttributionPullRequestState::Open,
            opened_at: Some(base() - Duration::hours(2)),
            closed_at: None,
            last_activity_at: Some(base() - Duration::minutes(30)),
            details: AttributionPullRequestDetails {
                is_draft: Some(true),
                first_review_at: Some(base() - Duration::hours(1)),
                review_count: Some(2),
                approval_count: Some(1),
                additions: Some(40),
                reverts: Some(AttributionRevertLink {
                    pull_request_number: Some(3),
                    commit_shas: vec!["abcdef1".to_string()],
                    evidence: AttributionRevertEvidence::ChangeMatch,
                }),
                ..AttributionPullRequestDetails::default()
            },
        },
        observed_at: base() - Duration::hours(1),
        ..merged.clone()
    };
    store
        .apply_attribution_pull_request(&open)
        .await
        .expect("apply open pull request");
    let listed = store
        .list_attribution_pull_requests(&day())
        .await
        .expect("report");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].merged_at, None);
    assert_eq!(listed[0].lifecycle, open.lifecycle);
    // The report reads an open pull request for its lifecycle, so its commits aren't joined.
    assert!(listed[0].commits.is_empty());

    assert_eq!(
        store
            .apply_attribution_pull_request(&merged)
            .await
            .expect("apply merge"),
        AttributionApplyOutcome::Applied
    );
    let listed = store
        .list_attribution_pull_requests(&day())
        .await
        .expect("report");
    assert_eq!(listed[0].merged_at, Some(base()));
    assert_eq!(listed[0].commits.len(), 1);
    // The merge write carries no activity time, so the stored one stays.
    let after_merge = AttributionPullRequestLifecycle {
        last_activity_at: open.lifecycle.last_activity_at,
        ..merged.lifecycle.clone()
    };
    assert_eq!(listed[0].lifecycle, after_merge);

    // A hook older than the lifecycle fields re-sends it merged with no lifecycle: the merge
    // is recorded and the stored lifecycle stays.
    let legacy = AttributionPullRequestWrite {
        title: "Retitled".to_string(),
        lifecycle: AttributionPullRequestLifecycle::default(),
        lifecycle_reported: false,
        observed_at: base() + Duration::minutes(1),
        ..merged.clone()
    };
    assert_eq!(
        store
            .apply_attribution_pull_request(&legacy)
            .await
            .expect("apply legacy re-send"),
        AttributionApplyOutcome::Applied
    );
    let listed = store
        .list_attribution_pull_requests(&day())
        .await
        .expect("report");
    assert_eq!(listed[0].title, "Retitled");
    assert_eq!(listed[0].merged_at, Some(base()));
    assert_eq!(listed[0].lifecycle, after_merge);
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
    harness_key: &str,
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
        harness_key: harness_key.to_string(),
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
    execution_id: Option<&str>,
    parent_execution_id: Option<&str>,
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
        agent_harness_key: session.harness_key.clone(),
        agent_harness_label: session.harness_key.clone(),
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
        execution_id: execution_id.map(str::to_string),
        parent_execution_id: parent_execution_id.map(str::to_string),
        normalized_session_id: Some(session_hash.to_string()),
        correlation_confidence: Confidence::High,
        limitation_codes: Vec::new(),
        occurred_at,
        completed_at: None,
        terminal_success: None,
    };
    store
        .append_agent_session_input(gateway_core::AgentSessionInput::request(&link))
        .await
        .map(|result| result.request_inserted)
        .expect("request link");
}

async fn request_facts_join_usage<S: GatewayStore>(store: &S, owner: &AttributionOwner) {
    let session = open_agent_session(store, owner, "claude_code").await;
    for (request_id, session_hash, offset) in [
        ("req-priced", "hash-a", 0),
        ("req-unpriced", "hash-a", 1),
        ("req-late", "hash-a", 7_200),
        ("req-other-session", "hash-b", 2),
    ] {
        let occurred_at = base() + Duration::seconds(offset);
        link_request(
            store,
            &session,
            request_id,
            session_hash,
            occurred_at,
            None,
            None,
        )
        .await;
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
            cost_nanos: cost_10000.map(|cost| cost * 100_000),
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

async fn request_sessions_override_native_identity<S: GatewayStore>(store: &S) {
    let alice = user_owner(store, "alice").await;
    let bob = user_owner(store, "bob").await;
    let first_id = Uuid::from_u128(201);
    let second_id = Uuid::from_u128(202);
    let native_id = Uuid::from_u128(203);
    let mapping =
        |owner: &AttributionOwner, request_id, hash: &str| AttributionRequestSessionWrite {
            owner: owner.clone(),
            request_id,
            session_hash: hash.to_string(),
            freshness: freshness("installation-a", 2, base()),
            received_at: base(),
        };
    let first = mapping(&alice, first_id, "journal-child-hash");
    assert_eq!(
        store
            .apply_attribution_request_session(&first)
            .await
            .expect("mapping before request"),
        AttributionApplyOutcome::Applied
    );
    assert!(
        store
            .list_attribution_request_facts(std::slice::from_ref(&first.session_hash), base())
            .await
            .expect("no request yet")
            .is_empty()
    );
    let stale_hash = AttributionRequestSessionWrite {
        session_hash: "must-not-replace".to_string(),
        ..first.clone()
    };
    assert_eq!(
        store
            .apply_attribution_request_session(&stale_hash)
            .await
            .expect("replay"),
        AttributionApplyOutcome::Duplicate
    );
    let mut stale = stale_hash.clone();
    stale.freshness.sequence = 1;
    assert_eq!(
        store
            .apply_attribution_request_session(&stale)
            .await
            .expect("older sequence"),
        AttributionApplyOutcome::Stale
    );
    stale.freshness.sequence = 3;
    stale.freshness.late_sequenced = true;
    stale.freshness.observed_at -= Duration::milliseconds(1);
    assert_eq!(
        store
            .apply_attribution_request_session(&stale)
            .await
            .expect("late stale observation"),
        AttributionApplyOutcome::Stale
    );
    stale.freshness = freshness("installation-b", 9, base() + Duration::microseconds(400));
    assert_eq!(
        store
            .apply_attribution_request_session(&stale)
            .await
            .expect("stored precision"),
        AttributionApplyOutcome::Stale
    );

    let native = open_agent_session(store, &alice, "claude_code").await;
    for id in [first_id, second_id, native_id] {
        link_request(
            store,
            &native,
            &id.to_string(),
            "provider-parent-hash",
            base(),
            None,
            None,
        )
        .await;
    }
    let parent_facts = store
        .list_attribution_request_facts(&["provider-parent-hash".to_string()], base())
        .await
        .expect("native fallback");
    assert_eq!(
        parent_facts.len(),
        2,
        "explicit mapping excludes the first request from its native parent"
    );
    assert!(
        parent_facts
            .iter()
            .all(|fact| fact.request_id != first_id.to_string())
    );

    let second = mapping(&alice, second_id, "journal-child-hash");
    store
        .apply_attribution_request_session(&second)
        .await
        .expect("mapping after request");
    let mut replacement = first.clone();
    replacement.session_hash = "old-journal-hash".to_string();
    replacement.freshness.sequence = 3;
    assert_eq!(
        store
            .apply_attribution_request_session(&replacement)
            .await
            .expect("newer sequence"),
        AttributionApplyOutcome::Applied
    );
    let remapped = store
        .list_attribution_request_facts(&["old-journal-hash".to_string()], base())
        .await
        .expect("updated journal hash");
    assert_eq!(remapped.len(), 1);
    assert_eq!(remapped[0].request_id, first_id.to_string());
    replacement.session_hash.clone_from(&first.session_hash);
    replacement.freshness = freshness("installation-b", 1, base() + Duration::seconds(1));
    assert_eq!(
        store
            .apply_attribution_request_session(&replacement)
            .await
            .expect("newer installation"),
        AttributionApplyOutcome::Applied
    );
    // Another owner can queue this UUID, but cannot remap or read Alice's request.
    for id in [first_id, native_id] {
        store
            .apply_attribution_request_session(&mapping(&bob, id, "bob-journal-hash"))
            .await
            .expect("other owner's mapping");
    }
    assert!(
        store
            .list_attribution_request_facts(&["bob-journal-hash".to_string()], base())
            .await
            .expect("owner bound lookup")
            .is_empty()
    );
    for (id, owner, cost) in [
        (first_id, &alice, 111),
        (second_id, &alice, 222),
        (native_id, &bob, 999),
    ] {
        let usage = build_usage_ledger_record(
            &id.to_string(),
            owner.ownership_scope_key.clone(),
            native.api_key_id,
            owner.owner_user_id,
            None,
            None,
            None,
            "gpt-test",
            UsagePricingStatus::Priced,
            cost,
            base(),
        );
        store
            .insert_usage_ledger_if_absent(&usage)
            .await
            .expect("usage");
    }
    let facts = store
        .list_attribution_request_facts(
            &[
                "journal-child-hash".to_string(),
                "provider-parent-hash".to_string(),
            ],
            base(),
        )
        .await
        .expect("effective sessions");
    assert_eq!(facts.len(), 3, "each request is returned once");
    assert_eq!(
        facts.iter().filter_map(|fact| fact.cost_nanos).sum::<i64>(),
        33_300_000
    );
    assert!(
        facts
            .iter()
            .all(|fact| fact.model_key.as_deref() == Some("resolved-model"))
    );
    for fact in &facts {
        assert_eq!(fact.ownership_scope_key, alice.ownership_scope_key);
        if fact.request_id == native_id.to_string() {
            assert_eq!(fact.session_hash, "provider-parent-hash");
            assert_eq!(fact.cost_nanos, None, "other owner's usage cannot join");
        } else {
            assert_eq!(fact.session_hash, "journal-child-hash");
        }
    }
    assert!(
        store
            .list_attribution_request_facts(
                &["journal-child-hash".to_string()],
                base() - Duration::milliseconds(1)
            )
            .await
            .expect("time bound")
            .is_empty()
    );
    assert_eq!(
        store
            .delete_attribution_for_owner(&alice.ownership_scope_key)
            .await
            .expect("delete mappings"),
        2
    );
    assert_eq!(
        store
            .list_attribution_request_facts(&["provider-parent-hash".to_string()], base())
            .await
            .expect("native fallback restored")
            .len(),
        3
    );
    assert_eq!(
        store
            .delete_attribution_for_owner(&bob.ownership_scope_key)
            .await
            .expect("other mappings retained"),
        2
    );
}

async fn codex_requests_use_exact_execution<S: GatewayStore>(store: &S) {
    let owner = user_owner(store, "codex-owner").await;
    let native = open_agent_session(store, &owner, "codex").await;
    let root_id = Uuid::from_u128(501);
    let child_id = Uuid::from_u128(502);
    let root_hash = "codex-root-thread-hash";
    let child_hash = "codex-child-thread-hash";
    // The analysis groups root and child requests by one session, but stores each exact
    // execution/thread hash independently. Parent lineage must not imply attribution.
    for (id, execution, parent, cost) in [
        (root_id, root_hash, None, 111),
        (child_id, child_hash, Some(root_hash), 222),
    ] {
        link_request(
            store,
            &native,
            &id.to_string(),
            root_hash,
            base(),
            Some(execution),
            parent,
        )
        .await;
        let usage = build_usage_ledger_record(
            &id.to_string(),
            owner.ownership_scope_key.clone(),
            native.api_key_id,
            owner.owner_user_id,
            None,
            None,
            None,
            "gpt-test",
            UsagePricingStatus::Priced,
            cost,
            base(),
        );
        store
            .insert_usage_ledger_if_absent(&usage)
            .await
            .expect("usage");
    }
    assert_eq!(
        store
            .list_attribution_request_facts(&[root_hash.to_string()], base())
            .await
            .expect("native fallback before notes arrive")
            .len(),
        2
    );
    assert!(
        store
            .list_attribution_request_facts(&[child_hash.to_string()], base())
            .await
            .expect("unattested execution is not an alias")
            .is_empty()
    );
    for (sha, hash) in [("root-note", root_hash), ("child-note", child_hash)] {
        store
            .apply_attribution_commit(&commit(
                &owner,
                REPOSITORY,
                sha,
                base() + Duration::minutes(10),
                vec![AttributionCommitSession {
                    agent_tool: "codex".to_string(),
                    harness_key: Some("codex".to_string()),
                    ..session("thread", &[hash])
                }],
            ))
            .await
            .expect("note naming exact thread");
    }
    let facts = store
        .list_attribution_request_facts(&[root_hash.to_string(), child_hash.to_string()], base())
        .await
        .expect("exact execution facts");
    assert_eq!(facts.len(), 2);
    assert_eq!(
        facts.iter().filter_map(|fact| fact.cost_nanos).sum::<i64>(),
        33_300_000
    );
    let notes = store.list_attribution_commits(&day()).await.expect("notes");
    for (sha, id, hash, cost) in [
        ("root-note", root_id, root_hash, 111),
        ("child-note", child_id, child_hash, 222),
    ] {
        let note = notes
            .iter()
            .find(|note| note.commit_sha == sha)
            .expect("thread note");
        assert_eq!(note.sessions[0].session_hashes, [hash.to_string()]);
        let matched = facts
            .iter()
            .filter(|fact| note.sessions[0].session_hashes.contains(&fact.session_hash))
            .collect::<Vec<_>>();
        assert_eq!(
            matched.len(),
            1,
            "root and child notes must not borrow each other's requests"
        );
        assert_eq!(matched[0].request_id, id.to_string());
        assert_eq!(matched[0].cost_nanos, Some(cost * 100_000));
        let single = store
            .list_attribution_request_facts(&[hash.to_string()], base())
            .await
            .expect("single thread lookup");
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].request_id, id.to_string());
    }
    let mapping = AttributionRequestSessionWrite {
        owner: owner.clone(),
        request_id: child_id,
        session_hash: "explicit-journal-hash".to_string(),
        freshness: freshness("installation-a", 1, base()),
        received_at: base(),
    };
    store
        .apply_attribution_request_session(&mapping)
        .await
        .expect("explicit override");
    assert!(
        store
            .list_attribution_request_facts(&[child_hash.to_string()], base())
            .await
            .expect("execution overridden")
            .is_empty()
    );
    let explicit = store
        .list_attribution_request_facts(&[mapping.session_hash], base())
        .await
        .expect("explicit journal");
    assert_eq!(explicit.len(), 1);
    assert_eq!(explicit[0].request_id, child_id.to_string());
    assert_eq!(explicit[0].cost_nanos, Some(22_200_000));

    // Without an execution id Codex still uses its normalized session. Merely naming a
    // parent execution never matches that parent's note.
    let fallback_id = Uuid::from_u128(503).to_string();
    link_request(
        store,
        &native,
        &fallback_id,
        "fallback-session-hash",
        base(),
        None,
        Some(root_hash),
    )
    .await;
    let fallback = store
        .list_attribution_request_facts(&["fallback-session-hash".to_string()], base())
        .await
        .expect("normalized fallback");
    assert_eq!(fallback.len(), 1);
    assert_eq!(fallback[0].request_id, fallback_id);
    assert_eq!(
        store
            .list_attribution_request_facts(&[root_hash.to_string()], base())
            .await
            .expect("parent lineage ignored")
            .len(),
        1
    );

    // Execution ids from other harness namespaces are not Codex thread identities.
    let claude = open_agent_session(store, &owner, "claude_code").await;
    link_request(
        store,
        &claude,
        "non-codex",
        "claude-session-hash",
        base(),
        Some(child_hash),
        None,
    )
    .await;
    assert!(
        store
            .list_attribution_request_facts(&[child_hash.to_string()], base())
            .await
            .expect("non-Codex execution ignored")
            .is_empty()
    );
    assert_eq!(
        store
            .list_attribution_request_facts(&["claude-session-hash".to_string()], base())
            .await
            .expect("non-Codex normalized identity")
            .len(),
        1
    );
}

async fn codex_turn_ids_keep_native_fallback<S: GatewayStore>(store: &S) {
    let owner = user_owner(store, "codex-turn-owner").await;
    let other = user_owner(store, "codex-other-owner").await;
    let native = open_agent_session(store, &owner, "codex").await;
    let request_id = Uuid::from_u128(504).to_string();
    let turn_hash = "codex-unattested-turn-hash";
    let session_hash = "codex-native-session-hash";
    link_request(
        store,
        &native,
        &request_id,
        session_hash,
        base(),
        Some(turn_hash),
        None,
    )
    .await;
    // Evidence in another owner's notes never attests this request's execution identity.
    store
        .apply_attribution_commit(&commit(
            &other,
            REPOSITORY,
            "other-owner-turn-note",
            base(),
            vec![session("thread", &[turn_hash])],
        ))
        .await
        .expect("other owner's note");
    let facts = store
        .list_attribution_request_facts(&[session_hash.to_string()], base())
        .await
        .expect("turn-only native fallback");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].request_id, request_id);
    assert_eq!(facts[0].session_hash, session_hash);
    assert!(
        store
            .list_attribution_request_facts(&[turn_hash.to_string()], base())
            .await
            .expect("other owner's evidence ignored")
            .is_empty()
    );
}
