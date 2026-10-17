//! The gateway's OAuth authorization server for MCP clients.
//!
//! See `docs/adr/2026-10-15-mcp-client-oauth-authorization.md`.

mod authorization;
mod authorized_clients;
mod error;
pub mod metadata_document;
pub(crate) use metadata_document::ClientLookup;
mod redirect;
mod registration;
mod secrets;
mod settings;
mod token;
mod validation;

use std::sync::Arc;

use gateway_core::{GatewayOauthClientRecord, GatewayOauthRepository};
use time::OffsetDateTime;

pub use authorization::{AuthorizationParams, AuthorizationRejection, PendingAuthorization};
pub use authorized_clients::{AuthorizedClients, MAX_LISTED_AUTHORIZED_CLIENTS};
pub use error::OauthError;
pub use metadata_document::{
    ClientMetadataDocumentSource, ClientMetadataDocuments, FetchedDocument,
    is_metadata_document_client_id,
};
pub use registration::{ClientRegistrationRequest, ClientRegistrationResponse};
pub use secrets::oauth_token_hash;
pub use settings::{
    DEFAULT_ACCESS_TOKEN_TTL_SECONDS, DEFAULT_REFRESH_TOKEN_TTL_SECONDS,
    DEFAULT_UNUSED_CLIENT_RETENTION_DAYS, McpClientOauthSettings, McpClientOauthSettingsError,
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

impl<R> McpClientOauthService<R>
where
    R: GatewayOauthRepository,
{
    /// Loads a client: one whose `client_id` is a metadata document URL from
    /// that document, any other from its registration. See [`ClientLookup`]
    /// for what may stand in for a document that cannot be used.
    pub(crate) async fn load_client(
        &self,
        client_id: &str,
        now: OffsetDateTime,
        lookup: ClientLookup,
    ) -> Result<Option<GatewayOauthClientRecord>, OauthError> {
        if is_metadata_document_client_id(client_id)
            && let Some(documents) = self.settings.client_id_metadata_documents()
        {
            return documents
                .resolve(self.repo.as_ref(), client_id, now, lookup)
                .await
                .map(Some);
        }
        // A row left from when documents were accepted must not stand in for
        // one once they are turned off, except to revoke, which is always safe.
        Ok(self
            .repo
            .get_gateway_oauth_client(client_id)
            .await?
            .filter(|client| {
                lookup == ClientLookup::Revoke
                    || client.registration_kind
                        != metadata_document::METADATA_DOCUMENT_REGISTRATION_KIND
            }))
    }
}
