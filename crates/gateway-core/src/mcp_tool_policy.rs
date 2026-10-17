//! Admin-owned MCP tool policy: annotation overrides, tags and tag rules.
//!
//! Discovery stores what the upstream server declares. Admins can override the
//! four behaviour hints and attach their own tags. Tag rules then grant or deny
//! tools by tag. Every surface evaluates tags through this module, so the
//! store, the gateway and the admin preview cannot disagree. See
//! `docs/adr/2026-10-17-mcp-tag-grants-and-tool-versioning.md`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::domain::{ExternalMcpToolRecord, McpGrantSubject, McpToolGrantSubjectKind};

pub const READ_ONLY_HINT: &str = "readOnlyHint";
pub const DESTRUCTIVE_HINT: &str = "destructiveHint";
pub const IDEMPOTENT_HINT: &str = "idempotentHint";
pub const OPEN_WORLD_HINT: &str = "openWorldHint";
/// The protocol hints an admin may override, in display order.
pub const OVERRIDABLE_HINTS: [&str; 4] = [
    READ_ONLY_HINT,
    DESTRUCTIVE_HINT,
    IDEMPOTENT_HINT,
    OPEN_WORLD_HINT,
];

pub const TAG_READ_ONLY: &str = "read_only";
pub const TAG_DESTRUCTIVE: &str = "destructive";
pub const TAG_IDEMPOTENT: &str = "idempotent";
pub const TAG_OPEN_WORLD: &str = "open_world";
/// Tags derived from effective hints. Admin tags cannot use these names.
pub const DERIVED_TAGS: [&str; 4] = [
    TAG_READ_ONLY,
    TAG_DESTRUCTIVE,
    TAG_IDEMPOTENT,
    TAG_OPEN_WORLD,
];

pub const MAX_TAG_LEN: usize = 64;
pub const MAX_TAGS_PER_TOOL: usize = 32;

/// Admin overrides for the protocol hints. `None` keeps the discovered value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpAnnotationOverrides {
    #[serde(
        rename = "readOnlyHint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub read_only: Option<bool>,
    #[serde(
        rename = "destructiveHint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub destructive: Option<bool>,
    #[serde(
        rename = "idempotentHint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub idempotent: Option<bool>,
    #[serde(
        rename = "openWorldHint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub open_world: Option<bool>,
}

impl McpAnnotationOverrides {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    #[must_use]
    pub fn get(&self, hint: &str) -> Option<bool> {
        match hint {
            READ_ONLY_HINT => self.read_only,
            DESTRUCTIVE_HINT => self.destructive,
            IDEMPOTENT_HINT => self.idempotent,
            OPEN_WORLD_HINT => self.open_world,
            _ => None,
        }
    }

    /// Parses a stored or requested override object. Unknown members and
    /// non-boolean values are rejected, so a typo cannot silently do nothing.
    pub fn from_object(object: &Map<String, Value>) -> Result<Self, String> {
        let mut overrides = Self::default();
        for (name, value) in object {
            let value = match value {
                Value::Bool(value) => Some(*value),
                Value::Null => None,
                _ => return Err(format!("annotation override `{name}` must be a boolean")),
            };
            match name.as_str() {
                READ_ONLY_HINT => overrides.read_only = value,
                DESTRUCTIVE_HINT => overrides.destructive = value,
                IDEMPOTENT_HINT => overrides.idempotent = value,
                OPEN_WORLD_HINT => overrides.open_world = value,
                _ => {
                    return Err(format!(
                        "`{name}` cannot be overridden; use one of {}",
                        OVERRIDABLE_HINTS.join(", ")
                    ));
                }
            }
        }
        Ok(overrides)
    }

    #[must_use]
    pub fn to_object(&self) -> Map<String, Value> {
        OVERRIDABLE_HINTS
            .iter()
            .filter_map(|hint| {
                self.get(hint)
                    .map(|value| (hint.to_string(), Value::Bool(value)))
            })
            .collect()
    }
}

/// Policy an admin keeps on a tool. Discovery never writes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolPolicy {
    /// Admin tags, normalized and sorted.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub annotation_overrides: McpAnnotationOverrides,
}

/// The hints a tool is treated as having: the admin override where one is set,
/// otherwise the discovered value. `None` means neither declared it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveHints {
    pub read_only: Option<bool>,
    pub destructive: Option<bool>,
    pub idempotent: Option<bool>,
    pub open_world: Option<bool>,
}

impl EffectiveHints {
    #[must_use]
    pub fn of(tool: &ExternalMcpToolRecord) -> Self {
        let overrides = &tool.policy.annotation_overrides;
        let pick = |hint: &str| overrides.get(hint).or_else(|| tool.metadata.hint(hint));
        Self {
            read_only: pick(READ_ONLY_HINT),
            destructive: pick(DESTRUCTIVE_HINT),
            idempotent: pick(IDEMPOTENT_HINT),
            open_world: pick(OPEN_WORLD_HINT),
        }
    }

    /// Derived tags under the protocol defaults. The defaults fail safe: an
    /// include rule needs a positive claim, and a tool that declares nothing is
    /// treated as destructive and open-world.
    #[must_use]
    pub fn derived_tags(&self) -> Vec<&'static str> {
        self.derived()
            .into_iter()
            .filter_map(|(tag, derived)| derived.then_some(tag))
            .collect()
    }

    /// Whether the hints derive `tag`, without building the tag list.
    #[must_use]
    pub fn derives(&self, tag: &str) -> bool {
        self.derived()
            .into_iter()
            .any(|(name, derived)| derived && name == tag)
    }

    fn derived(&self) -> [(&'static str, bool); 4] {
        let read_only = self.read_only.unwrap_or(false);
        [
            (TAG_READ_ONLY, read_only),
            (
                TAG_DESTRUCTIVE,
                !read_only && self.destructive.unwrap_or(true),
            ),
            (TAG_IDEMPOTENT, self.idempotent.unwrap_or(false)),
            (TAG_OPEN_WORLD, self.open_world.unwrap_or(true)),
        ]
    }
}

/// Whether `tag` is one of the tool's effective tags. Rule evaluation uses
/// this on every resolved tool, so it builds no tag set.
#[must_use]
pub fn has_effective_tag(tool: &ExternalMcpToolRecord, tag: &str) -> bool {
    tool.policy.tags.iter().any(|admin_tag| admin_tag == tag)
        || EffectiveHints::of(tool).derives(tag)
}

/// Derived tags plus admin tags, sorted.
#[must_use]
pub fn effective_tags(tool: &ExternalMcpToolRecord) -> BTreeSet<String> {
    let mut tags: BTreeSet<String> = EffectiveHints::of(tool)
        .derived_tags()
        .into_iter()
        .map(str::to_string)
        .collect();
    tags.extend(tool.policy.tags.iter().cloned());
    tags
}

/// The discovered annotations with the admin overrides applied. Returns `None`
/// only when neither declares anything.
#[must_use]
pub fn effective_annotations(tool: &ExternalMcpToolRecord) -> Option<Map<String, Value>> {
    let overrides = tool.policy.annotation_overrides.to_object();
    if overrides.is_empty() {
        return tool.metadata.annotations.clone();
    }
    let mut annotations = tool.metadata.annotations.clone().unwrap_or_default();
    annotations.extend(overrides);
    Some(annotations)
}

/// Normalizes one tag: trimmed and lowercased. Returns an error for an empty,
/// too long, reserved or malformed tag.
pub fn normalize_admin_tag(raw: &str) -> Result<String, String> {
    let tag = raw.trim().to_ascii_lowercase();
    validate_tag(&tag)?;
    if DERIVED_TAGS.contains(&tag.as_str()) {
        return Err(format!(
            "`{tag}` is derived from the tool's annotations; override the annotation instead"
        ));
    }
    Ok(tag)
}

/// Normalizes a tool's admin tags: deduplicated and sorted.
pub fn normalize_admin_tags(raw: &[String]) -> Result<Vec<String>, String> {
    let tags = raw
        .iter()
        .map(|tag| normalize_admin_tag(tag))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if tags.len() > MAX_TAGS_PER_TOOL {
        return Err(format!(
            "a tool can have at most {MAX_TAGS_PER_TOOL} admin tags"
        ));
    }
    Ok(tags.into_iter().collect())
}

/// Normalizes the tag a rule selects. Derived tags are allowed here.
pub fn normalize_rule_tag(raw: &str) -> Result<String, String> {
    let tag = raw.trim().to_ascii_lowercase();
    validate_tag(&tag)?;
    Ok(tag)
}

fn validate_tag(tag: &str) -> Result<(), String> {
    if tag.is_empty() || tag.len() > MAX_TAG_LEN {
        return Err(format!("tags must be 1 to {MAX_TAG_LEN} characters"));
    }
    if !tag.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '_' | '-' | '.' | ':')
    }) {
        return Err(format!(
            "tag `{tag}` may only use lowercase letters, digits, `_`, `-`, `.` and `:`"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTagRuleEffect {
    Include,
    Exclude,
}

impl McpTagRuleEffect {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Include => "include",
            Self::Exclude => "exclude",
        }
    }

    #[must_use]
    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "include" => Some(Self::Include),
            "exclude" => Some(Self::Exclude),
            _ => None,
        }
    }
}

/// Stored value of `server_scope` for a rule that applies to every server.
pub const ALL_SERVERS_SCOPE: &str = "*";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolTagRuleRecord {
    pub rule_id: Uuid,
    pub subject_kind: McpToolGrantSubjectKind,
    pub subject_id: Uuid,
    /// The server the rule applies to; `None` for every server (exclude only).
    pub mcp_server_id: Option<Uuid>,
    pub tag: String,
    pub effect: McpTagRuleEffect,
    pub is_active: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub revoked_at: Option<OffsetDateTime>,
}

impl McpToolTagRuleRecord {
    #[must_use]
    pub fn matches(&self, tool: &ExternalMcpToolRecord) -> bool {
        self.mcp_server_id
            .is_none_or(|server_id| server_id == tool.mcp_server_id)
            && has_effective_tag(tool, &self.tag)
    }
}

/// Identifies one tag rule; at most one active rule exists per key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolTagRuleKey {
    pub subject_kind: McpToolGrantSubjectKind,
    pub subject_id: Uuid,
    pub mcp_server_id: Option<Uuid>,
    pub tag: String,
    pub effect: McpTagRuleEffect,
}

impl McpToolTagRuleKey {
    /// Checks the rule's shape: an include rule must name a server, because one
    /// across every server would also grant every tool on servers added later.
    pub fn validate(&self) -> Result<(), String> {
        if self.effect == McpTagRuleEffect::Include && self.mcp_server_id.is_none() {
            return Err("an include rule must name a server".to_string());
        }
        Ok(())
    }

    #[must_use]
    pub fn server_scope(&self) -> String {
        server_scope(self.mcp_server_id)
    }

    #[must_use]
    pub fn subject(&self) -> McpGrantSubject {
        McpGrantSubject {
            subject_kind: self.subject_kind,
            subject_id: self.subject_id,
        }
    }
}

#[must_use]
pub fn server_scope(mcp_server_id: Option<Uuid>) -> String {
    mcp_server_id.map_or_else(|| ALL_SERVERS_SCOPE.to_string(), |id| id.to_string())
}

/// Parses a stored `server_scope`.
pub fn parse_server_scope(value: &str) -> Result<Option<Uuid>, String> {
    if value == ALL_SERVERS_SCOPE {
        return Ok(None);
    }
    Uuid::parse_str(value)
        .map(Some)
        .map_err(|_| format!("invalid MCP tag rule server scope `{value}`"))
}

/// The active tag rules of every subject of one caller.
#[derive(Debug, Clone, Default)]
pub struct McpTagRuleSet {
    includes: Vec<McpToolTagRuleRecord>,
    excludes: Vec<McpToolTagRuleRecord>,
}

impl McpTagRuleSet {
    #[must_use]
    pub fn new(rules: impl IntoIterator<Item = McpToolTagRuleRecord>) -> Self {
        let (includes, excludes) = rules
            .into_iter()
            .filter(|rule| rule.is_active)
            .partition(|rule| rule.effect == McpTagRuleEffect::Include);
        Self { includes, excludes }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.includes.is_empty() && self.excludes.is_empty()
    }

    /// Servers whose tools an include rule may add, limited to `scope` when set.
    #[must_use]
    pub fn include_server_ids(&self, scope: Option<Uuid>) -> BTreeSet<Uuid> {
        self.includes
            .iter()
            .filter_map(|rule| rule.mcp_server_id)
            .filter(|server_id| scope.is_none_or(|scope| scope == *server_id))
            .collect()
    }

    #[must_use]
    pub fn includes(&self, tool: &ExternalMcpToolRecord) -> bool {
        self.includes.iter().any(|rule| rule.matches(tool))
    }

    /// Whether any exclude rule removes the tool. Exclude wins over every
    /// grant, whichever of the caller's subjects it came from.
    #[must_use]
    pub fn excludes(&self, tool: &ExternalMcpToolRecord) -> bool {
        self.excludes.iter().any(|rule| rule.matches(tool))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::McpToolMetadata;
    use serde_json::json;

    fn tool(server: Uuid, annotations: Value, policy: McpToolPolicy) -> ExternalMcpToolRecord {
        ExternalMcpToolRecord {
            mcp_tool_id: Uuid::new_v4(),
            mcp_server_id: server,
            upstream_name: "tool".to_string(),
            display_name: "tool".to_string(),
            description: None,
            input_schema: json!({"type": "object"}),
            metadata: McpToolMetadata {
                title: None,
                annotations: annotations.as_object().cloned(),
                output_schema: None,
            },
            policy,
            schema_hash: "sha256:x".to_string(),
            schema_version: 1,
            is_active: true,
            first_discovered_at: OffsetDateTime::UNIX_EPOCH,
            last_discovered_at: OffsetDateTime::UNIX_EPOCH,
            deactivated_at: None,
            review: Default::default(),
        }
    }

    fn rule(server: Option<Uuid>, tag: &str, effect: McpTagRuleEffect) -> McpToolTagRuleRecord {
        McpToolTagRuleRecord {
            rule_id: Uuid::new_v4(),
            subject_kind: McpToolGrantSubjectKind::Team,
            subject_id: Uuid::new_v4(),
            mcp_server_id: server,
            tag: tag.to_string(),
            effect,
            is_active: true,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            revoked_at: None,
        }
    }

    #[test]
    fn undeclared_hints_fail_safe() {
        let tool = tool(Uuid::new_v4(), Value::Null, McpToolPolicy::default());
        let tags = effective_tags(&tool);
        assert!(tags.contains(TAG_DESTRUCTIVE));
        assert!(tags.contains(TAG_OPEN_WORLD));
        assert!(!tags.contains(TAG_READ_ONLY));
        assert!(!tags.contains(TAG_IDEMPOTENT));
    }

    #[test]
    fn read_only_tools_are_never_destructive() {
        let tool = tool(
            Uuid::new_v4(),
            json!({"readOnlyHint": true, "destructiveHint": true, "openWorldHint": false}),
            McpToolPolicy::default(),
        );
        let tags = effective_tags(&tool);
        assert_eq!(
            tags.into_iter().collect::<Vec<_>>(),
            vec![TAG_READ_ONLY.to_string()]
        );
    }

    #[test]
    fn overrides_take_precedence_over_discovered_hints() {
        let policy = McpToolPolicy {
            tags: vec!["finance".to_string()],
            annotation_overrides: McpAnnotationOverrides {
                read_only: Some(false),
                ..McpAnnotationOverrides::default()
            },
        };
        let tool = tool(Uuid::new_v4(), json!({"readOnlyHint": true}), policy);
        let tags = effective_tags(&tool);
        assert!(!tags.contains(TAG_READ_ONLY));
        assert!(tags.contains(TAG_DESTRUCTIVE));
        assert!(tags.contains("finance"));
        let annotations = effective_annotations(&tool).expect("annotations");
        assert_eq!(annotations["readOnlyHint"], json!(false));
        // The discovered value is kept on the record for display.
        assert_eq!(tool.metadata.hint(READ_ONLY_HINT), Some(true));
    }

    #[test]
    fn tag_checks_agree_with_the_effective_tag_set() {
        let server = Uuid::new_v4();
        let mut overridden = McpToolPolicy {
            tags: vec!["finance".to_string()],
            ..McpToolPolicy::default()
        };
        overridden.annotation_overrides.read_only = Some(true);
        for tool in [
            tool(server, json!({}), McpToolPolicy::default()),
            tool(
                server,
                json!({"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}),
                McpToolPolicy::default(),
            ),
            tool(server, json!({"destructiveHint": false}), overridden),
        ] {
            let tags = effective_tags(&tool);
            for tag in DERIVED_TAGS.iter().copied().chain(["finance", "other"]) {
                assert_eq!(has_effective_tag(&tool, tag), tags.contains(tag), "{tag}");
            }
        }
    }

    #[test]
    fn exclude_wins_over_include() {
        let server = Uuid::new_v4();
        let tool = tool(
            server,
            json!({"openWorldHint": true}),
            McpToolPolicy::default(),
        );
        let rules = McpTagRuleSet::new([
            rule(Some(server), TAG_OPEN_WORLD, McpTagRuleEffect::Include),
            rule(None, TAG_DESTRUCTIVE, McpTagRuleEffect::Exclude),
        ]);
        assert!(rules.includes(&tool));
        assert!(rules.excludes(&tool));
    }

    #[test]
    fn include_rules_only_match_their_server() {
        let server = Uuid::new_v4();
        let read_only = json!({"readOnlyHint": true});
        let rules =
            McpTagRuleSet::new([rule(Some(server), TAG_READ_ONLY, McpTagRuleEffect::Include)]);
        assert!(rules.includes(&tool(server, read_only.clone(), McpToolPolicy::default())));
        assert!(!rules.includes(&tool(Uuid::new_v4(), read_only, McpToolPolicy::default())));
        assert_eq!(rules.include_server_ids(None), BTreeSet::from([server]));
        assert!(rules.include_server_ids(Some(Uuid::new_v4())).is_empty());
    }

    #[test]
    fn revoked_rules_are_ignored() {
        let mut revoked = rule(None, TAG_DESTRUCTIVE, McpTagRuleEffect::Exclude);
        revoked.is_active = false;
        assert!(McpTagRuleSet::new([revoked]).is_empty());
    }

    #[test]
    fn admin_tags_are_normalized_and_cannot_claim_derived_names() {
        assert_eq!(
            normalize_admin_tags(&[
                " Finance ".to_string(),
                "finance".to_string(),
                "pii:low".to_string()
            ])
            .expect("tags"),
            vec!["finance".to_string(), "pii:low".to_string()]
        );
        assert!(normalize_admin_tag("read_only").is_err());
        assert!(normalize_admin_tag("has space").is_err());
        assert!(normalize_admin_tag("").is_err());
        assert_eq!(normalize_rule_tag("Read_Only").expect("tag"), "read_only");
    }

    #[test]
    fn overrides_reject_unknown_members_and_non_booleans() {
        let parsed = McpAnnotationOverrides::from_object(
            json!({"readOnlyHint": true, "destructiveHint": null})
                .as_object()
                .expect("object"),
        )
        .expect("overrides");
        assert_eq!(parsed.read_only, Some(true));
        assert_eq!(parsed.destructive, None);
        assert!(
            McpAnnotationOverrides::from_object(
                json!({"title": true}).as_object().expect("object")
            )
            .is_err()
        );
        assert!(
            McpAnnotationOverrides::from_object(
                json!({"readOnlyHint": "yes"}).as_object().expect("object")
            )
            .is_err()
        );
    }

    #[test]
    fn include_rules_require_a_server() {
        let key = McpToolTagRuleKey {
            subject_kind: McpToolGrantSubjectKind::Team,
            subject_id: Uuid::new_v4(),
            mcp_server_id: None,
            tag: TAG_READ_ONLY.to_string(),
            effect: McpTagRuleEffect::Include,
        };
        assert!(key.validate().is_err());
        assert!(
            McpToolTagRuleKey {
                effect: McpTagRuleEffect::Exclude,
                ..key
            }
            .validate()
            .is_ok()
        );
    }
}
