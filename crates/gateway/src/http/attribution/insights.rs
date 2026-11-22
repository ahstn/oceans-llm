//! Report views behind the Attribution tabs beyond the tables: daily buckets, session outcomes,
//! harness × model rows, per-PR commits and the period summaries the KPI deltas compare.

use gateway_core::AttributionRevertEvidence;
use gateway_service::attribution::{
    AttributionDayActivity, AttributionHarnessModelActivity, AttributionPeriodSummary,
    AttributionPullRequestCommitActivity, AttributionPullRequestLifecycleSummary,
    AttributionRevertActivity, AttributionSessionOutcome, AttributionSessionOutcomeTally,
};
use serde::Serialize;
use utoipa::ToSchema;

use super::report::AttributionRequestTallyView;
use crate::http::admin_contract::format_timestamp;

/// One UTC day of the window. Committed requests count on their commit's day.
#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionDayView {
    /// `YYYY-MM-DD`.
    pub day: String,
    pub commit_count: i64,
    pub ai_lines: i64,
    pub human_lines: i64,
    pub merged_pull_request_count: i64,
    /// Requests allocated to the day's commits.
    pub committed: AttributionRequestTallyView,
    /// Requests after their session's last commit, by the day they happened.
    pub uncommitted: AttributionRequestTallyView,
}

/// The furthest stage an agent session's work reached, counting only commits in the window.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttributionSessionOutcomeKind {
    /// A commit is in a pull request merged in the window.
    Merged,
    /// A commit landed on the default branch with no merged pull request.
    DefaultBranch,
    /// Committed, but nothing reached the default branch or a merged pull request yet.
    Unmerged,
    /// No commit in the window names the session.
    NoCommit,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionSessionOutcomeView {
    pub outcome: AttributionSessionOutcomeKind,
    pub session_count: i64,
    #[serde(flatten)]
    pub requests: AttributionRequestTallyView,
}

/// Agent sessions of one harness on one model.
#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionHarnessModelView {
    /// git-ai's tool name (`claude`, `codex`, `pi`), else the hook's harness key.
    pub harness: String,
    pub model_key: String,
    pub session_count: i64,
    /// Sessions whose work was merged or reached the default branch.
    pub shipped_session_count: i64,
    /// AI lines the sessions wrote into the window's commits.
    pub ai_lines: i64,
    #[serde(flatten)]
    pub requests: AttributionRequestTallyView,
    /// Sessions with requests, every one of them priced. Cost per line uses only these.
    pub fully_priced_session_count: i64,
    /// AI lines of the fully priced sessions.
    pub fully_priced_ai_lines: i64,
    /// Priced cost of the fully priced sessions, in 1/10000 USD.
    pub fully_priced_cost_usd_10000: i64,
}

/// A merged pull request's commit that has a stored note.
#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionPullRequestCommitView {
    pub commit_sha: String,
    pub committed_at: String,
    pub ai_lines: i64,
    pub human_lines: i64,
    pub agent_tools: Vec<String>,
    pub models: Vec<String>,
    #[serde(flatten)]
    pub requests: AttributionRequestTallyView,
}

/// Headline figures for one window.
#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionPeriodSummaryView {
    pub commit_count: i64,
    pub merged_pull_request_count: i64,
    pub ai_lines: i64,
    pub human_lines: i64,
    /// Lines of the commits in merged pull requests.
    pub merged_ai_lines: i64,
    pub merged_human_lines: i64,
    /// Priced cost of every counted request, committed or not.
    pub priced_cost_usd_10000: i64,
    /// Priced cost of sessions whose work was merged or reached the default branch.
    pub shipped_cost_usd_10000: i64,
    pub median_pull_request_cost_usd_10000: Option<i64>,
    /// Median first commit to merge.
    pub median_lead_time_seconds: Option<i64>,
    /// Merged pull requests whose title reads as a revert.
    pub reverted_pull_request_count: i64,
}

/// How a revert link was found, strongest first. `title` alone is a possible revert.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttributionRevertEvidenceView {
    /// A pull request commit carries git's `This reverts commit <sha>` line.
    RevertCommit,
    /// GitHub's revert button, backed by a revert title.
    GithubRevert,
    /// The change is the exact inverse of an earlier merged pull request's.
    ChangeMatch,
    /// A revert title and nothing else.
    Title,
}

/// What a merged pull request reverts.
#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionRevertView {
    pub evidence: AttributionRevertEvidenceView,
    /// Whether the evidence is stronger than a title.
    pub confirmed: bool,
    /// The reverted pull request in the same repository, when it could be resolved.
    pub reverts_pull_request_number: Option<i64>,
}

/// Lifecycle figures over every pull request merged, opened or closed in the window.
#[derive(Debug, Serialize, ToSchema)]
pub struct AttributionPullRequestLifecycleView {
    pub opened_count: i64,
    pub merged_count: i64,
    /// Closed without merging in the window.
    pub closed_unmerged_count: i64,
    /// Still open, of those opened or updated in the window.
    pub open_count: i64,
    pub draft_count: i64,
    /// Merged in the window with review data from the hook; older hooks sent none.
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

impl From<AttributionRevertActivity> for AttributionRevertView {
    fn from(revert: AttributionRevertActivity) -> Self {
        Self {
            evidence: match revert.evidence {
                AttributionRevertEvidence::RevertCommit => {
                    AttributionRevertEvidenceView::RevertCommit
                }
                AttributionRevertEvidence::GithubRevert => {
                    AttributionRevertEvidenceView::GithubRevert
                }
                AttributionRevertEvidence::ChangeMatch => {
                    AttributionRevertEvidenceView::ChangeMatch
                }
                AttributionRevertEvidence::Title => AttributionRevertEvidenceView::Title,
            },
            confirmed: revert.is_confirmed(),
            reverts_pull_request_number: revert.reverts_pull_request_number,
        }
    }
}

impl From<AttributionPullRequestLifecycleSummary> for AttributionPullRequestLifecycleView {
    fn from(summary: AttributionPullRequestLifecycleSummary) -> Self {
        Self {
            opened_count: summary.opened_count,
            merged_count: summary.merged_count,
            closed_unmerged_count: summary.closed_unmerged_count,
            open_count: summary.open_count,
            draft_count: summary.draft_count,
            review_known_count: summary.review_known_count,
            merged_without_review_count: summary.merged_without_review_count,
            median_review_wait_seconds: summary.median_review_wait_seconds,
            revert_count: summary.revert_count,
            possible_revert_count: summary.possible_revert_count,
            median_time_to_revert_seconds: summary.median_time_to_revert_seconds,
        }
    }
}

impl From<AttributionDayActivity> for AttributionDayView {
    fn from(day: AttributionDayActivity) -> Self {
        Self {
            day: day.day.to_string(),
            commit_count: day.commit_count,
            ai_lines: day.ai_lines,
            human_lines: day.human_lines,
            merged_pull_request_count: day.merged_pull_request_count,
            committed: day.committed.into(),
            uncommitted: day.uncommitted.into(),
        }
    }
}

impl From<AttributionSessionOutcomeTally> for AttributionSessionOutcomeView {
    fn from(tally: AttributionSessionOutcomeTally) -> Self {
        Self {
            outcome: match tally.outcome {
                AttributionSessionOutcome::Merged => AttributionSessionOutcomeKind::Merged,
                AttributionSessionOutcome::DefaultBranch => {
                    AttributionSessionOutcomeKind::DefaultBranch
                }
                AttributionSessionOutcome::Unmerged => AttributionSessionOutcomeKind::Unmerged,
                AttributionSessionOutcome::NoCommit => AttributionSessionOutcomeKind::NoCommit,
            },
            session_count: tally.session_count,
            requests: tally.requests.into(),
        }
    }
}

impl From<AttributionHarnessModelActivity> for AttributionHarnessModelView {
    fn from(row: AttributionHarnessModelActivity) -> Self {
        Self {
            harness: row.harness,
            model_key: row.model_key,
            session_count: row.session_count,
            shipped_session_count: row.shipped_session_count,
            ai_lines: row.ai_lines,
            requests: row.requests.into(),
            fully_priced_session_count: row.priced_session_count,
            fully_priced_ai_lines: row.priced_ai_lines,
            fully_priced_cost_usd_10000: gateway_core::Money9::from_nanos(row.priced_cost_nanos)
                .round_half_up_to_money4()
                .as_scaled_i64(),
        }
    }
}

impl From<AttributionPullRequestCommitActivity> for AttributionPullRequestCommitView {
    fn from(commit: AttributionPullRequestCommitActivity) -> Self {
        Self {
            commit_sha: commit.commit_sha,
            committed_at: format_timestamp(commit.committed_at),
            ai_lines: commit.ai_lines,
            human_lines: commit.human_lines,
            agent_tools: commit.agent_tools,
            models: commit.models,
            requests: commit.requests.into(),
        }
    }
}

impl From<AttributionPeriodSummary> for AttributionPeriodSummaryView {
    fn from(summary: AttributionPeriodSummary) -> Self {
        Self {
            commit_count: summary.commit_count,
            merged_pull_request_count: summary.merged_pull_request_count,
            ai_lines: summary.ai_lines,
            human_lines: summary.human_lines,
            merged_ai_lines: summary.merged_ai_lines,
            merged_human_lines: summary.merged_human_lines,
            priced_cost_usd_10000: summary.priced_cost_10000,
            shipped_cost_usd_10000: summary.shipped_cost_10000,
            median_pull_request_cost_usd_10000: summary.median_pull_request_cost_10000,
            median_lead_time_seconds: summary.median_lead_time_seconds,
            reverted_pull_request_count: summary.reverted_pull_request_count,
        }
    }
}
