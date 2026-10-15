use gateway_core::{
    GatewayOauthAuthorizationStatus, GatewayOauthClientAuthMethod, GatewayOauthClientRecord,
    GatewayOauthGrantRecord, GatewayOauthRepository, GatewayOauthTokenKind,
    GatewayOauthTokenRecord, McpRegistryRepository, OAUTH_ACCESS_TOKEN_PREFIX,
};
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    McpClientOauthService,
    error::OauthError,
    secrets::{generate_secret, oauth_secret_hash, pkce_s256_matches},
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
            let matches = verify_gateway_key_secret(secret, hash)
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
            .get_gateway_oauth_authorization_request_by_code_hash(&oauth_secret_hash(code))
            .await?
            .ok_or_else(invalid_code)?;
        if authorization.status == GatewayOauthAuthorizationStatus::Consumed {
            // A replayed code may have been stolen, so end the grant it created.
            if let Some(grant_id) = authorization.grant_id {
                self.repo
                    .revoke_gateway_oauth_grant(grant_id, "authorization_code_replayed", now)
                    .await?;
            }
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
            || authorization.client_id != client.client_id
            || code_expires_at <= now
        {
            return Err(invalid_code());
        }
        if request
            .redirect_uri
            .as_deref()
            .is_some_and(|redirect_uri| redirect_uri != authorization.redirect_uri)
        {
            return Err(OauthError::invalid_grant(
                "redirect_uri does not match the authorization request",
            ));
        }
        if !pkce_s256_matches(code_verifier, &authorization.code_challenge) {
            return Err(OauthError::invalid_grant(
                "code_verifier does not match the code challenge",
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

        let grant_id = Uuid::new_v4();
        if !self
            .repo
            .consume_gateway_oauth_authorization_code(authorization.request_id, grant_id, now)
            .await?
        {
            return Err(invalid_code());
        }
        let grant = GatewayOauthGrantRecord {
            grant_id,
            client_id: client.client_id.clone(),
            user_id,
            user_session_id,
            resource: authorization.resource,
            scope: authorization.scope,
            created_at: now,
            updated_at: now,
            revoked_at: None,
            revoked_reason: None,
        };
        self.repo.create_gateway_oauth_grant(&grant).await?;
        self.issue_tokens(&grant, session.expires_at, now).await
    }

    /// Issues an access token for `grant`, capped at the session's expiry.
    async fn issue_tokens(
        &self,
        grant: &GatewayOauthGrantRecord,
        session_expires_at: OffsetDateTime,
        now: OffsetDateTime,
    ) -> Result<TokenResponse, OauthError> {
        let access_token = generate_secret(OAUTH_ACCESS_TOKEN_PREFIX);
        let access_expires_at = (now + self.settings.access_token_ttl()).min(session_expires_at);
        self.repo
            .create_gateway_oauth_token(&GatewayOauthTokenRecord {
                token_hash: oauth_secret_hash(&access_token),
                grant_id: grant.grant_id,
                kind: GatewayOauthTokenKind::Access,
                scope: grant.scope.clone(),
                expires_at: access_expires_at,
                created_at: now,
                consumed_at: None,
            })
            .await?;
        Ok(TokenResponse {
            access_token,
            token_type: "Bearer",
            expires_in: (access_expires_at - now).whole_seconds(),
            refresh_token: None,
            scope: grant.scope.clone(),
        })
    }
}
