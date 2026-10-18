use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{
            AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, PRAGMA, RETRY_AFTER, WWW_AUTHENTICATE,
        },
    },
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use gateway_service::{ClientCredentials, OauthError, TokenRequest};
use serde_json::json;
use time::OffsetDateTime;

use super::{ClientSource, form::parse_unique_pairs, oauth_service_for};
use crate::http::state::AppState;

/// `POST /oauth/token`
pub async fn oauth_token(
    State(state): State<AppState>,
    source: ClientSource,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(service) = oauth_service_for(&state, source) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let used_basic = headers.contains_key(AUTHORIZATION);
    let outcome = async {
        let (credentials, request) = parse_token_request(&headers, &body)?;
        service
            .exchange_token(&credentials, request, OffsetDateTime::now_utc())
            .await
    }
    .await;
    match outcome {
        Ok(response) => no_store(Json(response).into_response()),
        Err(error) => oauth_error_response(&error, used_basic),
    }
}

pub(super) fn parse_token_request(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(ClientCredentials, TokenRequest), OauthError> {
    let (credentials, pairs) = parse_client_form(headers, body)?;
    let mut request = TokenRequest::default();
    for (key, value) in pairs {
        let slot = match key.as_str() {
            "grant_type" => &mut request.grant_type,
            "code" => &mut request.code,
            "redirect_uri" => &mut request.redirect_uri,
            "code_verifier" => &mut request.code_verifier,
            "resource" => &mut request.resource,
            "refresh_token" => &mut request.refresh_token,
            "scope" => &mut request.scope,
            _ => continue,
        };
        *slot = Some(value);
    }
    Ok((credentials, request))
}

/// Reads a form body and the client credentials sent with it, either as
/// HTTP Basic or as `client_id` / `client_secret` fields, never both.
pub(super) fn parse_client_form(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(ClientCredentials, Vec<(String, String)>), OauthError> {
    let is_form = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| {
            value
                .trim()
                .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        });
    if !is_form {
        return Err(OauthError::invalid_request(
            "content type must be application/x-www-form-urlencoded",
        ));
    }
    let raw = std::str::from_utf8(body)
        .map_err(|_| OauthError::invalid_request("request body must be UTF-8"))?;
    let mut pairs = parse_unique_pairs(raw)?;
    let mut take = |name: &str| {
        pairs
            .iter()
            .position(|(key, _)| key == name)
            .map(|index| pairs.remove(index).1)
    };
    let form_client_id = take("client_id");
    let form_client_secret = take("client_secret");

    let credentials = if let Some(header) = headers.get(AUTHORIZATION) {
        if form_client_secret.is_some() {
            return Err(OauthError::invalid_request(
                "use only one client authentication method",
            ));
        }
        let (client_id, client_secret) = parse_basic_credentials(header)?;
        if form_client_id
            .as_deref()
            .is_some_and(|form_id| form_id != client_id)
        {
            return Err(OauthError::invalid_client(
                "client_id does not match the Authorization header",
            ));
        }
        ClientCredentials::Basic {
            client_id,
            client_secret,
        }
    } else {
        let client_id =
            form_client_id.ok_or_else(|| OauthError::invalid_client("client_id is required"))?;
        match form_client_secret {
            Some(client_secret) => ClientCredentials::Post {
                client_id,
                client_secret,
            },
            None => ClientCredentials::Public { client_id },
        }
    };
    Ok((credentials, pairs))
}

/// RFC 6749 section 2.3.1: `Basic base64(urlencode(id) ":" urlencode(secret))`.
fn parse_basic_credentials(header: &HeaderValue) -> Result<(String, String), OauthError> {
    let invalid =
        || OauthError::invalid_client("Authorization header is not valid Basic credentials");
    let value = header.to_str().map_err(|_| invalid())?;
    let (scheme, encoded) = value.split_once(' ').ok_or_else(invalid)?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return Err(invalid());
    }
    let decoded = STANDARD.decode(encoded.trim()).map_err(|_| invalid())?;
    let decoded = String::from_utf8(decoded).map_err(|_| invalid())?;
    let (client_id, client_secret) = decoded.split_once(':').ok_or_else(invalid)?;
    let decode = |part: &str| {
        url::form_urlencoded::parse(format!("v={part}").as_bytes())
            .next()
            .map(|(_, value)| value.into_owned())
            .ok_or_else(invalid)
    };
    let client_id = decode(client_id)?;
    if client_id.is_empty() {
        return Err(invalid());
    }
    Ok((client_id, decode(client_secret)?))
}

pub(super) fn oauth_error_response(error: &OauthError, used_basic: bool) -> Response {
    let status =
        StatusCode::from_u16(error.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = (
        status,
        Json(json!({
            "error": error.error,
            "error_description": error.description,
        })),
    )
        .into_response();
    if let Some(seconds) = error.retry_after_seconds {
        response
            .headers_mut()
            .insert(RETRY_AFTER, HeaderValue::from(seconds));
    }
    if status == StatusCode::UNAUTHORIZED && used_basic {
        response.headers_mut().insert(
            WWW_AUTHENTICATE,
            HeaderValue::from_static(r#"Basic realm="oceans""#),
        );
    }
    no_store(response)
}

pub(super) fn no_store(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form_headers(authorization: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded; charset=utf-8"),
        );
        if let Some(authorization) = authorization {
            headers.insert(AUTHORIZATION, HeaderValue::from_str(authorization).unwrap());
        }
        headers
    }

    #[test]
    fn reads_public_post_and_basic_credentials() {
        let (credentials, request) =
            parse_token_request(&form_headers(None), b"grant_type=x&client_id=abc").unwrap();
        assert_eq!(
            credentials,
            ClientCredentials::Public {
                client_id: "abc".to_string()
            }
        );
        assert_eq!(request.grant_type.as_deref(), Some("x"));

        let (credentials, _) =
            parse_token_request(&form_headers(None), b"client_id=abc&client_secret=s%2B1").unwrap();
        assert_eq!(
            credentials,
            ClientCredentials::Post {
                client_id: "abc".to_string(),
                client_secret: "s+1".to_string()
            }
        );

        // "a%3Ab:s%20c" -> client "a:b", secret "s c"
        let basic = format!("Basic {}", STANDARD.encode("a%3Ab:s%20c"));
        let (credentials, _) =
            parse_token_request(&form_headers(Some(&basic)), b"grant_type=x").unwrap();
        assert_eq!(
            credentials,
            ClientCredentials::Basic {
                client_id: "a:b".to_string(),
                client_secret: "s c".to_string()
            }
        );
    }

    #[test]
    fn rejects_mixed_or_missing_credentials_and_non_form_bodies() {
        let basic = format!("Basic {}", STANDARD.encode("abc:secret"));
        assert_eq!(
            parse_token_request(&form_headers(Some(&basic)), b"client_secret=x")
                .unwrap_err()
                .error,
            "invalid_request"
        );
        assert_eq!(
            parse_token_request(&form_headers(Some(&basic)), b"client_id=other")
                .unwrap_err()
                .error,
            "invalid_client"
        );
        assert_eq!(
            parse_token_request(&form_headers(None), b"grant_type=x")
                .unwrap_err()
                .error,
            "invalid_client"
        );
        let mut json_headers = HeaderMap::new();
        json_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        assert_eq!(
            parse_token_request(&json_headers, b"{}").unwrap_err().error,
            "invalid_request"
        );
    }
}
