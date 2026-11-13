use std::sync::Mutex;

use gateway_core::{
    ATTRIBUTION_MAX_REQUEST_FACTS, AttributionAgentSessionRecord, AttributionCommitRecord,
    AttributionCommitSession, AttributionFreshness, AttributionOwner, AttributionPullRequestCommit,
    AttributionPullRequestRecord, AttributionReportQuery, AttributionRequestFact,
    AttributionSessionEventWrite,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::ingest::{entry, ingest, note, session};
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
        merged_at,
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
    let mut store = FakeStore::default();
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
    let mut store = FakeStore::default();
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

    let everyone = report(&store, &query()).await;
    assert_eq!(everyone.totals.uncommitted.request_count, 1);
}
