use axum::{
    extract::FromRequestParts,
    http::request::Parts,
    response::{IntoResponse, Response},
};

use gateway_core::SCOPE_INFERENCE;

use super::{
    AppError, AppState, AuthenticatedApiKey, anthropic_error_response,
    extract_anthropic_authorization_header, extract_authorization_header,
};

// A parts extractor runs before Json, so rejected callers never buffer a body.
pub struct InferenceAuth(pub AuthenticatedApiKey);

impl FromRequestParts<AppState> for InferenceAuth {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let anthropic = matches!(
            parts.uri.path(),
            "/v1/messages" | "/messages" | "/v1/messages/count_tokens" | "/messages/count_tokens"
        );
        let authorization = if anthropic {
            extract_anthropic_authorization_header(&parts.headers)
        } else {
            extract_authorization_header(&parts.headers).map(str::to_owned)
        };
        state
            .service
            .authenticate_for_scope(authorization.as_deref(), SCOPE_INFERENCE)
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
