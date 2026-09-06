use super::*;
use axum::http::{HeaderMap, HeaderValue};

#[test]
fn auth_extractor_accepts_authorization_only() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer gwk_id.secret"),
    );
    assert_eq!(
        extract_mcp_gateway_api_key(&headers).expect("token"),
        "gwk_id.secret"
    );
}

#[test]
fn auth_extractor_accepts_explicit_header_only() {
    let mut headers = HeaderMap::new();
    headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
    assert_eq!(
        extract_mcp_gateway_api_key(&headers).expect("token"),
        "gwk_id.secret"
    );
}

#[test]
fn auth_extractor_accepts_identical_dual_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer gwk_id.secret"),
    );
    headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
    assert_eq!(
        extract_mcp_gateway_api_key(&headers).expect("token"),
        "gwk_id.secret"
    );
}

#[test]
fn auth_extractor_rejects_missing_credentials() {
    let headers = HeaderMap::new();
    let error = extract_mcp_gateway_api_key(&headers).expect_err("missing");
    assert!(matches!(error, AuthError::MissingAuthorizationHeader));
}

#[test]
fn auth_extractor_rejects_malformed_authorization_even_with_explicit_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Basic gwk_id.secret"),
    );
    headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.secret"));
    let error = extract_mcp_gateway_api_key(&headers).expect_err("malformed");
    assert!(matches!(error, AuthError::InvalidAuthorizationHeader));
}

#[test]
fn auth_extractor_rejects_conflicting_dual_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer gwk_id.secret"),
    );
    headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.other"));
    let error = extract_mcp_gateway_api_key(&headers).expect_err("conflict");
    assert!(matches!(error, AuthError::ConflictingApiKeyHeaders));
}
