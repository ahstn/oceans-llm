//! Discovery planning and version writes for tools, shared by both store
//! backends.
//!
//! Each backend loads the server's tools and versions, asks `plan_tool_write`
//! what one discovered definition means, turns the result into `VersionOp`s
//! with `tool_write_ops` (or `review_ops` for an admin review), and runs each
//! operation as SQL. The sequence of statements is decided only here.

use std::collections::HashMap;

use gateway_core::{
    ExternalMcpToolRecord, McpToolApprovalStatus, McpToolDefinition, McpToolDefinitionKey,
    McpToolReviewDecision, McpToolVersionRecord, McpToolVersionStatus, StoreError,
    UpsertExternalMcpToolRecord,
    mcp_tool_review::{
        DiscoveryDecision, StoredToolForReview, decide_discovery, status_after_rejection,
    },
};
use time::OffsetDateTime;
use uuid::Uuid;

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

/// Refuses a guarded discovery commit whose results are stale: the server was
/// disabled while discovery ran, or a discovery that started later (to the
/// millisecond) has already committed. Committing either would serve tools
/// for a server an admin turned off, or roll versions back. The ownership
/// guard covers owner, URL and auth changes.
pub(crate) fn stale_discovery_check(
    server_active: bool,
    newer_run_committed: bool,
) -> Result<(), StoreError> {
    if !server_active {
        return Err(StoreError::Conflict(
            "the MCP server was disabled while discovery ran; its results were discarded"
                .to_string(),
        ));
    }
    if newer_run_committed {
        return Err(StoreError::Conflict(
            "a newer discovery of this MCP server has already been recorded".to_string(),
        ));
    }
    Ok(())
}

/// A discovery start in Unix milliseconds, which orders runs that start in
/// the same second.
pub(crate) fn unix_millis(at: OffsetDateTime) -> i64 {
    (at.unix_timestamp_nanos() / 1_000_000) as i64
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
    // Rows from before V58 kept no metadata. Without approval, capturing it
    // is not a change. With approval the captured claims were never reviewed,
    // and a waiting version must still be resolved, so both go through the
    // review decision below.
    let nothing_waiting = stored.tool.review.approval_status == McpToolApprovalStatus::Approved
        && stored.tool.review.pending_schema_version.is_none();
    if !approve_tool_changes
        && nothing_waiting
        && !stored.metadata_captured
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
    pub mcp_tool_id: Uuid,
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
    by_tool: &mut HashMap<Uuid, HistoricalVersion>,
    mcp_tool_id: Uuid,
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
    historical: &HashMap<Uuid, HistoricalVersion>,
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

/// One statement of a tool version write. Each backend runs these as SQL.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum VersionOp<'a> {
    /// Write the definition into the tool row as `version`, which also marks
    /// the row's metadata as captured.
    WriteDefinition {
        definition: &'a McpToolDefinition,
        version: i64,
    },
    /// Replace the stored definition of an existing version row.
    RewriteVersion {
        definition: &'a McpToolDefinition,
        version: i64,
    },
    InsertVersion {
        definition: &'a McpToolDefinition,
        version: i64,
        status: McpToolVersionStatus,
        discovered_at: OffsetDateTime,
    },
    /// Set a version's status and who reviewed it; `None` clears either.
    SetVersionStatus {
        version: i64,
        status: McpToolVersionStatus,
        reviewed_at: Option<OffsetDateTime>,
        reviewed_by_user_id: Option<Uuid>,
    },
    /// Approve a version because the server does not require review. A
    /// version an admin already approved keeps its reviewer; one an admin
    /// rejected loses the rejection's attribution.
    AutoApproveVersion { version: i64 },
    /// Mark pending versions other than `keep` superseded; `None` supersedes
    /// every pending version.
    SupersedePending { keep: Option<i64> },
    /// Set the tool's approval status and waiting version. `approved_version`
    /// replaces the served version when set and keeps it otherwise.
    SetToolReview {
        status: McpToolApprovalStatus,
        approved_version: Option<i64>,
        pending_version: Option<i64>,
    },
}

/// The statements for a planned write on an existing tool, in order. A new
/// tool's insert is backend-specific and not described here.
pub(crate) fn tool_write_ops<'a>(
    write: &ToolWrite,
    definition: &'a McpToolDefinition,
    discovered_at: OffsetDateTime,
) -> Vec<VersionOp<'a>> {
    match *write {
        ToolWrite::Insert { .. } | ToolWrite::Touch => Vec::new(),
        ToolWrite::CaptureMetadata { version } => vec![
            VersionOp::WriteDefinition {
                definition,
                version,
            },
            VersionOp::RewriteVersion {
                definition,
                version,
            },
        ],
        ToolWrite::Serve {
            version,
            insert_version,
        } => vec![
            VersionOp::WriteDefinition {
                definition,
                version,
            },
            VersionOp::SetToolReview {
                status: McpToolApprovalStatus::Approved,
                approved_version: Some(version),
                pending_version: None,
            },
            VersionOp::SupersedePending {
                keep: Some(version),
            },
            if insert_version {
                VersionOp::InsertVersion {
                    definition,
                    version,
                    status: McpToolVersionStatus::Approved,
                    discovered_at,
                }
            } else {
                VersionOp::AutoApproveVersion { version }
            },
        ],
        ToolWrite::Hold {
            version,
            status,
            write_definition,
            insert_version,
        } => {
            let mut ops = vec![
                VersionOp::SupersedePending {
                    keep: Some(version),
                },
                if insert_version {
                    VersionOp::InsertVersion {
                        definition,
                        version,
                        status: McpToolVersionStatus::Pending,
                        discovered_at,
                    }
                } else {
                    VersionOp::SetVersionStatus {
                        version,
                        status: McpToolVersionStatus::Pending,
                        reviewed_at: None,
                        reviewed_by_user_id: None,
                    }
                },
            ];
            if write_definition {
                ops.push(VersionOp::WriteDefinition {
                    definition,
                    version,
                });
            }
            ops.push(VersionOp::SetToolReview {
                status,
                approved_version: None,
                pending_version: Some(version),
            });
            ops
        }
        ToolWrite::Settle {
            status,
            restore_version,
        } => {
            let mut ops = vec![VersionOp::SupersedePending { keep: None }];
            if let Some(version) = restore_version {
                ops.push(VersionOp::WriteDefinition {
                    definition,
                    version,
                });
            }
            ops.push(VersionOp::SetToolReview {
                status,
                approved_version: None,
                pending_version: None,
            });
            ops
        }
        ToolWrite::Revert => vec![
            VersionOp::SupersedePending { keep: None },
            VersionOp::SetToolReview {
                status: McpToolApprovalStatus::Approved,
                approved_version: None,
                pending_version: None,
            },
        ],
    }
}

/// The version an admin review applies to. Only a pending version can be
/// reviewed, so a second review of the same version is a conflict.
pub(crate) fn reviewable_version(
    mcp_tool_id: Uuid,
    schema_version: i64,
    version: Option<McpToolVersionRecord>,
) -> Result<McpToolVersionRecord, StoreError> {
    let version = version.ok_or_else(|| {
        StoreError::NotFound(format!(
            "MCP tool `{mcp_tool_id}` has no version {schema_version}"
        ))
    })?;
    if version.status != McpToolVersionStatus::Pending {
        return Err(StoreError::Conflict(format!(
            "version {schema_version} of MCP tool `{mcp_tool_id}` is {}, not pending",
            version.status.as_str()
        )));
    }
    Ok(version)
}

/// The statements for an admin's review of a pending version, in order.
pub(crate) fn review_ops<'a>(
    decision: McpToolReviewDecision,
    tool: &ExternalMcpToolRecord,
    version: &'a McpToolVersionRecord,
    reviewed_at: OffsetDateTime,
    reviewed_by_user_id: Option<Uuid>,
) -> Vec<VersionOp<'a>> {
    let schema_version = version.schema_version;
    match decision {
        McpToolReviewDecision::Approve => vec![
            VersionOp::WriteDefinition {
                definition: &version.definition,
                version: schema_version,
            },
            VersionOp::SetToolReview {
                status: McpToolApprovalStatus::Approved,
                approved_version: Some(schema_version),
                pending_version: None,
            },
            VersionOp::SetVersionStatus {
                version: schema_version,
                status: McpToolVersionStatus::Approved,
                reviewed_at: Some(reviewed_at),
                reviewed_by_user_id,
            },
            VersionOp::SupersedePending {
                keep: Some(schema_version),
            },
        ],
        McpToolReviewDecision::Reject => vec![
            VersionOp::SetToolReview {
                status: status_after_rejection(
                    tool.review.approval_status,
                    tool.review.approved_schema_version,
                ),
                approved_version: None,
                pending_version: None,
            },
            VersionOp::SetVersionStatus {
                version: schema_version,
                status: McpToolVersionStatus::Rejected,
                reviewed_at: Some(reviewed_at),
                reviewed_by_user_id,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> McpToolDefinition {
        McpToolDefinition {
            description: Some("Search".to_string()),
            input_schema: serde_json::json!({"type": "object"}),
            title: None,
            annotations: None,
            output_schema: None,
            schema_hash: "h".to_string(),
        }
    }

    #[test]
    fn every_write_runs_the_statements_both_backends_share() {
        let definition = definition();
        let at = OffsetDateTime::UNIX_EPOCH;
        let ops = |write: ToolWrite| tool_write_ops(&write, &definition, at);
        assert!(ops(ToolWrite::Touch).is_empty());
        assert!(
            ops(ToolWrite::Insert {
                status: McpToolApprovalStatus::Approved
            })
            .is_empty()
        );
        assert_eq!(
            ops(ToolWrite::CaptureMetadata { version: 2 }),
            vec![
                VersionOp::WriteDefinition {
                    definition: &definition,
                    version: 2
                },
                VersionOp::RewriteVersion {
                    definition: &definition,
                    version: 2
                },
            ]
        );
        assert_eq!(
            ops(ToolWrite::Serve {
                version: 3,
                insert_version: true
            }),
            vec![
                VersionOp::WriteDefinition {
                    definition: &definition,
                    version: 3
                },
                VersionOp::SetToolReview {
                    status: McpToolApprovalStatus::Approved,
                    approved_version: Some(3),
                    pending_version: None
                },
                VersionOp::SupersedePending { keep: Some(3) },
                VersionOp::InsertVersion {
                    definition: &definition,
                    version: 3,
                    status: McpToolVersionStatus::Approved,
                    discovered_at: at
                },
            ]
        );
        assert_eq!(
            ops(ToolWrite::Serve {
                version: 1,
                insert_version: false
            })
            .last(),
            Some(&VersionOp::AutoApproveVersion { version: 1 })
        );
        assert_eq!(
            ops(ToolWrite::Hold {
                version: 4,
                status: McpToolApprovalStatus::PendingReview,
                write_definition: true,
                insert_version: false
            }),
            vec![
                VersionOp::SupersedePending { keep: Some(4) },
                VersionOp::SetVersionStatus {
                    version: 4,
                    status: McpToolVersionStatus::Pending,
                    reviewed_at: None,
                    reviewed_by_user_id: None
                },
                VersionOp::WriteDefinition {
                    definition: &definition,
                    version: 4
                },
                VersionOp::SetToolReview {
                    status: McpToolApprovalStatus::PendingReview,
                    approved_version: None,
                    pending_version: Some(4)
                },
            ]
        );
        assert_eq!(
            ops(ToolWrite::Settle {
                status: McpToolApprovalStatus::Rejected,
                restore_version: Some(1)
            }),
            vec![
                VersionOp::SupersedePending { keep: None },
                VersionOp::WriteDefinition {
                    definition: &definition,
                    version: 1
                },
                VersionOp::SetToolReview {
                    status: McpToolApprovalStatus::Rejected,
                    approved_version: None,
                    pending_version: None
                },
            ]
        );
        assert_eq!(
            ops(ToolWrite::Revert),
            vec![
                VersionOp::SupersedePending { keep: None },
                VersionOp::SetToolReview {
                    status: McpToolApprovalStatus::Approved,
                    approved_version: None,
                    pending_version: None
                },
            ]
        );
    }

    #[test]
    fn reviews_apply_only_to_pending_versions() {
        let tool_id = Uuid::new_v4();
        let version = |status| McpToolVersionRecord {
            mcp_tool_id: tool_id,
            schema_version: 2,
            definition: definition(),
            status,
            discovered_at: OffsetDateTime::UNIX_EPOCH,
            reviewed_at: None,
            reviewed_by_user_id: None,
        };
        assert!(matches!(
            reviewable_version(tool_id, 2, None),
            Err(StoreError::NotFound(_))
        ));
        assert!(matches!(
            reviewable_version(tool_id, 2, Some(version(McpToolVersionStatus::Approved))),
            Err(StoreError::Conflict(_))
        ));
        let pending = reviewable_version(tool_id, 2, Some(version(McpToolVersionStatus::Pending)))
            .expect("pending");
        let reviewer = Some(Uuid::new_v4());
        let at = OffsetDateTime::UNIX_EPOCH;
        let mut tool = ExternalMcpToolRecord {
            mcp_tool_id: tool_id,
            mcp_server_id: Uuid::new_v4(),
            upstream_name: "search".to_string(),
            display_name: "search".to_string(),
            description: None,
            input_schema: serde_json::json!({}),
            metadata: Default::default(),
            policy: Default::default(),
            review: Default::default(),
            schema_hash: "h0".to_string(),
            schema_version: 1,
            is_active: true,
            first_discovered_at: at,
            last_discovered_at: at,
            deactivated_at: None,
        };
        tool.review.approval_status = McpToolApprovalStatus::PendingChange;
        tool.review.approved_schema_version = Some(1);
        let approve = review_ops(
            McpToolReviewDecision::Approve,
            &tool,
            &pending,
            at,
            reviewer,
        );
        assert_eq!(
            approve[1],
            VersionOp::SetToolReview {
                status: McpToolApprovalStatus::Approved,
                approved_version: Some(2),
                pending_version: None
            }
        );
        assert_eq!(approve[3], VersionOp::SupersedePending { keep: Some(2) });
        let reject = review_ops(McpToolReviewDecision::Reject, &tool, &pending, at, reviewer);
        assert_eq!(
            reject,
            vec![
                VersionOp::SetToolReview {
                    status: McpToolApprovalStatus::Approved,
                    approved_version: None,
                    pending_version: None
                },
                VersionOp::SetVersionStatus {
                    version: 2,
                    status: McpToolVersionStatus::Rejected,
                    reviewed_at: Some(at),
                    reviewed_by_user_id: reviewer
                },
            ],
            "rejecting a text-only change keeps serving the approved version"
        );
    }

    /// A tool row from before V58: approved at version 1, metadata never
    /// stored. `pending` adds a waiting text change at version 2.
    fn legacy_tool(pending: bool) -> StoredTool {
        let at = OffsetDateTime::UNIX_EPOCH;
        let mut tool = ExternalMcpToolRecord {
            mcp_tool_id: Uuid::new_v4(),
            mcp_server_id: Uuid::new_v4(),
            upstream_name: "search".to_string(),
            display_name: "search".to_string(),
            description: Some("Search".to_string()),
            input_schema: serde_json::json!({"type": "object"}),
            metadata: Default::default(),
            policy: Default::default(),
            review: Default::default(),
            schema_hash: "legacy".to_string(),
            schema_version: 1,
            is_active: true,
            first_discovered_at: at,
            last_discovered_at: at,
            deactivated_at: None,
        };
        tool.review.approved_schema_version = Some(1);
        let mut versions = Vec::new();
        if pending {
            tool.review.approval_status = McpToolApprovalStatus::PendingChange;
            tool.review.pending_schema_version = Some(2);
            versions.push(StoredVersion {
                schema_version: 2,
                schema_hash: "legacy".to_string(),
                description: Some("Search, now with an injected note".to_string()),
                title: None,
                status: McpToolVersionStatus::Pending,
            });
        }
        StoredTool {
            tool,
            metadata_captured: false,
            latest_version: if pending { 2 } else { 1 },
            versions,
        }
    }

    /// The same input schema and description, now with metadata the upstream
    /// claims, such as `readOnlyHint`.
    fn discovered_with_metadata(stored: &StoredTool) -> UpsertExternalMcpToolRecord {
        UpsertExternalMcpToolRecord {
            mcp_server_id: stored.tool.mcp_server_id,
            upstream_name: "search".to_string(),
            display_name: "search".to_string(),
            description: stored.tool.description.clone(),
            input_schema: stored.tool.input_schema.clone(),
            metadata: gateway_core::McpToolMetadata {
                title: Some("Search".to_string()),
                annotations: serde_json::json!({"readOnlyHint": true})
                    .as_object()
                    .cloned(),
                output_schema: None,
            },
            schema_hash: "with-metadata".to_string(),
        }
    }

    #[test]
    fn captured_metadata_on_legacy_rows_is_reviewed_when_approval_is_on() {
        let stored = legacy_tool(false);
        let discovered = discovered_with_metadata(&stored);
        // Without approval, metadata an older gateway never stored is captured
        // into the served version, as it always was.
        assert_eq!(
            plan_tool_write(Some(&stored), &discovered, false),
            ToolWrite::CaptureMetadata { version: 1 }
        );
        // With approval, the claims were never reviewed: they wait as a new
        // version, and the approved definition keeps being served.
        assert_eq!(
            plan_tool_write(Some(&stored), &discovered, true),
            ToolWrite::Hold {
                version: 2,
                status: McpToolApprovalStatus::PendingReview,
                write_definition: false,
                insert_version: true,
            }
        );
    }

    #[test]
    fn captured_metadata_does_not_leave_a_stale_pending_version() {
        let stored = legacy_tool(true);
        let discovered = discovered_with_metadata(&stored);
        // The upstream dropped the waiting text change. Without approval the
        // discovered definition is served and the pending version superseded.
        assert_eq!(
            plan_tool_write(Some(&stored), &discovered, false),
            ToolWrite::Serve {
                version: 3,
                insert_version: true,
            }
        );
        // With approval it replaces the pending version for review.
        assert!(matches!(
            plan_tool_write(Some(&stored), &discovered, true),
            ToolWrite::Hold { version: 3, .. }
        ));
    }

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
