use axum::{
    extract::FromRequestParts,
    http::request::Parts,
    response::{IntoResponse, Response},
};

use gateway_core::CredentialHeaderPolicy;

use super::{AppError, AppState, AuthenticatedApiKey, anthropic_error_response};
use crate::http::credentials::authenticate_gateway_key;

// A parts extractor runs before Json, so rejected callers never buffer a body.
pub struct InferenceAuth(pub AuthenticatedApiKey);

impl FromRequestParts<AppState> for InferenceAuth {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let anthropic = matches!(parts.uri.path(), "/v1/messages" | "/messages");
        let policy = if anthropic {
            CredentialHeaderPolicy::BearerOrXApiKey
        } else {
            CredentialHeaderPolicy::Bearer
        };
        authenticate_gateway_key(state, &parts.headers, policy)
            .await
            .map(Self)
            .map_err(|error| {
                if anthropic {
                    anthropic_error_response(error)
                } else {
                    AppError(error).into_response()
                }
            })
    }
}
