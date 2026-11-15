//! Aggregations behind the Attribution tabs beyond the tables: daily buckets, where each agent
//! session's work ended up, harness × model efficiency, and a period summary for deltas.
//!
//! A session here is one agent session however it reached Oceans: a hook-reported session, a
//! session named by a window commit's note, or both, joined when they share a session hash.
//! Its requests are the window's requests under any of its hashes, each request counted once.

use std::collections::{BTreeMap, HashMap, HashSet};

use gateway_core::{AttributionCommitRecord, AttributionRequestFact};
use time::{Date, Duration, OffsetDateTime};

use super::{
    AttributionAgentSessionRecord, AttributionPullRequestActivity, AttributionReport, ReportInputs,
    RequestTally, count, model_label,
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

/// The furthest stage an agent session's work reached in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttributionSessionOutcome {
    /// A commit is in a pull request merged in the window.
    Merged,
    /// A commit landed on the default branch with no merged pull request.
    DefaultBranch,
    /// Committed, but no commit reached the default branch or a merged pull request yet.
    Unmerged,
    /// No commit names the session.
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
    pub reverted_pull_request_count: i64,
}

#[derive(Default)]
struct SessionRollup<'a> {
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
        let last = self.query.to.date();
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
            if let Some(entry) = days.get_mut(&activity.pull_request.merged_at.date()) {
                entry.merged_pull_request_count += 1;
            }
        }
        days.into_values().collect()
    }

    /// Joins hook-reported sessions and window commits' note sessions by shared hash, then
    /// gives each its window requests and the furthest stage its commits reached.
    fn sessions<'b>(
        &'b self,
        agent_sessions: &[&'b AttributionAgentSessionRecord],
    ) -> Vec<SessionRollup<'b>> {
        let mut rollups: Vec<SessionRollup<'b>> = Vec::new();
        let mut by_hash: HashMap<(&str, &str), usize> = HashMap::new();
        for session in agent_sessions {
            let scope = session.owner.ownership_scope_key.as_str();
            let index = rollups.len();
            rollups.push(SessionRollup {
                harness: Some(session.harness_key.as_str()),
                ..SessionRollup::default()
            });
            let hashes = session
                .session_hashes
                .iter()
                .chain(std::iter::once(&session.session_hash));
            for hash in hashes {
                by_hash.entry((scope, hash.as_str())).or_insert(index);
            }
        }
        let mut by_note: HashMap<(&str, &str), usize> = HashMap::new();
        for commit in self.window_commits() {
            let scope = commit.owner.ownership_scope_key.as_str();
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
                // The note names the harness the way git-ai does, so prefer it.
                rollup.harness = Some(session.agent_tool.as_str());
                rollup.note_model = rollup.note_model.or(session.agent_model.as_deref());
                rollup.ai_lines += session.ai_lines;
                if !rollup
                    .commits
                    .iter()
                    .any(|seen| seen.attribution_commit_id == commit.attribution_commit_id)
                {
                    rollup.commits.push(commit);
                }
            }
        }
        let window = self.query.from..=self.query.to;
        let mut counted: HashSet<(&str, &str)> = HashSet::new();
        for fact in self.facts {
            if !window.contains(&fact.occurred_at) {
                continue;
            }
            let scope = fact.ownership_scope_key.as_str();
            let Some(&index) = by_hash.get(&(scope, fact.session_hash.as_str())) else {
                continue;
            };
            if counted.insert((scope, fact.request_id.as_str())) {
                rollups[index].facts.push(fact);
            }
        }
        rollups
    }

    pub(super) fn session_insights<'b>(
        &'b self,
        agent_sessions: &[&'b AttributionAgentSessionRecord],
        merged_commits: &HashSet<(&str, &str)>,
    ) -> (
        Vec<AttributionSessionOutcomeTally>,
        Vec<AttributionHarnessModelActivity>,
    ) {
        let mut outcomes: BTreeMap<AttributionSessionOutcome, (i64, RequestTally)> =
            BTreeMap::new();
        let mut harness_models: BTreeMap<(String, String), AttributionHarnessModelActivity> =
            BTreeMap::new();
        for session in self.sessions(agent_sessions) {
            let outcome = outcome_of(&session.commits, merged_commits);
            let requests = RequestTally::of(session.facts.iter().copied());
            let entry = outcomes.entry(outcome).or_default();
            entry.0 += 1;
            entry.1 = entry.1.merged(requests);

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
                });
            row.session_count += 1;
            row.shipped_session_count += i64::from(matches!(
                outcome,
                AttributionSessionOutcome::Merged | AttributionSessionOutcome::DefaultBranch
            ));
            row.ai_lines += session.ai_lines;
            row.requests = row.requests.merged(requests);
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

/// The model with the most requests; ties by name.
fn dominant_model<'a>(facts: &[&'a AttributionRequestFact]) -> Option<&'a str> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for fact in facts {
        *counts.entry(model_label(fact)).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|left, right| left.1.cmp(&right.1).then(right.0.cmp(left.0)))
        .map(|(model, _)| model)
}

/// Whether a merged pull request reverts an earlier change, judged from its title as GitHub's
/// revert button writes it (`Revert "…"`) or as a conventional `revert:` commit type. A proxy
/// for change failure until incidents reach Oceans.
pub(super) fn is_revert_title(title: &str) -> bool {
    let lowered = title.trim_start().to_ascii_lowercase();
    lowered.starts_with("revert ")
        || lowered.starts_with("revert\"")
        || lowered.starts_with("revert:")
        || lowered.starts_with("revert(")
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
        Some((self.pull_request.merged_at - first).max(Duration::ZERO))
    }
}

/// The lower median of `values`, which it sorts.
fn median(values: &mut [i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[(values.len() - 1) / 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revert_titles_match_github_and_conventional_forms() {
        assert!(is_revert_title("Revert \"feat: add budgets\""));
        assert!(is_revert_title("revert: drop the cache"));
        assert!(is_revert_title("revert(api): undo pagination"));
        assert!(!is_revert_title(
            "Reverting to safer defaults is not a revert"
        ));
        assert!(!is_revert_title("fix: revert detection"));
    }

    #[test]
    fn median_takes_the_lower_middle() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [5]), Some(5));
        assert_eq!(median(&mut [9, 1, 5, 3]), Some(3));
    }
}
