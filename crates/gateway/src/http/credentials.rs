//! Gateway API key extraction shared by every route family, so precedence cannot drift.

use axum::http::HeaderMap;
use gateway_core::{
    AuthError, AuthenticatedApiKey, CredentialHeaderPolicy, GatewayCredential, GatewayError,
    resolve_gateway_credential,
};

use crate::http::state::AppState;

pub(crate) fn gateway_credential(
    headers: &HeaderMap,
    policy: CredentialHeaderPolicy,
) -> Result<Option<GatewayCredential<'_>>, AuthError> {
    resolve_gateway_credential(policy, |name| {
        headers
            .get(name)
            .map(|value| {
                value
                    .to_str()
                    .map_err(|_| AuthError::InvalidAuthorizationHeader)
            })
            .transpose()
    })
}

pub(crate) fn required_gateway_credential(
    headers: &HeaderMap,
    policy: CredentialHeaderPolicy,
) -> Result<GatewayCredential<'_>, AuthError> {
    gateway_credential(headers, policy)?.ok_or(AuthError::MissingAuthorizationHeader)
}

pub(crate) async fn authenticate_gateway_key(
    state: &AppState,
    headers: &HeaderMap,
    policy: CredentialHeaderPolicy,
) -> Result<AuthenticatedApiKey, GatewayError> {
    let credential = required_gateway_credential(headers, policy)?;
    state
        .service
        .authenticate_bearer_token(credential.token)
        .await
}

#[cfg(test)]
mod tests;
