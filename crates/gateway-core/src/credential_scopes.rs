//! Capability scopes shared by `gwk_` API keys and MCP client `oat_` access
//! tokens, and the optional MCP server list an API key may carry.
//!
//! See `docs/adr/2026-10-17-scoped-api-keys.md`.

use serde::{Deserialize, Serialize};

use crate::mcp_client_oauth::is_server_key_segment;

/// Allows model inference, model listing, batches, and guardrail evaluation.
pub const SCOPE_INFERENCE: &str = "inference";
/// Allows the aggregate endpoint `/mcp`.
pub const SCOPE_MCP_AGGREGATE: &str = "mcp:aggregate";
/// Allows per-server routes `/mcp/{server_key}`.
pub const SCOPE_MCP_PROXY: &str = "mcp:proxy";

/// Scopes an API key may carry, in the order they are stored and shown.
pub const API_KEY_SCOPES: [&str; 3] = [SCOPE_INFERENCE, SCOPE_MCP_AGGREGATE, SCOPE_MCP_PROXY];

/// What an API key may do beyond its owner's grants.
///
/// `None` in either field is the behaviour of keys created before scopes
/// existed: every scope, and every MCP server the owner's grants reach.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiKeyAccess {
    pub scopes: Option<Vec<String>>,
    pub mcp_server_keys: Option<Vec<String>>,
}

impl ApiKeyAccess {
    /// Validates and normalizes access from an admin request: scopes in
    /// [`API_KEY_SCOPES`] order, server keys sorted, duplicates removed.
    pub fn new(
        scopes: Option<Vec<String>>,
        mcp_server_keys: Option<Vec<String>>,
    ) -> Result<Self, String> {
        let scopes = scopes.map(normalize_scopes).transpose()?;
        let mcp_server_keys = mcp_server_keys.map(normalize_server_keys).transpose()?;
        if mcp_server_keys.is_some()
            && let Some(scopes) = &scopes
            && !scopes
                .iter()
                .any(|scope| scope == SCOPE_MCP_AGGREGATE || scope == SCOPE_MCP_PROXY)
        {
            return Err(
                "mcp_server_keys needs the `mcp:aggregate` or `mcp:proxy` scope".to_string(),
            );
        }
        Ok(Self {
            scopes,
            mcp_server_keys,
        })
    }

    /// Reads the stored space-delimited columns. Unknown scope names are kept,
    /// so they grant nothing rather than failing every request for the key.
    #[must_use]
    pub fn from_columns(scopes: Option<&str>, mcp_server_keys: Option<&str>) -> Self {
        let split = |raw: &str| raw.split_whitespace().map(str::to_string).collect();
        Self {
            scopes: scopes.map(split),
            mcp_server_keys: mcp_server_keys.map(split),
        }
    }

    #[must_use]
    pub fn scopes_column(&self) -> Option<String> {
        self.scopes.as_ref().map(|scopes| scopes.join(" "))
    }

    #[must_use]
    pub fn mcp_server_keys_column(&self) -> Option<String> {
        self.mcp_server_keys.as_ref().map(|keys| keys.join(" "))
    }

    /// Whether the key may call every route, as unscoped keys always could.
    #[must_use]
    pub fn is_full_access(&self) -> bool {
        self.scopes.is_none()
    }

    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes
            .as_ref()
            .is_none_or(|scopes| scopes.iter().any(|granted| granted == scope))
    }

    #[must_use]
    pub fn allows_mcp_server(&self, server_key: &str) -> bool {
        self.mcp_server_keys
            .as_ref()
            .is_none_or(|keys| keys.iter().any(|allowed| allowed == server_key))
    }
}

fn normalize_scopes(scopes: Vec<String>) -> Result<Vec<String>, String> {
    if scopes.is_empty() {
        return Err("scopes must name at least one scope, or be null for full access".to_string());
    }
    if let Some(unknown) = scopes
        .iter()
        .find(|scope| !API_KEY_SCOPES.contains(&scope.trim()))
    {
        return Err(format!(
            "unknown scope `{unknown}`; expected one of {}",
            API_KEY_SCOPES.join(", ")
        ));
    }
    Ok(API_KEY_SCOPES
        .iter()
        .filter(|known| scopes.iter().any(|scope| scope.trim() == **known))
        .map(|known| (*known).to_string())
        .collect())
}

fn normalize_server_keys(keys: Vec<String>) -> Result<Vec<String>, String> {
    if keys.is_empty() {
        return Err(
            "mcp_server_keys must name at least one server, or be null for every server"
                .to_string(),
        );
    }
    let mut normalized = keys
        .iter()
        .map(|key| key.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if let Some(invalid) = normalized.iter().find(|key| !is_server_key_segment(key)) {
        return Err(format!("`{invalid}` is not a valid MCP server key"));
    }
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::{ApiKeyAccess, SCOPE_INFERENCE, SCOPE_MCP_AGGREGATE, SCOPE_MCP_PROXY};

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn unscoped_access_allows_everything() {
        let access = ApiKeyAccess::default();
        assert!(access.is_full_access());
        assert!(access.has_scope(SCOPE_INFERENCE));
        assert!(access.has_scope("anything"));
        assert!(access.allows_mcp_server("github"));
        assert_eq!(access.scopes_column(), None);
        assert_eq!(access.mcp_server_keys_column(), None);
    }

    #[test]
    fn normalizes_order_case_and_duplicates() {
        let access = ApiKeyAccess::new(
            Some(strings(&["mcp:proxy", "inference", "mcp:proxy"])),
            Some(strings(&["Notes", "github", "notes"])),
        )
        .unwrap();
        assert_eq!(access.scopes, Some(strings(&["inference", "mcp:proxy"])));
        assert_eq!(access.mcp_server_keys, Some(strings(&["github", "notes"])));
        assert_eq!(
            access.scopes_column().as_deref(),
            Some("inference mcp:proxy")
        );
        assert_eq!(
            ApiKeyAccess::from_columns(
                access.scopes_column().as_deref(),
                access.mcp_server_keys_column().as_deref()
            ),
            access
        );
    }

    #[test]
    fn scopes_narrow_routes_and_servers() {
        let access =
            ApiKeyAccess::new(Some(strings(&[SCOPE_MCP_PROXY])), Some(strings(&["notes"])))
                .unwrap();
        assert!(!access.is_full_access());
        assert!(access.has_scope(SCOPE_MCP_PROXY));
        assert!(!access.has_scope(SCOPE_MCP_AGGREGATE));
        assert!(!access.has_scope(SCOPE_INFERENCE));
        assert!(access.allows_mcp_server("notes"));
        assert!(!access.allows_mcp_server("github"));
    }

    #[test]
    fn full_access_key_may_still_limit_servers() {
        let access = ApiKeyAccess::new(None, Some(strings(&["notes"]))).unwrap();
        assert!(access.is_full_access());
        assert!(access.allows_mcp_server("notes"));
        assert!(!access.allows_mcp_server("github"));
    }

    #[test]
    fn rejects_invalid_access() {
        assert!(ApiKeyAccess::new(Some(Vec::new()), None).is_err());
        assert!(ApiKeyAccess::new(Some(strings(&["admin"])), None).is_err());
        assert!(ApiKeyAccess::new(Some(strings(&["observability:read"])), None).is_err());
        assert!(ApiKeyAccess::new(None, Some(Vec::new())).is_err());
        assert!(ApiKeyAccess::new(None, Some(strings(&["not a key"]))).is_err());
        assert!(
            ApiKeyAccess::new(Some(strings(&["inference"])), Some(strings(&["notes"]))).is_err(),
            "a server list without an MCP scope does nothing"
        );
    }
}
