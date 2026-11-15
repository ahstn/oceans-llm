//! Pull request lifecycle and revert links. The store returns every pull request merged,
//! opened or closed in the window; the report's commit and spend figures use only those merged
//! in it, and this module reads the rest: how many opened, closed without merging or are still
//! open, how long they waited for review, and which merged pull requests revert others.
//!
//! Revert links come from `oceans hook prs`, which checks git's `This reverts commit` line,
//! GitHub's revert button and an exact inverse change before a title. Only those three count as
//! reverts; a title alone, and any row stored before the hook sent links, is a possible revert.

use std::collections::{BTreeMap, HashMap};

use gateway_core::{
    AttributionPullRequestRecord, AttributionPullRequestState, AttributionReportQuery,
    AttributionRevertEvidence,
};
use time::{Duration, OffsetDateTime};

use super::insights::{is_revert_title, median};
use super::{AttributionPullRequestActivity, count};

/// What a merged pull request reverts and how that was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributionRevertActivity {
    pub evidence: AttributionRevertEvidence,
    /// The reverted pull request in the same repository, when it could be resolved.
    pub reverts_pull_request_number: Option<i64>,
}

impl AttributionRevertActivity {
    pub fn is_confirmed(&self) -> bool {
        self.evidence.is_confirmed()
    }
}

/// Lifecycle figures over every pull request merged, opened or closed in the window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributionPullRequestLifecycleSummary {
    pub opened_count: i64,
    pub merged_count: i64,
    /// Closed without merging in the window.
    pub closed_unmerged_count: i64,
    /// Still open, of those opened or updated in the window.
    pub open_count: i64,
    pub draft_count: i64,
    /// Merged in the window with review data from the hook; the others predate it.
    pub review_known_count: i64,
    /// Merged in the window with no review by anyone but the author or a bot.
    pub merged_without_review_count: i64,
    /// Median opened to first human review, for reviews in the window.
    pub median_review_wait_seconds: Option<i64>,
    /// Merged in the window and reverting another pull request, by confirmed evidence.
    pub revert_count: i64,
    /// Merged in the window with only a revert title to go on.
    pub possible_revert_count: i64,
    /// Median merge of a reverted pull request to the merge of its revert.
    pub median_time_to_revert_seconds: Option<i64>,
}

/// Whether `pull_request` merged inside the window, which is what the commit, spend and
/// delivery figures count.
pub(super) fn merged_in(
    query: &AttributionReportQuery,
    pull_request: &AttributionPullRequestRecord,
) -> bool {
    pull_request.lifecycle.state == AttributionPullRequestState::Merged
        && pull_request
            .merged_at
            .is_some_and(|at| at >= query.from && at <= query.to)
}

/// A pull request's latest activity, as the store orders its read: the forge's last change,
/// else the latest lifecycle time an older hook sent.
pub(super) fn activity_at(pull_request: &AttributionPullRequestRecord) -> OffsetDateTime {
    let lifecycle = &pull_request.lifecycle;
    lifecycle
        .last_activity_at
        .or(pull_request.merged_at)
        .or(lifecycle.closed_at)
        .or(lifecycle.opened_at)
        .unwrap_or(pull_request.observed_at)
}

/// Whether anything happened to `pull_request` in the window: it opened, was reviewed, merged,
/// closed, or changed on the forge. A routine re-sync of an untouched pull request doesn't
/// count, so `observed_at` is left out.
fn active_in(query: &AttributionReportQuery, pull_request: &AttributionPullRequestRecord) -> bool {
    [
        pull_request.merged_at,
        pull_request.lifecycle.opened_at,
        pull_request.lifecycle.closed_at,
        pull_request.lifecycle.details.first_review_at,
        pull_request.lifecycle.last_activity_at,
    ]
    .into_iter()
    .flatten()
    .any(|at| at >= query.from && at <= query.to)
}

/// The merge time of a pull request the report already knows merged.
pub(super) fn merged_time(pull_request: &AttributionPullRequestRecord) -> OffsetDateTime {
    pull_request.merged_at.unwrap_or(pull_request.observed_at)
}

/// Opened, merged, closed and open counts and review wait over every loaded pull request.
pub(super) fn summary(
    pull_requests: &[AttributionPullRequestRecord],
    query: &AttributionReportQuery,
) -> AttributionPullRequestLifecycleSummary {
    let in_window =
        |at: Option<OffsetDateTime>| at.is_some_and(|at| at >= query.from && at <= query.to);
    let merged: Vec<_> = pull_requests
        .iter()
        .filter(|pr| merged_in(query, pr))
        .collect();
    let open: Vec<_> = pull_requests
        .iter()
        .filter(|pr| pr.lifecycle.state == AttributionPullRequestState::Open)
        .filter(|pr| active_in(query, pr))
        .collect();
    let mut review_waits: Vec<i64> = pull_requests
        .iter()
        .filter_map(|pr| {
            let reviewed = pr.lifecycle.details.first_review_at?;
            let opened = pr.lifecycle.opened_at?;
            in_window(Some(reviewed))
                .then(|| (reviewed - opened).max(Duration::ZERO).whole_seconds())
        })
        .collect();
    AttributionPullRequestLifecycleSummary {
        opened_count: count(
            pull_requests
                .iter()
                .filter(|pr| in_window(pr.lifecycle.opened_at))
                .count(),
        ),
        merged_count: count(merged.len()),
        closed_unmerged_count: count(
            pull_requests
                .iter()
                .filter(|pr| pr.lifecycle.state == AttributionPullRequestState::Closed)
                .filter(|pr| in_window(pr.lifecycle.closed_at))
                .count(),
        ),
        open_count: count(open.len()),
        draft_count: count(
            open.iter()
                .filter(|pr| pr.lifecycle.details.is_draft == Some(true))
                .count(),
        ),
        review_known_count: count(
            merged
                .iter()
                .filter(|pr| pr.lifecycle.details.review_count.is_some())
                .count(),
        ),
        merged_without_review_count: count(
            merged
                .iter()
                .filter(|pr| pr.lifecycle.details.review_count == Some(0))
                .count(),
        ),
        median_review_wait_seconds: median(&mut review_waits),
        ..AttributionPullRequestLifecycleSummary::default()
    }
}

/// Resolves each merged pull request's revert link against every loaded pull request: by the
/// reverted number, else by a reverted commit listed in, or merging, another pull request.
/// Marks confirmed reverts and their originals, and fills the summary's revert figures.
pub(super) fn apply_reverts(
    activities: &mut [AttributionPullRequestActivity],
    loaded: &[AttributionPullRequestRecord],
    summary: &mut AttributionPullRequestLifecycleSummary,
) {
    let mut merged_at: HashMap<(&str, i64), OffsetDateTime> = HashMap::new();
    // `(repository, sha) → pull request`, sorted so an abbreviated SHA is a range lookup.
    let mut shas: BTreeMap<(&str, &str), i64> = BTreeMap::new();
    // Only merged pull requests can be reverted: an open release-promotion PR listing commits
    // already merged elsewhere must not claim them.
    for pr in loaded
        .iter()
        .filter(|pr| pr.lifecycle.state == AttributionPullRequestState::Merged)
    {
        if let Some(at) = pr.merged_at {
            merged_at.insert((pr.repository.as_str(), pr.number), at);
        }
        if let Some(sha) = pr.merge_commit_sha.as_deref() {
            shas.entry((pr.repository.as_str(), sha))
                .or_insert(pr.number);
        }
        for commit in &pr.commits {
            shas.entry((pr.repository.as_str(), commit.commit_sha.as_str()))
                .or_insert(pr.number);
        }
    }
    let mut reverted_by: HashMap<(String, i64), i64> = HashMap::new();
    let mut times_to_revert: Vec<i64> = Vec::new();
    for activity in activities.iter_mut() {
        let pr = &activity.pull_request;
        let repository = pr.repository.as_str();
        let link = pr.lifecycle.details.reverts.as_ref();
        let evidence = match link {
            Some(link) => link.evidence,
            None if is_revert_title(&pr.title) => AttributionRevertEvidence::Title,
            None => continue,
        };
        let target = link.and_then(|link| {
            link.pull_request_number.or_else(|| {
                link.commit_shas.iter().find_map(|reverted| {
                    shas.range((repository, reverted.as_str())..)
                        .take_while(|((repo, sha), _)| {
                            *repo == repository && sha.starts_with(reverted.as_str())
                        })
                        .map(|(_, number)| *number)
                        .find(|number| *number != pr.number)
                })
            })
        });
        let target = target.filter(|number| *number != pr.number);
        let revert = AttributionRevertActivity {
            evidence,
            reverts_pull_request_number: target,
        };
        if revert.is_confirmed() {
            summary.revert_count += 1;
            if let Some(number) = target {
                reverted_by
                    .entry((repository.to_owned(), number))
                    .or_insert(pr.number);
                if let (Some(original), Some(revert_at)) =
                    (merged_at.get(&(repository, number)), pr.merged_at)
                {
                    times_to_revert
                        .push((revert_at - *original).max(Duration::ZERO).whole_seconds());
                }
            }
        } else {
            summary.possible_revert_count += 1;
        }
        activity.is_revert = revert.is_confirmed();
        activity.revert = Some(revert);
    }
    for activity in activities.iter_mut() {
        let key = (
            activity.pull_request.repository.clone(),
            activity.pull_request.number,
        );
        activity.reverted_by = reverted_by.get(&key).copied();
    }
    summary.median_time_to_revert_seconds = median(&mut times_to_revert);
}
