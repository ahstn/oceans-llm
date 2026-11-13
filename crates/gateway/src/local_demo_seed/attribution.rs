//! Git AI attribution for two demo developers, matching
//! `.agents/skills/verify-oceans-admin/features/attribution.md`. Gina has two commits joined
//! to her seeded Codex sessions; the newer session keeps making requests after its commit.
//! Alice has a commit whose Claude session matched no traffic, and owns a human-only commit
//! by an author who is not an Oceans user. Two merged PRs cover the `admin-console` commits.
//!
//! Every run deletes both owners' attribution rows and writes them again at times relative to
//! `now`. A replacement note keeps its commit's first observation time, so this is the only way
//! to move the commits forward. Sessions join `agent_session_requests.normalized_session_id`,
//! which the demo agent sessions store unhashed, so each seeded session's hash list holds just
//! that fixture id rather than one hash per session namespace.

use std::collections::HashMap;

use anyhow::Context;
use gateway_core::{
    AttributionCommitSession, AttributionCommitWrite, AttributionFreshness, AttributionOwner,
    AttributionPullRequestCommit, AttributionPullRequestWrite, AttributionRepository,
    AttributionSessionEventKind, AttributionSessionEventWrite,
};
use gateway_store::AnyStore;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::local_demo_uuid;

const ADMIN_CONSOLE: &str = "github.com/oceans-demo/admin-console";
const RELEASE_RUNBOOKS: &str = "github.com/oceans-demo/release-runbooks";
const GINA: &str = "gina@research.local";
const ALICE: &str = "alice@platform.local";
const INSTALLATION_ID: &str = "local-demo-seed";
/// Stands in for the scope-bound hash of an author email that matched no Oceans user.
const UNMATCHED_AUTHOR_HASH: &str = "local-demo-unmatched-author";

struct DemoSession {
    note_session_key: &'static str,
    agent_tool: &'static str,
    agent_model: &'static str,
    harness_key: &'static str,
    /// The fixture session's unhashed `normalized_session_id`, which its requests and hook
    /// events carry.
    session_hash: &'static str,
    /// When the harness reported the session's start and end.
    started: Duration,
    ended: Option<Duration>,
    ai_lines: i64,
    /// Puts the commit midway between this many of the session's linked requests and the
    /// next, instead of at its `age`. A reseed re-dates request logs, but a session keeps the
    /// link times from its first seed, so only the stored times split the requests reliably.
    commit_after_requests: Option<usize>,
}

struct DemoCommit {
    owner: &'static str,
    /// `None` for an author who is not an Oceans user.
    author: Option<&'static str>,
    repository: &'static str,
    commit_sha: &'static str,
    subject: &'static str,
    branch: &'static str,
    age: Duration,
    human_lines: i64,
    session: Option<DemoSession>,
}

struct DemoPullRequest {
    owner: &'static str,
    number: i64,
    title: &'static str,
    head_ref: &'static str,
    merge_commit_sha: &'static str,
    merged: Duration,
    commit_shas: &'static [&'static str],
}

const GINA_ROUTE_COMMIT: &str = "4f2c9a1e7b3d5f60a8c2e4b6d8f0a1c3e5b7d9f1";
const GINA_RUNBOOK_COMMIT: &str = "9b1d3f5a7c9e0b2d4f6a8c0e2b4d6f8a0c2e4b6d";
const ALICE_DASHBOARD_COMMIT: &str = "2e8a4c6f0b2d4e6a8c0f2b4d6e8a0c2f4b6d8e0a";
const CONTRACTOR_COMMIT: &str = "7a0c2e4b6d8f1a3c5e7b9d0f2a4c6e8b0d2f4a6c";

const DEMO_COMMITS: &[DemoCommit] = &[
    // After every request of the route migration session (1h56m to 1h2m ago).
    DemoCommit {
        owner: GINA,
        author: Some(GINA),
        repository: ADMIN_CONSOLE,
        commit_sha: GINA_ROUTE_COMMIT,
        subject: "Rename the observability route to sessions",
        branch: "rename-observability-route",
        age: Duration::minutes(55),
        human_lines: 12,
        session: Some(DemoSession {
            note_session_key: "s_3a9f1c2e7b4d60",
            agent_tool: "codex",
            agent_model: "gpt-oss-120b",
            harness_key: "codex",
            session_hash: "demo-repository-route-migration-2026-07",
            started: Duration::minutes(117),
            ended: Some(Duration::minutes(61)),
            ai_lines: 148,
            commit_after_requests: None,
        }),
    },
    // Seven of the Jira session's requests come before it and three after.
    DemoCommit {
        owner: GINA,
        author: Some(GINA),
        repository: RELEASE_RUNBOOKS,
        commit_sha: GINA_RUNBOOK_COMMIT,
        subject: "Record the 2026.07 release handoff",
        branch: "main",
        age: Duration::seconds(150),
        human_lines: 4,
        session: Some(DemoSession {
            note_session_key: "s_7c2e9a4f1b6d30",
            agent_tool: "codex",
            agent_model: "claude-sonnet",
            harness_key: "codex",
            session_hash: "demo-jira-release-coordination-2026-07",
            started: Duration::minutes(30),
            ended: None,
            ai_lines: 36,
            commit_after_requests: Some(7),
        }),
    },
    // No gateway request carries this session, so the commit shows as unjoined.
    DemoCommit {
        owner: ALICE,
        author: Some(ALICE),
        repository: ADMIN_CONSOLE,
        commit_sha: ALICE_DASHBOARD_COMMIT,
        subject: "Summarise release blockers on the dashboard",
        branch: "release-blockers-summary",
        age: Duration::days(2),
        human_lines: 20,
        session: Some(DemoSession {
            note_session_key: "s_5d1b8e3a9c7f20",
            agent_tool: "claude",
            agent_model: "claude-opus",
            harness_key: "claude_code",
            session_hash: "demo-attribution-dashboard-claude-2026-07",
            started: Duration::hours(49),
            ended: Some(Duration::hours(48)),
            ai_lines: 64,
            commit_after_requests: None,
        }),
    },
    DemoCommit {
        owner: ALICE,
        author: None,
        repository: ADMIN_CONSOLE,
        commit_sha: CONTRACTOR_COMMIT,
        subject: "Add the release blocker query",
        branch: "release-blockers-summary",
        age: Duration::days(4),
        human_lines: 42,
        session: None,
    },
];

const DEMO_PULL_REQUESTS: &[DemoPullRequest] = &[
    DemoPullRequest {
        owner: GINA,
        number: 42,
        title: "Rename the observability route to sessions",
        head_ref: "rename-observability-route",
        merge_commit_sha: "c3e5a7b9d1f3a5c7e9b1d3f5a7c9e1b3d5f7a9c1",
        merged: Duration::minutes(30),
        commit_shas: &[GINA_ROUTE_COMMIT],
    },
    DemoPullRequest {
        owner: ALICE,
        number: 41,
        title: "Summarise release blockers on the dashboard",
        head_ref: "release-blockers-summary",
        merge_commit_sha: "d4f6b8a0c2e4f6a8b0d2c4e6f8a0b2d4c6e8f0a2",
        merged: Duration::days(1),
        commit_shas: &[ALICE_DASHBOARD_COMMIT, CONTRACTOR_COMMIT],
    },
];

pub(super) async fn seed_demo_attribution(
    store: &AnyStore,
    user_ids: &HashMap<&'static str, Uuid>,
    now: OffsetDateTime,
) -> anyhow::Result<()> {
    let user_id = |email: &str| {
        user_ids
            .get(email)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("missing demo user `{email}` for attribution"))
    };
    let owners = [GINA, ALICE]
        .into_iter()
        .map(|email| Ok((email, owner(user_id(email)?))))
        .collect::<anyhow::Result<HashMap<_, _>>>()?;
    for owner in owners.values() {
        store
            .delete_attribution_for_owner(&owner.ownership_scope_key)
            .await
            .context("failed deleting previously seeded attribution")?;
    }
    let mut committed = HashMap::new();
    for commit in DEMO_COMMITS {
        committed.insert(commit.commit_sha, committed_at(store, commit, now).await?);
    }
    for (sequence, commit) in (1..).zip(DEMO_COMMITS) {
        let write = commit_write(
            commit,
            &owners[commit.owner],
            &user_id,
            sequence,
            committed[commit.commit_sha],
            now,
        )?;
        store
            .apply_attribution_commit(&write)
            .await
            .with_context(|| {
                format!("failed seeding attribution commit `{}`", commit.commit_sha)
            })?;
        if let Some(session) = &commit.session {
            seed_session_events(store, commit, session, &owners[commit.owner], now).await?;
        }
    }
    for pull_request in DEMO_PULL_REQUESTS {
        let write = pull_request_write(
            pull_request,
            &owners[pull_request.owner],
            &user_id,
            &committed,
            now,
        )?;
        store
            .apply_attribution_pull_request(&write)
            .await
            .with_context(|| format!("failed seeding pull request #{}", pull_request.number))?;
    }
    Ok(())
}

/// `now - age`, or the point [`DemoSession::commit_after_requests`] picks when the session has
/// enough linked requests.
async fn committed_at(
    store: &AnyStore,
    commit: &DemoCommit,
    now: OffsetDateTime,
) -> anyhow::Result<OffsetDateTime> {
    let fallback = now - commit.age;
    let Some((session, after)) = commit
        .session
        .as_ref()
        .and_then(|session| Some((session, session.commit_after_requests?)))
    else {
        return Ok(fallback);
    };
    let mut times = store
        .list_attribution_request_facts(&[session.session_hash.to_string()], now)
        .await
        .context("failed reading the demo session's requests")?
        .into_iter()
        .map(|fact| fact.occurred_at)
        .collect::<Vec<_>>();
    times.sort_unstable();
    Ok(
        match (
            after.checked_sub(1).and_then(|index| times.get(index)),
            times.get(after),
        ) {
            (Some(before), Some(next)) => *before + (*next - *before) / 2,
            _ => fallback,
        },
    )
}

fn owner(user_id: Uuid) -> AttributionOwner {
    AttributionOwner {
        ownership_scope_key: format!("user:{user_id}"),
        owner_user_id: Some(user_id),
        owner_service_account_id: None,
    }
}

fn commit_write(
    commit: &DemoCommit,
    owner: &AttributionOwner,
    user_id: &impl Fn(&str) -> anyhow::Result<Uuid>,
    sequence: i64,
    committed_at: OffsetDateTime,
    now: OffsetDateTime,
) -> anyhow::Result<AttributionCommitWrite> {
    let author_user_id = commit.author.map(user_id).transpose()?;
    let sessions = commit
        .session
        .iter()
        .map(|session| AttributionCommitSession {
            note_session_key: session.note_session_key.to_string(),
            agent_tool: session.agent_tool.to_string(),
            agent_model: Some(session.agent_model.to_string()),
            harness_key: Some(session.harness_key.to_string()),
            session_hashes: vec![session.session_hash.to_string()],
            ai_lines: session.ai_lines,
            human_author_user_id: author_user_id,
            human_author_hash: None,
        })
        .collect::<Vec<_>>();
    Ok(AttributionCommitWrite {
        attribution_commit_id: local_demo_uuid("attribution_commit", commit.commit_sha),
        owner: owner.clone(),
        repository: commit.repository.to_string(),
        commit_sha: commit.commit_sha.to_string(),
        branch: Some(commit.branch.to_string()),
        is_default_branch: commit.branch == "main",
        author_user_id,
        author_email_hash: author_user_id
            .is_none()
            .then(|| UNMATCHED_AUTHOR_HASH.to_string()),
        schema_version: "authorship/3.0.0".to_string(),
        git_ai_version: Some("1.7.6".to_string()),
        ai_lines: sessions.iter().map(|session| session.ai_lines).sum(),
        human_lines: commit.human_lines,
        freshness: AttributionFreshness {
            installation_id: INSTALLATION_ID.to_string(),
            sequence,
            observed_at: committed_at,
            late_sequenced: false,
        },
        sessions,
        received_at: now,
    })
}

async fn seed_session_events(
    store: &AnyStore,
    commit: &DemoCommit,
    session: &DemoSession,
    owner: &AttributionOwner,
    now: OffsetDateTime,
) -> anyhow::Result<()> {
    let events = std::iter::once((AttributionSessionEventKind::Start, session.started)).chain(
        session
            .ended
            .map(|ended| (AttributionSessionEventKind::End, ended)),
    );
    for (event, age) in events {
        store
            .apply_attribution_session_event(&AttributionSessionEventWrite {
                owner: owner.clone(),
                harness_key: session.harness_key.to_string(),
                session_hash: session.session_hash.to_string(),
                session_hashes: vec![session.session_hash.to_string()],
                event,
                repository: Some(commit.repository.to_string()),
                branch: Some(commit.branch.to_string()),
                head_sha: None,
                harness_version: None,
                observed_at: now - age,
                received_at: now,
            })
            .await
            .with_context(|| format!("failed seeding session `{}`", session.session_hash))?;
    }
    Ok(())
}

fn pull_request_write(
    pull_request: &DemoPullRequest,
    owner: &AttributionOwner,
    user_id: &impl Fn(&str) -> anyhow::Result<Uuid>,
    committed: &HashMap<&str, OffsetDateTime>,
    now: OffsetDateTime,
) -> anyhow::Result<AttributionPullRequestWrite> {
    let commits = pull_request
        .commit_shas
        .iter()
        .map(|sha| {
            let commit = DEMO_COMMITS
                .iter()
                .find(|commit| commit.commit_sha == *sha)
                .ok_or_else(|| anyhow::anyhow!("pull request lists unknown demo commit `{sha}`"))?;
            let author_user_id = commit.author.map(user_id).transpose()?;
            Ok(AttributionPullRequestCommit {
                commit_sha: commit.commit_sha.to_string(),
                subject: Some(commit.subject.to_string()),
                authored_at: committed.get(commit.commit_sha).copied(),
                author_user_id,
                author_email_hash: author_user_id
                    .is_none()
                    .then(|| UNMATCHED_AUTHOR_HASH.to_string()),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(AttributionPullRequestWrite {
        attribution_pull_request_id: local_demo_uuid(
            "attribution_pull_request",
            &format!("{ADMIN_CONSOLE}#{}", pull_request.number),
        ),
        owner: owner.clone(),
        repository: ADMIN_CONSOLE.to_string(),
        number: pull_request.number,
        title: pull_request.title.to_string(),
        url: Some(format!(
            "https://{ADMIN_CONSOLE}/pull/{}",
            pull_request.number
        )),
        base_ref: Some("main".to_string()),
        head_ref: Some(pull_request.head_ref.to_string()),
        merge_commit_sha: Some(pull_request.merge_commit_sha.to_string()),
        merged_at: now - pull_request.merged,
        author_user_id: owner.owner_user_id,
        author_email_hash: None,
        commits,
        observed_at: now - pull_request.merged,
        received_at: now,
    })
}

#[cfg(test)]
mod tests {
    use gateway_core::{ATTRIBUTION_MAX_REPORT_ROWS, AttributionReportQuery};
    use gateway_service::attribution::{AttributionReport, build_attribution_report};
    use gateway_store::GatewayStore;

    use super::*;
    use crate::local_demo_seed::{seed_local_demo_data, tests::seeded_demo_store};

    async fn report(
        store: &AnyStore,
        user_id: Option<Uuid>,
        repository: Option<&str>,
    ) -> AttributionReport {
        let now = OffsetDateTime::now_utc();
        let query = AttributionReportQuery {
            from: now - Duration::days(7),
            to: now + Duration::days(1),
            user_id,
            repository: repository.map(str::to_string),
            limit: ATTRIBUTION_MAX_REPORT_ROWS,
        };
        build_attribution_report(store, &query)
            .await
            .expect("attribution report")
    }

    /// The facts `features/attribution.md` checks, after a reseed.
    #[tokio::test]
    async fn attribution_seed_matches_the_admin_recipe_after_a_reseed() {
        let (_directory, store) = seeded_demo_store().await;
        seed_local_demo_data(&store)
            .await
            .expect("reseed demo data");

        let everyone = report(&store, None, None).await;
        assert_eq!(everyone.totals.commit_count, 4);
        assert_eq!(everyone.totals.unmatched_author_commit_count, 1);
        assert_eq!(everyone.totals.pull_request_count, 2);
        assert_eq!(everyone.totals.requests.request_count, 17);
        assert!(everyone.totals.requests.priced_request_count > 0);
        assert_eq!(everyone.totals.uncommitted.request_count, 3);
        assert_eq!(everyone.totals.joined_session_count, 2);
        assert_eq!(everyone.totals.agent_session_count, 3);
        assert_eq!(everyone.users.len(), 2);
        let change = |sha: &str| {
            everyone
                .changes
                .iter()
                .find(|change| change.commit.commit_sha == sha)
                .expect("seeded change")
        };
        assert_eq!(change(GINA_ROUTE_COMMIT).pull_request_number, Some(42));
        assert_eq!(change(GINA_ROUTE_COMMIT).joined_session_count, 1);
        assert_eq!(change(ALICE_DASHBOARD_COMMIT).joined_session_count, 0);
        assert_eq!(change(ALICE_DASHBOARD_COMMIT).requests.request_count, 0);
        assert_eq!(change(GINA_RUNBOOK_COMMIT).pull_request_number, None);

        let runbooks = report(&store, None, Some(RELEASE_RUNBOOKS)).await;
        assert_eq!(runbooks.changes.len(), 1);
        assert_eq!(runbooks.changes[0].commit.commit_sha, GINA_RUNBOOK_COMMIT);
        assert!(runbooks.pull_requests.is_empty());

        let gina = store
            .get_user_by_email_normalized(GINA)
            .await
            .expect("load Gina")
            .expect("Gina");
        let gina_only = report(&store, Some(gina.user_id), None).await;
        assert_eq!(gina_only.totals.commit_count, 2);
    }
}
