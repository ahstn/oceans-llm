use super::*;
use axum::http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
use gateway_core::{X_LITELLM_API_KEY_HEADER, X_OCEANS_API_KEY_HEADER as X_OCEANS_API_KEY};

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

#[test]
fn auth_extractor_prefers_litellm_alias_over_unrelated_authorization() {
    for value in ["gwk_id.secret", "Bearer gwk_id.secret"] {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer sk-ant-oat01-unrelated"),
        );
        headers.insert(X_LITELLM_API_KEY_HEADER, HeaderValue::from_static(value));
        assert_eq!(
            extract_mcp_gateway_api_key(&headers).expect("token"),
            "gwk_id.secret"
        );
    }
}

#[test]
fn auth_extractor_rejects_litellm_alias_conflicting_with_explicit_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        X_LITELLM_API_KEY_HEADER,
        HeaderValue::from_static("gwk_id.secret"),
    );
    headers.insert(X_OCEANS_API_KEY, HeaderValue::from_static("gwk_id.other"));
    let error = extract_mcp_gateway_api_key(&headers).expect_err("conflict");
    assert!(matches!(error, AuthError::ConflictingApiKeyHeaders));
}
