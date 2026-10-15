use gateway_core::{
    ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthError, AuthenticatedApiKey, GatewayError,
    GatewayOauthRepository, GatewayOauthTokenKind, McpOauthResource, OauthAccessContext,
    parse_scope_list,
};
use time::OffsetDateTime;

use super::{McpClientOauthService, secrets::oauth_token_hash};

impl<R> McpClientOauthService<R>
where
    R: GatewayOauthRepository,
{
    /// Resolves an `oat_` access token presented on the MCP route `route`.
    ///
    /// The token must be live, issued for a resource that covers the route
    /// (RFC 8707), and carry the route's scope. It acts as its user, so grants
    /// resolve exactly as for that user's API keys.
    pub async fn authenticate_access_token(
        &self,
        raw: &str,
        route: &McpOauthResource,
        now: OffsetDateTime,
    ) -> Result<AuthenticatedApiKey, GatewayError> {
        let context = self
            .repo
            .get_gateway_oauth_token_context(&oauth_token_hash(raw.trim()))
            .await?
            .filter(|context| {
                context.token.kind == GatewayOauthTokenKind::Access
                    && context.token.expires_at > now
                    && context.grant_is_live(now)
            })
            .ok_or(AuthError::OauthTokenInvalid)?;
        let resource = self
            .settings
            .parse_resource(&context.grant.resource)
            .ok_or(AuthError::OauthTokenWrongResource)?;
        if !resource.covers(route) {
            return Err(AuthError::OauthTokenWrongResource.into());
        }
        let scopes = parse_scope_list(&context.token.scope);
        let required = route.required_scope();
        if !scopes.iter().any(|scope| scope == required) {
            return Err(AuthError::OauthInsufficientScope {
                required: required.to_string(),
            }
            .into());
        }
        let grant = context.grant;
        Ok(AuthenticatedApiKey {
            id: grant.grant_id,
            public_id: grant.client_id.clone(),
            name: format!("OAuth client {}", grant.client_id),
            // OAuth tokens never reach model routes.
            model_grant_mode: ApiKeyModelGrantMode::Explicit,
            owner_kind: ApiKeyOwnerKind::User,
            owner_user_id: Some(grant.user_id),
            owner_team_id: None,
            owner_service_account_id: None,
            oauth: Some(OauthAccessContext {
                grant_id: grant.grant_id,
                client_id: grant.client_id,
                resource,
                scopes,
            }),
        })
    }
}
