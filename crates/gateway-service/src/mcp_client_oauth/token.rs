use gateway_core::{
    GatewayOauthAuthorizationStatus, GatewayOauthClientAuthMethod, GatewayOauthClientRecord,
    GatewayOauthGrantRecord, GatewayOauthRepository, GatewayOauthTokenKind,
    GatewayOauthTokenRecord, McpRegistryRepository, OAUTH_ACCESS_TOKEN_PREFIX,
    OAUTH_REFRESH_TOKEN_PREFIX, parse_scope_list,
};
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    McpClientOauthService,
    error::OauthError,
    secrets::{generate_token, oauth_token_hash, pkce_s256_matches, run_argon2},
};
use crate::authenticator::verify_gateway_key_secret;

/// Form fields of `POST /oauth/token`, apart from client credentials.
#[derive(Debug, Clone, Default)]
pub struct TokenRequest {
    pub grant_type: Option<String>,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_verifier: Option<String>,
    pub resource: Option<String>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

/// How the client presented its credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCredentials {
    /// A public client: `client_id` in the form, no secret.
    Public { client_id: String },
    /// `client_id` and `client_secret` in the form.
    Post {
        client_id: String,
        client_secret: String,
    },
    /// HTTP Basic authentication.
    Basic {
        client_id: String,
        client_secret: String,
    },
}

impl ClientCredentials {
    fn client_id(&self) -> &str {
        match self {
            Self::Public { client_id }
            | Self::Post { client_id, .. }
            | Self::Basic { client_id, .. } => client_id,
        }
    }
}

/// RFC 6749 section 5.1 success response.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub scope: String,
}

impl<R> McpClientOauthService<R>
where
    R: GatewayOauthRepository + McpRegistryRepository,
{
    pub async fn exchange_token(
        &self,
        credentials: &ClientCredentials,
        request: TokenRequest,
        now: OffsetDateTime,
    ) -> Result<TokenResponse, OauthError> {
        let grant_type = request
            .grant_type
            .clone()
            .ok_or_else(|| OauthError::invalid_request("grant_type is required"))?;
        let client = self.authenticate_client(credentials).await?;
        if !client.allows_grant_type(&grant_type) {
            return Err(match grant_type.as_str() {
                "authorization_code" | "refresh_token" => OauthError::unauthorized_client(format!(
                    "client is not registered for the {grant_type} grant"
                )),
                _ => OauthError::unsupported_grant_type("grant_type is not supported"),
            });
        }
        match grant_type.as_str() {
            "authorization_code" => self.exchange_code(&client, request, now).await,
            "refresh_token" => self.refresh(&client, request, now).await,
            _ => Err(OauthError::unsupported_grant_type(
                "grant_type is not supported",
            )),
        }
    }

    /// Authenticates the client with the method it registered.
    pub(crate) async fn authenticate_client(
        &self,
        credentials: &ClientCredentials,
    ) -> Result<GatewayOauthClientRecord, OauthError> {
        let client = self
            .repo
            .get_gateway_oauth_client(credentials.client_id())
            .await?
            .ok_or_else(|| OauthError::invalid_client("client authentication failed"))?;
        let secret = match (client.token_endpoint_auth_method, credentials) {
            (GatewayOauthClientAuthMethod::None, ClientCredentials::Public { .. }) => None,
            (
                GatewayOauthClientAuthMethod::ClientSecretPost,
                ClientCredentials::Post { client_secret, .. },
            )
            | (
                GatewayOauthClientAuthMethod::ClientSecretBasic,
                ClientCredentials::Basic { client_secret, .. },
            ) => Some(client_secret),
            _ => {
                return Err(OauthError::invalid_client(
                    "client authentication method does not match the registration",
                ));
            }
        };
        if let Some(secret) = secret {
            let hash = client
                .client_secret_hash
                .as_deref()
                .ok_or_else(|| OauthError::invalid_client("client authentication failed"))?;
            let (secret, hash) = (secret.clone(), hash.to_string());
            let matches = run_argon2(move || verify_gateway_key_secret(&secret, &hash))
                .await?
                .map_err(|error| OauthError::server_error(error.to_string()))?;
            if !matches {
                return Err(OauthError::invalid_client("client authentication failed"));
            }
        }
        Ok(client)
    }

    async fn exchange_code(
        &self,
        client: &GatewayOauthClientRecord,
        request: TokenRequest,
        now: OffsetDateTime,
    ) -> Result<TokenResponse, OauthError> {
        let code = request
            .code
            .as_deref()
            .ok_or_else(|| OauthError::invalid_request("code is required"))?;
        let code_verifier = request
            .code_verifier
            .as_deref()
            .ok_or_else(|| OauthError::invalid_request("code_verifier is required"))?;
        let invalid_code = || OauthError::invalid_grant("authorization code is invalid");
        let authorization = self
            .repo
            .get_gateway_oauth_authorization_request_by_code_hash(&oauth_token_hash(code))
            .await?
            .ok_or_else(invalid_code)?;
        // Only the client the code was issued to, holding its PKCE verifier,
        // can redeem it or trigger replay revocation. Anyone else who learned
        // the code from a log cannot end the victim's grant.
        if authorization.client_id != client.client_id {
            return Err(invalid_code());
        }
        if !pkce_s256_matches(code_verifier, &authorization.code_challenge) {
            return Err(OauthError::invalid_grant(
                "code_verifier does not match the code challenge",
            ));
        }
        if authorization.status == GatewayOauthAuthorizationStatus::Consumed {
            // A replayed code may have been stolen, so end the grant it created.
            self.revoke_code_grant(authorization.grant_id, now).await?;
            return Err(invalid_code());
        }
        let (Some(user_id), Some(user_session_id), Some(code_expires_at)) = (
            authorization.user_id,
            authorization.user_session_id,
            authorization.code_expires_at,
        ) else {
            return Err(invalid_code());
        };
        if authorization.status != GatewayOauthAuthorizationStatus::Approved
            || code_expires_at <= now
        {
            return Err(invalid_code());
        }
        let redirect_matches = match request.redirect_uri.as_deref() {
            Some(redirect_uri) => redirect_uri == authorization.redirect_uri,
            // RFC 6749 section 4.1.3: required when the authorization request sent it.
            None => !authorization.redirect_uri_supplied,
        };
        if !redirect_matches {
            return Err(OauthError::invalid_grant(
                "redirect_uri does not match the authorization request",
            ));
        }
        if let Some(resource) = request.resource.as_deref() {
            let resource = self.resolve_resource(Some(resource)).await?;
            if self.settings.resource_uri(&resource) != authorization.resource {
                return Err(OauthError::invalid_target(
                    "resource does not match the authorization request",
                ));
            }
        }
        let session = self
            .repo
            .get_gateway_oauth_session_state(user_session_id)
            .await?
            .filter(|session| session.user_id == user_id && session.is_live(now))
            .ok_or_else(|| OauthError::invalid_grant("the approving session has ended"))?;

        // Issue first and consume the code last: a failed write leaves the
        // code redeemable for a retry, and the conditional consume still makes
        // the exchange single-use.
        let grant = GatewayOauthGrantRecord {
            grant_id: Uuid::new_v4(),
            client_id: client.client_id.clone(),
            user_id,
            user_session_id,
            resource: authorization.resource.clone(),
            scope: authorization.scope.clone(),
            created_at: now,
            updated_at: now,
            revoked_at: None,
            revoked_reason: None,
        };
        if !self.repo.create_gateway_oauth_grant(&grant).await? {
            // The approving session was signed out after the check above.
            return Err(OauthError::invalid_grant("the approving session has ended"));
        }
        let refresh_scope = client
            .allows_grant_type("refresh_token")
            .then_some(grant.scope.as_str());
        let issued = async {
            let response = self
                .issue_tokens(
                    grant.grant_id,
                    &grant.scope,
                    refresh_scope,
                    session.expires_at,
                    now,
                )
                .await?;
            let consumed = self
                .repo
                .consume_gateway_oauth_authorization_code(
                    authorization.request_id,
                    grant.grant_id,
                    now,
                )
                .await?;
            Ok::<_, OauthError>((response, consumed))
        }
        .await;
        let (response, consumed) = match issued {
            Ok(issued) => issued,
            Err(error) => {
                // The client never received these tokens, and the code stays
                // redeemable, so a retry would start a second grant. End this
                // one so nothing is left half issued.
                self.abandon_grant(grant.grant_id, now).await;
                return Err(error);
            }
        };
        if !consumed {
            // A concurrent request redeemed the code first: this is a replay,
            // so neither grant may survive.
            self.revoke_code_grant(Some(grant.grant_id), now).await?;
            let winner = self
                .repo
                .get_gateway_oauth_authorization_request(authorization.request_id)
                .await?
                .and_then(|request| request.grant_id);
            self.revoke_code_grant(winner, now).await?;
            return Err(invalid_code());
        }
        Ok(response)
    }

    async fn abandon_grant(&self, grant_id: Uuid, now: OffsetDateTime) {
        if let Err(error) = self
            .repo
            .revoke_gateway_oauth_grant(grant_id, "issue_failed", now)
            .await
        {
            tracing::warn!(error = %error, %grant_id, "could not revoke a half-issued MCP OAuth grant");
        }
    }

    async fn revoke_code_grant(
        &self,
        grant_id: Option<Uuid>,
        now: OffsetDateTime,
    ) -> Result<(), OauthError> {
        if let Some(grant_id) = grant_id {
            self.repo
                .revoke_gateway_oauth_grant(grant_id, "authorization_code_replayed", now)
                .await?;
        }
        Ok(())
    }

    /// Rotates a refresh token. Presenting a consumed refresh token revokes
    /// the whole grant (OAuth 2.1 section 4.3.1).
    async fn refresh(
        &self,
        client: &GatewayOauthClientRecord,
        request: TokenRequest,
        now: OffsetDateTime,
    ) -> Result<TokenResponse, OauthError> {
        let raw = request
            .refresh_token
            .as_deref()
            .ok_or_else(|| OauthError::invalid_request("refresh_token is required"))?;
        let invalid = || OauthError::invalid_grant("refresh token is invalid");
        let token_hash = oauth_token_hash(raw);
        let context = self
            .repo
            .get_gateway_oauth_token_context(&token_hash)
            .await?
            .filter(|context| {
                context.token.kind == GatewayOauthTokenKind::Refresh
                    && context.grant.client_id == client.client_id
            })
            .ok_or_else(invalid)?;
        let grant_id = context.grant.grant_id;
        if context.token.consumed_at.is_some() {
            self.repo
                .revoke_gateway_oauth_grant(grant_id, "refresh_token_reused", now)
                .await?;
            return Err(invalid());
        }
        if context.token.expires_at <= now || !context.grant_is_live(now) {
            return Err(invalid());
        }
        if let Some(resource) = request.resource.as_deref() {
            let resource = self.resolve_resource(Some(resource)).await?;
            if self.settings.resource_uri(&resource) != context.grant.resource {
                return Err(OauthError::invalid_target(
                    "resource does not match the grant",
                ));
            }
        }
        let access_scope = narrowed_scope(&context.token.scope, request.scope.as_deref())?;
        // Issue the new pair before consuming the old token, so a failed write
        // leaves the old refresh token usable for a retry.
        let response = self
            .issue_tokens(
                grant_id,
                &access_scope,
                Some(&context.token.scope),
                context.session_expires_at,
                now,
            )
            .await?;
        if !self
            .repo
            .consume_gateway_oauth_refresh_token(&token_hash, now)
            .await?
        {
            // Another request rotated this token first: treat it as reuse.
            // Revoking the grant also ends the pair just issued.
            self.repo
                .revoke_gateway_oauth_grant(grant_id, "refresh_token_reused", now)
                .await?;
            return Err(invalid());
        }
        Ok(response)
    }

    /// Revokes the grant behind `token` (RFC 7009). Unknown tokens and tokens
    /// issued to another client succeed without effect.
    pub async fn revoke_token(
        &self,
        credentials: &ClientCredentials,
        token: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<(), OauthError> {
        let client = self.authenticate_client(credentials).await?;
        let token = token.ok_or_else(|| OauthError::invalid_request("token is required"))?;
        if let Some(context) = self
            .repo
            .get_gateway_oauth_token_context(&oauth_token_hash(token))
            .await?
            .filter(|context| context.grant.client_id == client.client_id)
        {
            self.repo
                .revoke_gateway_oauth_grant(context.grant.grant_id, "client_revoked", now)
                .await?;
        }
        Ok(())
    }

    /// Issues an access token, and a refresh token when `refresh_scope` is
    /// set. Both expire no later than the approving session.
    async fn issue_tokens(
        &self,
        grant_id: Uuid,
        access_scope: &str,
        refresh_scope: Option<&str>,
        session_expires_at: OffsetDateTime,
        now: OffsetDateTime,
    ) -> Result<TokenResponse, OauthError> {
        let record = |raw: &str, kind, scope: &str, expires_at| GatewayOauthTokenRecord {
            token_hash: oauth_token_hash(raw),
            grant_id,
            kind,
            scope: scope.to_string(),
            expires_at,
            created_at: now,
            consumed_at: None,
        };
        let access_token = generate_token(OAUTH_ACCESS_TOKEN_PREFIX);
        let access_expires_at = (now + self.settings.access_token_ttl()).min(session_expires_at);
        let mut records = vec![record(
            &access_token,
            GatewayOauthTokenKind::Access,
            access_scope,
            access_expires_at,
        )];
        let refresh_token = refresh_scope.map(|scope| {
            let refresh_token = generate_token(OAUTH_REFRESH_TOKEN_PREFIX);
            let refresh_expires_at =
                (now + self.settings.refresh_token_ttl()).min(session_expires_at);
            records.push(record(
                &refresh_token,
                GatewayOauthTokenKind::Refresh,
                scope,
                refresh_expires_at,
            ));
            refresh_token
        });
        // One transaction, so a failure never leaves half a pair behind.
        self.repo.create_gateway_oauth_tokens(&records).await?;
        Ok(TokenResponse {
            access_token,
            token_type: "Bearer",
            expires_in: (access_expires_at - now).whole_seconds(),
            refresh_token,
            scope: access_scope.to_string(),
        })
    }
}

/// The scope for a refreshed access token: the requested subset of the
/// refresh token's scope, or all of it (RFC 6749 section 6).
fn narrowed_scope(granted: &str, requested: Option<&str>) -> Result<String, OauthError> {
    let requested = requested.map(parse_scope_list).unwrap_or_default();
    if requested.is_empty() {
        return Ok(granted.to_string());
    }
    let granted = parse_scope_list(granted);
    if let Some(scope) = requested.iter().find(|scope| !granted.contains(scope)) {
        return Err(OauthError::invalid_scope(format!(
            "scope `{scope}` was not granted"
        )));
    }
    Ok(requested.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_scope_can_only_narrow() {
        assert_eq!(
            narrowed_scope("mcp:aggregate mcp:proxy", None).unwrap(),
            "mcp:aggregate mcp:proxy"
        );
        assert_eq!(
            narrowed_scope("mcp:aggregate mcp:proxy", Some("mcp:proxy")).unwrap(),
            "mcp:proxy"
        );
        assert_eq!(
            narrowed_scope("mcp:proxy", Some("mcp:aggregate"))
                .unwrap_err()
                .error,
            "invalid_scope"
        );
    }
}
