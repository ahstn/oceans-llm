//! The gateway's OAuth authorization server for MCP clients.
//!
//! See `docs/adr/2026-10-15-mcp-client-oauth-authorization.md`.

mod authorization;
mod error;
mod redirect;
mod registration;
mod secrets;
mod settings;
mod token;

use std::sync::Arc;

pub use authorization::{AuthorizationParams, AuthorizationRejection, PendingAuthorization};
pub use error::OauthError;
pub use registration::{ClientRegistrationRequest, ClientRegistrationResponse};
pub use secrets::oauth_secret_hash;
pub use settings::{
    DEFAULT_ACCESS_TOKEN_TTL_SECONDS, DEFAULT_REFRESH_TOKEN_TTL_SECONDS, McpClientOauthSettings,
    McpClientOauthSettingsError,
};
pub use token::{ClientCredentials, TokenRequest, TokenResponse};

#[derive(Clone)]
pub struct McpClientOauthService<R> {
    repo: Arc<R>,
    settings: Arc<McpClientOauthSettings>,
}

impl<R> McpClientOauthService<R> {
    #[must_use]
    pub fn new(repo: Arc<R>, settings: Arc<McpClientOauthSettings>) -> Self {
        Self { repo, settings }
    }

    #[must_use]
    pub fn settings(&self) -> &McpClientOauthSettings {
        &self.settings
    }
}
