//! Discovery planning for tool versions, shared by both store backends.
//!
//! Each backend loads the server's tools and versions, asks `plan_tool_write`
//! what one discovered definition means, and runs the matching statements.

use std::collections::HashMap;

use gateway_core::{
    ExternalMcpToolRecord, McpToolApprovalStatus, McpToolDefinition, McpToolDefinitionKey,
    McpToolVersionStatus, UpsertExternalMcpToolRecord,
    mcp_tool_review::{DiscoveryDecision, StoredToolForReview, decide_discovery},
};

/// One stored version, as discovery needs it.
#[derive(Debug, Clone)]
pub(crate) struct StoredVersion {
    pub schema_version: i64,
    pub schema_hash: String,
    pub description: Option<String>,
    pub title: Option<String>,
    pub status: McpToolVersionStatus,
}

impl StoredVersion {
    fn key(&self) -> McpToolDefinitionKey {
        McpToolDefinitionKey {
            schema_hash: self.schema_hash.clone(),
            description: self.description.clone(),
            title: self.title.clone(),
        }
    }
}

/// A stored tool row with the columns discovery reads.
#[derive(Debug, Clone)]
pub(crate) struct StoredTool {
    pub tool: ExternalMcpToolRecord,
    pub metadata_captured: bool,
    /// Only the versions review still depends on: pending and rejected ones.
    pub versions: Vec<StoredVersion>,
    /// The highest version number the tool has, of any status.
    pub latest_version: i64,
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
        /// False when an older version with this definition is reused.
        insert_version: bool,
    },
    /// Serve the approved definition again; pending versions are superseded.
    Revert,
    /// Supersede pending versions and set the tool's status, for an upstream
    /// back on a definition an admin rejected. A tool never approved holds
    /// its newest definition in the row, so `restore_version` puts the
    /// rejected one back there; otherwise a superseded definition would pass
    /// for the current one.
    Settle {
        status: McpToolApprovalStatus,
        restore_version: Option<i64>,
    },
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
        && stored.tool.description == discovered.description
        && stored.tool.schema_hash != discovered.schema_hash
    {
        return ToolWrite::CaptureMetadata {
            version: stored.tool.schema_version,
        };
    }
    let current = definition_of(&stored.tool);
    let pending = stored
        .versions
        .iter()
        .find(|version| version.status == McpToolVersionStatus::Pending)
        .map(StoredVersion::key);
    let rejected = stored
        .versions
        .iter()
        .filter(|version| version.status == McpToolVersionStatus::Rejected)
        .map(StoredVersion::key)
        .collect::<Vec<_>>();
    let latest_version = stored.latest_version.max(stored.tool.schema_version);
    let review = StoredToolForReview {
        current: &current,
        schema_version: stored.tool.schema_version,
        approval_status: stored.tool.review.approval_status,
        approved_schema_version: stored.tool.review.approved_schema_version,
        pending: pending.as_ref(),
        pending_version: stored.tool.review.pending_schema_version,
        rejected: &rejected,
        latest_version,
    };
    let write = match decide_discovery(Some(&review), &discovered_def, approve_tool_changes) {
        DiscoveryDecision::Insert { status } => ToolWrite::Insert { status },
        DiscoveryDecision::Unchanged => ToolWrite::Touch,
        DiscoveryDecision::AutoApprove {
            version,
            reuse_version,
        } => ToolWrite::Serve {
            version,
            // A reused version is the row's own or the pending one; both have
            // a version row already.
            insert_version: !reuse_version,
        },
        DiscoveryDecision::Hold { version, status } => ToolWrite::Hold {
            version,
            status,
            write_definition: stored.tool.review.approved_schema_version.is_none(),
            insert_version: true,
        },
        DiscoveryDecision::Revert => ToolWrite::Revert,
        DiscoveryDecision::Settle { status } => ToolWrite::Settle {
            status,
            restore_version: if stored.tool.review.approved_schema_version.is_none() {
                let key = discovered_def.key();
                stored
                    .versions
                    .iter()
                    .find(|version| {
                        version.status == McpToolVersionStatus::Rejected && version.key() == key
                    })
                    .map(|version| version.schema_version)
            } else {
                None
            },
        },
    };
    // A row from before V58 seen unchanged is marked captured, so that later
    // metadata from the upstream counts as a change rather than a capture.
    if write == ToolWrite::Touch
        && !stored.metadata_captured
        && discovered_def.same_definition(&current)
    {
        return ToolWrite::CaptureMetadata {
            version: stored.tool.schema_version,
        };
    }
    write
}

impl ToolWrite {
    /// Whether this write would create a new version number, so discovery
    /// should first look for an older version with the same definition.
    pub(crate) fn allocates_version(&self) -> bool {
        matches!(
            self,
            Self::Serve {
                insert_version: true,
                ..
            } | Self::Hold { .. }
        )
    }
}

/// Reuses an older version with the same definition instead of allocating a
/// new one. A definition an admin approved before is served again; one that
/// was superseded, or that discovery approved itself while approval was off,
/// waits for review under its old number. Discovery reads only
/// pending and rejected versions up front, so older approved and superseded
/// ones are looked up here, only when a new version would be created.
pub(crate) fn reuse_historical_version(
    write: ToolWrite,
    historical: Option<HistoricalVersion>,
) -> ToolWrite {
    let Some(HistoricalVersion {
        schema_version: version,
        status: historical_status,
        reviewed,
    }) = historical
    else {
        return write;
    };
    match write {
        ToolWrite::Serve {
            insert_version: true,
            ..
        } => ToolWrite::Serve {
            version,
            insert_version: false,
        },
        ToolWrite::Hold { .. }
            if historical_status == McpToolVersionStatus::Approved && reviewed =>
        {
            ToolWrite::Serve {
                version,
                insert_version: false,
            }
        }
        ToolWrite::Hold {
            status,
            write_definition,
            ..
        } => ToolWrite::Hold {
            version,
            status,
            write_definition,
            insert_version: false,
        },
        other => other,
    }
}

/// One tool's discovered definition, to look up among its stored versions.
#[derive(Debug, Clone)]
pub(crate) struct HistoricalLookup {
    pub mcp_tool_id: uuid::Uuid,
    pub key: McpToolDefinitionKey,
}

/// The newest stored version with a looked-up definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HistoricalVersion {
    pub schema_version: i64,
    pub status: McpToolVersionStatus,
    /// An admin approved it. Discovery approves versions itself while
    /// approval is off, and those must not skip review once it is on.
    pub reviewed: bool,
}

/// Records `found` for the tool unless a newer match is already recorded.
pub(crate) fn keep_newest(
    by_tool: &mut HashMap<uuid::Uuid, HistoricalVersion>,
    mcp_tool_id: uuid::Uuid,
    found: HistoricalVersion,
) {
    by_tool
        .entry(mcp_tool_id)
        .and_modify(|current| {
            if found.schema_version > current.schema_version {
                *current = found;
            }
        })
        .or_insert(found);
}

/// One discovered tool with its stored row and planned write.
pub(crate) struct PlannedWrite<'a> {
    pub tool: &'a UpsertExternalMcpToolRecord,
    pub existing: Option<&'a StoredTool>,
    pub write: ToolWrite,
}

/// Plans every discovered tool's write against the stored snapshot.
pub(crate) fn plan_tool_writes<'a>(
    stored: &'a HashMap<String, StoredTool>,
    tools: &'a [UpsertExternalMcpToolRecord],
    approve_tool_changes: bool,
) -> Vec<PlannedWrite<'a>> {
    tools
        .iter()
        .map(|tool| {
            let existing = stored.get(&tool.upstream_name);
            PlannedWrite {
                tool,
                existing,
                write: plan_tool_write(existing, tool, approve_tool_changes),
            }
        })
        .collect()
}

/// The tool and definition of every planned write that would create a
/// version, so a batched query can find older versions to reuse.
pub(crate) fn historical_lookups(planned: &[PlannedWrite<'_>]) -> Vec<HistoricalLookup> {
    planned
        .iter()
        .filter(|plan| plan.write.allocates_version())
        .filter_map(|plan| {
            plan.existing.map(|existing| HistoricalLookup {
                mcp_tool_id: existing.tool.mcp_tool_id,
                key: discovered_definition(plan.tool).key(),
            })
        })
        .collect()
}

/// Rewrites planned writes to reuse older versions with the same
/// definition. `historical` holds the newest version with each looked-up
/// definition, by tool.
pub(crate) fn reuse_historical_versions(
    planned: &mut [PlannedWrite<'_>],
    historical: &HashMap<uuid::Uuid, HistoricalVersion>,
) {
    for plan in planned {
        let Some(existing) = plan.existing else {
            continue;
        };
        if !plan.write.allocates_version() {
            continue;
        }
        let found = historical.get(&existing.tool.mcp_tool_id).copied();
        plan.write = reuse_historical_version(plan.write.clone(), found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_versions_are_reused_instead_of_duplicated() {
        let found = |schema_version, status, reviewed| {
            Some(HistoricalVersion {
                schema_version,
                status,
                reviewed,
            })
        };
        let hold = ToolWrite::Hold {
            version: 5,
            status: McpToolApprovalStatus::PendingReview,
            write_definition: false,
            insert_version: true,
        };
        assert_eq!(
            reuse_historical_version(hold.clone(), found(2, McpToolVersionStatus::Approved, true)),
            ToolWrite::Serve {
                version: 2,
                insert_version: false
            },
            "a definition an admin approved before is served again"
        );
        assert_eq!(
            reuse_historical_version(
                hold.clone(),
                found(2, McpToolVersionStatus::Approved, false)
            ),
            ToolWrite::Hold {
                version: 2,
                status: McpToolApprovalStatus::PendingReview,
                write_definition: false,
                insert_version: false,
            },
            "one discovery approved while approval was off waits for review"
        );
        assert_eq!(
            reuse_historical_version(
                hold.clone(),
                found(3, McpToolVersionStatus::Superseded, false)
            ),
            ToolWrite::Hold {
                version: 3,
                status: McpToolApprovalStatus::PendingReview,
                write_definition: false,
                insert_version: false,
            }
        );
        assert_eq!(reuse_historical_version(hold.clone(), None), hold);
        assert_eq!(
            reuse_historical_version(
                ToolWrite::Serve {
                    version: 5,
                    insert_version: true
                },
                found(2, McpToolVersionStatus::Superseded, false)
            ),
            ToolWrite::Serve {
                version: 2,
                insert_version: false
            }
        );
    }
}
