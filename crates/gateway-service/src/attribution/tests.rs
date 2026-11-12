//! Attribution ingestion and report tests over an in-memory store that follows the
//! repository contract's ordering, filtering and freshness rules.

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use async_trait::async_trait;
use gateway_core::{
    ATTRIBUTION_MAX_HASHES_PER_LOOKUP, ATTRIBUTION_MAX_REQUEST_FACTS, ApiKeyModelGrantMode,
    ApiKeyOwnerKind, AttributionAgentSessionRecord, AttributionApplyOutcome,
    AttributionCommitRecord, AttributionCommitWrite, AttributionOwner,
    AttributionPullRequestRecord, AttributionPullRequestWrite, AttributionReportQuery,
    AttributionRepository, AttributionRequestFact, AttributionSessionCommitTime,
    AttributionSessionEventWrite, AuthenticatedApiKey, StoreError,
};
use time::OffsetDateTime;
use uuid::Uuid;

use super::AttributionUserLookup;

mod ingest;
mod report;

fn t0() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("valid timestamp")
}

fn user_key(user_id: Uuid) -> AuthenticatedApiKey {
    AuthenticatedApiKey {
        id: Uuid::new_v4(),
        public_id: "attr123".to_string(),
        name: "hooks".to_string(),
        model_grant_mode: ApiKeyModelGrantMode::Explicit,
        owner_kind: ApiKeyOwnerKind::User,
        owner_user_id: Some(user_id),
        owner_team_id: None,
        owner_service_account_id: None,
        access: Default::default(),
        oauth: None,
    }
}

fn service_account_key(service_account_id: Uuid) -> AuthenticatedApiKey {
    AuthenticatedApiKey {
        owner_kind: ApiKeyOwnerKind::ServiceAccount,
        owner_user_id: None,
        owner_team_id: Some(Uuid::new_v4()),
        owner_service_account_id: Some(service_account_id),
        ..user_key(Uuid::nil())
    }
}

fn user_owner(user_id: Uuid) -> AttributionOwner {
    AttributionOwner {
        ownership_scope_key: format!("user:{user_id}"),
        owner_user_id: Some(user_id),
        owner_service_account_id: None,
    }
}

#[derive(Default)]
struct FakeStore {
    commits: Mutex<Vec<AttributionCommitRecord>>,
    pull_requests: Mutex<Vec<AttributionPullRequestRecord>>,
    agent_sessions: Vec<AttributionAgentSessionRecord>,
    session_events: Mutex<Vec<AttributionSessionEventWrite>>,
    facts: Vec<AttributionRequestFact>,
    /// Hash count of every request-fact lookup.
    fact_lookups: Mutex<Vec<usize>>,
}

impl FakeStore {
    fn with_commits(commits: Vec<AttributionCommitRecord>) -> Self {
        Self {
            commits: Mutex::new(commits),
            ..Self::default()
        }
    }

    fn commits(&self) -> Vec<AttributionCommitRecord> {
        self.commits.lock().expect("commits lock").clone()
    }

    fn pull_requests(&self) -> Vec<AttributionPullRequestRecord> {
        self.pull_requests
            .lock()
            .expect("pull requests lock")
            .clone()
    }

    fn session_events(&self) -> Vec<AttributionSessionEventWrite> {
        self.session_events.lock().expect("events lock").clone()
    }

    /// Every stored row as JSON, for privacy assertions.
    fn stored_json(&self) -> String {
        serde_json::to_string(&(self.commits(), self.pull_requests(), self.session_events()))
            .expect("rows serialize")
    }
}

fn in_range(query: &AttributionReportQuery, at: OffsetDateTime) -> bool {
    query.from <= at && at <= query.to
}

fn visible_to(
    query: &AttributionReportQuery,
    owner: &AttributionOwner,
    author: Option<Uuid>,
) -> bool {
    query
        .user_id
        .is_none_or(|user_id| owner.owner_user_id == Some(user_id) || author == Some(user_id))
}

fn commit_record(
    write: &AttributionCommitWrite,
    id: Uuid,
    first: OffsetDateTime,
) -> AttributionCommitRecord {
    AttributionCommitRecord {
        attribution_commit_id: id,
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
        first_observed_at: first,
        sessions: write.sessions.clone(),
    }
}

fn pull_request_record(
    write: &AttributionPullRequestWrite,
    id: Uuid,
) -> AttributionPullRequestRecord {
    AttributionPullRequestRecord {
        attribution_pull_request_id: id,
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
        commits: write.commits.clone(),
        observed_at: write.observed_at,
    }
}

#[async_trait]
impl AttributionRepository for FakeStore {
    async fn apply_attribution_commit(
        &self,
        write: &AttributionCommitWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        let mut commits = self.commits.lock().expect("commits lock");
        let stored = commits.iter_mut().find(|commit| {
            commit.owner.ownership_scope_key == write.owner.ownership_scope_key
                && commit.repository == write.repository
                && commit.commit_sha == write.commit_sha
        });
        let Some(stored) = stored else {
            commits.push(commit_record(
                write,
                write.attribution_commit_id,
                write.freshness.observed_at,
            ));
            return Ok(AttributionApplyOutcome::Applied);
        };
        let outcome = write.freshness.decide_against(&stored.freshness);
        if outcome == AttributionApplyOutcome::Applied {
            *stored = commit_record(
                write,
                stored.attribution_commit_id,
                stored.first_observed_at,
            );
        }
        Ok(outcome)
    }

    async fn apply_attribution_session_event(
        &self,
        write: &AttributionSessionEventWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        let mut events = self.session_events.lock().expect("events lock");
        let replay = events.iter().any(|event| {
            event.session_hash == write.session_hash
                && event.event == write.event
                && event.observed_at == write.observed_at
        });
        if replay {
            return Ok(AttributionApplyOutcome::Duplicate);
        }
        events.push(write.clone());
        Ok(AttributionApplyOutcome::Applied)
    }

    async fn apply_attribution_pull_request(
        &self,
        write: &AttributionPullRequestWrite,
    ) -> Result<AttributionApplyOutcome, StoreError> {
        let mut pull_requests = self.pull_requests.lock().expect("pull requests lock");
        let stored = pull_requests.iter_mut().find(|stored| {
            stored.owner.ownership_scope_key == write.owner.ownership_scope_key
                && stored.repository == write.repository
                && stored.number == write.number
        });
        let Some(stored) = stored else {
            pull_requests.push(pull_request_record(
                write,
                write.attribution_pull_request_id,
            ));
            return Ok(AttributionApplyOutcome::Applied);
        };
        Ok(match write.observed_at.cmp(&stored.observed_at) {
            Ordering::Greater => {
                *stored = pull_request_record(write, stored.attribution_pull_request_id);
                AttributionApplyOutcome::Applied
            }
            Ordering::Equal => AttributionApplyOutcome::Duplicate,
            Ordering::Less => AttributionApplyOutcome::Stale,
        })
    }

    async fn list_attribution_commits(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionCommitRecord>, StoreError> {
        let mut rows = self
            .commits()
            .into_iter()
            .filter(|commit| {
                in_range(query, commit.first_observed_at)
                    && visible_to(query, &commit.owner, commit.author_user_id)
                    && query
                        .repository
                        .as_ref()
                        .is_none_or(|repository| *repository == commit.repository)
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            (right.first_observed_at, right.attribution_commit_id)
                .cmp(&(left.first_observed_at, left.attribution_commit_id))
        });
        rows.truncate(query.limit as usize);
        Ok(rows)
    }

    async fn list_attribution_pull_requests(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionPullRequestRecord>, StoreError> {
        let mut rows = self
            .pull_requests()
            .into_iter()
            .filter(|pull_request| {
                in_range(query, pull_request.merged_at)
                    && visible_to(query, &pull_request.owner, pull_request.author_user_id)
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| std::cmp::Reverse(row.merged_at));
        rows.truncate(query.limit as usize);
        Ok(rows)
    }

    async fn list_attribution_agent_sessions(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionAgentSessionRecord>, StoreError> {
        Ok(self
            .agent_sessions
            .iter()
            .filter(|session| {
                [session.started_at, session.ended_at]
                    .into_iter()
                    .flatten()
                    .any(|at| in_range(query, at))
                    && visible_to(query, &session.owner, None)
            })
            .cloned()
            .collect())
    }

    async fn list_attribution_session_commit_times(
        &self,
        session_hashes: &[String],
        until: OffsetDateTime,
    ) -> Result<Vec<AttributionSessionCommitTime>, StoreError> {
        assert!(session_hashes.len() <= ATTRIBUTION_MAX_HASHES_PER_LOOKUP);
        let mut times = Vec::new();
        for commit in self
            .commits()
            .iter()
            .filter(|commit| commit.first_observed_at <= until)
        {
            // One row per commit and matched hash, however many of its sessions carry it.
            let matched = commit
                .sessions
                .iter()
                .flat_map(|session| &session.session_hashes)
                .filter(|hash| session_hashes.contains(hash))
                .collect::<BTreeSet<_>>();
            times.extend(
                matched
                    .into_iter()
                    .map(|hash| AttributionSessionCommitTime {
                        ownership_scope_key: commit.owner.ownership_scope_key.clone(),
                        session_hash: hash.clone(),
                        attribution_commit_id: commit.attribution_commit_id,
                        committed_at: commit.first_observed_at,
                    }),
            );
        }
        Ok(times)
    }

    async fn list_attribution_request_facts(
        &self,
        session_hashes: &[String],
        until: OffsetDateTime,
    ) -> Result<Vec<AttributionRequestFact>, StoreError> {
        assert!(session_hashes.len() <= ATTRIBUTION_MAX_HASHES_PER_LOOKUP);
        self.fact_lookups
            .lock()
            .expect("lookups lock")
            .push(session_hashes.len());
        let mut facts = self
            .facts
            .iter()
            .filter(|fact| session_hashes.contains(&fact.session_hash) && fact.occurred_at <= until)
            .cloned()
            .collect::<Vec<_>>();
        facts.sort_by(|left, right| {
            (left.occurred_at, &left.request_id).cmp(&(right.occurred_at, &right.request_id))
        });
        facts.truncate(ATTRIBUTION_MAX_REQUEST_FACTS as usize);
        Ok(facts)
    }

    async fn delete_attribution_for_owner(
        &self,
        ownership_scope_key: &str,
    ) -> Result<u64, StoreError> {
        let mut commits = self.commits.lock().expect("commits lock");
        let before = commits.len();
        commits.retain(|commit| commit.owner.ownership_scope_key != ownership_scope_key);
        Ok((before - commits.len()) as u64)
    }
}

#[derive(Default)]
struct FakeUsers {
    users: HashMap<String, Uuid>,
    lookups: Mutex<Vec<String>>,
}

impl FakeUsers {
    fn with(users: &[(&str, Uuid)]) -> Self {
        Self {
            users: users
                .iter()
                .map(|(email, user_id)| ((*email).to_string(), *user_id))
                .collect(),
            lookups: Mutex::default(),
        }
    }

    fn lookup_count(&self) -> usize {
        self.lookups.lock().expect("lookups lock").len()
    }
}

#[async_trait]
impl AttributionUserLookup for FakeUsers {
    async fn user_id_by_normalized_email(
        &self,
        email_normalized: &str,
    ) -> Result<Option<Uuid>, StoreError> {
        self.lookups
            .lock()
            .expect("lookups lock")
            .push(email_normalized.to_string());
        Ok(self.users.get(email_normalized).copied())
    }
}
