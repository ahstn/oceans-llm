use axum::{
    extract::{RawQuery, State},
    http::{StatusCode, header::CACHE_CONTROL},
    response::{Html, IntoResponse, Redirect, Response},
};
use gateway_service::{AuthorizationParams, AuthorizationRejection, OauthError};
use time::OffsetDateTime;

use super::{
    form::{parse_pairs, repeated_parameter},
    oauth_service,
};
use crate::http::state::AppState;

/// Where the admin UI shows the consent screen.
const CONSENT_PATH: &str = "/admin/oauth/consent";

/// `GET /oauth/authorize`: validates the request and sends the browser to the
/// consent screen, which uses the normal admin sign-in.
pub async fn oauth_authorize(State(state): State<AppState>, RawQuery(query): RawQuery) -> Response {
    let Some(service) = oauth_service(&state) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let parsed = match parse_pairs(query.as_deref().unwrap_or_default()) {
        Ok(parsed) => parsed,
        Err(error) => return error_page(&error),
    };
    // A repeated client_id or redirect_uri leaves nothing trustworthy to
    // redirect to, wherever it appears. Any other repeat is reported to the
    // client once the service has verified both (RFC 6749 section 4.1.2.1).
    if let Some(name) = parsed
        .repeated
        .iter()
        .find(|name| matches!(name.as_str(), "client_id" | "redirect_uri"))
    {
        return error_page(&repeated_parameter(name));
    }
    let mut params = authorization_params(parsed.pairs);
    params.invalid_request = parsed.repeated.first().map(|name| repeated_parameter(name));
    match service
        .begin_authorization(params, OffsetDateTime::now_utc())
        .await
    {
        Ok(request_id) => {
            Redirect::to(&format!("{CONSENT_PATH}?request_id={request_id}")).into_response()
        }
        Err(AuthorizationRejection::Redirect(location)) => Redirect::to(&location).into_response(),
        Err(AuthorizationRejection::Display(error)) => error_page(&error),
    }
}

fn authorization_params(pairs: Vec<(String, String)>) -> AuthorizationParams {
    let mut params = AuthorizationParams::default();
    for (key, value) in pairs {
        let slot = match key.as_str() {
            "response_type" => &mut params.response_type,
            "client_id" => &mut params.client_id,
            "redirect_uri" => &mut params.redirect_uri,
            "state" => &mut params.state,
            "scope" => &mut params.scope,
            "code_challenge" => &mut params.code_challenge,
            "code_challenge_method" => &mut params.code_challenge_method,
            "resource" => &mut params.resource,
            _ => continue,
        };
        *slot = Some(value);
    }
    params
}

/// Shown when the client or redirect URI cannot be trusted, so the gateway
/// must not redirect (RFC 6749 section 4.1.2.1).
fn error_page(error: &OauthError) -> Response {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Authorization failed</title></head>\
         <body><h1>Authorization failed</h1><p>{}</p><p><code>{}</code></p>\
         <p>Return to your MCP client and start sign-in again.</p></body></html>",
        escape_html(&error.description),
        escape_html(error.error),
    );
    (
        StatusCode::BAD_REQUEST,
        [(CACHE_CONTROL, "no-store")],
        Html(body),
    )
        .into_response()
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
