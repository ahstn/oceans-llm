//! Discovery planning for tool versions, shared by both store backends.
//!
//! Each backend loads the server's tools and versions, asks `plan_tool_write`
//! what one discovered definition means, and runs the matching statements.

use gateway_core::{
    ExternalMcpToolRecord, McpToolApprovalStatus, McpToolDefinition, McpToolVersionStatus,
    UpsertExternalMcpToolRecord,
    mcp_tool_review::{DiscoveryDecision, StoredToolForReview, decide_discovery},
};

/// One stored version, as discovery needs it.
#[derive(Debug, Clone)]
pub(crate) struct StoredVersion {
    pub schema_version: i64,
    pub schema_hash: String,
    pub status: McpToolVersionStatus,
}

/// A stored tool row with the columns discovery reads.
#[derive(Debug, Clone)]
pub(crate) struct StoredTool {
    pub tool: ExternalMcpToolRecord,
    pub metadata_captured: bool,
    pub versions: Vec<StoredVersion>,
}

/// What discovery writes for one discovered tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolWrite {
    /// Insert a new tool at version 1.
    Insert { status: McpToolApprovalStatus },
    /// Mark the tool seen; its definition and versions stay as they are.
    Touch,
    /// A row discovered before metadata was stored: write the newly captured
    /// metadata into the row and its version without a new version number.
    CaptureMetadata { version: i64 },
    /// Serve the discovered definition as `version`, approved. When
    /// `insert_version` is false the version row exists and is approved.
    Serve { version: i64, insert_version: bool },
    /// Store `version` as pending. `write_definition` is set for a tool that
    /// was never approved, whose row holds the waiting definition.
    Hold {
        version: i64,
        status: McpToolApprovalStatus,
        write_definition: bool,
    },
    /// Serve the approved definition again; pending versions are superseded.
    Revert,
}

pub(crate) fn definition_of(tool: &ExternalMcpToolRecord) -> McpToolDefinition {
    McpToolDefinition {
        description: tool.description.clone(),
        input_schema: tool.input_schema.clone(),
        title: tool.metadata.title.clone(),
        annotations: tool.metadata.annotations.clone(),
        output_schema: tool.metadata.output_schema.clone(),
        schema_hash: tool.schema_hash.clone(),
    }
}

pub(crate) fn discovered_definition(tool: &UpsertExternalMcpToolRecord) -> McpToolDefinition {
    McpToolDefinition {
        description: tool.description.clone(),
        input_schema: tool.input_schema.clone(),
        title: tool.metadata.title.clone(),
        annotations: tool.metadata.annotations.clone(),
        output_schema: tool.metadata.output_schema.clone(),
        schema_hash: tool.schema_hash.clone(),
    }
}

pub(crate) fn plan_tool_write(
    stored: Option<&StoredTool>,
    discovered: &UpsertExternalMcpToolRecord,
    approve_tool_changes: bool,
) -> ToolWrite {
    let discovered_def = discovered_definition(discovered);
    let Some(stored) = stored else {
        return match decide_discovery(None, &discovered_def, approve_tool_changes) {
            DiscoveryDecision::Insert { status } => ToolWrite::Insert { status },
            other => unreachable!("a new tool is always inserted, got {other:?}"),
        };
    };
    // Rows from before V58 kept no metadata. Capturing it is not a change.
    if !stored.metadata_captured
        && stored.tool.input_schema == discovered.input_schema
        && stored.tool.schema_hash != discovered.schema_hash
    {
        return ToolWrite::CaptureMetadata {
            version: stored.tool.schema_version,
        };
    }
    let current = definition_of(&stored.tool);
    let pending_hash = stored
        .versions
        .iter()
        .find(|version| version.status == McpToolVersionStatus::Pending)
        .map(|version| version.schema_hash.as_str());
    let rejected_hashes = stored
        .versions
        .iter()
        .filter(|version| version.status == McpToolVersionStatus::Rejected)
        .map(|version| version.schema_hash.clone())
        .collect::<Vec<_>>();
    let latest_version = stored
        .versions
        .iter()
        .map(|version| version.schema_version)
        .max()
        .unwrap_or(stored.tool.schema_version)
        .max(stored.tool.schema_version);
    let review = StoredToolForReview {
        current: &current,
        schema_version: stored.tool.schema_version,
        approval_status: stored.tool.review.approval_status,
        approved_schema_version: stored.tool.review.approved_schema_version,
        pending_hash,
        rejected_hashes: &rejected_hashes,
        latest_version,
    };
    match decide_discovery(Some(&review), &discovered_def, approve_tool_changes) {
        DiscoveryDecision::Insert { status } => ToolWrite::Insert { status },
        DiscoveryDecision::Unchanged => ToolWrite::Touch,
        DiscoveryDecision::AutoApprove {
            version,
            reuse_version,
        } => ToolWrite::Serve {
            version,
            insert_version: !reuse_version
                || !stored
                    .versions
                    .iter()
                    .any(|stored_version| stored_version.schema_version == version),
        },
        DiscoveryDecision::Hold { version, status } => ToolWrite::Hold {
            version,
            status,
            write_definition: stored.tool.review.approved_schema_version.is_none(),
        },
        DiscoveryDecision::Revert => ToolWrite::Revert,
    }
}
