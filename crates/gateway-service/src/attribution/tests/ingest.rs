use gateway_core::{
    ATTRIBUTION_MAX_BATCH_ENTRIES, AttributionOwner, AttributionSessionEventKind, GatewayError,
    StoreError,
};
use time::Duration;
use uuid::Uuid;

use super::{FakeStore, FakeUsers, service_account_key, t0, user_key};
use crate::agent_analysis::hash_lineage_candidate;
use crate::attribution::note::MAX_NOTE_BYTES;
use crate::attribution::{
    AttributionBatchInput, AttributionBatchOutcome, AttributionEntryCandidate,
    AttributionEntryInput, AttributionEntryStatus, AttributionNoteInput, AttributionPayloadInput,
    AttributionPullRequestCommitInput, AttributionPullRequestInput, AttributionPullRequestsInput,
    AttributionRejectReason, AttributionSessionInput, ingest_attribution_batch,
};

const INSTALLATION: &str = "0b8e3e6c-8f6f-4d55-9d7a-2a3c1b1e9f10";
const REPOSITORY: &str = "github.com/acme/widgets";
const SHA_A: &str = "1111111111111111111111111111111111111111";
const SHA_B: &str = "2222222222222222222222222222222222222222";

/// Claude and pi sessions plus known-human lines, all by one developer.
const NOTE: &str = r#"src/main.rs
  s_aa::t_01 1-10
  s_bb::t_02 11-14
  h_cc 15-16
---
{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.7.6",
  "base_commit_sha": "7734793b756b3921c88db5375a8c156e9532447b",
  "prompts": {},
  "sessions": {
    "s_aa": {
      "agent_id": {"tool": "claude", "id": "conv_abc123", "model": "claude-sonnet-4-6"},
      "human_author": "Dev Eloper <Dev@Example.com>"
    },
    "s_bb": {
      "agent_id": {"tool": "pi", "id": "pi-session-42", "model": "unknown"},
      "human_author": "Dev Eloper <dev@example.com>"
    }
  },
  "humans": {"h_cc": {"author": "Dev Eloper <dev@example.com>"}}
}"#;

const HUMAN_NOTE: &str = "a.rs\n  h_cc 1-3\n---\n{\"schema_version\": \"authorship/3.0.0\", \"base_commit_sha\": \"x\", \"prompts\": {}, \"humans\": {\"h_cc\": {\"author\": \"Contractor <contractor@example.com>\"}}}";

pub(super) fn entry(
    entry_id: &str,
    sequence: i64,
    payload: AttributionPayloadInput,
) -> AttributionEntryCandidate {
    AttributionEntryCandidate::Entry(Box::new(AttributionEntryInput {
        entry_id: entry_id.to_string(),
        installation_id: INSTALLATION.to_string(),
        sequence,
        observed_at: t0() + Duration::seconds(sequence),
        late_sequenced: false,
        payload,
    }))
}

fn note_in(repository: &str, sha: &str, note: &str) -> AttributionPayloadInput {
    AttributionPayloadInput::Note(AttributionNoteInput {
        repository: repository.to_string(),
        commit_sha: sha.to_string(),
        branch: Some("main".to_string()),
        is_default_branch: true,
        note_content: note.to_string(),
    })
}

pub(super) fn note(sha: &str, note: &str) -> AttributionPayloadInput {
    note_in(REPOSITORY, sha, note)
}

/// `id` hashed under each namespace the gateway records request sessions under, ascending.
fn every_namespace(scope: &str, id: &str) -> Vec<String> {
    let mut hashes = [
        "claude_code",
        "codex",
        "opencode",
        "pi",
        "oh_my_pi",
        "oceans",
    ]
    .map(|namespace| hash_lineage_candidate(scope, namespace, id))
    .to_vec();
    hashes.sort();
    hashes
}

pub(super) fn session(harness: &str, session_id: &str) -> AttributionPayloadInput {
    AttributionPayloadInput::Session(AttributionSessionInput {
        harness: harness.to_string(),
        session_id: session_id.to_string(),
        event: AttributionSessionEventKind::Start,
        repository: Some(REPOSITORY.to_string()),
        branch: Some("feature/x".to_string()),
        head_sha: Some(SHA_A.to_string()),
        harness_version: Some("2.1.0".to_string()),
    })
}

fn pull_request(number: i64, commits: &[(&str, Option<&str>)]) -> AttributionPullRequestInput {
    AttributionPullRequestInput {
        number,
        title: format!("Change {number}"),
        url: Some(format!("https://github.com/acme/widgets/pull/{number}")),
        base_ref: Some("main".to_string()),
        head_ref: Some(format!("feature/{number}")),
        merge_commit_sha: None,
        merged_at: t0() - Duration::hours(1),
        commits: commits
            .iter()
            .map(|(sha, email)| AttributionPullRequestCommitInput {
                sha: (*sha).to_string(),
                subject: Some("Subject".to_string()),
                authored_at: Some(t0() - Duration::hours(2)),
                author_email: email.map(str::to_string),
            })
            .collect(),
    }
}

fn pull_requests(pull_requests: Vec<AttributionPullRequestInput>) -> AttributionPayloadInput {
    AttributionPayloadInput::PullRequests(AttributionPullRequestsInput {
        repository: REPOSITORY.to_string(),
        pull_requests,
    })
}

pub(super) async fn ingest(
    store: &FakeStore,
    users: &FakeUsers,
    owner_scope: &str,
    key: &gateway_core::AuthenticatedApiKey,
    entries: Vec<AttributionEntryCandidate>,
) -> Result<AttributionBatchOutcome, GatewayError> {
    let batch = AttributionBatchInput {
        expected_ownership_scope: owner_scope.to_string(),
        entries,
    };
    ingest_attribution_batch(store, users, key, batch, t0() + Duration::minutes(5)).await
}

fn statuses(outcome: &AttributionBatchOutcome) -> Vec<(&str, AttributionEntryStatus)> {
    outcome
        .results
        .iter()
        .map(|result| (result.entry_id.as_str(), result.status))
        .collect()
}

use AttributionEntryStatus::{Applied, Duplicate, Rejected, Stale};

#[tokio::test]
async fn applies_notes_sessions_and_pull_requests() {
    let dev = Uuid::new_v4();
    let (store, users) = (
        FakeStore::default(),
        FakeUsers::with(&[("dev@example.com", dev)]),
    );
    let scope = format!("user:{dev}");
    let entries = vec![
        entry("n-1", 1, note(SHA_A, NOTE)),
        entry("s-1", 2, session("claude_code", "conv_abc123")),
        entry(
            "p-1",
            3,
            pull_requests(vec![pull_request(12, &[(SHA_A, Some("Dev@Example.com"))])]),
        ),
    ];
    let outcome = ingest(&store, &users, &scope, &user_key(dev), entries)
        .await
        .expect("batch");

    assert_eq!(outcome.ownership_scope, scope);
    assert_eq!(
        statuses(&outcome),
        [("n-1", Applied), ("s-1", Applied), ("p-1", Applied)]
    );
    let commit = &store.commits()[0];
    assert_eq!(commit.owner.owner_user_id, Some(dev));
    assert_eq!(
        (commit.author_user_id, commit.author_email_hash.as_deref()),
        (Some(dev), None)
    );
    assert_eq!((commit.ai_lines, commit.human_lines), (14, 2));
    assert_eq!(commit.freshness.installation_id, INSTALLATION);
    let claude = &commit.sessions[0];
    assert_eq!(claude.harness_key.as_deref(), Some("claude_code"));
    assert_eq!(
        claude.session_hashes,
        every_namespace(&scope, "conv_abc123")
    );
    assert_eq!(claude.human_author_user_id, Some(dev));
    // git-ai's `pi` preset covers pi and OMP, whose Messages traffic the gateway records as
    // Claude Code: every namespace is hashed, whichever the tool names.
    let pi = &commit.sessions[1];
    assert_eq!(pi.harness_key.as_deref(), Some("oh_my_pi"));
    assert_eq!(pi.session_hashes, every_namespace(&scope, "pi-session-42"));
    assert_eq!(
        (pi.agent_tool.as_str(), pi.agent_model.as_deref()),
        ("pi", None)
    );

    // The hook's session is keyed under its harness and, like the note's session, carries the
    // id's hash under every namespace.
    let event = &store.session_events()[0];
    assert_eq!(
        event.session_hash,
        hash_lineage_candidate(&scope, "claude_code", "conv_abc123")
    );
    assert_eq!(event.session_hashes, claude.session_hashes);
    assert_eq!(event.received_at, t0() + Duration::minutes(5));

    let pull_request = &store.pull_requests()[0];
    assert_eq!(
        (pull_request.number, pull_request.author_user_id),
        (12, Some(dev))
    );
    assert_eq!(pull_request.commits[0].author_user_id, Some(dev));
    assert_eq!(pull_request.observed_at, t0() + Duration::seconds(3));
}

#[tokio::test]
async fn hook_sessions_of_any_harness_hash_under_every_namespace() {
    let dev = Uuid::new_v4();
    let (store, users) = (FakeStore::default(), FakeUsers::default());
    let scope = format!("user:{dev}");
    let entries = vec![entry("s-1", 1, session("cursor", "cursor-session-7"))];
    let outcome = ingest(&store, &users, &scope, &user_key(dev), entries)
        .await
        .expect("batch");
    assert_eq!(statuses(&outcome), [("s-1", Applied)]);

    let event = &store.session_events()[0];
    assert_eq!(
        (event.harness_key.as_str(), event.session_hash.clone()),
        (
            "cursor",
            hash_lineage_candidate(&scope, "cursor", "cursor-session-7")
        )
    );
    assert_eq!(
        event.session_hashes,
        every_namespace(&scope, "cursor-session-7")
    );
}

#[tokio::test]
async fn hashes_any_tool_but_only_ids_a_request_could_carry() {
    let dev = Uuid::new_v4();
    let (store, users) = (FakeStore::default(), FakeUsers::default());
    let scope = format!("user:{dev}");
    let note_content = r#"a.rs
  s_aa::t_01 1-2
  s_bb::t_02 3-4
  s_cc::t_03 5-6
---
{"schema_version": "authorship/3.0.0", "base_commit_sha": "x", "prompts": {}, "sessions": {
  "s_aa": {"agent_id": {"tool": "cursor", "id": "cursor-session-7", "model": "gpt-6"}},
  "s_bb": {"agent_id": {"tool": "claude", "id": "has space", "model": "claude-sonnet"}},
  "s_cc": {"agent_id": {"tool": "claude", "model": "claude-sonnet"}}
}}"#;
    let entries = vec![entry("n-1", 1, note(SHA_A, note_content))];
    let outcome = ingest(&store, &users, &scope, &user_key(dev), entries)
        .await
        .expect("batch");
    assert_eq!(statuses(&outcome), [("n-1", Applied)]);

    let sessions = &store.commits()[0].sessions;
    let summary = sessions
        .iter()
        .map(|session| {
            (
                session.note_session_key.as_str(),
                session.agent_tool.as_str(),
                session.harness_key.as_deref(),
                session.session_hashes.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        summary,
        [
            (
                "s_aa",
                "cursor",
                None,
                every_namespace(&scope, "cursor-session-7")
            ),
            ("s_bb", "claude", Some("claude_code"), Vec::new()),
            ("s_cc", "claude", Some("claude_code"), Vec::new()),
        ]
    );
}

#[tokio::test]
async fn replays_are_duplicates_and_older_revisions_are_stale() {
    let dev = Uuid::new_v4();
    let (store, users) = (FakeStore::default(), FakeUsers::default());
    let (scope, key) = (format!("user:{dev}"), user_key(dev));
    let first = || {
        vec![
            entry("n-1", 5, note(SHA_A, NOTE)),
            entry(
                "p-1",
                6,
                pull_requests(vec![pull_request(12, &[(SHA_A, None)])]),
            ),
        ]
    };
    let applied = ingest(&store, &users, &scope, &key, first())
        .await
        .expect("first");
    assert_eq!(statuses(&applied), [("n-1", Applied), ("p-1", Applied)]);
    let replayed = ingest(&store, &users, &scope, &key, first())
        .await
        .expect("replay");
    assert_eq!(
        statuses(&replayed),
        [("n-1", Duplicate), ("p-1", Duplicate)]
    );

    let revisions = vec![
        entry("n-2", 4, note(SHA_A, HUMAN_NOTE)),
        entry("n-3", 7, note(SHA_A, HUMAN_NOTE)),
        entry(
            "p-2",
            6,
            pull_requests(vec![pull_request(12, &[]), pull_request(13, &[])]),
        ),
        entry("p-3", 1, pull_requests(vec![pull_request(12, &[])])),
        entry("p-4", 8, pull_requests(Vec::new())),
    ];
    let outcome = ingest(&store, &users, &scope, &key, revisions)
        .await
        .expect("revisions");
    assert_eq!(
        statuses(&outcome),
        [
            ("n-2", Stale),
            ("n-3", Applied),
            ("p-2", Applied),
            ("p-3", Stale),
            ("p-4", Applied)
        ]
    );
    let commits = store.commits();
    assert_eq!(commits.len(), 1);
    assert_eq!((commits[0].ai_lines, commits[0].human_lines), (0, 3));
    assert_eq!(commits[0].first_observed_at, t0() + Duration::seconds(5));
    assert_eq!(store.pull_requests().len(), 2);
}

fn with_entry(
    candidate: AttributionEntryCandidate,
    change: impl FnOnce(&mut AttributionEntryInput),
) -> AttributionEntryCandidate {
    let AttributionEntryCandidate::Entry(mut input) = candidate else {
        unreachable!("entry() builds entries")
    };
    change(&mut input);
    AttributionEntryCandidate::Entry(input)
}

#[tokio::test]
async fn rejects_bad_entries_one_at_a_time() {
    let dev = Uuid::new_v4();
    let (store, users) = (FakeStore::default(), FakeUsers::default());
    let scope = format!("user:{dev}");
    let http_link = AttributionPullRequestInput {
        url: Some("http://github.com/acme/widgets/pull/9".to_string()),
        ..pull_request(9, &[])
    };
    let entries = vec![
        AttributionEntryCandidate::Malformed {
            entry_id: Some("m-1".to_string()),
        },
        AttributionEntryCandidate::Malformed {
            entry_id: Some("bad id!".to_string()),
        },
        entry("../x", 1, note(SHA_A, NOTE)),
        with_entry(entry("i-1", 1, note(SHA_A, NOTE)), |input| {
            input.installation_id = "laptop".into()
        }),
        with_entry(entry("q-1", 1, note(SHA_A, NOTE)), |input| {
            input.sequence = -1
        }),
        entry(
            "r-1",
            1,
            note_in("https://github.com/acme/widgets", SHA_A, NOTE),
        ),
        entry("o-1", 1, note(SHA_A, &"x".repeat(MAX_NOTE_BYTES + 1))),
        entry("v-1", 1, note(SHA_A, "src/a.rs\n  zz 1\n---\n{}")),
        entry(
            "u-1",
            1,
            note(SHA_A, "---\n{\"schema_version\": \"authorship/4.0.0\"}"),
        ),
        entry("c-1", 1, note(&"A".repeat(40), NOTE)),
        entry("h-1", 1, session("Claude Code", "abc")),
        entry("h-2", 1, session("claude_code", "has space")),
        entry(
            "p-1",
            1,
            pull_requests(vec![pull_request(10, &[]), http_link]),
        ),
        entry("p-2", 1, pull_requests(vec![pull_request(0, &[])])),
        entry("ok-1", 1, note(SHA_B, NOTE)),
    ];
    let outcome = ingest(&store, &users, &scope, &user_key(dev), entries)
        .await
        .expect("batch");

    use AttributionRejectReason::*;
    assert_eq!(
        statuses(&outcome),
        [
            ("m-1", Rejected(InvalidEntry)),
            ("", Rejected(InvalidEntry)),
            ("", Rejected(InvalidEntry)),
            ("i-1", Rejected(InvalidEntry)),
            ("q-1", Rejected(InvalidEntry)),
            ("r-1", Rejected(InvalidRepository)),
            ("o-1", Rejected(OversizedNote)),
            ("v-1", Rejected(InvalidNote)),
            ("u-1", Rejected(UnsupportedSchema)),
            ("c-1", Rejected(InvalidEntry)),
            ("h-1", Rejected(InvalidEntry)),
            ("h-2", Rejected(InvalidEntry)),
            ("p-1", Rejected(InvalidEntry)),
            ("p-2", Rejected(InvalidEntry)),
            ("ok-1", Applied),
        ]
    );
    let commits = store.commits();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].commit_sha, SHA_B);
    assert!(
        store.pull_requests().is_empty(),
        "a rejected PR entry stores none of its PRs"
    );
    assert!(store.session_events().is_empty());
}

#[tokio::test]
async fn ownership_mismatch_and_oversized_batches_fail_whole() {
    let dev = Uuid::new_v4();
    let (store, users) = (FakeStore::default(), FakeUsers::default());
    let key = user_key(dev);
    let error = ingest(
        &store,
        &users,
        "user:someone-else",
        &key,
        vec![entry("n-1", 1, note(SHA_A, NOTE))],
    )
    .await
    .expect_err("scope mismatch");
    let GatewayError::Store(StoreError::Conflict(message)) = &error else {
        panic!("expected a conflict, got {error:?}");
    };
    assert!(
        !message.contains(&dev.to_string()),
        "the key's scope is not echoed"
    );
    assert!(store.commits().is_empty());

    let entries = (0..=ATTRIBUTION_MAX_BATCH_ENTRIES)
        .map(|_| AttributionEntryCandidate::Malformed { entry_id: None })
        .collect();
    let error = ingest(&store, &users, &format!("user:{dev}"), &key, entries)
        .await
        .expect_err("too many entries");
    assert!(matches!(error, GatewayError::InvalidRequest(_)));
}

#[tokio::test]
async fn service_account_keys_own_their_rows_and_match_note_authors_by_email() {
    let service_account = Uuid::new_v4();
    let (store, users) = (FakeStore::default(), FakeUsers::default());
    let scope = format!("service_account:{service_account}");
    let entries = vec![entry("n-1", 1, note(SHA_A, NOTE))];
    let outcome = ingest(
        &store,
        &users,
        &scope,
        &service_account_key(service_account),
        entries,
    )
    .await
    .expect("batch");
    assert_eq!(statuses(&outcome), [("n-1", Applied)]);
    let commit = &store.commits()[0];
    assert_eq!(
        commit.owner,
        AttributionOwner {
            ownership_scope_key: scope.clone(),
            owner_user_id: None,
            owner_service_account_id: Some(service_account),
        }
    );
    // No user owns the key, so the note's author emails decide; none is an Oceans user.
    let email_hash = hash_lineage_candidate(&scope, "git_ai_human", "dev@example.com");
    assert_eq!(
        (commit.author_user_id, commit.author_email_hash.as_deref()),
        (None, Some(email_hash.as_str()))
    );
    assert_eq!(
        commit.sessions[0].human_author_hash.as_deref(),
        Some(email_hash.as_str())
    );
}

#[tokio::test]
async fn user_key_notes_credit_the_key_owner_and_store_only_hashes() {
    let dev = Uuid::new_v4();
    let other = Uuid::new_v4();
    // The note's author email belongs to another user, and the pull request's to nobody.
    let (store, users) = (
        FakeStore::default(),
        FakeUsers::with(&[("dev@example.com", other)]),
    );
    let scope = format!("user:{dev}");
    let entries = vec![
        entry("n-1", 1, note(SHA_A, NOTE)),
        entry("n-2", 2, note(SHA_B, NOTE)),
        entry("s-1", 3, session("oh_my_pi", "pi-session-42")),
        entry(
            "p-1",
            4,
            pull_requests(vec![pull_request(
                12,
                &[(SHA_A, Some("outside@example.com"))],
            )]),
        ),
    ];
    ingest(&store, &users, &scope, &user_key(dev), entries)
        .await
        .expect("batch");

    let stored = store.stored_json();
    for raw in [
        "conv_abc123",
        "pi-session-42",
        "dev@example.com",
        "Dev@Example.com",
        "Eloper",
        "outside@example.com",
    ] {
        assert!(!stored.contains(raw), "stored rows contain {raw}");
    }
    for commit in store.commits() {
        assert_eq!(
            (commit.author_user_id, commit.author_email_hash.as_deref()),
            (Some(dev), None)
        );
        for session in &commit.sessions {
            assert_eq!(
                (
                    session.human_author_user_id,
                    session.human_author_hash.as_deref()
                ),
                (Some(dev), None)
            );
        }
    }
    let email_hash = hash_lineage_candidate(&scope, "git_ai_human", "outside@example.com");
    assert_eq!(
        store.pull_requests()[0].author_email_hash.as_deref(),
        Some(email_hash.as_str())
    );
    assert_eq!(
        users.lookup_count(),
        1,
        "only the pull request's email is looked up"
    );
}

#[tokio::test]
async fn prefers_the_first_author_who_is_an_oceans_user() {
    let dev = Uuid::new_v4();
    let reviewer = Uuid::new_v4();
    let (store, users) = (
        FakeStore::default(),
        FakeUsers::with(&[("reviewer@example.com", reviewer)]),
    );
    let scope = format!("user:{dev}");
    let commits = [
        (SHA_A, Some("outside@example.com")),
        (SHA_B, Some("Reviewer@example.com")),
    ];
    let entries = vec![entry(
        "p-1",
        1,
        pull_requests(vec![pull_request(12, &commits)]),
    )];
    ingest(&store, &users, &scope, &user_key(dev), entries)
        .await
        .expect("batch");

    let pull_request = &store.pull_requests()[0];
    assert_eq!(
        (
            pull_request.author_user_id,
            pull_request.author_email_hash.as_deref()
        ),
        (Some(reviewer), None)
    );
    let outside = hash_lineage_candidate(&scope, "git_ai_human", "outside@example.com");
    assert_eq!(
        pull_request.commits[0].author_email_hash.as_deref(),
        Some(outside.as_str())
    );
    assert_eq!(pull_request.commits[1].author_user_id, Some(reviewer));
}
