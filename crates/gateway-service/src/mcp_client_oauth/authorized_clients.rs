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

impl<R> McpClientOauthService<R>
where
    R: GatewayOauthRepository,
{
    /// The MCP clients `user_id` has authorized that can still get tokens,
    /// each with the resource it is bound to.
    ///
    /// Grants issued under a previous issuer are left out: their resource no
    /// longer parses, and token validation already rejects them.
    pub async fn authorized_clients(
        &self,
        user_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<Vec<(GatewayOauthGrantSummary, McpOauthResource)>, GatewayError> {
        Ok(self
            .repo
            .list_live_gateway_oauth_grants_for_user(user_id, now)
            .await?
            .into_iter()
            .filter_map(|grant| {
                let resource = self.settings.parse_resource(&grant.resource)?;
                Some((grant, resource))
            })
            .collect())
    }

    /// Disconnects one authorized client. Returns `false` when the grant is
    /// not the user's or is already revoked.
    pub async fn revoke_authorized_client(
        &self,
        user_id: Uuid,
        grant_id: Uuid,
        reason: &str,
        now: OffsetDateTime,
    ) -> Result<bool, GatewayError> {
        Ok(self
            .repo
            .revoke_gateway_oauth_grant_for_user(grant_id, user_id, reason, now)
            .await?)
    }

    /// Disconnects every client `user_id` has authorized.
    pub async fn revoke_all_authorized_clients(
        &self,
        user_id: Uuid,
        reason: &str,
        now: OffsetDateTime,
    ) -> Result<u64, GatewayError> {
        Ok(self
            .repo
            .revoke_gateway_oauth_grants_for_user(user_id, reason, now)
            .await?)
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
            )
            .await?)
    }
}
