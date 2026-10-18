use url::Url;

use super::{error::OauthError, settings::is_loopback_host};

const MAX_REDIRECT_URI_LENGTH: usize = 2048;

/// Checks a redirect URI offered at registration: HTTPS, or `http` on a
/// loopback host, with no fragment. Custom schemes are rejected.
pub(crate) fn validate_redirect_uri(raw: &str) -> Result<(), OauthError> {
    if raw.len() > MAX_REDIRECT_URI_LENGTH {
        return Err(OauthError::invalid_redirect_uri("redirect URI is too long"));
    }
    let url = Url::parse(raw)
        .map_err(|_| OauthError::invalid_redirect_uri("redirect URI must be an absolute URL"))?;
    if url.fragment().is_some() {
        return Err(OauthError::invalid_redirect_uri(
            "redirect URI must not include a fragment",
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(OauthError::invalid_redirect_uri(
            "redirect URI must not include user information",
        ));
    }
    match url.scheme() {
        "https" if url.host().is_some() => Ok(()),
        "http" if is_loopback_host(&url) => Ok(()),
        _ => Err(OauthError::invalid_redirect_uri(
            "redirect URI must use https, or http on localhost, 127.0.0.1, or [::1]",
        )),
    }
}

#[must_use]
pub(crate) fn is_loopback_redirect(raw: &str) -> bool {
    Url::parse(raw).is_ok_and(|url| url.scheme() == "http" && is_loopback_host(&url))
}

/// Whether `requested` matches a registered redirect URI. Matching is exact,
/// except that a loopback `http` URI may use any port (RFC 8252 section 7.3).
#[must_use]
pub(crate) fn redirect_uri_is_registered(registered: &[String], requested: &str) -> bool {
    if registered.iter().any(|candidate| candidate == requested) {
        return true;
    }
    let Some(requested) = loopback_without_port(requested) else {
        return false;
    };
    registered
        .iter()
        .filter_map(|candidate| loopback_without_port(candidate))
        .any(|candidate| candidate == requested)
}

fn loopback_without_port(raw: &str) -> Option<Url> {
    let mut url = Url::parse(raw).ok()?;
    if url.scheme() != "http" || !is_loopback_host(&url) || url.fragment().is_some() {
        return None;
    }
    url.set_port(None).ok()?;
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_accepts_https_and_loopback_http_only() {
        for valid in [
            "https://app.example.com/callback",
            "http://127.0.0.1:3000/callback",
            "http://[::1]/callback",
            "http://localhost:8765/oauth/callback?x=1",
        ] {
            assert!(validate_redirect_uri(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "http://app.example.com/callback",
            "https://app.example.com/callback#frag",
            "https://user:pw@app.example.com/callback",
            "custom-scheme://callback",
            "/relative",
        ] {
            assert_eq!(
                validate_redirect_uri(invalid).unwrap_err().error,
                "invalid_redirect_uri",
                "{invalid}"
            );
        }
    }

    #[test]
    fn matching_is_exact_except_for_loopback_ports() {
        let registered = vec![
            "https://app.example.com/callback".to_string(),
            "http://127.0.0.1/callback".to_string(),
        ];
        assert!(redirect_uri_is_registered(
            &registered,
            "https://app.example.com/callback"
        ));
        assert!(redirect_uri_is_registered(
            &registered,
            "http://127.0.0.1:49152/callback"
        ));
        assert!(!redirect_uri_is_registered(
            &registered,
            "https://app.example.com/callback/"
        ));
        assert!(!redirect_uri_is_registered(
            &registered,
            "https://app.example.com:8443/callback"
        ));
        assert!(!redirect_uri_is_registered(
            &registered,
            "http://127.0.0.1:49152/other"
        ));
        assert!(!redirect_uri_is_registered(
            &registered,
            "http://localhost:49152/callback"
        ));
    }
}
