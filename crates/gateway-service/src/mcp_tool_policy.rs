//! Admin operations on MCP tool policy: tags, annotation overrides and tag
//! rules. Evaluation lives in `gateway_core::mcp_tool_policy`; this service
//! validates admin input and previews which tools a rule selects.

use std::sync::Arc;

use gateway_core::{
    ExternalMcpServerRecord, ExternalMcpServerStatus, ExternalMcpToolRecord, GatewayError,
    McpAccessRepository, McpAnnotationOverrides, McpRegistryRepository, McpTagRuleEffect,
    McpToolTagRuleKey, McpToolTagRuleRecord, StoreError, Updated,
    mcp_tool_policy::{effective_tags, normalize_admin_tags, normalize_rule_tag},
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

/// The tools a tag rule would select.
#[derive(Debug, Clone, Default)]
pub struct McpTagRulePreview {
    pub matches: Vec<McpTagRuleMatch>,
    /// More tools match than `MAX_TAG_RULE_PREVIEW_MATCHES`.
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

    pub async fn revoke_rule(
        &self,
        rule_id: Uuid,
    ) -> Result<Option<McpToolTagRuleRecord>, GatewayError> {
        Ok(self
            .repo
            .revoke_mcp_tool_tag_rule(rule_id, OffsetDateTime::now_utc())
            .await?)
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
        let server_ids = match mcp_server_id {
            // A disabled server serves no tools, so its rule matches nothing.
            Some(server_id) => {
                let server = self.rule_server(server_id).await?;
                if server.status == ExternalMcpServerStatus::Active {
                    vec![server_id]
                } else {
                    Vec::new()
                }
            }
            None => self
                .repo
                .list_external_mcp_servers(false)
                .await?
                .into_iter()
                .map(|server| server.mcp_server_id)
                .collect(),
        };
        let mut preview = McpTagRulePreview::default();
        for server_id in server_ids {
            for tool in self.repo.list_external_mcp_tools(server_id, false).await? {
                let tags = effective_tags(&tool);
                if !tags.contains(&tag) {
                    continue;
                }
                if preview.matches.len() == MAX_TAG_RULE_PREVIEW_MATCHES {
                    preview.truncated = true;
                    return Ok(preview);
                }
                preview.matches.push(McpTagRuleMatch {
                    tool,
                    effective_tags: tags.into_iter().collect(),
                });
            }
        }
        Ok(preview)
    }
}
