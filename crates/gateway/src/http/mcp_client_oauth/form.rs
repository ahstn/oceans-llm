use std::collections::HashSet;

use gateway_service::OauthError;

/// The most parameters an authorization or token request may carry. Both
/// endpoints use fewer than a dozen; the cap stops an unauthenticated caller
/// from making the gateway decode and keep thousands of unknown names.
const MAX_PARAMETERS: usize = 64;

/// Parses `application/x-www-form-urlencoded` pairs. OAuth parameters must
/// not repeat (RFC 6749 section 3.1), so a repeated name is rejected.
pub(super) fn parse_unique_pairs(raw: &str) -> Result<Vec<(String, String)>, OauthError> {
    let mut seen = HashSet::new();
    let mut pairs = Vec::new();
    for (key, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        if pairs.len() == MAX_PARAMETERS {
            return Err(OauthError::invalid_request(format!(
                "requests may carry at most {MAX_PARAMETERS} parameters"
            )));
        }
        if !seen.insert(key.clone()) {
            return Err(OauthError::invalid_request(format!(
                "parameter `{key}` is repeated"
            )));
        }
        pairs.push((key.into_owned(), value.into_owned()));
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_repeated_parameters() {
        assert_eq!(
            parse_unique_pairs("a=1&b=two+words").unwrap(),
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "two words".to_string())
            ]
        );
        assert_eq!(
            parse_unique_pairs("a=1&a=2").unwrap_err().error,
            "invalid_request"
        );
    }

    #[test]
    fn rejects_too_many_parameters() {
        let at_limit = (0..MAX_PARAMETERS)
            .map(|index| format!("p{index}=x"))
            .collect::<Vec<_>>()
            .join("&");
        assert_eq!(parse_unique_pairs(&at_limit).unwrap().len(), MAX_PARAMETERS);
        assert_eq!(
            parse_unique_pairs(&format!("{at_limit}&extra=x"))
                .unwrap_err()
                .error,
            "invalid_request"
        );
    }
}
