use gateway_core::{
    ExternalMcpServerStatus, GatewayError, GatewayOauthAuthorizationDecision,
    GatewayOauthAuthorizationRequestRecord, GatewayOauthAuthorizationStatus,
    GatewayOauthClientRecord, GatewayOauthRepository, McpOauthResource, McpRegistryRepository,
    StoreError, parse_scope_list,
};
use time::{Duration, OffsetDateTime};
use url::Url;
use uuid::Uuid;

use super::{
    McpClientOauthService,
    error::OauthError,
    metadata_document::metadata_document_host,
    redirect::{is_loopback_redirect, redirect_uri_is_registered},
    secrets::{generate_token, is_valid_s256_challenge, oauth_token_hash},
};

const AUTHORIZATION_REQUEST_TTL: Duration = Duration::minutes(10);
const AUTHORIZATION_CODE_TTL: Duration = Duration::seconds(60);
const MAX_STATE_LENGTH: usize = 2048;
const AUTHORIZATION_CODE_PREFIX: &str = "oac_";

/// Query parameters of `GET /oauth/authorize`.
#[derive(Debug, Clone, Default)]
pub struct AuthorizationParams {
    pub response_type: Option<String>,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub state: Option<String>,
    pub scope: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub resource: Option<String>,
    /// A malformed-request error found while parsing, such as a repeated
    /// parameter. It is reported only after the client and redirect URI are
    /// verified, so it can go back to the client.
    pub invalid_request: Option<OauthError>,
}

/// Why an authorization request was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationRejection {
    /// The client or redirect URI could not be verified. Show the error to the
    /// user and never redirect.
    Display(OauthError),
    /// Send the user agent back to the client with this URL.
    Redirect(String),
}

/// A pending request with what the consent screen shows.
#[derive(Debug, Clone)]
pub struct PendingAuthorization {
    pub request: GatewayOauthAuthorizationRequestRecord,
    pub client: GatewayOauthClientRecord,
    pub resource: McpOauthResource,
    pub server_display_name: Option<String>,
    pub scopes: Vec<String>,
    /// The code goes to a loopback URI, so the receiving app runs on the
    /// user's device and the gateway cannot verify who published it.
    pub redirect_is_loopback: bool,
    /// For a metadata document client, the host that published the
    /// document. It is the only thing about the client the gateway checked.
    pub client_metadata_host: Option<String>,
}

impl<R> McpClientOauthService<R>
where
    R: GatewayOauthRepository + McpRegistryRepository,
{
    /// Validates `/oauth/authorize` and stores a pending request for consent.
    pub async fn begin_authorization(
        &self,
        params: AuthorizationParams,
        now: OffsetDateTime,
    ) -> Result<Uuid, AuthorizationRejection> {
        let client_id = params.client_id.as_deref().ok_or_else(|| {
            AuthorizationRejection::Display(OauthError::invalid_request("client_id is required"))
        })?;
        let client = self
            .load_client(client_id, now, false)
            .await
            .map_err(AuthorizationRejection::Display)?
            .ok_or_else(|| {
                AuthorizationRejection::Display(OauthError::invalid_client(
                    "client_id is not registered",
                ))
            })?;
        let redirect_uri = resolve_redirect_uri(&client, params.redirect_uri.as_deref())
            .map_err(AuthorizationRejection::Display)?;

        // From here the redirect URI is trusted, so errors go back to the client.
        let state = params.state.clone();
        let reject = |error: OauthError| {
            AuthorizationRejection::Redirect(authorization_error_redirect(
                self.settings.issuer(),
                &redirect_uri,
                state.as_deref(),
                &error,
            ))
        };
        if let Some(error) = params.invalid_request.clone() {
            return Err(reject(error));
        }
        let validated = self
            .validate_authorization_params(&client, &params)
            .await
            .map_err(reject)?;

        let request_id = Uuid::new_v4();
        let record = GatewayOauthAuthorizationRequestRecord {
            request_id,
            client_id: client.client_id.clone(),
            redirect_uri: redirect_uri.clone(),
            redirect_uri_supplied: params.redirect_uri.is_some(),
            state: params.state,
            code_challenge: validated.code_challenge,
            resource: self.settings.resource_uri(&validated.resource),
            scope: validated.scopes.join(" "),
            status: GatewayOauthAuthorizationStatus::Pending,
            user_id: None,
            user_session_id: None,
            code_hash: None,
            code_expires_at: None,
            grant_id: None,
            expires_at: now + AUTHORIZATION_REQUEST_TTL,
            created_at: now,
            decided_at: None,
            consumed_at: None,
        };
        match self
            .repo
            .create_gateway_oauth_authorization_request(&record)
            .await
        {
            Ok(()) => Ok(request_id),
            // Cleanup can delete a long-unused client between the lookup
            // above and this insert, which then fails its foreign key. That
            // is the outcome cleanup intended, so report an expired
            // registration rather than a server error.
            Err(StoreError::Conflict(_)) if self.client_was_removed(&client.client_id).await => {
                Err(AuthorizationRejection::Display(OauthError::invalid_client(
                    "client registration has expired; register the client again",
                )))
            }
            Err(error) => Err(reject(error.into())),
        }
    }

    async fn client_was_removed(&self, client_id: &str) -> bool {
        matches!(
            self.repo.get_gateway_oauth_client(client_id).await,
            Ok(None)
        )
    }

    async fn validate_authorization_params(
        &self,
        client: &GatewayOauthClientRecord,
        params: &AuthorizationParams,
    ) -> Result<ValidatedAuthorization, OauthError> {
        match params.response_type.as_deref() {
            Some("code") => {}
            Some(_) => {
                return Err(OauthError::unsupported_response_type(
                    "only response_type=code is supported",
                ));
            }
            None => return Err(OauthError::invalid_request("response_type is required")),
        }
        if !client.allows_grant_type("authorization_code") {
            return Err(OauthError::unauthorized_client(
                "client is not registered for the authorization_code grant",
            ));
        }
        let code_challenge = params
            .code_challenge
            .as_deref()
            .ok_or_else(|| OauthError::invalid_request("code_challenge is required"))?;
        if params.code_challenge_method.as_deref() != Some("S256") {
            return Err(OauthError::invalid_request(
                "code_challenge_method must be S256",
            ));
        }
        if !is_valid_s256_challenge(code_challenge) {
            return Err(OauthError::invalid_request(
                "code_challenge is not a valid S256 challenge",
            ));
        }
        if params
            .state
            .as_ref()
            .is_some_and(|state| state.len() > MAX_STATE_LENGTH)
        {
            return Err(OauthError::invalid_request("state is too long"));
        }
        let resource = self.resolve_resource(params.resource.as_deref()).await?;
        let scopes = resolve_scopes(&resource, params.scope.as_deref())?;
        Ok(ValidatedAuthorization {
            code_challenge: code_challenge.to_string(),
            resource,
            scopes,
        })
    }

    /// Resolves `resource` to a gateway MCP resource, defaulting to `/mcp`.
    pub(crate) async fn resolve_resource(
        &self,
        raw: Option<&str>,
    ) -> Result<McpOauthResource, OauthError> {
        let Some(raw) = raw else {
            return Ok(McpOauthResource::Aggregate);
        };
        let resource = self.settings.parse_resource(raw).ok_or_else(|| {
            OauthError::invalid_target("resource is not an MCP resource of this gateway")
        })?;
        if let McpOauthResource::Server(server_key) = &resource {
            self.active_server_name(server_key)
                .await?
                .ok_or_else(|| OauthError::invalid_target("MCP server is not available"))?;
        }
        Ok(resource)
    }

    async fn active_server_name(&self, server_key: &str) -> Result<Option<String>, OauthError> {
        Ok(self
            .repo
            .get_external_mcp_server_by_key(server_key)
            .await?
            .filter(|server| server.status == ExternalMcpServerStatus::Active)
            .map(|server| server.display_name))
    }

    /// Loads a request that is still waiting for consent.
    pub async fn pending_authorization(
        &self,
        request_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<PendingAuthorization, GatewayError> {
        let request = self.load_pending_request(request_id, now).await?;
        let client = self
            .repo
            .get_gateway_oauth_client(&request.client_id)
            .await?
            .ok_or_else(authorization_request_not_found)?;
        let resource = self
            .settings
            .parse_resource(&request.resource)
            .ok_or_else(|| GatewayError::Internal("stored OAuth resource is invalid".into()))?;
        let server_display_name = match &resource {
            McpOauthResource::Aggregate => None,
            McpOauthResource::Server(server_key) => self
                .active_server_name(server_key)
                .await
                .map_err(|_| GatewayError::Internal("failed loading MCP server".into()))?,
        };
        // Judge the URI this request sends the code to, not the registration:
        // a client may register an HTTPS URI next to a loopback one.
        let redirect_is_loopback = is_loopback_redirect(&request.redirect_uri);
        Ok(PendingAuthorization {
            client_metadata_host: metadata_document_host(&client),
            scopes: parse_scope_list(&request.scope),
            request,
            client,
            resource,
            server_display_name,
            redirect_is_loopback,
        })
    }

    /// Records the user's consent and returns the client redirect URL.
    pub async fn decide_authorization(
        &self,
        request_id: Uuid,
        user_id: Uuid,
        user_session_id: Uuid,
        approve: bool,
        now: OffsetDateTime,
    ) -> Result<String, GatewayError> {
        let request = self.load_pending_request(request_id, now).await?;
        let (decision, redirect) = if approve {
            let code = generate_token(AUTHORIZATION_CODE_PREFIX);
            let redirect = authorization_success_redirect(
                self.settings.issuer(),
                &request.redirect_uri,
                &code,
                request.state.as_deref(),
            );
            let decision = GatewayOauthAuthorizationDecision::Approved {
                user_id,
                user_session_id,
                code_hash: oauth_token_hash(&code),
                code_expires_at: now + AUTHORIZATION_CODE_TTL,
            };
            (decision, redirect)
        } else {
            let redirect = authorization_error_redirect(
                self.settings.issuer(),
                &request.redirect_uri,
                request.state.as_deref(),
                &OauthError::access_denied("the user denied the request"),
            );
            (
                GatewayOauthAuthorizationDecision::Denied { user_id },
                redirect,
            )
        };
        if !self
            .repo
            .decide_gateway_oauth_authorization_request(request_id, &decision, now)
            .await?
        {
            return Err(authorization_request_decided());
        }
        Ok(redirect)
    }

    async fn load_pending_request(
        &self,
        request_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<GatewayOauthAuthorizationRequestRecord, GatewayError> {
        let request = self
            .repo
            .get_gateway_oauth_authorization_request(request_id)
            .await?
            .filter(|request| request.expires_at > now)
            .ok_or_else(authorization_request_not_found)?;
        if request.status != GatewayOauthAuthorizationStatus::Pending {
            return Err(authorization_request_decided());
        }
        Ok(request)
    }
}

struct ValidatedAuthorization {
    code_challenge: String,
    resource: McpOauthResource,
    scopes: Vec<String>,
}

fn authorization_request_not_found() -> GatewayError {
    GatewayError::Store(StoreError::NotFound(
        "authorization request was not found or has expired".to_string(),
    ))
}

fn authorization_request_decided() -> GatewayError {
    GatewayError::Store(StoreError::Conflict(
        "authorization request was already decided".to_string(),
    ))
}

/// Picks the redirect URI: the requested one if registered, or the only
/// registered non-loopback URI when the client sent none.
fn resolve_redirect_uri(
    client: &GatewayOauthClientRecord,
    requested: Option<&str>,
) -> Result<String, OauthError> {
    match requested {
        Some(requested) if redirect_uri_is_registered(&client.redirect_uris, requested) => {
            Ok(requested.to_string())
        }
        Some(_) => Err(OauthError::invalid_request(
            "redirect_uri is not registered for this client",
        )),
        None => match client.redirect_uris.as_slice() {
            [only] if !is_loopback_redirect(only) => Ok(only.clone()),
            _ => Err(OauthError::invalid_request("redirect_uri is required")),
        },
    }
}

/// Defaults the scope from the resource and rejects scopes it cannot grant.
pub(crate) fn resolve_scopes(
    resource: &McpOauthResource,
    raw: Option<&str>,
) -> Result<Vec<String>, OauthError> {
    let scopes = raw.map(parse_scope_list).unwrap_or_default();
    if scopes.is_empty() {
        return Ok(vec![resource.required_scope().to_string()]);
    }
    if let Some(scope) = scopes.iter().find(|scope| !resource.allows_scope(scope)) {
        return Err(OauthError::invalid_scope(format!(
            "scope `{scope}` cannot be granted for this resource"
        )));
    }
    Ok(scopes)
}

fn redirect_with(redirect_uri: &str, params: &[(&str, &str)]) -> String {
    let Ok(mut url) = Url::parse(redirect_uri) else {
        return redirect_uri.to_string();
    };
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in params {
            query.append_pair(key, value);
        }
    }
    url.into()
}

fn authorization_success_redirect(
    issuer: &str,
    redirect_uri: &str,
    code: &str,
    state: Option<&str>,
) -> String {
    let mut params = vec![("code", code)];
    if let Some(state) = state {
        params.push(("state", state));
    }
    params.push(("iss", issuer));
    redirect_with(redirect_uri, &params)
}

/// The RFC 6749 section 4.1.2.1 error redirect, with `iss` (RFC 9207).
pub(crate) fn authorization_error_redirect(
    issuer: &str,
    redirect_uri: &str,
    state: Option<&str>,
    error: &OauthError,
) -> String {
    let mut params = vec![
        ("error", error.error),
        ("error_description", error.description.as_str()),
    ];
    if let Some(state) = state {
        params.push(("state", state));
    }
    params.push(("iss", issuer));
    redirect_with(redirect_uri, &params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_append_to_an_existing_query() {
        assert_eq!(
            authorization_success_redirect(
                "https://oceans.example.com",
                "http://127.0.0.1:4000/cb?client=1",
                "code123",
                Some("st ate"),
            ),
            "http://127.0.0.1:4000/cb?client=1&code=code123&state=st+ate&iss=https%3A%2F%2Foceans.example.com"
        );
        assert_eq!(
            authorization_error_redirect(
                "https://oceans.example.com",
                "https://app.example.com/cb",
                None,
                &OauthError::access_denied("denied"),
            ),
            "https://app.example.com/cb?error=access_denied&error_description=denied&iss=https%3A%2F%2Foceans.example.com"
        );
    }

    #[test]
    fn scopes_default_from_the_resource() {
        assert_eq!(
            resolve_scopes(&McpOauthResource::Aggregate, None).unwrap(),
            vec!["mcp:aggregate"]
        );
        assert_eq!(
            resolve_scopes(&McpOauthResource::Server("docs".into()), Some(" ")).unwrap(),
            vec!["mcp:proxy"]
        );
        assert_eq!(
            resolve_scopes(
                &McpOauthResource::Server("docs".into()),
                Some("mcp:aggregate")
            )
            .unwrap_err()
            .error,
            "invalid_scope"
        );
        assert_eq!(
            resolve_scopes(&McpOauthResource::Aggregate, Some("mcp:aggregate admin"))
                .unwrap_err()
                .error,
            "invalid_scope"
        );
    }
}
