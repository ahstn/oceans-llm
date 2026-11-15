//! Git AI attribution routes (`docs/adr/2026-11-01-git-ai-attribution-via-oceans-hooks.md`):
//! the ownership lookup and entry ingestion for `oceans hook`, which need the
//! `attribution:write` scope or full access, and the admin report.

use serde::Serialize;
use utoipa::ToSchema;

pub mod ingest;
pub mod report;
#[cfg(test)]
mod tests;

pub use ingest::{get_my_attribution_ownership, post_attribution_entries};
pub use report::get_attribution_report;

/// Who owns attribution rows: the user or service account of the key that sent them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttributionOwnerKind {
    User,
    ServiceAccount,
}
