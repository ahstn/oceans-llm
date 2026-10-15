use std::collections::HashSet;

use gateway_service::OauthError;

/// The most parameters an authorization or token request may carry. Both
/// endpoints use fewer than a dozen; the cap stops an unauthenticated caller
/// from making the gateway decode and keep thousands of unknown names.
const MAX_PARAMETERS: usize = 64;

/// Parses `application/x-www-form-urlencoded` pairs. OAuth parameters must
/// not repeat (RFC 6749 section 3.1), so a repeated name is rejected.
pub(super) fn parse_unique_pairs(raw: &str) -> Result<Vec<(String, String)>, OauthError> {
    let parsed = parse_pairs(raw)?;
    match parsed.repeated {
        Some(name) => Err(repeated_parameter(&name)),
        None => Ok(parsed.pairs),
    }
}

/// Form pairs with the first occurrence of each name kept.
pub(super) struct ParsedPairs {
    pub pairs: Vec<(String, String)>,
    /// The first name that appeared more than once.
    pub repeated: Option<String>,
}

/// Parses pairs without failing on a repeated name, for the authorization
/// endpoint: it must check the client and redirect URI before deciding
/// whether that error can be sent back to the client.
pub(super) fn parse_pairs(raw: &str) -> Result<ParsedPairs, OauthError> {
    let mut seen = HashSet::new();
    let mut parsed = ParsedPairs {
        pairs: Vec::new(),
        repeated: None,
    };
    for (index, (key, value)) in url::form_urlencoded::parse(raw.as_bytes()).enumerate() {
        if index == MAX_PARAMETERS {
            return Err(OauthError::invalid_request(format!(
                "requests may carry at most {MAX_PARAMETERS} parameters"
            )));
        }
        if !seen.insert(key.clone()) {
            parsed.repeated.get_or_insert_with(|| key.into_owned());
            continue;
        }
        parsed.pairs.push((key.into_owned(), value.into_owned()));
    }
    Ok(parsed)
}

pub(super) fn repeated_parameter(name: &str) -> OauthError {
    OauthError::invalid_request(format!("parameter `{name}` is repeated"))
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
    fn keeps_the_first_value_and_reports_the_first_repeat() {
        let parsed = parse_pairs("a=1&b=2&a=3&b=4").unwrap();
        assert_eq!(
            parsed.pairs,
            vec![
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string())
            ]
        );
        assert_eq!(parsed.repeated.as_deref(), Some("a"));
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
