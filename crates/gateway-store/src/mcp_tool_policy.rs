//! Row helpers for admin tool policy shared by both store backends.

use gateway_core::{
    McpAnnotationOverrides, McpTagRuleEffect, McpToolGrantSubjectKind, McpToolPolicy,
    McpToolTagRuleRecord, StoreError, mcp_tool_policy::parse_server_scope,
};

use crate::shared::{json_object_from_str, parse_uuid, unix_to_datetime};

/// Decodes `admin_tags_json` and `annotation_overrides_json`.
pub(crate) fn tool_policy_from_json(
    tags_json: &str,
    overrides_json: &str,
) -> Result<McpToolPolicy, StoreError> {
    let tags: Vec<String> = serde_json::from_str(tags_json)
        .map_err(|error| StoreError::Serialization(error.to_string()))?;
    let annotation_overrides =
        McpAnnotationOverrides::from_object(&json_object_from_str(overrides_json)?)
            .map_err(StoreError::Serialization)?;
    Ok(McpToolPolicy {
        tags,
        annotation_overrides,
    })
}

/// Raw columns of one `mcp_tool_tag_rules` row, in `TAG_RULE_COLUMNS` order.
pub(crate) struct TagRuleRow {
    pub rule_id: String,
    pub subject_kind: String,
    pub subject_id: String,
    pub server_scope: String,
    pub tag: String,
    pub effect: String,
    pub is_active: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub revoked_at: Option<i64>,
}

pub(crate) const TAG_RULE_COLUMNS: &str = "rule_id, subject_kind, subject_id, server_scope, tag, effect, is_active, created_at, updated_at, revoked_at";

impl TagRuleRow {
    pub(crate) fn decode(self) -> Result<McpToolTagRuleRecord, StoreError> {
        Ok(McpToolTagRuleRecord {
            rule_id: parse_uuid(&self.rule_id)?,
            subject_kind: McpToolGrantSubjectKind::from_db(&self.subject_kind).ok_or_else(
                || {
                    StoreError::Serialization(format!(
                        "invalid MCP tag rule subject `{}`",
                        self.subject_kind
                    ))
                },
            )?,
            subject_id: parse_uuid(&self.subject_id)?,
            mcp_server_id: parse_server_scope(&self.server_scope)
                .map_err(StoreError::Serialization)?,
            tag: self.tag,
            effect: McpTagRuleEffect::from_db(&self.effect).ok_or_else(|| {
                StoreError::Serialization(format!("invalid MCP tag rule effect `{}`", self.effect))
            })?,
            is_active: self.is_active == 1,
            created_at: unix_to_datetime(self.created_at)?,
            updated_at: unix_to_datetime(self.updated_at)?,
            revoked_at: self.revoked_at.map(unix_to_datetime).transpose()?,
        })
    }
}
