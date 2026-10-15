use url::Url;

use super::settings::is_loopback_host;

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
