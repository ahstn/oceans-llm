//! The attribution report: commits, merged PRs and harness sessions in a window, joined to
//! gateway requests through session hashes.
//!
//! A commit gets each of its sessions' requests that occurred after that session's previous
//! commit (in any repository of the same ownership scope) and at or before this commit.
//! Requests after a session's last commit are uncommitted. A commit's time is its
//! `first_observed_at`, the client's observation time of its first stored note, while request
//! times come from the gateway clock, so client clock skew shifts the windows.
//!
//! A commit session carries its id's hash under every session namespace, and every commit of
//! that session carries the same hashes, so each hash sees the session's full commit list.
//! Requests are pooled across a session's hashes by request id, so a session whose requests
//! were recorded under two namespaces counts each request once.
//! A hook-reported session carries the same hashes, so a session with no commit yet has its
//! requests counted as uncommitted whichever namespace the gateway recorded them under.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use gateway_core::{
    ATTRIBUTION_MAX_HASHES_PER_LOOKUP, ATTRIBUTION_MAX_REQUEST_FACTS,
    AttributionAgentSessionRecord, AttributionCommitRecord, AttributionCommitSession,
    AttributionOwner, AttributionPullRequestRecord, AttributionReportQuery, AttributionRepository,
    AttributionRequestFact, AttributionSessionCommitTime, StoreError,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

/// How far before the window commits are loaded, so a merged PR finds the notes of commits
/// made before the window opened.
const PULL_REQUEST_COMMIT_LOOKBACK: Duration = Duration::days(30);
/// Request facts one report holds; past this the coverage reports truncation.
const MAX_REPORT_REQUEST_FACTS: usize = 200_000;
/// Label for requests whose log has no model key.
const UNKNOWN_MODEL: &str = "unknown";

/// Request counts and priced cost, split by pricing outcome as in the spend report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestTally {
    pub request_count: i64,
    pub priced_request_count: i64,
    pub unpriced_request_count: i64,
    pub usage_missing_request_count: i64,
    /// Cost of the priced requests in 1/10000 USD.
    pub priced_cost_10000: i64,
}

impl RequestTally {
    fn add(&mut self, fact: &AttributionRequestFact) {
        self.request_count += 1;
        match (fact.cost_10000, fact.pricing_status.as_deref()) {
            (Some(cost), _) => {
                self.priced_request_count += 1;
                self.priced_cost_10000 += cost;
            }
            (None, Some("unpriced")) => self.unpriced_request_count += 1,
            (None, _) => self.usage_missing_request_count += 1,
        }
    }

    fn of<'a>(facts: impl IntoIterator<Item = &'a AttributionRequestFact>) -> Self {
        let mut tally = Self::default();
        facts.into_iter().for_each(|fact| tally.add(fact));
        tally
    }

    fn merged(self, other: Self) -> Self {
        Self {
            request_count: self.request_count + other.request_count,
            priced_request_count: self.priced_request_count + other.priced_request_count,
            unpriced_request_count: self.unpriced_request_count + other.unpriced_request_count,
            usage_missing_request_count: self.usage_missing_request_count
                + other.usage_missing_request_count,
            priced_cost_10000: self.priced_cost_10000 + other.priced_cost_10000,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributionReportTotals {
    pub commit_count: i64,
    pub pull_request_count: i64,
    pub ai_lines: i64,
    pub human_lines: i64,
    /// Distinct AI sessions named by the window's commit notes.
    pub ai_session_count: i64,
    /// Those sessions that matched gateway requests.
    pub joined_session_count: i64,
    /// Harness sessions reported by hook events in the window.
    pub agent_session_count: i64,
    /// Requests allocated to the window's commits.
    pub requests: RequestTally,
    /// Requests in the window after their session's last commit.
    pub uncommitted: RequestTally,
    /// Window commits whose author is not an Oceans user.
    pub unmatched_author_commit_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributionReportCoverage {
    /// Every request the report counts, allocated or uncommitted.
    pub requests: RequestTally,
    /// The row cap cut commits inside the window.
    pub commits_truncated: bool,
    pub pull_requests_truncated: bool,
    /// A session had more requests than one lookup returns, or the report's fact cap was hit.
    pub request_facts_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionUserActivity {
    pub user_id: Uuid,
    pub commit_count: i64,
    pub merged_pull_request_count: i64,
    pub ai_lines: i64,
    pub human_lines: i64,
    pub ai_session_count: i64,
    pub agent_session_count: i64,
    pub requests: RequestTally,
    /// Gateway model keys, most requests first.
    pub models: Vec<String>,
    pub last_activity_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionModelActivity {
    pub model_key: String,
    pub requests: RequestTally,
    /// Window commits with at least one request on this model.
    pub commit_count: i64,
    pub uncommitted: RequestTally,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionPullRequestActivity {
    pub pull_request: AttributionPullRequestRecord,
    /// Listed commits with a stored note; the merge commit when none of them has one.
    pub attributed_commit_count: i64,
    pub ai_lines: i64,
    pub human_lines: i64,
    pub requests: RequestTally,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionChangeActivity {
    pub commit: AttributionCommitRecord,
    /// The most recently merged window PR listing this commit.
    pub pull_request_number: Option<i64>,
    pub joined_session_count: i64,
    pub requests: RequestTally,
    pub models: Vec<String>,
    /// Distinct git-ai agent models named by the note.
    pub note_models: Vec<String>,
    /// Distinct git-ai agent tools named by the note.
    pub agent_tools: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributionReport {
    pub totals: AttributionReportTotals,
    pub coverage: AttributionReportCoverage,
    /// Most cost first, then most AI lines.
    pub users: Vec<AttributionUserActivity>,
    /// Most requests first.
    pub models: Vec<AttributionModelActivity>,
    /// Most recently merged first.
    pub pull_requests: Vec<AttributionPullRequestActivity>,
    /// Newest first.
    pub changes: Vec<AttributionChangeActivity>,
}

/// Builds the report for `query`. `query.limit` caps commits and PRs separately.
pub async fn build_attribution_report<S>(
    store: &S,
    query: &AttributionReportQuery,
) -> Result<AttributionReport, StoreError>
where
    S: AttributionRepository + Sync,
{
    let commit_query = AttributionReportQuery {
        from: query.from - PULL_REQUEST_COMMIT_LOOKBACK,
        ..query.clone()
    };
    let (commits, pull_requests, agent_sessions) = tokio::try_join!(
        store.list_attribution_commits(&commit_query),
        store.list_attribution_pull_requests(query),
        store.list_attribution_agent_sessions(query),
    )?;
    let hashes = session_hashes(&commits, &agent_sessions);
    let windows = commit_windows(load_commit_times(store, &hashes, query.to).await?);
    let (facts, request_facts_truncated) = load_request_facts(store, &hashes, query.to).await?;
    let inputs = ReportInputs::new(query, &commits, &facts, &windows);
    let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
    let coverage = AttributionReportCoverage {
        requests: RequestTally::default(),
        commits_truncated: commits.len() >= limit
            && commits
                .last()
                .is_some_and(|commit| commit.first_observed_at >= query.from),
        pull_requests_truncated: pull_requests.len() >= limit,
        request_facts_truncated,
    };
    Ok(inputs.report(pull_requests, &agent_sessions, coverage))
}

fn session_hashes(
    commits: &[AttributionCommitRecord],
    agent_sessions: &[AttributionAgentSessionRecord],
) -> Vec<String> {
    let mut hashes = BTreeSet::new();
    for session in commits.iter().flat_map(|commit| &commit.sessions) {
        hashes.extend(session.session_hashes.iter().map(String::as_str));
    }
    hashes.extend(
        agent_sessions
            .iter()
            .flat_map(|session| &session.session_hashes)
            .map(String::as_str),
    );
    hashes.into_iter().map(str::to_string).collect()
}

async fn load_commit_times<S>(
    store: &S,
    hashes: &[String],
    until: OffsetDateTime,
) -> Result<Vec<AttributionSessionCommitTime>, StoreError>
where
    S: AttributionRepository + Sync,
{
    let mut times = Vec::new();
    for chunk in hashes.chunks(ATTRIBUTION_MAX_HASHES_PER_LOOKUP) {
        times.extend(
            store
                .list_attribution_session_commit_times(chunk, until)
                .await?,
        );
    }
    Ok(times)
}

/// Loads request facts for every hash. A lookup that reaches the per-call cap is split in half
/// and retried; a single hash at the cap, or the report-wide cap, marks the result truncated.
async fn load_request_facts<S>(
    store: &S,
    hashes: &[String],
    until: OffsetDateTime,
) -> Result<(Vec<AttributionRequestFact>, bool), StoreError>
where
    S: AttributionRepository + Sync,
{
    let per_call_cap = usize::try_from(ATTRIBUTION_MAX_REQUEST_FACTS).unwrap_or(usize::MAX);
    let mut facts = Vec::new();
    let mut truncated = false;
    let mut pending = hashes
        .chunks(ATTRIBUTION_MAX_HASHES_PER_LOOKUP)
        .rev()
        .collect::<Vec<_>>();
    while let Some(chunk) = pending.pop() {
        if facts.len() >= MAX_REPORT_REQUEST_FACTS {
            truncated = true;
            break;
        }
        let found = store.list_attribution_request_facts(chunk, until).await?;
        if found.len() >= per_call_cap && chunk.len() > 1 {
            let (first, second) = chunk.split_at(chunk.len() / 2);
            pending.extend([second, first]);
            continue;
        }
        truncated |= found.len() >= per_call_cap;
        facts.extend(found);
    }
    Ok((facts, truncated))
}

/// Commit times per scope and hash, ascending; equal times order by commit id.
type CommitWindows = HashMap<String, HashMap<String, Vec<(OffsetDateTime, Uuid)>>>;

fn commit_windows(times: Vec<AttributionSessionCommitTime>) -> CommitWindows {
    let mut windows = CommitWindows::new();
    for time in times {
        windows
            .entry(time.ownership_scope_key)
            .or_default()
            .entry(time.session_hash)
            .or_default()
            .push((time.committed_at, time.attribution_commit_id));
    }
    for times in windows.values_mut().flat_map(HashMap::values_mut) {
        times.sort_unstable();
        times.dedup();
    }
    windows
}

/// The commit whose window holds a request: the first commit of the session at or after it.
fn holding_commit(
    windows: &CommitWindows,
    fact: &AttributionRequestFact,
) -> Option<(OffsetDateTime, Uuid)> {
    let times = windows
        .get(&fact.ownership_scope_key)?
        .get(&fact.session_hash)?;
    let index = times.partition_point(|(committed_at, _)| *committed_at < fact.occurred_at);
    times.get(index).copied()
}

/// Loaded rows and the request allocation, borrowed for one report.
struct ReportInputs<'a> {
    query: &'a AttributionReportQuery,
    /// Every loaded commit, including the PR lookback before the window.
    commits: &'a [AttributionCommitRecord],
    by_commit: HashMap<Uuid, Vec<&'a AttributionRequestFact>>,
    uncommitted: Vec<&'a AttributionRequestFact>,
    /// `(scope, hash)` pairs that matched at least one request.
    joined: HashSet<(&'a str, &'a str)>,
}

impl<'a> ReportInputs<'a> {
    /// Allocates each distinct request (per scope and request id) once. A request reached
    /// through two hashes, such as two namespaces of one session, keeps the earliest commit
    /// that holds it.
    fn new(
        query: &'a AttributionReportQuery,
        commits: &'a [AttributionCommitRecord],
        facts: &'a [AttributionRequestFact],
        windows: &CommitWindows,
    ) -> Self {
        let mut joined = HashSet::new();
        let mut requests = HashMap::new();
        for fact in facts {
            let scope = fact.ownership_scope_key.as_str();
            joined.insert((scope, fact.session_hash.as_str()));
            let commit = holding_commit(windows, fact);
            requests
                .entry((scope, fact.request_id.as_str()))
                .and_modify(|held: &mut (Option<(OffsetDateTime, Uuid)>, _)| {
                    if earlier(commit, held.0) {
                        *held = (commit, fact);
                    }
                })
                .or_insert((commit, fact));
        }
        let mut by_commit: HashMap<Uuid, Vec<_>> = HashMap::new();
        let mut uncommitted = Vec::new();
        for (commit, fact) in requests.into_values() {
            match commit {
                Some((_, commit_id)) => by_commit.entry(commit_id).or_default().push(fact),
                None if (query.from..=query.to).contains(&fact.occurred_at) => {
                    uncommitted.push(fact);
                }
                None => {}
            }
        }
        Self {
            query,
            commits,
            by_commit,
            uncommitted,
            joined,
        }
    }

    fn window_commits(&self) -> impl Iterator<Item = &'a AttributionCommitRecord> + '_ {
        self.commits
            .iter()
            .filter(|commit| commit.first_observed_at >= self.query.from)
    }

    fn facts_for(&self, commit_id: Uuid) -> &[&'a AttributionRequestFact] {
        self.by_commit.get(&commit_id).map_or(&[], Vec::as_slice)
    }

    fn is_joined(&self, scope: &'a str, session: &'a AttributionCommitSession) -> bool {
        session
            .session_hashes
            .iter()
            .any(|hash| self.joined.contains(&(scope, hash.as_str())))
    }

    fn report(
        &self,
        pull_requests: Vec<AttributionPullRequestRecord>,
        agent_sessions: &[AttributionAgentSessionRecord],
        coverage: AttributionReportCoverage,
    ) -> AttributionReport {
        let totals = self.totals(pull_requests.len(), agent_sessions.len());
        let coverage = AttributionReportCoverage {
            requests: totals.requests.merged(totals.uncommitted),
            ..coverage
        };
        let changes = self.changes(&pull_request_numbers(&pull_requests));
        let users = self.users(&pull_requests, agent_sessions);
        AttributionReport {
            totals,
            coverage,
            users,
            models: self.models(),
            pull_requests: self.pull_requests(pull_requests),
            changes,
        }
    }

    fn totals(
        &self,
        pull_request_count: usize,
        agent_session_count: usize,
    ) -> AttributionReportTotals {
        let mut totals = AttributionReportTotals {
            pull_request_count: count(pull_request_count),
            agent_session_count: count(agent_session_count),
            uncommitted: RequestTally::of(self.uncommitted.iter().copied()),
            ..AttributionReportTotals::default()
        };
        let (mut sessions, mut joined) = (HashSet::new(), HashSet::new());
        for commit in self.window_commits() {
            totals.commit_count += 1;
            totals.ai_lines += commit.ai_lines;
            totals.human_lines += commit.human_lines;
            totals.unmatched_author_commit_count += i64::from(commit.author_user_id.is_none());
            let scope = commit.owner.ownership_scope_key.as_str();
            for session in &commit.sessions {
                let key = (scope, session.note_session_key.as_str());
                sessions.insert(key);
                if self.is_joined(scope, session) {
                    joined.insert(key);
                }
            }
            for fact in self.facts_for(commit.attribution_commit_id) {
                totals.requests.add(fact);
            }
        }
        totals.ai_session_count = count(sessions.len());
        totals.joined_session_count = count(joined.len());
        totals
    }

    fn changes(
        &self,
        pull_request_numbers: &HashMap<(&str, &str), i64>,
    ) -> Vec<AttributionChangeActivity> {
        self.window_commits()
            .map(|commit| {
                let facts = self.facts_for(commit.attribution_commit_id);
                let scope = commit.owner.ownership_scope_key.as_str();
                let joined = commit
                    .sessions
                    .iter()
                    .filter(|session| self.is_joined(scope, session))
                    .count();
                AttributionChangeActivity {
                    pull_request_number: pull_request_numbers
                        .get(&(commit.repository.as_str(), commit.commit_sha.as_str()))
                        .copied(),
                    joined_session_count: count(joined),
                    requests: RequestTally::of(facts.iter().copied()),
                    models: ranked_models(facts),
                    note_models: distinct(
                        commit
                            .sessions
                            .iter()
                            .filter_map(|session| session.agent_model.as_deref()),
                    ),
                    agent_tools: distinct(
                        commit
                            .sessions
                            .iter()
                            .map(|session| session.agent_tool.as_str()),
                    ),
                    commit: commit.clone(),
                }
            })
            .collect()
    }

    /// One row per Oceans user: commits and PRs by their matched author, hook-reported sessions
    /// by owner. See [`credited_user`] for commits and PRs without a matched author.
    fn users(
        &self,
        pull_requests: &[AttributionPullRequestRecord],
        agent_sessions: &[AttributionAgentSessionRecord],
    ) -> Vec<AttributionUserActivity> {
        let mut users: HashMap<Uuid, UserRollup<'a>> = HashMap::new();
        for commit in self.window_commits() {
            let Some(user_id) = credited_user(
                commit.author_user_id,
                commit.author_email_hash.as_deref(),
                &commit.owner,
            ) else {
                continue;
            };
            users
                .entry(user_id)
                .or_default()
                .add_commit(commit, self.facts_for(commit.attribution_commit_id));
        }
        for pull_request in pull_requests {
            if let Some(user_id) = credited_user(
                pull_request.author_user_id,
                pull_request.author_email_hash.as_deref(),
                &pull_request.owner,
            ) {
                let rollup = users.entry(user_id).or_default();
                rollup.merged_pull_request_count += 1;
                rollup.touch(pull_request.merged_at);
            }
        }
        for session in agent_sessions {
            if let Some(user_id) = session.owner.owner_user_id {
                let rollup = users.entry(user_id).or_default();
                rollup.agent_session_count += 1;
                rollup.touch(session.last_observed_at);
            }
        }
        let mut rows = users
            .into_iter()
            .map(|(user_id, rollup)| rollup.into_activity(user_id))
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            right
                .requests
                .priced_cost_10000
                .cmp(&left.requests.priced_cost_10000)
                .then(right.ai_lines.cmp(&left.ai_lines))
                .then(left.user_id.cmp(&right.user_id))
        });
        rows
    }

    fn models(&self) -> Vec<AttributionModelActivity> {
        let mut models: BTreeMap<&str, (RequestTally, HashSet<Uuid>, RequestTally)> =
            BTreeMap::new();
        for commit in self.window_commits() {
            for fact in self.facts_for(commit.attribution_commit_id) {
                let (requests, commits, _) = models.entry(model_label(fact)).or_default();
                requests.add(fact);
                commits.insert(commit.attribution_commit_id);
            }
        }
        for fact in &self.uncommitted {
            models.entry(model_label(fact)).or_default().2.add(fact);
        }
        let mut rows = models
            .into_iter()
            .map(
                |(model_key, (requests, commits, uncommitted))| AttributionModelActivity {
                    model_key: model_key.to_string(),
                    requests,
                    commit_count: count(commits.len()),
                    uncommitted,
                },
            )
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            right
                .requests
                .request_count
                .cmp(&left.requests.request_count)
                .then(
                    right
                        .uncommitted
                        .request_count
                        .cmp(&left.uncommitted.request_count),
                )
                .then_with(|| left.model_key.cmp(&right.model_key))
        });
        rows
    }

    /// Matches each PR's commits to stored notes by repository and SHA, preferring the PR
    /// owner's note when several scopes stored one.
    fn pull_requests(
        &self,
        pull_requests: Vec<AttributionPullRequestRecord>,
    ) -> Vec<AttributionPullRequestActivity> {
        let mut notes: HashMap<(&str, &str), Vec<&AttributionCommitRecord>> = HashMap::new();
        for commit in self.commits {
            notes
                .entry((commit.repository.as_str(), commit.commit_sha.as_str()))
                .or_default()
                .push(commit);
        }
        pull_requests
            .into_iter()
            .map(|pull_request| {
                let matched = matched_commits(&notes, &pull_request);
                let facts = matched
                    .iter()
                    .flat_map(|commit| self.facts_for(commit.attribution_commit_id))
                    .copied()
                    .collect::<Vec<_>>();
                AttributionPullRequestActivity {
                    attributed_commit_count: count(matched.len()),
                    ai_lines: matched.iter().map(|commit| commit.ai_lines).sum(),
                    human_lines: matched.iter().map(|commit| commit.human_lines).sum(),
                    requests: RequestTally::of(facts.iter().copied()),
                    models: ranked_models(&facts),
                    pull_request,
                }
            })
            .collect()
    }
}

#[derive(Default)]
struct UserRollup<'a> {
    commit_count: i64,
    merged_pull_request_count: i64,
    ai_lines: i64,
    human_lines: i64,
    agent_session_count: i64,
    sessions: HashSet<(&'a str, &'a str)>,
    facts: Vec<&'a AttributionRequestFact>,
    last_activity_at: Option<OffsetDateTime>,
}

impl<'a> UserRollup<'a> {
    fn add_commit(
        &mut self,
        commit: &'a AttributionCommitRecord,
        facts: &[&'a AttributionRequestFact],
    ) {
        self.commit_count += 1;
        self.ai_lines += commit.ai_lines;
        self.human_lines += commit.human_lines;
        let scope = commit.owner.ownership_scope_key.as_str();
        self.sessions.extend(
            commit
                .sessions
                .iter()
                .map(|session| (scope, session.note_session_key.as_str())),
        );
        self.facts.extend_from_slice(facts);
        self.touch(commit.first_observed_at);
    }

    fn touch(&mut self, at: OffsetDateTime) {
        self.last_activity_at = self.last_activity_at.max(Some(at));
    }

    fn into_activity(self, user_id: Uuid) -> AttributionUserActivity {
        AttributionUserActivity {
            user_id,
            commit_count: self.commit_count,
            merged_pull_request_count: self.merged_pull_request_count,
            ai_lines: self.ai_lines,
            human_lines: self.human_lines,
            ai_session_count: count(self.sessions.len()),
            agent_session_count: self.agent_session_count,
            requests: RequestTally::of(self.facts.iter().copied()),
            models: ranked_models(&self.facts),
            last_activity_at: self.last_activity_at,
        }
    }
}

fn matched_commits<'a>(
    notes: &HashMap<(&str, &str), Vec<&'a AttributionCommitRecord>>,
    pull_request: &AttributionPullRequestRecord,
) -> Vec<&'a AttributionCommitRecord> {
    let scope = pull_request.owner.ownership_scope_key.as_str();
    let find = |sha: &str| {
        let found = notes.get(&(pull_request.repository.as_str(), sha))?;
        found
            .iter()
            .find(|commit| commit.owner.ownership_scope_key == scope)
            .or(found.first())
            .copied()
    };
    let mut matched = pull_request
        .commits
        .iter()
        .filter_map(|commit| find(&commit.commit_sha))
        .collect::<Vec<_>>();
    if matched.is_empty() {
        matched.extend(pull_request.merge_commit_sha.as_deref().and_then(find));
    }
    matched.sort_by_key(|commit| commit.attribution_commit_id);
    matched.dedup_by_key(|commit| commit.attribution_commit_id);
    matched
}

/// The Oceans user a commit or PR counts for: its matched author, else the key's owner when it
/// names no author email at all. An email that matched no user belongs to an unnamed
/// contributor, counted in the totals but credited to nobody.
fn credited_user(
    author_user_id: Option<Uuid>,
    author_email_hash: Option<&str>,
    owner: &AttributionOwner,
) -> Option<Uuid> {
    match (author_user_id, author_email_hash) {
        (Some(user_id), _) => Some(user_id),
        (None, None) => owner.owner_user_id,
        (None, Some(_)) => None,
    }
}

/// `(repository, sha)` → PR number for every window PR's commits and merge commit; the most
/// recently merged PR wins.
fn pull_request_numbers(
    pull_requests: &[AttributionPullRequestRecord],
) -> HashMap<(&str, &str), i64> {
    let mut numbers = HashMap::new();
    for pull_request in pull_requests {
        let shas = pull_request
            .commits
            .iter()
            .map(|commit| commit.commit_sha.as_str())
            .chain(pull_request.merge_commit_sha.as_deref());
        for sha in shas {
            numbers
                .entry((pull_request.repository.as_str(), sha))
                .or_insert(pull_request.number);
        }
    }
    numbers
}

fn earlier(
    candidate: Option<(OffsetDateTime, Uuid)>,
    held: Option<(OffsetDateTime, Uuid)>,
) -> bool {
    match (candidate, held) {
        (Some(candidate), Some(held)) => candidate < held,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn model_label(fact: &AttributionRequestFact) -> &str {
    fact.model_key.as_deref().unwrap_or(UNKNOWN_MODEL)
}

/// Model keys by request count, most first; ties by name.
fn ranked_models(facts: &[&AttributionRequestFact]) -> Vec<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for fact in facts {
        *counts.entry(model_label(fact)).or_default() += 1;
    }
    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(right.0)));
    ranked
        .into_iter()
        .map(|(model, _)| model.to_string())
        .collect()
}

fn distinct<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    values
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn count(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}
