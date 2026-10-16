use async_trait::async_trait;
use gateway_core::{
    GatewayOauthAuthorizationDecision, GatewayOauthAuthorizationRequestRecord,
    GatewayOauthClientRecord, GatewayOauthGrantRecord, GatewayOauthGrantSummary,
    GatewayOauthPurgeCounts, GatewayOauthRepository, GatewayOauthSessionState,
    GatewayOauthTokenContext, GatewayOauthTokenRecord, StoreError,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::store::AnyStore;

macro_rules! dispatch {
    ($self:expr, $method:ident ( $($arg:expr),* )) => {
        match $self {
            AnyStore::Libsql(store) => store.$method($($arg),*).await,
            AnyStore::Postgres(store) => store.$method($($arg),*).await,
        }
    };
}

#[async_trait]
impl GatewayOauthRepository for AnyStore {
    async fn create_gateway_oauth_client(
        &self,
        client: &GatewayOauthClientRecord,
    ) -> Result<(), StoreError> {
        dispatch!(self, create_gateway_oauth_client(client))
    }

    async fn get_gateway_oauth_client(
        &self,
        client_id: &str,
    ) -> Result<Option<GatewayOauthClientRecord>, StoreError> {
        dispatch!(self, get_gateway_oauth_client(client_id))
    }

    async fn create_gateway_oauth_authorization_request(
        &self,
        request: &GatewayOauthAuthorizationRequestRecord,
    ) -> Result<(), StoreError> {
        dispatch!(self, create_gateway_oauth_authorization_request(request))
    }

    async fn get_gateway_oauth_authorization_request(
        &self,
        request_id: Uuid,
    ) -> Result<Option<GatewayOauthAuthorizationRequestRecord>, StoreError> {
        dispatch!(self, get_gateway_oauth_authorization_request(request_id))
    }

    async fn get_gateway_oauth_authorization_request_by_code_hash(
        &self,
        code_hash: &str,
    ) -> Result<Option<GatewayOauthAuthorizationRequestRecord>, StoreError> {
        dispatch!(
            self,
            get_gateway_oauth_authorization_request_by_code_hash(code_hash)
        )
    }

    async fn decide_gateway_oauth_authorization_request(
        &self,
        request_id: Uuid,
        decision: &GatewayOauthAuthorizationDecision,
        decided_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        dispatch!(
            self,
            decide_gateway_oauth_authorization_request(request_id, decision, decided_at)
        )
    }

    async fn consume_gateway_oauth_authorization_code(
        &self,
        request_id: Uuid,
        grant_id: Uuid,
        consumed_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        dispatch!(
            self,
            consume_gateway_oauth_authorization_code(request_id, grant_id, consumed_at)
        )
    }

    async fn create_gateway_oauth_grant(
        &self,
        grant: &GatewayOauthGrantRecord,
    ) -> Result<bool, StoreError> {
        dispatch!(self, create_gateway_oauth_grant(grant))
    }

    async fn create_gateway_oauth_tokens(
        &self,
        tokens: &[GatewayOauthTokenRecord],
    ) -> Result<(), StoreError> {
        dispatch!(self, create_gateway_oauth_tokens(tokens))
    }

    async fn get_gateway_oauth_token_context(
        &self,
        token_hash: &str,
    ) -> Result<Option<GatewayOauthTokenContext>, StoreError> {
        dispatch!(self, get_gateway_oauth_token_context(token_hash))
    }

    async fn live_gateway_oauth_access_tokens(
        &self,
        token_hashes: &[String],
        now: OffsetDateTime,
    ) -> Result<Vec<String>, StoreError> {
        dispatch!(self, live_gateway_oauth_access_tokens(token_hashes, now))
    }

    async fn get_gateway_oauth_session_state(
        &self,
        user_session_id: Uuid,
    ) -> Result<Option<GatewayOauthSessionState>, StoreError> {
        dispatch!(self, get_gateway_oauth_session_state(user_session_id))
    }

    async fn consume_gateway_oauth_refresh_token(
        &self,
        token_hash: &str,
        consumed_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        dispatch!(
            self,
            consume_gateway_oauth_refresh_token(token_hash, consumed_at)
        )
    }

    async fn revoke_gateway_oauth_grant(
        &self,
        grant_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<bool, StoreError> {
        dispatch!(
            self,
            revoke_gateway_oauth_grant(grant_id, reason, revoked_at)
        )
    }

    async fn revoke_gateway_oauth_grants_for_user(
        &self,
        user_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<u64, StoreError> {
        dispatch!(
            self,
            revoke_gateway_oauth_grants_for_user(user_id, reason, revoked_at)
        )
    }

    async fn list_live_gateway_oauth_grants_for_user(
        &self,
        user_id: Uuid,
        aggregate_resource: Option<&str>,
        now: OffsetDateTime,
        limit: u32,
    ) -> Result<Vec<GatewayOauthGrantSummary>, StoreError> {
        dispatch!(
            self,
            list_live_gateway_oauth_grants_for_user(user_id, aggregate_resource, now, limit)
        )
    }

    async fn revoke_gateway_oauth_grant_for_user(
        &self,
        grant_id: Uuid,
        user_id: Uuid,
        reason: &str,
        revoked_at: OffsetDateTime,
    ) -> Result<Option<GatewayOauthGrantRecord>, StoreError> {
        dispatch!(
            self,
            revoke_gateway_oauth_grant_for_user(grant_id, user_id, reason, revoked_at)
        )
    }

    async fn purge_gateway_oauth_records(
        &self,
        unused_clients_before: OffsetDateTime,
        expired_before: OffsetDateTime,
        retire_before: OffsetDateTime,
    ) -> Result<GatewayOauthPurgeCounts, StoreError> {
        dispatch!(
            self,
            purge_gateway_oauth_records(unused_clients_before, expired_before, retire_before)
        )
    }
}
