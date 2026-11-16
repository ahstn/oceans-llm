//! Aggregations behind the Attribution tabs beyond the tables: daily buckets, where each agent
//! session's work ended up, harness × model efficiency, and a period summary for deltas.
//!
//! A session here is one agent session however it reached Oceans: a hook-reported session, a
//! session named by a commit's note (a window commit, or a lookback commit when the session still
//! spent in the window), or both, joined when they share a session hash. Its requests are the
//! window's requests under any of its hashes, each request counted once. Only window commits set
//! its lines and outcome.

use std::collections::{BTreeMap, HashMap, HashSet, hash_map::Entry};

use gateway_core::{AttributionCommitRecord, AttributionRequestFact};
use time::{Date, Duration, OffsetDateTime};

use super::{
    AttributionAgentSessionRecord, AttributionPullRequestActivity, AttributionReport, ReportInputs,
    RequestTally, count,
};

/// One UTC day of the window. Committed requests count on their commit's day; uncommitted ones
/// on the day they happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionDayActivity {
    pub day: Date,
    pub commit_count: i64,
    pub ai_lines: i64,
    pub human_lines: i64,
    pub merged_pull_request_count: i64,
    pub committed: RequestTally,
    pub uncommitted: RequestTally,
}

/// The furthest stage an agent session's work reached in the window. Only window commits count: a
/// commit before the window belongs to the previous window's outcomes, so a session that spent in
/// the window without committing in it is `NoCommit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttributionSessionOutcome {
    /// A commit is in a pull request merged in the window.
    Merged,
    /// A commit landed on the default branch with no merged pull request.
    DefaultBranch,
    /// Committed, but no commit reached the default branch or a merged pull request yet.
    Unmerged,
    /// No window commit names the session.
    NoCommit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionSessionOutcomeTally {
    pub outcome: AttributionSessionOutcome,
    pub session_count: i64,
    pub requests: RequestTally,
}

/// Sessions of one harness on one model: cost against committed AI lines and how many shipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionHarnessModelActivity {
    /// The note's `agent_id.tool` (`claude`, `codex`, `pi`), else the hook's harness key.
    pub harness: String,
    /// The gateway model with most of the session's requests, else the note's model.
    pub model_key: String,
    pub session_count: i64,
    /// Sessions whose work was merged or reached the default branch.
    pub shipped_session_count: i64,
    /// AI lines the sessions wrote into window commits.
    pub ai_lines: i64,
    pub requests: RequestTally,
    /// Sessions with requests, every one of them priced. Cost per line uses only these, so a
    /// session the gateway could not join or price never reads as free output.
    pub priced_session_count: i64,
    /// AI lines of the fully priced sessions.
    pub priced_ai_lines: i64,
    /// Priced cost of the fully priced sessions, in 1/10000 USD.
    pub priced_cost_10000: i64,
}

/// Headline figures for a window, so the UI can compare a window with the one before it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributionPeriodSummary {
    pub commit_count: i64,
    pub merged_pull_request_count: i64,
    pub ai_lines: i64,
    pub human_lines: i64,
    /// AI and human lines of the commits in merged pull requests.
    pub merged_ai_lines: i64,
    pub merged_human_lines: i64,
    /// Priced cost of every request the report counts, committed or not.
    pub priced_cost_10000: i64,
    /// Priced cost of sessions whose work was merged or reached the default branch.
    pub shipped_cost_10000: i64,
    /// Median priced cost of a merged pull request that has priced requests.
    pub median_pull_request_cost_10000: Option<i64>,
    /// Median first commit to merge.
    pub median_lead_time_seconds: Option<i64>,
    /// Merged pull requests in the window that revert another, by confirmed evidence.
    pub reverted_pull_request_count: i64,
}

#[derive(Default)]
struct SessionRollup<'a> {
    /// The hook reported the session starting or ending in the window.
    reported: bool,
    harness: Option<&'a str>,
    note_model: Option<&'a str>,
    commits: Vec<&'a AttributionCommitRecord>,
    ai_lines: i64,
    facts: Vec<&'a AttributionRequestFact>,
}

impl ReportInputs<'_> {
    pub(super) fn daily(
        &self,
        pull_requests: &[AttributionPullRequestActivity],
    ) -> Vec<AttributionDayActivity> {
        let first = self.query.from.date();
        // The window ends at an exclusive midnight in the HTTP report, so the last day is the one
        // before `to`, not the date `to` falls on.
        let last = (self.query.to - Duration::nanoseconds(1)).date().max(first);
        let mut days: BTreeMap<Date, AttributionDayActivity> = BTreeMap::new();
        let mut day = first;
        while day <= last {
            days.insert(
                day,
                AttributionDayActivity {
                    day,
                    commit_count: 0,
                    ai_lines: 0,
                    human_lines: 0,
                    merged_pull_request_count: 0,
                    committed: RequestTally::default(),
                    uncommitted: RequestTally::default(),
                },
            );
            day += Duration::days(1);
        }
        for commit in self.window_commits() {
            if let Some(entry) = days.get_mut(&commit.first_observed_at.date()) {
                entry.commit_count += 1;
                entry.ai_lines += commit.ai_lines;
                entry.human_lines += commit.human_lines;
                for fact in self.facts_for(commit.attribution_commit_id) {
                    entry.committed.add(fact);
                }
            }
        }
        for fact in &self.uncommitted {
            if let Some(entry) = days.get_mut(&fact.occurred_at.date()) {
                entry.uncommitted.add(fact);
            }
        }
        for activity in pull_requests {
            if let Some(entry) =
                days.get_mut(&super::lifecycle::merged_time(&activity.pull_request).date())
            {
                entry.merged_pull_request_count += 1;
            }
        }
        days.into_values().collect()
    }

    /// Joins hook-reported sessions and commits' note sessions by shared hash, then gives each its
    /// window requests and the furthest stage its window commits reached. A lookback commit joins
    /// its session, so the session's window spend is still counted, but sets neither its lines nor
    /// its outcome.
    fn sessions<'b>(
        &'b self,
        agent_sessions: &[&'b AttributionAgentSessionRecord],
        reported: &[&'b AttributionAgentSessionRecord],
    ) -> Vec<SessionRollup<'b>> {
        // Reported sessions are a subset of `agent_sessions`, matched by identity in one lookup.
        let reported = reported
            .iter()
            .map(|session| std::ptr::from_ref(*session))
            .collect::<HashSet<_>>();
        let mut rollups: Vec<SessionRollup<'b>> = Vec::new();
        let mut by_hash: HashMap<(&str, &str), usize> = HashMap::new();
        // The hashes each rollup holds in `by_hash`, so merging two rollups repoints only the
        // absorbed one's hashes. Only hook rows merge, so this lives for the first loop alone.
        let mut members: Vec<Vec<(&str, &str)>> = Vec::new();
        for session in agent_sessions {
            let scope = session.owner.ownership_scope_key.as_str();
            let hashes = || {
                session
                    .session_hashes
                    .iter()
                    .chain(std::iter::once(&session.session_hash))
            };
            let is_reported = reported.contains(&std::ptr::from_ref(*session));
            // Hook rows that share an owner-scoped hash are one session: the second row joins the
            // first rollup, so it isn't counted twice or left as an empty `NoCommit` session.
            let mut found = hashes()
                .filter_map(|hash| by_hash.get(&(scope, hash.as_str())).copied())
                .collect::<Vec<_>>();
            found.sort_unstable();
            found.dedup();
            let index = match found.split_first() {
                Some((&index, others)) => {
                    rollups[index].reported |= is_reported;
                    // A row that bridges rollups already made for other rows (older rows stored
                    // without namespace hashes) merges them into one. Nothing but `reported` has
                    // been gathered yet, and a rollup left unreported and empty is dropped below.
                    for &other in others {
                        rollups[index].reported |= rollups[other].reported;
                        rollups[other].reported = false;
                        let moved = std::mem::take(&mut members[other]);
                        for key in &moved {
                            by_hash.insert(*key, index);
                        }
                        members[index].extend(moved);
                    }
                    index
                }
                None => {
                    rollups.push(SessionRollup {
                        reported: is_reported,
                        harness: Some(hook_harness(&session.harness_key)),
                        ..SessionRollup::default()
                    });
                    members.push(Vec::new());
                    rollups.len() - 1
                }
            };
            for hash in hashes() {
                let key = (scope, hash.as_str());
                if let Entry::Vacant(entry) = by_hash.entry(key) {
                    entry.insert(index);
                    members[index].push(key);
                }
            }
        }
        let mut by_note: HashMap<(&str, &str), usize> = HashMap::new();
        for commit in self.commits {
            let scope = commit.owner.ownership_scope_key.as_str();
            let in_window = commit.first_observed_at >= self.query.from;
            for session in &commit.sessions {
                let known = session
                    .session_hashes
                    .iter()
                    .find_map(|hash| by_hash.get(&(scope, hash.as_str())).copied())
                    .or_else(|| {
                        by_note
                            .get(&(scope, session.note_session_key.as_str()))
                            .copied()
                    });
                let index = known.unwrap_or_else(|| {
                    rollups.push(SessionRollup::default());
                    rollups.len() - 1
                });
                by_note.insert((scope, session.note_session_key.as_str()), index);
                for hash in &session.session_hashes {
                    by_hash.entry((scope, hash.as_str())).or_insert(index);
                }
                let rollup = &mut rollups[index];
                // The hook names the exact harness, which git-ai's shared presets can't: its `pi`
                // covers Pi and Oh My Pi. So the note's tool only names a session with no hook row.
                rollup.harness = rollup.harness.or(Some(session.agent_tool.as_str()));
                rollup.note_model = rollup.note_model.or(session.agent_model.as_deref());
                if !in_window {
                    continue;
                }
                rollup.ai_lines += session.ai_lines;
                // Commits are visited once each, so a commit already on this rollup can only be the
                // last one pushed: two of its note sessions joined the same session.
                if !rollup
                    .commits
                    .last()
                    .is_some_and(|seen| seen.attribution_commit_id == commit.attribution_commit_id)
                {
                    rollup.commits.push(commit);
                }
            }
        }
        // Only the requests the report already counts: those allocated to the window's commits and
        // the uncommitted ones in the window. That keeps the totals and these aggregates on one
        // population, and keeps the report's ownership rule: another owner's requests after their
        // session's last commit never reach a user's report.
        let allocated = self
            .window_commits()
            .flat_map(|commit| self.facts_for(commit.attribution_commit_id).iter().copied())
            .chain(self.uncommitted.iter().copied());
        let mut counted: HashSet<(&str, &str)> = HashSet::new();
        for fact in allocated {
            let scope = fact.ownership_scope_key.as_str();
            let Some(&index) = by_hash.get(&(scope, fact.session_hash.as_str())) else {
                continue;
            };
            if counted.insert((scope, fact.request_id.as_str())) {
                rollups[index].facts.push(fact);
            }
        }
        // A session open across the window, or known only from a lookback commit, counts only
        // when it did something in the window.
        rollups.retain(|rollup| {
            rollup.reported || !rollup.commits.is_empty() || !rollup.facts.is_empty()
        });
        rollups
    }

    /// Session outcomes and, for a full report, harness × model rows. A summary-only build
    /// returns no harness rows, since the summary reads only the outcomes.
    pub(super) fn session_insights<'b>(
        &'b self,
        agent_sessions: &[&'b AttributionAgentSessionRecord],
        reported: &[&'b AttributionAgentSessionRecord],
        merged_commits: &HashSet<(&str, &str)>,
    ) -> (
        Vec<AttributionSessionOutcomeTally>,
        Vec<AttributionHarnessModelActivity>,
    ) {
        let mut outcomes: BTreeMap<AttributionSessionOutcome, (i64, RequestTally)> =
            BTreeMap::new();
        let mut harness_models: BTreeMap<(String, String), AttributionHarnessModelActivity> =
            BTreeMap::new();
        for session in self.sessions(agent_sessions, reported) {
            let outcome = outcome_of(&session.commits, merged_commits);
            let requests = RequestTally::of(session.facts.iter().copied());
            let entry = outcomes.entry(outcome).or_default();
            entry.0 += 1;
            entry.1 = entry.1.merged(requests);
            // A summary-only build reads the outcomes, for shipped spend, but not the harness rows.
            if self.shape == super::Shape::Summary {
                continue;
            }

            let harness = session.harness.unwrap_or("unknown").to_string();
            let model = dominant_model(&session.facts)
                .or(session.note_model)
                .unwrap_or(super::UNKNOWN_MODEL)
                .to_string();
            let row = harness_models
                .entry((harness.clone(), model.clone()))
                .or_insert_with(|| AttributionHarnessModelActivity {
                    harness,
                    model_key: model,
                    session_count: 0,
                    shipped_session_count: 0,
                    ai_lines: 0,
                    requests: RequestTally::default(),
                    priced_session_count: 0,
                    priced_ai_lines: 0,
                    priced_cost_10000: 0,
                });
            row.session_count += 1;
            row.shipped_session_count += i64::from(matches!(
                outcome,
                AttributionSessionOutcome::Merged | AttributionSessionOutcome::DefaultBranch
            ));
            row.ai_lines += session.ai_lines;
            row.requests = row.requests.merged(requests);
            if requests.request_count > 0 && requests.priced_request_count == requests.request_count
            {
                row.priced_session_count += 1;
                row.priced_ai_lines += session.ai_lines;
                row.priced_cost_10000 += requests.priced_cost_10000;
            }
        }
        let outcomes = outcomes
            .into_iter()
            .map(
                |(outcome, (session_count, requests))| AttributionSessionOutcomeTally {
                    outcome,
                    session_count,
                    requests,
                },
            )
            .collect();
        let mut harness_models = harness_models.into_values().collect::<Vec<_>>();
        harness_models.sort_by(|left, right| {
            right
                .session_count
                .cmp(&left.session_count)
                .then_with(|| left.harness.cmp(&right.harness))
                .then_with(|| left.model_key.cmp(&right.model_key))
        });
        (outcomes, harness_models)
    }
}

fn outcome_of(
    commits: &[&AttributionCommitRecord],
    merged_commits: &HashSet<(&str, &str)>,
) -> AttributionSessionOutcome {
    if commits.is_empty() {
        return AttributionSessionOutcome::NoCommit;
    }
    if commits.iter().any(|commit| {
        merged_commits.contains(&(commit.repository.as_str(), commit.commit_sha.as_str()))
    }) {
        return AttributionSessionOutcome::Merged;
    }
    if commits.iter().any(|commit| commit.is_default_branch) {
        return AttributionSessionOutcome::DefaultBranch;
    }
    AttributionSessionOutcome::Unmerged
}

/// The model with the most requests; ties by name. Requests whose log names no model don't
/// count, so the note's model can stand in for them.
fn dominant_model<'a>(facts: &[&'a AttributionRequestFact]) -> Option<&'a str> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for fact in facts {
        if let Some(model) = fact.model_key.as_deref() {
            *counts.entry(model).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then(right.0.cmp(left.0)))
        .map(|(model, _)| model)
}

/// A hook's harness key in the report's vocabulary: git-ai's `claude` for the Claude Code hook,
/// so one harness doesn't split into two rows. Every other hook key already names its harness.
fn hook_harness(key: &str) -> &str {
    match key {
        "claude_code" => "claude",
        other => other,
    }
}

impl AttributionReport {
    /// Headline figures for comparing this window with another.
    #[must_use]
    pub fn summary(&self) -> AttributionPeriodSummary {
        let mut costs = self
            .pull_requests
            .iter()
            .filter(|activity| activity.requests.priced_request_count > 0)
            .map(|activity| activity.requests.priced_cost_10000)
            .collect::<Vec<_>>();
        let mut lead_times = self
            .pull_requests
            .iter()
            .filter_map(AttributionPullRequestActivity::lead_time)
            .map(|lead| lead.whole_seconds())
            .collect::<Vec<_>>();
        let shipped = self
            .session_outcomes
            .iter()
            .filter(|tally| {
                matches!(
                    tally.outcome,
                    AttributionSessionOutcome::Merged | AttributionSessionOutcome::DefaultBranch
                )
            })
            .map(|tally| tally.requests.priced_cost_10000)
            .sum();
        AttributionPeriodSummary {
            commit_count: self.totals.commit_count,
            merged_pull_request_count: self.totals.pull_request_count,
            ai_lines: self.totals.ai_lines,
            human_lines: self.totals.human_lines,
            merged_ai_lines: self.pull_requests.iter().map(|pr| pr.ai_lines).sum(),
            merged_human_lines: self.pull_requests.iter().map(|pr| pr.human_lines).sum(),
            priced_cost_10000: self.totals.requests.priced_cost_10000
                + self.totals.uncommitted.priced_cost_10000,
            shipped_cost_10000: shipped,
            median_pull_request_cost_10000: median(&mut costs),
            median_lead_time_seconds: median(&mut lead_times),
            reverted_pull_request_count: count(
                self.pull_requests
                    .iter()
                    .filter(|activity| activity.is_revert)
                    .count(),
            ),
        }
    }
}

impl AttributionPullRequestActivity {
    /// First commit to merge, when the first commit's time is known.
    #[must_use]
    pub fn lead_time(&self) -> Option<Duration> {
        let first: OffsetDateTime = self.first_commit_at?;
        Some((super::lifecycle::merged_time(&self.pull_request) - first).max(Duration::ZERO))
    }
}

/// The median of `values`, which it sorts: the mean of the two middle values for an even count,
/// rounded down, as the admin UI computes its own medians.
pub(super) fn median(values: &mut [i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        return Some(values[middle]);
    }
    Some(values[middle - 1] + (values[middle] - values[middle - 1]) / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_averages_the_two_middle_values() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [5]), Some(5));
        assert_eq!(median(&mut [9, 1, 5, 3]), Some(4));
        assert_eq!(median(&mut [10_000, 90_000]), Some(50_000));
    }
}
