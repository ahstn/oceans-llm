use gateway_core::{
    GatewayError, GatewayOauthGrantSummary, GatewayOauthPurgeCounts, GatewayOauthRepository,
    McpOauthResource,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::McpClientOauthService;

/// Expired codes and tokens are kept this long after expiry, so a late replay
/// still meets the record and is rejected as reused rather than unknown.
const EXPIRED_RECORD_GRACE: Duration = Duration::days(1);

/// The most authorized clients one listing returns. Each sign-in creates a
/// grant, so the list is capped rather than left to grow with the account.
pub const MAX_LISTED_AUTHORIZED_CLIENTS: u32 = 100;

/// The newest live grants for a user, each with the resource it is bound to.
#[derive(Debug, Clone)]
pub struct AuthorizedClients {
    pub clients: Vec<(GatewayOauthGrantSummary, McpOauthResource)>,
    /// More live grants exist than [`MAX_LISTED_AUTHORIZED_CLIENTS`].
    pub has_more: bool,
}

impl<R> McpClientOauthService<R>
where
    R: GatewayOauthRepository,
{
    /// The newest MCP clients `user_id` has authorized that can still get
    /// tokens, up to [`MAX_LISTED_AUTHORIZED_CLIENTS`].
    ///
    /// Grants issued under a previous issuer are left out before the limit
    /// applies; token validation already rejects them.
    pub async fn authorized_clients(
        &self,
        user_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<AuthorizedClients, GatewayError> {
        let mut grants = self
            .repo
            .list_live_gateway_oauth_grants_for_user(
                user_id,
                Some(&self.settings.resource_uri(&McpOauthResource::Aggregate)),
                now,
                MAX_LISTED_AUTHORIZED_CLIENTS + 1,
            )
            .await?;
        let has_more = grants.len() > MAX_LISTED_AUTHORIZED_CLIENTS as usize;
        grants.truncate(MAX_LISTED_AUTHORIZED_CLIENTS as usize);
        let clients = grants
            .into_iter()
            .filter_map(|grant| {
                let resource = self.settings.parse_resource(&grant.resource)?;
                Some((grant, resource))
            })
            .collect();
        Ok(AuthorizedClients { clients, has_more })
    }

    /// Removes registered clients that never got a grant within the
    /// retention window, and long-expired codes and tokens.
    pub async fn purge_stale_records(
        &self,
        now: OffsetDateTime,
    ) -> Result<GatewayOauthPurgeCounts, GatewayError> {
        Ok(self
            .repo
            .purge_gateway_oauth_records(
                now - self.settings.unused_client_retention(),
                now - EXPIRED_RECORD_GRACE,
                now,
            )
            .await?)
    }
}
