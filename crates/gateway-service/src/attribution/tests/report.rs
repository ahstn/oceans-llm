use std::sync::Mutex;

use gateway_core::{
    ATTRIBUTION_MAX_REQUEST_FACTS, AttributionAgentSessionRecord, AttributionCommitRecord,
    AttributionCommitSession, AttributionFreshness, AttributionOwner, AttributionPullRequestCommit,
    AttributionPullRequestRecord, AttributionPullRequestState, AttributionReportQuery,
    AttributionRequestFact, AttributionRevertEvidence, AttributionRevertLink,
    AttributionSessionEventWrite,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::ingest::{entry, ingest, note, request_session, session};
use super::{FakeStore, FakeUsers, t0, user_key, user_owner};
use crate::agent_analysis::hash_lineage_candidate;
use crate::attribution::{AttributionReport, RequestTally, build_attribution_report};

const REPOSITORY: &str = "github.com/acme/widgets";
const OTHER_REPOSITORY: &str = "github.com/acme/gadgets";

fn minutes(count: i64) -> OffsetDateTime {
    t0() + Duration::minutes(count)
}

fn query() -> AttributionReportQuery {
    AttributionReportQuery {
        from: t0(),
        to: t0() + Duration::days(1),
        user_id: None,
        repository: None,
        limit: 1_000,
    }
}

fn sha(id: u128) -> String {
    format!("{id:040x}")
}

fn note_session(key: &str, hashes: &[&str]) -> AttributionCommitSession {
    AttributionCommitSession {
        note_session_key: key.to_string(),
        agent_tool: "claude".to_string(),
        agent_model: Some("claude-sonnet-4-6".to_string()),
        harness_key: Some("claude_code".to_string()),
        session_hashes: hashes.iter().map(|hash| (*hash).to_string()).collect(),
        ai_lines: 10,
        human_author_user_id: None,
        human_author_hash: None,
    }
}

fn commit(
    id: u128,
    owner: &AttributionOwner,
    repository: &str,
    at: OffsetDateTime,
    sessions: Vec<AttributionCommitSession>,
) -> AttributionCommitRecord {
    AttributionCommitRecord {
        attribution_commit_id: Uuid::from_u128(id),
        owner: owner.clone(),
        repository: repository.to_string(),
        commit_sha: sha(id),
        branch: Some("main".to_string()),
        is_default_branch: true,
        author_user_id: owner.owner_user_id,
        author_email_hash: None,
        schema_version: "authorship/3.0.0".to_string(),
        git_ai_version: None,
        ai_lines: 10,
        human_lines: 2,
        freshness: AttributionFreshness {
            installation_id: Uuid::nil().to_string(),
            sequence: 1,
            observed_at: at,
            late_sequenced: false,
        },
        first_observed_at: at,
        sessions,
    }
}

fn request(
    owner: &AttributionOwner,
    hash: &str,
    request_id: &str,
    at: OffsetDateTime,
) -> AttributionRequestFact {
    AttributionRequestFact {
        ownership_scope_key: owner.ownership_scope_key.clone(),
        session_hash: hash.to_string(),
        request_id: request_id.to_string(),
        occurred_at: at,
        model_key: Some("claude-sonnet".to_string()),
        cost_10000: Some(100),
        pricing_status: Some("priced".to_string()),
    }
}

fn pull_request(
    number: i64,
    owner: &AttributionOwner,
    merged_at: OffsetDateTime,
    commit_ids: &[u128],
) -> AttributionPullRequestRecord {
    AttributionPullRequestRecord {
        attribution_pull_request_id: Uuid::new_v4(),
        owner: owner.clone(),
        repository: REPOSITORY.to_string(),
        number,
        title: format!("Change {number}"),
        url: None,
        base_ref: Some("main".to_string()),
        head_ref: None,
        merge_commit_sha: None,
        merged_at: Some(merged_at),
        lifecycle: gateway_core::AttributionPullRequestLifecycle {
            closed_at: Some(merged_at),
            ..Default::default()
        },
        author_user_id: None,
        author_email_hash: None,
        commits: commit_ids
            .iter()
            .map(|id| AttributionPullRequestCommit {
                commit_sha: sha(*id),
                subject: None,
                authored_at: None,
                author_user_id: None,
                author_email_hash: None,
            })
            .collect(),
        observed_at: merged_at,
    }
}

fn agent_session(
    owner: &AttributionOwner,
    hash: &str,
    at: OffsetDateTime,
) -> AttributionAgentSessionRecord {
    AttributionAgentSessionRecord {
        owner: owner.clone(),
        harness_key: "claude_code".to_string(),
        session_hash: hash.to_string(),
        session_hashes: vec![hash.to_string()],
        repository: Some(REPOSITORY.to_string()),
        branch: None,
        start_head_sha: None,
        end_head_sha: None,
        harness_version: None,
        started_at: Some(at),
        ended_at: None,
        last_observed_at: at,
    }
}

/// The session row a store creates for `event`, its session's first.
fn stored_session(event: AttributionSessionEventWrite) -> AttributionAgentSessionRecord {
    AttributionAgentSessionRecord {
        owner: event.owner,
        harness_key: event.harness_key,
        session_hash: event.session_hash,
        session_hashes: event.session_hashes,
        repository: event.repository,
        branch: event.branch,
        start_head_sha: event.head_sha,
        end_head_sha: None,
        harness_version: event.harness_version,
        started_at: Some(event.observed_at),
        ended_at: None,
        last_observed_at: event.observed_at,
    }
}

async fn report(store: &FakeStore, query: &AttributionReportQuery) -> AttributionReport {
    build_attribution_report(store, query)
        .await
        .expect("report builds")
}

fn change_requests(report: &AttributionReport, id: u128) -> i64 {
    report
        .changes
        .iter()
        .find(|change| change.commit.attribution_commit_id == Uuid::from_u128(id))
        .map(|change| change.requests.request_count)
        .expect("change in report")
}

#[tokio::test]
async fn allocates_each_request_to_the_first_commit_at_or_after_it() {
    let dev = user_owner(Uuid::new_v4());
    let session = || vec![note_session("s_aa", &["h1"])];
    let mut store = FakeStore::with_commits(vec![
        commit(1, &dev, REPOSITORY, minutes(10), session()),
        commit(2, &dev, OTHER_REPOSITORY, minutes(20), session()),
    ]);
    store.facts = vec![
        request(&dev, "h1", "before-first", minutes(5)),
        request(&dev, "h1", "at-first", minutes(10)),
        request(&dev, "h1", "between", minutes(15)),
        request(&dev, "h1", "after-last", minutes(25)),
    ];
    let report = report(&store, &query()).await;

    assert_eq!(change_requests(&report, 1), 2);
    assert_eq!(
        change_requests(&report, 2),
        1,
        "a commit in another repository closes the window"
    );
    assert_eq!(report.totals.requests.request_count, 3);
    assert_eq!(report.totals.uncommitted.request_count, 1);
    assert_eq!(report.coverage.requests.request_count, 4);
}

#[tokio::test]
async fn window_commits_keep_their_requests_from_before_the_window() {
    let dev = user_owner(Uuid::new_v4());
    let mut store = FakeStore::with_commits(vec![
        commit(
            1,
            &dev,
            REPOSITORY,
            minutes(-120),
            vec![note_session("s_aa", &["h1"])],
        ),
        commit(
            2,
            &dev,
            REPOSITORY,
            minutes(30),
            vec![note_session("s_bb", &["h2"])],
        ),
    ]);
    store.facts = vec![
        request(&dev, "h1", "consumed-before-window", minutes(-150)),
        request(&dev, "h2", "committed-in-window", minutes(-60)),
        request(&dev, "h1", "uncommitted-before-window", minutes(-30)),
    ];
    let report = report(&store, &query()).await;

    assert_eq!(report.changes.len(), 1);
    assert_eq!(change_requests(&report, 2), 1);
    assert_eq!(report.totals.commit_count, 1);
    assert_eq!(report.totals.uncommitted.request_count, 0);
}

#[tokio::test]
async fn sessions_sharing_a_hash_count_its_requests_once() {
    let dev = user_owner(Uuid::new_v4());
    let sessions = vec![
        note_session("s_aa", &["h1"]),
        note_session("abcd123", &["h1"]),
        note_session("s_bb", &[]),
    ];
    let mut store =
        FakeStore::with_commits(vec![commit(1, &dev, REPOSITORY, minutes(30), sessions)]);
    store.facts = vec![
        request(&dev, "h1", "r1", minutes(5)),
        request(&dev, "h1", "r2", minutes(6)),
    ];
    let report = report(&store, &query()).await;

    let change = &report.changes[0];
    assert_eq!(
        (change.requests.request_count, change.joined_session_count),
        (2, 2)
    );
    assert_eq!(
        (
            report.totals.ai_session_count,
            report.totals.joined_session_count
        ),
        (3, 2)
    );
    assert_eq!(report.totals.requests.request_count, 2);
}

#[tokio::test]
async fn a_pi_note_joins_requests_the_gateway_recorded_as_claude_code() {
    let dev = Uuid::new_v4();
    let scope = format!("user:{dev}");
    let mut store = FakeStore::with_commits(vec![]);
    let note_content = r#"a.rs
  s_aa::t_01 1-5
---
{"schema_version": "authorship/3.0.0", "base_commit_sha": "x", "prompts": {}, "sessions": {
  "s_aa": {"agent_id": {"tool": "pi", "id": "omp-session-1", "model": "claude-opus"}}
}}"#;
    let entries = vec![entry("n-1", 60, note(&sha(1), note_content))];
    ingest(
        &store,
        &FakeUsers::default(),
        &scope,
        &user_key(dev),
        entries,
    )
    .await
    .expect("batch");
    // OMP over the Messages API sends Claude Code headers, so the gateway hashed the
    // session's requests under `claude_code`, not the `oh_my_pi` the note's tool suggests.
    let recorded = hash_lineage_candidate(&scope, "claude_code", "omp-session-1");
    let owner = user_owner(dev);
    store.facts = vec![
        request(&owner, &recorded, "r1", t0() + Duration::seconds(10)),
        request(&owner, &recorded, "r2", t0() + Duration::seconds(50)),
    ];
    let report = report(&store, &query()).await;

    let change = &report.changes[0];
    assert_eq!(
        change.commit.sessions[0].harness_key.as_deref(),
        Some("oh_my_pi")
    );
    assert_eq!(
        (change.joined_session_count, change.requests.request_count),
        (1, 2)
    );
    assert_eq!(report.totals.uncommitted.request_count, 0);
}

#[tokio::test]
async fn a_session_recorded_under_two_namespaces_counts_each_request_once() {
    let dev = user_owner(Uuid::new_v4());
    // One session's hashes; the gateway recorded its requests under two of them and r2,
    // which carried both session headers, under both.
    let session = || {
        vec![note_session(
            "s_aa",
            &["claude-hash", "oceans-hash", "omp-hash"],
        )]
    };
    let mut store = FakeStore::with_commits(vec![
        commit(1, &dev, REPOSITORY, minutes(10), session()),
        commit(2, &dev, OTHER_REPOSITORY, minutes(20), session()),
    ]);
    store.facts = vec![
        request(&dev, "claude-hash", "r1", minutes(5)),
        request(&dev, "claude-hash", "r2", minutes(6)),
        request(&dev, "omp-hash", "r2", minutes(6)),
        request(&dev, "omp-hash", "r3", minutes(15)),
        request(&dev, "claude-hash", "r4", minutes(25)),
    ];
    let report = report(&store, &query()).await;

    assert_eq!(
        (change_requests(&report, 1), change_requests(&report, 2)),
        (2, 1),
        "both hashes share the session's commit windows"
    );
    assert_eq!(report.totals.requests.request_count, 3);
    assert_eq!(report.totals.uncommitted.request_count, 1);
    assert_eq!(report.coverage.requests.request_count, 4);
    assert_eq!(
        (
            report.totals.ai_session_count,
            report.totals.joined_session_count
        ),
        (1, 1)
    );
}

#[tokio::test]
async fn a_hook_session_finds_its_requests_under_any_namespace() {
    let dev = user_owner(Uuid::new_v4());
    let hashes = ["claude-hash", "oceans-hash", "omp-hash"];
    let sessions = vec![note_session("s_aa", &hashes)];
    let mut store =
        FakeStore::with_commits(vec![commit(1, &dev, REPOSITORY, minutes(30), sessions)]);
    // The OMP hook keys the session under `oh_my_pi`; the gateway recorded its requests under
    // `oceans`.
    store.agent_sessions = vec![AttributionAgentSessionRecord {
        session_hashes: hashes.map(str::to_string).to_vec(),
        ..agent_session(&dev, "omp-hash", minutes(1))
    }];
    store.facts = vec![
        request(&dev, "oceans-hash", "r1", minutes(5)),
        request(&dev, "oceans-hash", "r2", minutes(40)),
    ];
    let report = report(&store, &query()).await;

    let change = &report.changes[0];
    assert_eq!(
        (change.joined_session_count, change.requests.request_count),
        (1, 1)
    );
    assert_eq!(
        report.totals.uncommitted.request_count, 1,
        "the hook's session keeps its requests after the last commit"
    );
    assert_eq!(report.users[0].agent_session_count, 1);
}

#[tokio::test]
async fn a_hook_only_omp_session_counts_its_oceans_requests_as_uncommitted() {
    let dev = Uuid::new_v4();
    let scope = format!("user:{dev}");
    let mut store = FakeStore::with_commits(vec![]);
    let entries = vec![entry("s-1", 5, session("oh_my_pi", "omp-session-9"))];
    ingest(
        &store,
        &FakeUsers::default(),
        &scope,
        &user_key(dev),
        entries,
    )
    .await
    .expect("batch");
    store.agent_sessions = store
        .session_events()
        .into_iter()
        .map(stored_session)
        .collect();
    // OMP names its requests with `x-oceans-session-id`, so the gateway hashed them under
    // `oceans` while the hook reported the session as `oh_my_pi`.
    let recorded = hash_lineage_candidate(&scope, "oceans", "omp-session-9");
    let owner = user_owner(dev);
    store.facts = vec![
        request(&owner, &recorded, "r1", t0() + Duration::seconds(10)),
        request(&owner, &recorded, "r2", t0() + Duration::seconds(50)),
    ];
    let report = report(&store, &query()).await;

    assert!(report.changes.is_empty());
    assert_eq!(report.totals.uncommitted.request_count, 2);
    assert_eq!(report.coverage.requests.request_count, 2);
    assert_eq!(report.users[0].agent_session_count, 1);
}

#[tokio::test]
async fn a_hook_session_finds_requests_under_its_own_key_without_namespace_hashes() {
    use crate::attribution::AttributionSessionOutcome;

    let dev = user_owner(Uuid::new_v4());
    let mut store = FakeStore::default();
    // A row stored without namespace hashes still names its requests by its own key.
    let mut session = agent_session(&dev, "primary", minutes(5));
    session.session_hashes = Vec::new();
    store.agent_sessions = vec![session];
    store.facts = vec![request(&dev, "primary", "r1", minutes(10))];
    let report = report(&store, &query()).await;

    assert_eq!(report.totals.uncommitted.request_count, 1);
    let outcomes = report
        .session_outcomes
        .iter()
        .map(|tally| (tally.outcome, tally.requests.request_count))
        .collect::<Vec<_>>();
    assert_eq!(outcomes, vec![(AttributionSessionOutcome::NoCommit, 1)]);
}

#[tokio::test]
async fn equal_commit_times_go_to_the_lowest_commit_id() {
    let dev = user_owner(Uuid::new_v4());
    let session = || vec![note_session("s_aa", &["h1"])];
    let mut store = FakeStore::with_commits(vec![
        commit(7, &dev, REPOSITORY, minutes(30), session()),
        commit(3, &dev, OTHER_REPOSITORY, minutes(30), session()),
    ]);
    store.facts = vec![request(&dev, "h1", "r1", minutes(10))];
    let report = report(&store, &query()).await;

    assert_eq!(
        (change_requests(&report, 3), change_requests(&report, 7)),
        (1, 0)
    );
}

#[tokio::test]
async fn aggregates_users_models_pull_requests_and_pricing() {
    let (dev, reviewer) = (Uuid::new_v4(), Uuid::new_v4());
    let owner = user_owner(dev);
    let robot = AttributionOwner {
        ownership_scope_key: "service_account:robot".to_string(),
        owner_user_id: None,
        owner_service_account_id: Some(Uuid::new_v4()),
    };
    let mut reviewed = commit(
        2,
        &owner,
        REPOSITORY,
        minutes(40),
        vec![note_session("s_bb", &["h2"])],
    );
    reviewed.author_user_id = Some(reviewer);
    let mut store = FakeStore::with_commits(vec![
        commit(
            1,
            &owner,
            REPOSITORY,
            minutes(30),
            vec![note_session("s_aa", &["h1"])],
        ),
        reviewed,
        commit(3, &robot, REPOSITORY, minutes(50), Vec::new()),
        commit(4, &owner, REPOSITORY, minutes(-3 * 24 * 60), Vec::new()),
    ]);
    let gpt = Some("gpt-5".to_string());
    store.facts = vec![
        request(&owner, "h1", "r1", minutes(5)),
        AttributionRequestFact {
            cost_10000: None,
            pricing_status: Some("unpriced".to_string()),
            model_key: gpt.clone(),
            ..request(&owner, "h1", "r2", minutes(6))
        },
        AttributionRequestFact {
            cost_10000: None,
            pricing_status: None,
            model_key: None,
            ..request(&owner, "h2", "r3", minutes(35))
        },
        AttributionRequestFact {
            model_key: gpt,
            ..request(&owner, "h1", "r4", minutes(60))
        },
    ];
    let listed = AttributionPullRequestRecord {
        author_user_id: Some(dev),
        ..pull_request(12, &owner, minutes(70), &[1, 4])
    };
    let merged_only = AttributionPullRequestRecord {
        merge_commit_sha: Some(sha(2)),
        ..pull_request(13, &owner, minutes(65), &[99])
    };
    store.pull_requests = Mutex::new(vec![listed, merged_only]);
    store.agent_sessions = vec![agent_session(&owner, "h1", minutes(1))];
    let report = report(&store, &query()).await;

    let totals = &report.totals;
    assert_eq!(
        (
            totals.commit_count,
            totals.pull_request_count,
            totals.agent_session_count
        ),
        (3, 2, 1)
    );
    assert_eq!(
        (
            totals.ai_lines,
            totals.human_lines,
            totals.unmatched_author_commit_count
        ),
        (30, 6, 1)
    );
    let allocated = RequestTally {
        request_count: 3,
        priced_request_count: 1,
        unpriced_request_count: 1,
        usage_missing_request_count: 1,
        priced_cost_10000: 100,
    };
    assert_eq!(totals.requests, allocated);
    assert_eq!(
        (
            totals.uncommitted.request_count,
            totals.uncommitted.priced_cost_10000
        ),
        (1, 100)
    );
    assert_eq!(
        (
            report.coverage.requests.priced_request_count,
            report.coverage.requests.priced_cost_10000
        ),
        (2, 200)
    );

    let users = report
        .users
        .iter()
        .map(|user| user.user_id)
        .collect::<Vec<_>>();
    assert_eq!(
        users,
        [dev, reviewer],
        "the service account's commit counts toward totals only"
    );
    let dev_row = &report.users[0];
    assert_eq!(
        (
            dev_row.commit_count,
            dev_row.merged_pull_request_count,
            dev_row.agent_session_count
        ),
        (1, 2, 1)
    );
    assert_eq!(
        (
            dev_row.requests.request_count,
            dev_row.requests.priced_cost_10000
        ),
        (2, 100)
    );
    assert_eq!(dev_row.models, ["claude-sonnet", "gpt-5"]);
    assert_eq!(dev_row.last_activity_at, Some(minutes(70)));
    let reviewer_row = &report.users[1];
    assert_eq!(
        (
            reviewer_row.commit_count,
            reviewer_row.requests.usage_missing_request_count
        ),
        (1, 1)
    );
    assert_eq!(reviewer_row.models, ["unknown"]);

    let models = report
        .models
        .iter()
        .map(|model| model.model_key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(models, ["gpt-5", "claude-sonnet", "unknown"]);
    assert_eq!(
        (
            report.models[0].commit_count,
            report.models[0].uncommitted.request_count
        ),
        (1, 1)
    );

    let prs = &report.pull_requests;
    assert_eq!(
        (
            prs[0].pull_request.number,
            prs[0].attributed_commit_count,
            prs[0].ai_lines
        ),
        (12, 2, 20)
    );
    assert_eq!(prs[0].requests.request_count, 2);
    assert_eq!(
        (prs[1].pull_request.number, prs[1].attributed_commit_count),
        (13, 1)
    );
    assert_eq!(prs[1].requests.usage_missing_request_count, 1);

    let changes = report
        .changes
        .iter()
        .map(|change| {
            (
                change.commit.attribution_commit_id.as_u128(),
                change.pull_request_number,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(changes, [(3, None), (2, Some(13)), (1, Some(12))]);
    assert_eq!(report.changes[2].note_models, ["claude-sonnet-4-6"]);
    assert_eq!(report.changes[2].agent_tools, ["claude"]);
}

#[tokio::test]
async fn an_unmatched_commit_author_is_counted_but_not_credited_to_the_owner() {
    let dev = Uuid::new_v4();
    let owner = user_owner(dev);
    let contractor = AttributionCommitRecord {
        author_user_id: None,
        author_email_hash: Some("contractor-hash".to_string()),
        ..commit(1, &owner, REPOSITORY, minutes(10), Vec::new())
    };
    let without_email = AttributionCommitRecord {
        author_user_id: None,
        ..commit(2, &owner, REPOSITORY, minutes(20), Vec::new())
    };
    let store = FakeStore::with_commits(vec![contractor, without_email]);
    let report = report(&store, &query()).await;

    assert_eq!(
        (
            report.totals.commit_count,
            report.totals.unmatched_author_commit_count
        ),
        (2, 2)
    );
    let users = report
        .users
        .iter()
        .map(|user| (user.user_id, user.commit_count, user.last_activity_at))
        .collect::<Vec<_>>();
    assert_eq!(
        users,
        [(dev, 1, Some(minutes(20)))],
        "only the commit without an email counts for the owner"
    );
}

#[tokio::test]
async fn an_unmatched_pull_request_author_is_counted_but_not_credited_to_the_owner() {
    let dev = Uuid::new_v4();
    let owner = user_owner(dev);
    let contractor = AttributionPullRequestRecord {
        author_email_hash: Some("contractor-hash".to_string()),
        ..pull_request(12, &owner, minutes(10), &[])
    };
    let without_email = pull_request(13, &owner, minutes(20), &[]);
    let store = FakeStore {
        pull_requests: Mutex::new(vec![contractor, without_email]),
        ..FakeStore::default()
    };
    let report = report(&store, &query()).await;

    assert_eq!(report.totals.pull_request_count, 2);
    let users = report
        .users
        .iter()
        .map(|user| {
            (
                user.user_id,
                user.merged_pull_request_count,
                user.last_activity_at,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        users,
        [(dev, 1, Some(minutes(20)))],
        "only the pull request without an email counts for the owner"
    );
}

#[tokio::test]
async fn splits_request_lookups_that_reach_the_cap() {
    let dev = user_owner(Uuid::new_v4());
    let per_session = ATTRIBUTION_MAX_REQUEST_FACTS as usize * 3 / 5;
    let mut store = FakeStore {
        agent_sessions: vec![
            agent_session(&dev, "h1", minutes(1)),
            agent_session(&dev, "h2", minutes(1)),
        ],
        ..FakeStore::default()
    };
    for hash in ["h1", "h2"] {
        for index in 0..per_session {
            store
                .facts
                .push(request(&dev, hash, &format!("{hash}-{index}"), minutes(2)));
        }
    }
    let report = report(&store, &query()).await;

    assert_eq!(
        report.totals.uncommitted.request_count,
        2 * per_session as i64
    );
    assert!(!report.coverage.request_facts_truncated);
    assert_eq!(*store.fact_lookups.lock().expect("lookups"), [2, 1, 1]);
}

#[tokio::test]
async fn a_session_past_the_lookup_cap_is_reported_truncated() {
    let dev = user_owner(Uuid::new_v4());
    let cap = ATTRIBUTION_MAX_REQUEST_FACTS as usize;
    let mut store = FakeStore {
        agent_sessions: vec![agent_session(&dev, "h1", minutes(1))],
        ..FakeStore::default()
    };
    for index in 0..=cap {
        store
            .facts
            .push(request(&dev, "h1", &format!("r-{index:06}"), minutes(2)));
    }
    let report = report(&store, &query()).await;

    assert!(report.coverage.request_facts_truncated);
    assert_eq!(report.totals.uncommitted.request_count, cap as i64);
}

#[tokio::test]
async fn flags_commit_truncation_only_when_it_affects_the_report() {
    let dev = user_owner(Uuid::new_v4());
    let capped = AttributionReportQuery {
        limit: 2,
        ..query()
    };
    let store = FakeStore::with_commits(vec![
        commit(1, &dev, REPOSITORY, minutes(10), Vec::new()),
        commit(2, &dev, REPOSITORY, minutes(20), Vec::new()),
    ]);
    assert!(report(&store, &capped).await.coverage.commits_truncated);

    let store = FakeStore::with_commits(vec![
        commit(1, &dev, REPOSITORY, minutes(10), Vec::new()),
        commit(2, &dev, REPOSITORY, minutes(-60), Vec::new()),
    ]);
    let lookback_only = report(&store, &capped).await;
    assert!(
        !lookback_only.coverage.commits_truncated,
        "the cap was reached by lookback rows"
    );
    assert_eq!(lookback_only.totals.commit_count, 1);

    // A window pull request may read lookback commits the cap cut.
    let store = FakeStore {
        pull_requests: Mutex::new(vec![pull_request(7, &dev, minutes(30), &[3])]),
        ..FakeStore::with_commits(vec![
            commit(1, &dev, REPOSITORY, minutes(10), Vec::new()),
            commit(2, &dev, REPOSITORY, minutes(-60), Vec::new()),
            commit(3, &dev, REPOSITORY, minutes(-120), Vec::new()),
        ])
    };
    assert!(report(&store, &capped).await.coverage.commits_truncated);
}

#[tokio::test]
async fn a_users_report_counts_only_their_own_uncommitted_requests() {
    let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
    let bob_owner = user_owner(bob);
    // Bob's key reported a commit Alice authored, from Bob's agent session.
    let authored = AttributionCommitRecord {
        author_user_id: Some(alice),
        ..commit(
            1,
            &bob_owner,
            REPOSITORY,
            minutes(10),
            vec![note_session("s_aa", &["h1"])],
        )
    };
    let mut store = FakeStore::with_commits(vec![authored]);
    store.facts = vec![
        request(&bob_owner, "h1", "r1", minutes(5)),
        request(&bob_owner, "h1", "r2", minutes(20)),
    ];
    let for_alice = AttributionReportQuery {
        user_id: Some(alice),
        ..query()
    };

    let alice_report = report(&store, &for_alice).await;
    assert_eq!(change_requests(&alice_report, 1), 1);
    assert_eq!(alice_report.totals.uncommitted.request_count, 0);
    // The tab aggregates follow the same rule: Bob's later request never reaches Alice's report.
    let session_requests = |report: &AttributionReport| -> (i64, i64) {
        (
            report
                .session_outcomes
                .iter()
                .map(|tally| tally.requests.request_count)
                .sum(),
            report
                .harness_models
                .iter()
                .map(|row| row.requests.request_count)
                .sum(),
        )
    };
    assert_eq!(session_requests(&alice_report), (1, 1));
    let summary = alice_report.summary();
    assert_eq!(summary.priced_cost_10000, 100);
    assert_eq!(
        summary.shipped_cost_10000, 100,
        "shipped and total spend count the same requests"
    );

    let everyone = report(&store, &query()).await;
    assert_eq!(everyone.totals.uncommitted.request_count, 1);
    assert_eq!(session_requests(&everyone), (2, 2));
}

#[tokio::test]
async fn daily_buckets_stop_before_an_exclusive_midnight() {
    let start = t0().replace_time(time::Time::MIDNIGHT);
    let window = AttributionReportQuery {
        from: start,
        to: start + Duration::days(2),
        ..query()
    };
    let report = report(&FakeStore::default(), &window).await;
    assert_eq!(
        report.daily.len(),
        2,
        "two whole days, not the empty third date of `to`"
    );
    assert_eq!(report.daily[0].day, start.date());
}

#[tokio::test]
async fn a_session_open_across_the_window_counts_its_window_requests_as_uncommitted() {
    let dev = user_owner(Uuid::new_v4());
    let mut store = FakeStore {
        // Started a day before the window and never reported an end.
        agent_sessions: vec![agent_session(&dev, "h1", minutes(-24 * 60))],
        facts: vec![
            request(&dev, "h1", "before", minutes(-60)),
            request(&dev, "h1", "inside", minutes(30)),
        ],
        ..FakeStore::default()
    };

    let report = report(&store, &query()).await;
    assert_eq!(report.totals.uncommitted.request_count, 1);
    assert_eq!(
        report.totals.agent_session_count, 0,
        "it neither started nor ended in the window"
    );
    assert!(report.users.is_empty());
    // Its spend still shows in the outcome flow, as a session with no commit.
    let no_commit = report
        .session_outcomes
        .iter()
        .find(|tally| tally.outcome == crate::attribution::AttributionSessionOutcome::NoCommit)
        .expect("the open session's spend reaches the outcome flow");
    assert_eq!(
        (no_commit.session_count, no_commit.requests.request_count),
        (1, 1)
    );

    // One that ended before the window opened adds nothing.
    store.agent_sessions[0].ended_at = Some(minutes(-30));
    let ended = build_attribution_report(&store, &query()).await.unwrap();
    assert_eq!(ended.totals.uncommitted.request_count, 0);
}

#[tokio::test]
async fn flags_session_truncation_only_when_the_cap_cut_window_sessions() {
    let dev = user_owner(Uuid::new_v4());
    let capped = AttributionReportQuery {
        limit: 2,
        ..query()
    };
    let mut store = FakeStore {
        agent_sessions: vec![
            agent_session(&dev, "h1", minutes(10)),
            agent_session(&dev, "h2", minutes(20)),
        ],
        ..FakeStore::default()
    };
    assert!(
        report(&store, &capped)
            .await
            .coverage
            .agent_sessions_truncated
    );

    store.agent_sessions[1] = agent_session(&dev, "h2", minutes(-60));
    assert!(
        !report(&store, &capped)
            .await
            .coverage
            .agent_sessions_truncated,
        "the cap was reached by a session last observed before the window"
    );
}

#[tokio::test]
async fn a_pull_request_two_owners_reported_counts_once_for_its_author() {
    let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
    let (alice_owner, bob_owner) = (user_owner(alice), user_owner(bob));
    let reported_by = |owner: &AttributionOwner| AttributionPullRequestRecord {
        author_user_id: Some(alice),
        ..pull_request(7, owner, minutes(30), &[1])
    };
    let store = FakeStore {
        pull_requests: Mutex::new(vec![reported_by(&bob_owner), reported_by(&alice_owner)]),
        ..FakeStore::with_commits(vec![commit(
            1,
            &alice_owner,
            REPOSITORY,
            minutes(10),
            Vec::new(),
        )])
    };

    for query in [
        query(),
        AttributionReportQuery {
            user_id: Some(alice),
            ..query()
        },
    ] {
        let report = report(&store, &query).await;
        assert_eq!(report.totals.pull_request_count, 1);
        assert_eq!(report.pull_requests.len(), 1);
        assert_eq!(report.pull_requests[0].pull_request.owner, alice_owner);
        let author = report
            .users
            .iter()
            .find(|user| user.user_id == alice)
            .unwrap();
        assert_eq!(author.merged_pull_request_count, 1);
    }
}

#[tokio::test]
async fn sessions_reach_their_furthest_stage_and_feed_the_tab_insights() {
    use crate::attribution::AttributionSessionOutcome;

    let dev = user_owner(Uuid::new_v4());
    let mut on_branch = commit(
        2,
        &dev,
        REPOSITORY,
        minutes(40),
        vec![note_session("s_b", &["hb"])],
    );
    on_branch.is_default_branch = false;
    let mut unmerged = commit(
        3,
        &dev,
        REPOSITORY,
        minutes(50),
        vec![note_session("s_c", &["hc"])],
    );
    unmerged.is_default_branch = false;
    let store = FakeStore {
        commits: Mutex::new(vec![
            commit(
                1,
                &dev,
                REPOSITORY,
                minutes(30),
                vec![note_session("s_a", &["ha"])],
            ),
            on_branch,
            unmerged,
        ]),
        pull_requests: Mutex::new(vec![{
            let mut merged = pull_request(7, &dev, minutes(90), &[2]);
            merged.title = "Revert \"Change 6\"".to_string();
            merged.commits[0].authored_at = Some(minutes(35));
            merged
        }]),
        // A hook session with requests but no commit, and the merged commit's session.
        agent_sessions: vec![
            agent_session(&dev, "hidle", minutes(5)),
            agent_session(&dev, "hb", minutes(20)),
        ],
        facts: vec![
            request(&dev, "ha", "a1", minutes(25)),
            request(&dev, "hb", "b1", minutes(35)),
            request(&dev, "hb", "b2", minutes(38)),
            request(&dev, "hidle", "i1", minutes(60)),
        ],
        ..FakeStore::default()
    };
    let report = report(&store, &query()).await;

    let outcome = |kind: AttributionSessionOutcome| {
        report
            .session_outcomes
            .iter()
            .find(|tally| tally.outcome == kind)
            .map(|tally| (tally.session_count, tally.requests.request_count))
    };
    assert_eq!(
        outcome(AttributionSessionOutcome::Merged),
        Some((1, 2)),
        "the hook and note session join by hash"
    );
    assert_eq!(
        outcome(AttributionSessionOutcome::DefaultBranch),
        Some((1, 1))
    );
    assert_eq!(outcome(AttributionSessionOutcome::Unmerged), Some((1, 0)));
    assert_eq!(outcome(AttributionSessionOutcome::NoCommit), Some((1, 1)));

    let claude = report
        .harness_models
        .iter()
        .find(|row| row.harness == "claude" && row.model_key == "claude-sonnet")
        .expect("claude row by request model");
    assert_eq!((claude.session_count, claude.shipped_session_count), (2, 2));
    assert_eq!(claude.ai_lines, 20);

    let day = &report.daily[0];
    assert_eq!(
        report.daily.len(),
        2,
        "a window from 08:00 to 08:00 the next day spans two UTC dates"
    );
    assert_eq!(
        report.daily.iter().map(|day| day.commit_count).sum::<i64>(),
        3
    );
    assert_eq!(
        report
            .daily
            .iter()
            .map(|day| day.merged_pull_request_count)
            .sum::<i64>(),
        1
    );
    assert!(day.committed.request_count + report.daily[1].committed.request_count >= 3);

    let merged = &report.pull_requests[0];
    // A title with no revert line, button or matching change is a possible revert only.
    assert!(!merged.is_revert);
    assert_eq!(
        merged.revert.map(|revert| revert.evidence),
        Some(AttributionRevertEvidence::Title)
    );
    assert_eq!(report.pull_request_lifecycle.possible_revert_count, 1);
    assert_eq!(report.pull_request_lifecycle.revert_count, 0);
    assert_eq!(
        merged.first_commit_at,
        Some(minutes(35)),
        "the author time precedes the note"
    );
    assert_eq!(merged.lead_time(), Some(Duration::minutes(55)));
    assert_eq!(merged.commits.len(), 1);
    assert_eq!(merged.commits[0].requests.request_count, 2);

    let summary = report.summary();
    assert_eq!(
        summary.reverted_pull_request_count, 0,
        "possible reverts are not counted"
    );
    assert_eq!(summary.median_lead_time_seconds, Some(55 * 60));
    assert_eq!(summary.shipped_cost_10000, 300);
}

#[tokio::test]
async fn shipped_and_total_spend_share_requests_from_before_the_window() {
    use crate::attribution::AttributionSessionOutcome;

    let dev = user_owner(Uuid::new_v4());
    // The session worked before the window and committed to the default branch inside it.
    let mut store = FakeStore::with_commits(vec![commit(
        1,
        &dev,
        REPOSITORY,
        minutes(30),
        vec![note_session("s_aa", &["h1"])],
    )]);
    store.facts = vec![
        request(&dev, "h1", "before", minutes(-20)),
        request(&dev, "h1", "inside", minutes(10)),
        request(&dev, "h1", "after", minutes(40)),
    ];
    let report = report(&store, &query()).await;

    let summary = report.summary();
    assert_eq!(
        summary.priced_cost_10000, 300,
        "two allocated requests and one uncommitted"
    );
    assert_eq!(
        summary.shipped_cost_10000, 300,
        "the same three requests, all on a shipped session"
    );
    let shipped = report
        .session_outcomes
        .iter()
        .find(|tally| tally.outcome == AttributionSessionOutcome::DefaultBranch)
        .expect("default-branch outcome");
    assert_eq!(shipped.requests.request_count, 3);
}

#[tokio::test]
async fn a_commit_before_the_window_does_not_set_a_sessions_outcome() {
    use crate::attribution::AttributionSessionOutcome;

    let dev = user_owner(Uuid::new_v4());
    // The session's only commit landed in the lookback, and it kept spending inside the window.
    let mut store = FakeStore::with_commits(vec![commit(
        1,
        &dev,
        REPOSITORY,
        minutes(-30),
        vec![note_session("s_aa", &["h1"])],
    )]);
    store.facts = vec![request(&dev, "h1", "inside", minutes(10))];
    let report = report(&store, &query()).await;

    let outcomes = report
        .session_outcomes
        .iter()
        .map(|tally| (tally.outcome, tally.requests.request_count))
        .collect::<Vec<_>>();
    assert_eq!(outcomes, vec![(AttributionSessionOutcome::NoCommit, 1)]);
    assert_eq!(report.summary().shipped_cost_10000, 0);
}

#[tokio::test]
async fn harness_cost_per_line_counts_only_fully_priced_sessions() {
    let dev = user_owner(Uuid::new_v4());
    // One session's request is priced; the other's request is unpriced.
    let mut store = FakeStore::with_commits(vec![commit(
        1,
        &dev,
        REPOSITORY,
        minutes(30),
        vec![note_session("s_aa", &["h1"]), note_session("s_bb", &["h2"])],
    )]);
    let mut unpriced = request(&dev, "h2", "unpriced", minutes(20));
    unpriced.cost_10000 = None;
    unpriced.pricing_status = Some("unpriced".to_string());
    store.facts = vec![request(&dev, "h1", "priced", minutes(10)), unpriced];
    let report = report(&store, &query()).await;

    let row = report
        .harness_models
        .iter()
        .find(|row| row.harness == "claude" && row.model_key == "claude-sonnet")
        .expect("claude row");
    assert_eq!((row.session_count, row.ai_lines), (2, 20));
    assert_eq!(
        (
            row.priced_session_count,
            row.priced_ai_lines,
            row.priced_cost_10000
        ),
        (1, 10, 100),
        "the unpriced session's lines never read as free output"
    );
}

#[tokio::test]
async fn pull_request_commit_details_stop_at_the_cap() {
    use crate::attribution::report::MAX_PULL_REQUEST_COMMIT_DETAILS;

    let dev = user_owner(Uuid::new_v4());
    let ids = (1..=60_u128).collect::<Vec<_>>();
    let mut store = FakeStore::with_commits(
        ids.iter()
            .map(|id| {
                commit(
                    *id,
                    &dev,
                    REPOSITORY,
                    minutes(i64::try_from(*id).unwrap()),
                    vec![],
                )
            })
            .collect(),
    );
    store.pull_requests = Mutex::new(vec![pull_request(7, &dev, minutes(120), &ids)]);
    let report = report(&store, &query()).await;

    let merged = &report.pull_requests[0];
    assert_eq!(
        merged.attributed_commit_count, 60,
        "totals use every commit"
    );
    assert_eq!(merged.ai_lines, 600);
    assert_eq!(merged.commits.len(), MAX_PULL_REQUEST_COMMIT_DETAILS);
    assert_eq!(merged.commits[0].commit_sha, sha(1), "oldest first");
}

#[tokio::test]
async fn the_report_commit_detail_budget_goes_to_the_newest_pull_requests() {
    use crate::attribution::report::{MAX_PULL_REQUEST_COMMIT_DETAILS, MAX_REPORT_COMMIT_DETAILS};

    let dev = user_owner(Uuid::new_v4());
    let per_pr = u128::try_from(MAX_PULL_REQUEST_COMMIT_DETAILS).unwrap();
    let full = u128::try_from(MAX_REPORT_COMMIT_DETAILS).unwrap() / per_pr;
    // One more pull request than the budget covers, each with a full set of commits.
    let pull_requests = (0..=full)
        .map(|index| {
            let ids = (index * per_pr + 1..=(index + 1) * per_pr).collect::<Vec<_>>();
            let number = i64::try_from(index).unwrap();
            (number, ids)
        })
        .collect::<Vec<_>>();
    let commits = pull_requests
        .iter()
        .flat_map(|(_, ids)| ids.iter().copied())
        .map(|id| commit(id, &dev, REPOSITORY, minutes(1), vec![]))
        .collect();
    let mut store = FakeStore::with_commits(commits);
    // Newest first, as the store returns them.
    store.pull_requests = Mutex::new(
        pull_requests
            .iter()
            .rev()
            .map(|(number, ids)| pull_request(*number, &dev, minutes(600 - *number), ids))
            .collect(),
    );
    let mut query = query();
    query.limit = 100_000;
    let report = report(&store, &query).await;

    let detailed = report
        .pull_requests
        .iter()
        .map(|pull_request| pull_request.commits.len())
        .sum::<usize>();
    assert_eq!(detailed, MAX_REPORT_COMMIT_DETAILS);
    let last = report.pull_requests.last().expect("oldest pull request");
    assert!(
        last.commits.is_empty(),
        "past the budget, details are left out"
    );
    assert_eq!(last.attributed_commit_count, i64::try_from(per_pr).unwrap());
}

#[tokio::test]
async fn the_summary_build_matches_the_full_reports_summary() {
    use crate::attribution::build_attribution_summary;

    let dev = user_owner(Uuid::new_v4());
    let mut store = FakeStore::with_commits(vec![commit(
        1,
        &dev,
        REPOSITORY,
        minutes(30),
        vec![note_session("s_aa", &["h1"])],
    )]);
    store.pull_requests = Mutex::new(vec![pull_request(7, &dev, minutes(60), &[1])]);
    store.facts = vec![request(&dev, "h1", "inside", minutes(10))];
    let full = report(&store, &query()).await;
    let (summary, coverage) = build_attribution_summary(&store, &query())
        .await
        .expect("summary builds");

    assert_eq!(summary, full.summary());
    assert_eq!(coverage, full.coverage);
    assert_eq!(
        full.pull_requests[0].commits.len(),
        1,
        "the full report keeps its details"
    );
}

#[tokio::test]
async fn the_harness_build_keeps_only_what_agent_harnesses_reads() {
    use crate::attribution::build_attribution_harness_report;

    let dev = user_owner(Uuid::new_v4());
    let mut store = FakeStore::with_commits(vec![commit(
        1,
        &dev,
        REPOSITORY,
        minutes(30),
        vec![note_session("s_aa", &["h1"])],
    )]);
    store.pull_requests = Mutex::new(vec![pull_request(7, &dev, minutes(60), &[1])]);
    store.facts = vec![request(&dev, "h1", "inside", minutes(10))];
    let full = report(&store, &query()).await;
    let harness = build_attribution_harness_report(&store, &query())
        .await
        .expect("harness report builds");

    assert!(!full.harness_models.is_empty());
    assert_eq!(harness.harness_models, full.harness_models);
    assert_eq!(harness.session_outcomes, full.session_outcomes);
    assert_eq!(harness.coverage, full.coverage);
    assert_eq!(harness.totals, full.totals);
    assert!(harness.changes.is_empty() && harness.users.is_empty());
    assert!(harness.daily.is_empty() && harness.pull_requests.is_empty());
}

#[tokio::test]
async fn daily_buckets_put_commits_and_uncommitted_spend_on_their_own_dates() {
    let dev = user_owner(Uuid::new_v4());
    let start = t0().replace_time(time::Time::MIDNIGHT);
    let window = AttributionReportQuery {
        from: start,
        to: start + Duration::days(2),
        ..query()
    };
    let day_one = start + Duration::hours(10);
    let day_two = start + Duration::days(1) + Duration::hours(9);
    let mut store = FakeStore::with_commits(vec![
        commit(
            1,
            &dev,
            REPOSITORY,
            day_one,
            vec![note_session("s_a", &["ha"])],
        ),
        commit(
            2,
            &dev,
            REPOSITORY,
            day_two,
            vec![note_session("s_b", &["hb"])],
        ),
    ]);
    store.facts = vec![
        request(&dev, "ha", "a1", day_one - Duration::minutes(5)),
        request(&dev, "hb", "b1", day_two - Duration::minutes(5)),
        // After session b's last commit, so uncommitted, on day two.
        request(&dev, "hb", "b2", day_two + Duration::hours(1)),
    ];
    let report = report(&store, &window).await;

    let days = report
        .daily
        .iter()
        .map(|day| {
            (
                day.day,
                day.commit_count,
                day.committed.request_count,
                day.uncommitted.request_count,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        days,
        vec![
            (start.date(), 1, 1, 0),
            ((start + Duration::days(1)).date(), 1, 1, 1),
        ]
    );
}

#[tokio::test]
async fn explicit_journals_override_provider_parents_and_count_requests_and_cost_once() {
    for mapping_before_request in [true, false] {
        let user = Uuid::from_u128(301);
        let owner = user_owner(user);
        let other_user = Uuid::from_u128(302);
        let other = user_owner(other_user);
        let ids = [303, 304, 305].map(|id| Uuid::from_u128(id).to_string());
        let native =
            hash_lineage_candidate(&owner.ownership_scope_key, "claude_code", "provider-parent");
        let child = hash_lineage_candidate(&owner.ownership_scope_key, "oceans", "journal-child");
        let mut store = FakeStore::with_commits(vec![
            commit(
                1,
                &owner,
                REPOSITORY,
                minutes(10),
                vec![note_session("parent", &[&native])],
            ),
            commit(
                2,
                &owner,
                REPOSITORY,
                minutes(20),
                vec![
                    note_session("child-a", &[&child]),
                    note_session("child-b", &[&child]),
                ],
            ),
        ]);
        let facts = ids
            .iter()
            .enumerate()
            .map(|(i, id)| request(&owner, &native, id, minutes(5 + i as i64)))
            .collect::<Vec<_>>();
        if !mapping_before_request {
            store.facts = facts.clone();
        }
        let users = FakeUsers::default();
        ingest(
            &store,
            &users,
            &owner.ownership_scope_key,
            &user_key(user),
            vec![
                entry(
                    "child-request-1",
                    1,
                    request_session(&ids[0], "journal-child"),
                ),
                entry(
                    "child-request-2",
                    2,
                    request_session(&ids[1], "journal-child"),
                ),
                entry("replay", 2, request_session(&ids[1], "wrong-journal")),
            ],
        )
        .await
        .expect("journal mappings");
        // Same request UUID but a different owner: it must neither steal nor suppress the
        // parent's remaining request, including its native fallback.
        ingest(
            &store,
            &users,
            &other.ownership_scope_key,
            &user_key(other_user),
            vec![entry(
                "other-owner",
                1,
                request_session(&ids[2], "journal-child"),
            )],
        )
        .await
        .expect("other owner's mapping");
        if mapping_before_request {
            store.facts = facts;
        }
        let output = report(&store, &query()).await;
        assert_eq!(
            change_requests(&output, 1),
            1,
            "native parent excludes explicitly mapped children"
        );
        assert_eq!(change_requests(&output, 2), 2);
        assert_eq!(output.totals.requests.request_count, 3);
        assert_eq!(output.totals.requests.priced_cost_10000, 300);
        assert_eq!(output.coverage.requests.request_count, 3);
        assert_eq!(output.coverage.requests.priced_cost_10000, 300);
        assert_eq!(output.totals.uncommitted.request_count, 0);
        let child_change = output
            .changes
            .iter()
            .find(|change| change.commit.commit_sha == sha(2))
            .expect("child change");
        assert_eq!(child_change.requests.priced_cost_10000, 200);
        assert_eq!(
            output
                .models
                .iter()
                .map(|model| model.requests.request_count)
                .sum::<i64>(),
            3
        );
    }
}

#[tokio::test]
async fn revert_links_resolve_to_the_reverted_pull_request() {
    let dev = user_owner(Uuid::new_v4());
    let mut original = pull_request(5, &dev, minutes(60), &[11]);
    original.commits[0].commit_sha = "abcdef0123456789abcdef0123456789abcdef01".to_string();
    // Reverts that commit by an abbreviated SHA, with a title that does not say revert.
    let mut by_commit = pull_request(6, &dev, minutes(180), &[12]);
    by_commit.title = "Tidy the widgets module".to_string();
    by_commit.lifecycle.details.reverts = Some(AttributionRevertLink {
        pull_request_number: None,
        commit_shas: vec!["abcdef0123".to_string()],
        evidence: AttributionRevertEvidence::RevertCommit,
    });
    let mut by_title = pull_request(7, &dev, minutes(240), &[13]);
    by_title.lifecycle.details.reverts = Some(AttributionRevertLink {
        pull_request_number: Some(5),
        commit_shas: vec![],
        evidence: AttributionRevertEvidence::Title,
    });
    let mut store = FakeStore::with_commits(vec![]);
    store.pull_requests = Mutex::new(vec![original, by_commit, by_title]);
    let report = report(&store, &query()).await;

    let find = |number: i64| {
        report
            .pull_requests
            .iter()
            .find(|activity| activity.pull_request.number == number)
            .unwrap()
    };
    let revert = find(6);
    assert!(revert.is_revert);
    assert_eq!(
        revert
            .revert
            .and_then(|revert| revert.reverts_pull_request_number),
        Some(5)
    );
    assert_eq!(
        find(5).reverted_by,
        Some(6),
        "a possible revert does not mark the original"
    );
    assert!(!find(7).is_revert);
    let lifecycle = &report.pull_request_lifecycle;
    assert_eq!(
        (lifecycle.revert_count, lifecycle.possible_revert_count),
        (1, 1)
    );
    assert_eq!(lifecycle.median_time_to_revert_seconds, Some(2 * 3600));
    assert_eq!(report.summary().reverted_pull_request_count, 1);
}

#[tokio::test]
async fn a_revert_of_the_pull_requests_own_commit_is_not_a_revert() {
    let dev = user_owner(Uuid::new_v4());
    // The branch added a commit and reverted it before merging; an older CLI still sent the link.
    let mut cleanup = pull_request(8, &dev, minutes(120), &[21, 22]);
    cleanup.commits[0].commit_sha = "fedcba9876543210fedcba9876543210fedcba98".to_string();
    cleanup.lifecycle.details.reverts = Some(AttributionRevertLink {
        pull_request_number: None,
        commit_shas: vec!["fedcba98".to_string()],
        evidence: AttributionRevertEvidence::RevertCommit,
    });
    let mut store = FakeStore::with_commits(vec![]);
    store.pull_requests = Mutex::new(vec![cleanup]);
    let report = report(&store, &query()).await;

    let activity = &report.pull_requests[0];
    assert!(!activity.is_revert);
    assert!(activity.revert.is_none());
    let lifecycle = &report.pull_request_lifecycle;
    assert_eq!(
        (lifecycle.revert_count, lifecycle.possible_revert_count),
        (0, 0)
    );
    assert_eq!(report.summary().reverted_pull_request_count, 0);
}

#[tokio::test]
async fn the_revert_lookback_is_read_only_for_a_target_outside_the_window() {
    let dev = user_owner(Uuid::new_v4());
    let original = pull_request(5, &dev, minutes(30), &[11]);
    let mut revert = pull_request(6, &dev, minutes(60), &[12]);
    revert.title = "Tidy".to_string();
    revert.lifecycle.details.reverts = Some(AttributionRevertLink {
        pull_request_number: Some(5),
        commit_shas: Vec::new(),
        evidence: AttributionRevertEvidence::ChangeMatch,
    });
    let mut store = FakeStore::with_commits(vec![]);
    store.pull_requests = Mutex::new(vec![revert, original]);
    let report = report(&store, &query()).await;

    assert_eq!(report.pull_request_lifecycle.revert_count, 1);
    assert_eq!(
        store.pull_request_reads.lock().unwrap().len(),
        1,
        "the original merged in the window, so nothing older is read"
    );
}

#[tokio::test]
async fn open_and_closed_pull_requests_feed_lifecycle_figures_but_not_merged_ones() {
    let dev = user_owner(Uuid::new_v4());
    let mut reviewed = pull_request(1, &dev, minutes(300), &[]);
    reviewed.lifecycle.opened_at = Some(minutes(60));
    reviewed.lifecycle.details.first_review_at = Some(minutes(120));
    reviewed.lifecycle.details.review_count = Some(2);
    let mut unreviewed = pull_request(2, &dev, minutes(310), &[]);
    unreviewed.lifecycle.opened_at = Some(minutes(200));
    unreviewed.lifecycle.details.review_count = Some(0);
    let mut open = pull_request(3, &dev, minutes(0), &[]);
    open.merged_at = None;
    open.lifecycle.state = AttributionPullRequestState::Open;
    open.lifecycle.closed_at = None;
    open.lifecycle.opened_at = Some(minutes(30));
    open.lifecycle.details.is_draft = Some(true);
    let mut closed = pull_request(4, &dev, minutes(0), &[]);
    closed.merged_at = None;
    closed.lifecycle.state = AttributionPullRequestState::Closed;
    closed.lifecycle.opened_at = Some(minutes(20));
    closed.lifecycle.closed_at = Some(minutes(90));
    closed.lifecycle.details.first_review_at = Some(minutes(80));
    // Reviewed only after it merged, which still counts as merged without review.
    let mut reviewed_late = pull_request(6, &dev, minutes(320), &[]);
    reviewed_late.lifecycle.opened_at = Some(minutes(250));
    reviewed_late.lifecycle.details.first_review_at = Some(minutes(400));
    reviewed_late.lifecycle.details.review_count = Some(1);
    // Opened in the window but merged after it, as a previous-window read sees it.
    let mut merged_later = pull_request(5, &dev, minutes(60 * 30), &[]);
    merged_later.lifecycle.opened_at = Some(minutes(10));
    let mut store = FakeStore::with_commits(vec![]);
    store.pull_requests = Mutex::new(vec![
        reviewed,
        unreviewed,
        open,
        closed,
        reviewed_late,
        merged_later,
    ]);
    let report = report(&store, &query()).await;

    let merged: Vec<i64> = report
        .pull_requests
        .iter()
        .map(|activity| activity.pull_request.number)
        .collect();
    assert_eq!(
        merged.len(),
        3,
        "only pull requests merged in the window: {merged:?}"
    );
    assert_eq!(report.totals.pull_request_count, 3);
    let lifecycle = &report.pull_request_lifecycle;
    assert_eq!(lifecycle.opened_count, 6);
    assert_eq!(lifecycle.merged_count, 3);
    assert_eq!(lifecycle.closed_unmerged_count, 1);
    assert_eq!((lifecycle.open_count, lifecycle.draft_count), (1, 1));
    assert_eq!(lifecycle.review_known_count, 3);
    assert_eq!(lifecycle.merged_without_review_count, 2);
    // Reviews 60, 60 and 150 minutes after opening.
    assert_eq!(lifecycle.median_review_wait_seconds, Some(3600));
}

#[tokio::test]
async fn a_revert_resolves_an_original_merged_before_the_window() {
    let dev = user_owner(Uuid::new_v4());
    // Merged 60 days before the window: loaded only to resolve the link, never counted.
    let original = pull_request(5, &dev, t0() - Duration::days(60), &[11]);
    let mut revert = pull_request(6, &dev, minutes(60), &[12]);
    revert.lifecycle.details.reverts = Some(AttributionRevertLink {
        pull_request_number: Some(5),
        commit_shas: vec![],
        evidence: AttributionRevertEvidence::RevertCommit,
    });
    let mut store = FakeStore::with_commits(vec![]);
    store.pull_requests = Mutex::new(vec![original, revert]);
    let report = report(&store, &query()).await;

    assert_eq!(
        *store.pull_request_reads.lock().unwrap(),
        [
            (query().from, query().to),
            (query().from - Duration::days(90), query().from)
        ],
        "the window, then before it for the revert's target"
    );
    assert_eq!(
        report.pull_requests.len(),
        1,
        "the original is not a window merge"
    );
    assert_eq!(report.totals.pull_request_count, 1);
    let lifecycle = &report.pull_request_lifecycle;
    assert_eq!(lifecycle.revert_count, 1);
    assert_eq!(
        lifecycle.median_time_to_revert_seconds,
        Some((Duration::days(60) + Duration::minutes(60)).whole_seconds())
    );
    assert_eq!(lifecycle.merged_count, 1);
    assert!(!report.coverage.pull_requests_truncated);
}

#[tokio::test]
async fn a_later_merged_copy_wins_over_the_authors_earlier_open_copy() {
    let author_id = Uuid::new_v4();
    let author = user_owner(author_id);
    let teammate = user_owner(Uuid::new_v4());
    let mut open_copy = pull_request(9, &author, minutes(0), &[]);
    open_copy.author_user_id = Some(author_id);
    open_copy.merged_at = None;
    open_copy.lifecycle.state = AttributionPullRequestState::Open;
    open_copy.lifecycle.closed_at = None;
    open_copy.lifecycle.opened_at = Some(minutes(10));
    open_copy.observed_at = minutes(20);
    let mut merged_copy = pull_request(9, &teammate, minutes(90), &[]);
    merged_copy.author_user_id = Some(author_id);
    merged_copy.lifecycle.opened_at = Some(minutes(10));
    merged_copy.observed_at = minutes(95);
    let mut store = FakeStore::with_commits(vec![]);
    store.pull_requests = Mutex::new(vec![open_copy, merged_copy]);
    let report = report(&store, &query()).await;

    assert_eq!(report.pull_requests.len(), 1);
    let merged = &report.pull_requests[0].pull_request;
    assert_eq!(merged.merged_at, Some(minutes(90)));
    assert_eq!(
        merged.owner.owner_user_id,
        Some(author_id),
        "attribution stays with the author"
    );
    assert_eq!(report.pull_request_lifecycle.open_count, 0);
}

#[tokio::test]
async fn an_older_open_pull_request_counts_only_when_it_changed_in_the_window() {
    let dev = user_owner(Uuid::new_v4());
    let open = |number: i64, changed_at: OffsetDateTime| {
        let mut pr = pull_request(number, &dev, minutes(0), &[]);
        pr.merged_at = None;
        pr.lifecycle.state = AttributionPullRequestState::Open;
        pr.lifecycle.closed_at = None;
        pr.lifecycle.opened_at = Some(t0() - Duration::days(10));
        pr.lifecycle.last_activity_at = Some(changed_at);
        // Re-synced in the window either way: a sync alone is not activity.
        pr.observed_at = minutes(30);
        pr
    };
    let mut store = FakeStore::with_commits(vec![]);
    store.pull_requests = Mutex::new(vec![
        open(1, minutes(30)),
        open(2, t0() - Duration::days(9)),
    ]);
    let report = report(&store, &query()).await;
    assert_eq!(report.pull_request_lifecycle.open_count, 1);
    assert_eq!(report.pull_request_lifecycle.opened_count, 0);
}

#[tokio::test]
async fn an_open_pull_request_listing_a_merged_commit_is_not_the_reverted_one() {
    let dev = user_owner(Uuid::new_v4());
    let mut original = pull_request(5, &dev, minutes(60), &[11]);
    original.commits[0].commit_sha = "abcdef0123456789abcdef0123456789abcdef01".to_string();
    // A newer open promotion PR lists the same commit.
    let mut promotion = pull_request(8, &dev, minutes(0), &[11]);
    promotion.commits[0].commit_sha = original.commits[0].commit_sha.clone();
    promotion.merged_at = None;
    promotion.lifecycle.state = AttributionPullRequestState::Open;
    promotion.lifecycle.closed_at = None;
    promotion.lifecycle.opened_at = Some(minutes(70));
    let mut revert = pull_request(6, &dev, minutes(180), &[12]);
    revert.lifecycle.details.reverts = Some(AttributionRevertLink {
        pull_request_number: None,
        commit_shas: vec!["abcdef0123".to_string()],
        evidence: AttributionRevertEvidence::RevertCommit,
    });
    let mut store = FakeStore::with_commits(vec![]);
    store.pull_requests = Mutex::new(vec![promotion, revert, original]);
    let report = report(&store, &query()).await;

    let revert = report
        .pull_requests
        .iter()
        .find(|activity| activity.pull_request.number == 6)
        .unwrap();
    assert_eq!(
        revert
            .revert
            .and_then(|revert| revert.reverts_pull_request_number),
        Some(5)
    );
    assert_eq!(
        report.pull_request_lifecycle.median_time_to_revert_seconds,
        Some(2 * 3600)
    );
}

#[tokio::test]
async fn a_read_saturated_by_owner_copies_is_reported_truncated() {
    let devs = [user_owner(Uuid::new_v4()), user_owner(Uuid::new_v4())];
    let mut store = FakeStore::with_commits(vec![]);
    // Two owners each stored the same two pull requests: four rows, two distinct.
    store.pull_requests = Mutex::new(
        devs.iter()
            .flat_map(|dev| {
                [
                    pull_request(1, dev, minutes(60), &[]),
                    pull_request(2, dev, minutes(90), &[]),
                ]
            })
            .collect(),
    );
    let mut capped = query();
    capped.limit = 4;
    let report = report(&store, &capped).await;
    assert_eq!(report.pull_requests.len(), 2);
    assert!(
        report.coverage.pull_requests_truncated,
        "the cap counted every copy"
    );
}

#[tokio::test]
async fn a_shared_commit_links_the_latest_merge_not_the_latest_activity() {
    let dev = user_owner(Uuid::new_v4());
    // The original merged first and was commented on later; a promotion PR merged the same
    // commit after it. The store reads latest activity first, so the original comes first.
    let mut original = pull_request(5, &dev, minutes(60), &[1]);
    original.lifecycle.last_activity_at = Some(minutes(600));
    let mut promotion = pull_request(6, &dev, minutes(120), &[1]);
    promotion.lifecycle.last_activity_at = Some(minutes(130));
    let mut store = FakeStore::with_commits(vec![commit(1, &dev, REPOSITORY, minutes(30), vec![])]);
    store.pull_requests = Mutex::new(vec![original, promotion]);
    let report = report(&store, &query()).await;

    assert_eq!(report.changes[0].pull_request_number, Some(6));
    let merged: Vec<i64> = report
        .pull_requests
        .iter()
        .map(|activity| activity.pull_request.number)
        .collect();
    assert_eq!(merged, [6, 5], "most recently merged first");
}
