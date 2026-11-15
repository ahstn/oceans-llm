//! Git AI attribution: ingestion of `oceans hook` entries and the attribution report.
//!
//! See `docs/adr/2026-11-01-git-ai-attribution-via-oceans-hooks.md`. Raw agent session ids and
//! author emails exist only while an entry is processed: sessions are stored as scope-bound
//! lineage hashes and authors as an Oceans user id or a scope-bound email hash.

use async_trait::async_trait;
use gateway_core::StoreError;
use uuid::Uuid;

mod ingest;
pub mod note;
mod report;
mod validate;

pub use ingest::{
    AttributionBatchInput, AttributionBatchOutcome, AttributionEntryCandidate,
    AttributionEntryInput, AttributionEntryResult, AttributionNoteInput, AttributionPayloadInput,
    AttributionPullRequestCommitInput, AttributionPullRequestInput, AttributionPullRequestsInput,
    AttributionSessionInput, attribution_owner, ingest_attribution_batch,
};
pub use report::{
    AttributionChangeActivity, AttributionModelActivity, AttributionPullRequestActivity,
    AttributionReport, AttributionReportCoverage, AttributionReportTotals, AttributionUserActivity,
    RequestTally, build_attribution_report,
};

/// Finds the Oceans user behind an author email. Implemented by the gateway over its store's
/// normalized-email lookup and the users' configured `additional_attribution_emails`.
#[async_trait]
pub trait AttributionUserLookup: Send + Sync {
    /// `email_normalized` is trimmed and lowercased, as `users.email_normalized` is stored.
    async fn user_id_by_normalized_email(
        &self,
        email_normalized: &str,
    ) -> Result<Option<Uuid>, StoreError>;
}

/// Why an entry was refused. The client quarantines it and never retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionRejectReason {
    OversizedNote,
    InvalidNote,
    UnsupportedSchema,
    InvalidRepository,
    InvalidEntry,
}

impl AttributionRejectReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OversizedNote => "oversized_note",
            Self::InvalidNote => "invalid_note",
            Self::UnsupportedSchema => "unsupported_schema",
            Self::InvalidRepository => "invalid_repository",
            Self::InvalidEntry => "invalid_entry",
        }
    }
}

/// What happened to one entry. Every status but `Rejected` lets the client delete it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionEntryStatus {
    Applied,
    Duplicate,
    Stale,
    Rejected(AttributionRejectReason),
}

impl AttributionEntryStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Duplicate => "duplicate",
            Self::Stale => "stale",
            Self::Rejected(_) => "rejected",
        }
    }

    #[must_use]
    pub fn reason(self) -> Option<AttributionRejectReason> {
        match self {
            Self::Rejected(reason) => Some(reason),
            _ => None,
        }
    }
}

impl From<gateway_core::AttributionApplyOutcome> for AttributionEntryStatus {
    fn from(outcome: gateway_core::AttributionApplyOutcome) -> Self {
        match outcome {
            gateway_core::AttributionApplyOutcome::Applied => Self::Applied,
            gateway_core::AttributionApplyOutcome::Duplicate => Self::Duplicate,
            gateway_core::AttributionApplyOutcome::Stale => Self::Stale,
        }
    }
}

#[cfg(test)]
mod tests;
