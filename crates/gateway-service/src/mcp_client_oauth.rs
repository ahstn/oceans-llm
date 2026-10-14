//! The gateway's OAuth authorization server for MCP clients.
//!
//! See `docs/adr/2026-10-15-mcp-client-oauth-authorization.md`.

mod settings;

pub use settings::{
    DEFAULT_ACCESS_TOKEN_TTL_SECONDS, DEFAULT_REFRESH_TOKEN_TTL_SECONDS, McpClientOauthSettings,
    McpClientOauthSettingsError,
};
