use gateway_service::OauthError;

/// Parses `application/x-www-form-urlencoded` pairs. OAuth parameters must
/// not repeat (RFC 6749 section 3.1), so a repeated name is rejected.
pub(super) fn parse_unique_pairs(raw: &str) -> Result<Vec<(String, String)>, OauthError> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (key, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        if pairs.iter().any(|(existing, _)| *existing == key) {
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
}
