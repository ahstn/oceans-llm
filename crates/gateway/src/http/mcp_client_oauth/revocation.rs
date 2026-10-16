use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
};
use time::OffsetDateTime;

use super::{
    oauth_service,
    token::{no_store, oauth_error_response, parse_client_form},
};
use crate::http::state::AppState;

/// `POST /oauth/revoke`: RFC 7009 token revocation. Revoking either token
/// ends the whole grant.
pub async fn oauth_revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(service) = oauth_service(&state) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let used_basic = headers.contains_key(AUTHORIZATION);
    let outcome = async {
        let (credentials, pairs) = parse_client_form(&headers, &body)?;
        // `token_type_hint` is optional, and every token kind is looked up the same way.
        let token = pairs
            .iter()
            .find(|(key, _)| key == "token")
            .map(|(_, value)| value.as_str());
        service
            .revoke_token(&credentials, token, OffsetDateTime::now_utc())
            .await
    }
    .await;
    match outcome {
        Ok(()) => no_store(StatusCode::OK.into_response()),
        Err(error) => oauth_error_response(&error, used_basic),
    }
}
