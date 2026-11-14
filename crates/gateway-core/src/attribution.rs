//! Git AI attribution records and repository contract.
//!
//! See `docs/adr/2026-11-01-git-ai-attribution-via-oceans-hooks.md`. Raw external identifiers
//! (agent session ids, author emails) never reach these records: sessions are stored as
//! scope-bound lineage hashes and authors as an Oceans user id or a scope-bound email hash.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::StoreError;

/// Maximum entries accepted in one ingestion request.
pub const ATTRIBUTION_MAX_BATCH_ENTRIES: usize = 100;
/// Maximum ingestion request body, enforced before JSON parsing.
pub const ATTRIBUTION_MAX_BATCH_BYTES: usize = 2 * 1024 * 1024;
/// Maximum commits or pull requests a single report query loads.
pub const ATTRIBUTION_MAX_REPORT_ROWS: u32 = 1_000;
/// Maximum session hashes passed to one store lookup; callers chunk larger sets. A commit
/// session carries one hash per session namespace (six), so this covers 500 sessions.
pub const ATTRIBUTION_MAX_HASHES_PER_LOOKUP: usize = 3_000;
/// Maximum request facts one store lookup returns.
pub const ATTRIBUTION_MAX_REQUEST_FACTS: u32 = 50_000;

/// Client-assigned revision of a queued entry (ADR "Freshness").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionFreshness {
    pub installation_id: String,
    pub sequence: i64,
    pub observed_at: OffsetDateTime,
    pub late_sequenced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributionApplyOutcome {
    /// The write was stored (new row or replacement).
    Applied,
    /// The same revision is already stored.
    Duplicate,
    /// A newer revision is already stored; the write was acknowledged without being applied.
    Stale,
}

impl AttributionFreshness {
    /// Decides whether `self` (incoming) replaces `stored`.
    ///
    /// Same installation: a higher sequence wins; a `late_sequenced` entry must also be observed
    /// later than the stored one. Different installations: the later `observed_at` wins.
    pub fn decide_against(&self, stored: &AttributionFreshness) -> AttributionApplyOutcome {
        if self.installation_id == stored.installation_id {
            if self.sequence == stored.sequence {
                return AttributionApplyOutcome::Duplicate;
            }
            if self.sequence < stored.sequence {
                return AttributionApplyOutcome::Stale;
            }
            if self.late_sequenced && self.observed_at <= stored.observed_at {
                return AttributionApplyOutcome::Stale;
            }
            return AttributionApplyOutcome::Applied;
        }
        if self.observed_at > stored.observed_at {
            AttributionApplyOutcome::Applied
        } else {
            AttributionApplyOutcome::Stale
        }
    }
}

/// Ownership columns copied from the ingesting API key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionOwner {
    /// `user:{id}` or `service_account:{id}` from `usage_ownership_scope_key`.
    pub ownership_scope_key: String,
    pub owner_user_id: Option<Uuid>,
    pub owner_service_account_id: Option<Uuid>,
}

/// One AI session (or legacy prompt) mentioned by a commit's note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionCommitSession {
    /// The note's own key: `s_<14hex>` or a legacy hex prompt hash. Derived by git-ai, not raw.
    pub note_session_key: String,
    /// `agent_id.tool`, e.g. `claude`, `pi`, `codex`. A display label; the join ignores it.
    pub agent_tool: String,
    /// `agent_id.model`; `None` when the note says `unknown` or omits it.
    pub agent_model: Option<String>,
    /// Display label: the Oceans harness key for `agent_tool` (`claude` → `claude_code`, `pi` →
    /// `oh_my_pi`, ...); `None` for other tools. The join ignores it.
    pub harness_key: Option<String>,
    /// `hash_lineage_candidate(scope, namespace, agent_id.id)` for every session namespace the
    /// gateway records request sessions under. The session joins the requests of any of them,
    /// whichever harness the gateway classified the traffic as. Empty when the note names no
    /// session id the gateway could have recorded. Stored and read back ascending and distinct.
    pub session_hashes: Vec<String>,
    /// Attested lines for this session in the commit.
    pub ai_lines: i64,
    pub human_author_user_id: Option<Uuid>,
    pub human_author_hash: Option<String>,
}

/// A parsed note ready to store. Replaces any stored note for
/// `(ownership_scope_key, repository, commit_sha)` when `freshness` wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionCommitWrite {
    /// Used only when inserting; a replacement keeps the stored id.
    pub attribution_commit_id: Uuid,
    pub owner: AttributionOwner,
    /// Normalised `host/org/repo`, never containing userinfo.
    pub repository: String,
    pub commit_sha: String,
    pub branch: Option<String>,
    pub is_default_branch: bool,
    pub author_user_id: Option<Uuid>,
    pub author_email_hash: Option<String>,
    pub schema_version: String,
    pub git_ai_version: Option<String>,
    pub ai_lines: i64,
    pub human_lines: i64,
    pub freshness: AttributionFreshness,
    pub sessions: Vec<AttributionCommitSession>,
    pub received_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributionSessionEventKind {
    Start,
    End,
}

/// A harness session start or end reported by `oceans hook claude` / `oceans hook omp`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionSessionEventWrite {
    pub owner: AttributionOwner,
    /// The reporting hook's harness, e.g. `claude_code` or `oh_my_pi`. A display label and part
    /// of the session row's key; the join ignores it.
    pub harness_key: String,
    /// The session row's key: `hash_lineage_candidate(scope, harness_key, session_id)`.
    pub session_hash: String,
    /// `hash_lineage_candidate(scope, namespace, session_id)` for every session namespace the
    /// gateway records request sessions under. The session finds its requests under any of
    /// them, whichever harness the gateway classified the traffic as.
    pub session_hashes: Vec<String>,
    pub event: AttributionSessionEventKind,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    pub harness_version: Option<String>,
    pub observed_at: OffsetDateTime,
    pub received_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionPullRequestCommit {
    pub commit_sha: String,
    pub subject: Option<String>,
    pub authored_at: Option<OffsetDateTime>,
    pub author_user_id: Option<Uuid>,
    pub author_email_hash: Option<String>,
}

/// A merged pull request reported by `oceans hook prs`. Replaces the stored PR and its commit
/// list for `(ownership_scope_key, repository, number)` when `observed_at` is later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionPullRequestWrite {
    /// Used only when inserting; a replacement keeps the stored id.
    pub attribution_pull_request_id: Uuid,
    pub owner: AttributionOwner,
    pub repository: String,
    pub number: i64,
    pub title: String,
    pub url: Option<String>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub merged_at: OffsetDateTime,
    /// Matched from the first commit author email that belongs to an Oceans user.
    pub author_user_id: Option<Uuid>,
    pub author_email_hash: Option<String>,
    pub commits: Vec<AttributionPullRequestCommit>,
    pub observed_at: OffsetDateTime,
    pub received_at: OffsetDateTime,
}

/// Filters shared by the report reads. Bounds are inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributionReportQuery {
    pub from: OffsetDateTime,
    pub to: OffsetDateTime,
    /// Restricts rows to those where `owner_user_id` or `author_user_id` equals this user.
    pub user_id: Option<Uuid>,
    pub repository: Option<String>,
    /// Row cap, at most [`ATTRIBUTION_MAX_REPORT_ROWS`]; newest rows first.
    pub limit: u32,
}

/// A stored commit. `first_observed_at` is the commit time used for request windows and
/// report ranges; it is kept when a newer note replaces the row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionCommitRecord {
    pub attribution_commit_id: Uuid,
    pub owner: AttributionOwner,
    pub repository: String,
    pub commit_sha: String,
    pub branch: Option<String>,
    pub is_default_branch: bool,
    pub author_user_id: Option<Uuid>,
    pub author_email_hash: Option<String>,
    pub schema_version: String,
    pub git_ai_version: Option<String>,
    pub ai_lines: i64,
    pub human_lines: i64,
    pub freshness: AttributionFreshness,
    pub first_observed_at: OffsetDateTime,
    pub sessions: Vec<AttributionCommitSession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionPullRequestRecord {
    pub attribution_pull_request_id: Uuid,
    pub owner: AttributionOwner,
    pub repository: String,
    pub number: i64,
    pub title: String,
    pub url: Option<String>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub merge_commit_sha: Option<String>,
    pub merged_at: OffsetDateTime,
    pub author_user_id: Option<Uuid>,
    pub author_email_hash: Option<String>,
    pub commits: Vec<AttributionPullRequestCommit>,
    pub observed_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionAgentSessionRecord {
    pub owner: AttributionOwner,
    /// The reporting hook's harness; a display label and part of the row's key.
    pub harness_key: String,
    /// The row's key, the session id hashed under `harness_key`'s namespace.
    pub session_hash: String,
    /// The session id hashed under every session namespace, ascending and distinct: every
    /// hash its events carried. The report finds the session's requests under any of them.
    pub session_hashes: Vec<String>,
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub start_head_sha: Option<String>,
    pub end_head_sha: Option<String>,
    pub harness_version: Option<String>,
    pub started_at: Option<OffsetDateTime>,
    pub ended_at: Option<OffsetDateTime>,
    pub last_observed_at: OffsetDateTime,
}

/// A commit time for one session hash, used to bound that session's request windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionSessionCommitTime {
    pub ownership_scope_key: String,
    /// The matched hash, one of a commit session's `session_hashes`.
    pub session_hash: String,
    pub attribution_commit_id: Uuid,
    pub committed_at: OffsetDateTime,
}

/// One gateway request linked to a session hash through `agent_session_requests`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttributionRequestFact {
    pub ownership_scope_key: String,
    pub session_hash: String,
    pub request_id: String,
    pub occurred_at: OffsetDateTime,
    /// `request_logs.resolved_model_key`, falling back to `model_key`.
    pub model_key: Option<String>,
    /// `usage_cost_events.computed_cost_10000` when the request was priced
    /// (`priced` or `legacy_estimated`); `None` when unpriced or usage is missing.
    pub cost_10000: Option<i64>,
    pub pricing_status: Option<String>,
}

#[async_trait]
pub trait AttributionRepository {
    /// Stores a note under its replacement key in one transaction: freshness decision, commit
    /// upsert and full replacement of that commit's session rows and their hashes.
    async fn apply_attribution_commit(
        &self,
        write: &AttributionCommitWrite,
    ) -> Result<AttributionApplyOutcome, StoreError>;

    /// Upserts `(ownership_scope_key, harness_key, session_hash)` with the event's hashes in
    /// one transaction. `Start` keeps the earliest start, `End` the latest end, and the hashes
    /// accumulate; returns `Duplicate` when nothing changed.
    async fn apply_attribution_session_event(
        &self,
        write: &AttributionSessionEventWrite,
    ) -> Result<AttributionApplyOutcome, StoreError>;

    /// Stores a merged PR and replaces its commit list in one transaction when
    /// `observed_at` is later than the stored PR (`Duplicate` when equal, `Stale` when older).
    async fn apply_attribution_pull_request(
        &self,
        write: &AttributionPullRequestWrite,
    ) -> Result<AttributionApplyOutcome, StoreError>;

    /// Commits with `first_observed_at` in range, newest first, with their sessions and each
    /// session's hashes in ascending order.
    async fn list_attribution_commits(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionCommitRecord>, StoreError>;

    /// PRs with `merged_at` in range, newest first, with their commits.
    async fn list_attribution_pull_requests(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionPullRequestRecord>, StoreError>;

    /// Harness sessions whose start or end falls in range, newest first, each with its hashes
    /// in ascending order.
    async fn list_attribution_agent_sessions(
        &self,
        query: &AttributionReportQuery,
    ) -> Result<Vec<AttributionAgentSessionRecord>, StoreError>;

    /// Every stored commit time at or before `until` for the given hashes: one row per commit
    /// and matched hash, whichever of a commit session's hashes matched. At most
    /// [`ATTRIBUTION_MAX_HASHES_PER_LOOKUP`] hashes per call.
    async fn list_attribution_session_commit_times(
        &self,
        session_hashes: &[String],
        until: OffsetDateTime,
    ) -> Result<Vec<AttributionSessionCommitTime>, StoreError>;

    /// Gateway requests whose `agent_session_requests.normalized_session_id` is one of the
    /// hashes and that occurred at or before `until`, oldest first, capped at
    /// [`ATTRIBUTION_MAX_REQUEST_FACTS`]. At most [`ATTRIBUTION_MAX_HASHES_PER_LOOKUP`] hashes.
    async fn list_attribution_request_facts(
        &self,
        session_hashes: &[String],
        until: OffsetDateTime,
    ) -> Result<Vec<AttributionRequestFact>, StoreError>;

    /// Deletes every attribution row for an ownership scope; returns rows deleted.
    async fn delete_attribution_for_owner(
        &self,
        ownership_scope_key: &str,
    ) -> Result<u64, StoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn freshness(
        installation: &str,
        sequence: i64,
        observed: i64,
        late: bool,
    ) -> AttributionFreshness {
        AttributionFreshness {
            installation_id: installation.to_string(),
            sequence,
            observed_at: OffsetDateTime::from_unix_timestamp(observed).unwrap(),
            late_sequenced: late,
        }
    }

    #[test]
    fn same_installation_orders_by_sequence() {
        let stored = freshness("a", 5, 100, false);
        assert_eq!(
            freshness("a", 5, 200, false).decide_against(&stored),
            AttributionApplyOutcome::Duplicate
        );
        assert_eq!(
            freshness("a", 4, 200, false).decide_against(&stored),
            AttributionApplyOutcome::Stale
        );
        assert_eq!(
            freshness("a", 6, 50, false).decide_against(&stored),
            AttributionApplyOutcome::Applied
        );
    }

    #[test]
    fn late_sequenced_entry_needs_a_later_observation() {
        let stored = freshness("a", 5, 100, false);
        assert_eq!(
            freshness("a", 6, 100, true).decide_against(&stored),
            AttributionApplyOutcome::Stale
        );
        assert_eq!(
            freshness("a", 6, 101, true).decide_against(&stored),
            AttributionApplyOutcome::Applied
        );
    }

    #[test]
    fn different_installations_order_by_observation_time() {
        let stored = freshness("a", 50, 100, false);
        assert_eq!(
            freshness("b", 1, 101, false).decide_against(&stored),
            AttributionApplyOutcome::Applied
        );
        assert_eq!(
            freshness("b", 99, 100, false).decide_against(&stored),
            AttributionApplyOutcome::Stale
        );
    }
}
