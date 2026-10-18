//! Admin operations on MCP tool policy: tags, annotation overrides, tag rules
//! and reviews of changed tool versions. Tag evaluation lives in
//! `gateway_core::mcp_tool_policy`; this service
//! validates admin input and previews which tools a rule selects.

use std::{collections::HashSet, sync::Arc};

use gateway_core::{
    ExternalMcpServerRecord, ExternalMcpToolRecord, GatewayError, McpAccessRepository,
    McpAnnotationOverrides, McpRegistryRepository, McpTagRuleEffect, McpToolReviewDecision,
    McpToolTagRuleKey, McpToolTagRuleRecord, McpToolVersionQuery, McpToolVersionRecord, StoreError,
    Updated,
    mcp_tool_policy::{
        effective_tags, has_effective_tag, include_rule_selects, normalize_admin_tags,
        normalize_rule_tag,
    },
};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone)]
pub struct McpToolPolicyService<R> {
    repo: Arc<R>,
}

/// Most tools a tag rule preview returns. A preview across every server stops
/// reading servers once it has this many matches.
pub const MAX_TAG_RULE_PREVIEW_MATCHES: usize = 200;
/// Candidate tools read per query while building a preview.
const TAG_RULE_PREVIEW_PAGE_SIZE: u32 = 256;
/// Most candidate tools a preview checks, so a rare tag cannot make one
/// request read the whole registry. Past this the preview is incomplete.
pub const MAX_TAG_RULE_PREVIEW_SCAN: usize = 20 * TAG_RULE_PREVIEW_PAGE_SIZE as usize;

/// The tools a tag rule would select.
#[derive(Debug, Clone, Default)]
pub struct McpTagRulePreview {
    pub matches: Vec<McpTagRuleMatch>,
    /// The preview stopped early: more tools match than
    /// `MAX_TAG_RULE_PREVIEW_MATCHES`, or it checked
    /// `MAX_TAG_RULE_PREVIEW_SCAN` tools before reaching the end. More tools
    /// may match than it lists.
    pub truncated: bool,
}

/// One tool a tag rule would select, with the tags it was matched on.
#[derive(Debug, Clone)]
pub struct McpTagRuleMatch {
    pub tool: ExternalMcpToolRecord,
    pub effective_tags: Vec<String>,
}

impl<R> McpToolPolicyService<R>
where
    R: McpRegistryRepository + McpAccessRepository,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    /// Loads a tool and checks it belongs to the server in the request path.
    pub async fn tool_on_server(
        &self,
        mcp_server_id: Uuid,
        mcp_tool_id: Uuid,
    ) -> Result<ExternalMcpToolRecord, GatewayError> {
        self.repo
            .get_external_mcp_tool(mcp_tool_id)
            .await?
            .filter(|tool| tool.mcp_server_id == mcp_server_id)
            .ok_or_else(|| {
                GatewayError::Store(StoreError::NotFound("MCP tool was not found".to_string()))
            })
    }

    pub async fn set_tags(
        &self,
        mcp_server_id: Uuid,
        mcp_tool_id: Uuid,
        raw_tags: &[String],
    ) -> Result<Updated<ExternalMcpToolRecord>, GatewayError> {
        let tags = normalize_admin_tags(raw_tags).map_err(GatewayError::InvalidRequest)?;
        self.tool_on_server(mcp_server_id, mcp_tool_id).await?;
        Ok(self.repo.set_mcp_tool_tags(mcp_tool_id, &tags).await?)
    }

    pub async fn set_annotation_overrides(
        &self,
        mcp_server_id: Uuid,
        mcp_tool_id: Uuid,
        overrides: &McpAnnotationOverrides,
    ) -> Result<Updated<ExternalMcpToolRecord>, GatewayError> {
        self.tool_on_server(mcp_server_id, mcp_tool_id).await?;
        Ok(self
            .repo
            .set_mcp_tool_annotation_overrides(mcp_tool_id, overrides)
            .await?)
    }

    /// Versions of a tool on the server, newest first: named ones or one
    /// page of history.
    pub async fn list_versions(
        &self,
        mcp_server_id: Uuid,
        mcp_tool_id: Uuid,
        query: &McpToolVersionQuery,
    ) -> Result<Vec<McpToolVersionRecord>, GatewayError> {
        self.tool_on_server(mcp_server_id, mcp_tool_id).await?;
        Ok(self.repo.list_mcp_tool_versions(mcp_tool_id, query).await?)
    }

    /// Approves or rejects the tool's pending version. A version that is no
    /// longer pending, for example because another admin reviewed it or a later
    /// discovery replaced it, is a conflict.
    pub async fn review_version(
        &self,
        mcp_server_id: Uuid,
        mcp_tool_id: Uuid,
        schema_version: i64,
        decision: McpToolReviewDecision,
        reviewer: Option<Uuid>,
    ) -> Result<Updated<ExternalMcpToolRecord>, GatewayError> {
        self.tool_on_server(mcp_server_id, mcp_tool_id).await?;
        Ok(self
            .repo
            .review_mcp_tool_version(
                mcp_tool_id,
                schema_version,
                decision,
                reviewer,
                OffsetDateTime::now_utc(),
            )
            .await?)
    }

    /// Validates and normalizes a rule. A named server must exist.
    pub async fn normalize_rule(
        &self,
        mut key: McpToolTagRuleKey,
    ) -> Result<McpToolTagRuleKey, GatewayError> {
        key.tag = normalize_rule_tag(&key.tag).map_err(GatewayError::InvalidRequest)?;
        key.validate().map_err(GatewayError::InvalidRequest)?;
        if let Some(server_id) = key.mcp_server_id {
            self.rule_server(server_id).await?;
        }
        Ok(key)
    }

    /// The server a rule names. Saving and previewing a rule both reject a
    /// server that does not exist.
    async fn rule_server(&self, server_id: Uuid) -> Result<ExternalMcpServerRecord, GatewayError> {
        self.repo
            .get_external_mcp_server(server_id)
            .await?
            .ok_or_else(|| {
                GatewayError::InvalidRequest("MCP tag rule server was not found".to_string())
            })
    }

    /// Creates the rule. The flag is `true` when this call created it.
    pub async fn create_rule(
        &self,
        key: McpToolTagRuleKey,
    ) -> Result<(McpToolTagRuleRecord, bool), GatewayError> {
        let key = self.normalize_rule(key).await?;
        Ok(self
            .repo
            .upsert_mcp_tool_tag_rule(&key, OffsetDateTime::now_utc())
            .await?)
    }

    /// Like [`Self::create_rule`], written only while the server keeps the
    /// owner and visibility in `expected_server` and, when set, the subject is
    /// still in `expected_subject_team`, checked in the same write.
    pub async fn create_rule_guarded(
        &self,
        key: McpToolTagRuleKey,
        expected_server: &gateway_core::McpServerOwnershipGuard,
        expected_subject_team: Option<Option<Uuid>>,
    ) -> Result<(McpToolTagRuleRecord, bool), GatewayError> {
        let key = self.normalize_rule(key).await?;
        Ok(self
            .repo
            .upsert_mcp_tool_tag_rule_guarded(
                &key,
                OffsetDateTime::now_utc(),
                expected_server,
                expected_subject_team,
            )
            .await?)
    }

    pub async fn revoke_rule(
        &self,
        rule_id: Uuid,
    ) -> Result<Option<McpToolTagRuleRecord>, GatewayError> {
        Ok(self
            .repo
            .revoke_mcp_tool_tag_rule(rule_id, OffsetDateTime::now_utc())
            .await?)
    }

    /// The tools in `page` whose hints only an admin review of their served
    /// version could vouch for, and that have one, in one query. Discovery
    /// approving a version while the server took changes unreviewed does
    /// not count.
    async fn reviewed_candidates(
        &self,
        page: &[ExternalMcpToolRecord],
        tag: &str,
    ) -> Result<HashSet<Uuid>, GatewayError> {
        let candidates = page
            .iter()
            .filter(|tool| {
                !include_rule_selects(tool, tag, false) && include_rule_selects(tool, tag, true)
            })
            .filter_map(|tool| Some((tool.mcp_tool_id, tool.review.approved_schema_version?)))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(HashSet::new());
        }
        Ok(self
            .repo
            .reviewed_mcp_tool_versions(&candidates)
            .await?
            .into_iter()
            .map(|(mcp_tool_id, _)| mcp_tool_id)
            .collect())
    }

    /// The active tools a rule with this tag would select on one server, or on
    /// every active server when `mcp_server_id` is `None`. It applies only the
    /// one rule, so it shows what the rule matches, not a subject's access.
    pub async fn preview_rule(
        &self,
        mcp_server_id: Option<Uuid>,
        raw_tag: &str,
        effect: McpTagRuleEffect,
    ) -> Result<McpTagRulePreview, GatewayError> {
        let tag = normalize_rule_tag(raw_tag).map_err(GatewayError::InvalidRequest)?;
        if effect == McpTagRuleEffect::Include && mcp_server_id.is_none() {
            return Err(GatewayError::InvalidRequest(
                "an include rule must name a server".to_string(),
            ));
        }
        let server = match mcp_server_id {
            Some(server_id) => Some(self.rule_server(server_id).await?),
            None => None,
        };
        // Active tools on active servers, the same candidates runtime
        // resolution reads, read a page at a time until the cap is passed.
        // A disabled server yields nothing.
        let mut matched = Vec::new();
        let mut after: Option<(Uuid, String)> = None;
        let mut scanned = 0;
        let truncated = loop {
            let page = self
                .repo
                .list_active_mcp_tools(
                    mcp_server_id,
                    after
                        .as_ref()
                        .map(|(server, name)| (*server, name.as_str())),
                    TAG_RULE_PREVIEW_PAGE_SIZE,
                )
                .await?;
            let last_page = page.len() < TAG_RULE_PREVIEW_PAGE_SIZE as usize;
            scanned += page.len();
            after = page
                .last()
                .map(|tool| (tool.mcp_server_id, tool.upstream_name.clone()));
            let reviewed = match (&server, effect) {
                (Some(server), McpTagRuleEffect::Include) if server.approve_tool_changes => {
                    self.reviewed_candidates(&page, &tag).await?
                }
                _ => HashSet::new(),
            };
            let mut full = false;
            for tool in page {
                // Include rules apply the same trust rule as runtime resolution.
                let selected = match (&server, effect) {
                    (Some(_), McpTagRuleEffect::Include) => {
                        include_rule_selects(&tool, &tag, reviewed.contains(&tool.mcp_tool_id))
                    }
                    _ => has_effective_tag(&tool, &tag),
                };
                if !selected {
                    continue;
                }
                if matched.len() == MAX_TAG_RULE_PREVIEW_MATCHES {
                    full = true;
                    break;
                }
                matched.push(tool);
            }
            if full {
                break true;
            }
            if last_page {
                break false;
            }
            if scanned >= MAX_TAG_RULE_PREVIEW_SCAN {
                break true;
            }
        };
        // The scan reads only the columns tag rules need; the matches are
        // loaded in full, in one query, for the response.
        let ids = matched
            .iter()
            .map(|tool| tool.mcp_tool_id)
            .collect::<Vec<_>>();
        let mut full_rows = self
            .repo
            .get_external_mcp_tools(&ids)
            .await?
            .into_iter()
            .map(|tool| (tool.mcp_tool_id, tool))
            .collect::<std::collections::HashMap<_, _>>();
        let matches = matched
            .into_iter()
            .map(|tool| {
                let tool = full_rows.remove(&tool.mcp_tool_id).unwrap_or(tool);
                let effective_tags = effective_tags(&tool).into_iter().collect();
                McpTagRuleMatch {
                    tool,
                    effective_tags,
                }
            })
            .collect();
        Ok(McpTagRulePreview { matches, truncated })
    }
}
