use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use gateway_service::{ClientRegistrationRequest, OauthError};
use time::OffsetDateTime;

use super::{
    ClientSource, oauth_service_for,
    token::{no_store, oauth_error_response},
};
use crate::http::state::AppState;

/// Registration bodies are small JSON documents.
pub(crate) const MAX_REGISTRATION_BODY_BYTES: usize = 16 * 1024;

/// `POST /oauth/register`: RFC 7591 Dynamic Client Registration.
pub async fn oauth_register(
    State(state): State<AppState>,
    source: ClientSource,
    body: Bytes,
) -> Response {
    let Some(service) = oauth_service_for(&state, source)
        .filter(|service| service.settings().dynamic_client_registration())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if let Err(error) = service.check_registration_limit() {
        return oauth_error_response(&error, false);
    }
    let request: ClientRegistrationRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => {
            return oauth_error_response(
                &OauthError::invalid_client_metadata("request body must be a JSON object"),
                false,
            );
        }
    };
    match service
        .register_client(request, OffsetDateTime::now_utc())
        .await
    {
        Ok(response) => no_store((StatusCode::CREATED, Json(response)).into_response()),
        Err(error) => oauth_error_response(&error, false),
    }
}
