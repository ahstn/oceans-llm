use axum::http::HeaderMap;
use gateway_core::{
    GatewayError, MAX_ENTITY_TAGS, RequestTag, RequestTags, validate_tag_key, validate_tag_value,
};

const HEADER_SERVICE: &str = "x-oceans-service";
const HEADER_COMPONENT: &str = "x-oceans-component";
const HEADER_ENV: &str = "x-oceans-env";
const HEADER_TAGS: &str = "x-oceans-tags";
const HEADER_LITELLM_TAGS: &str = "x-litellm-tags";
/// Value given to a bare `x-litellm-tags` entry, which becomes a key with this value.
const BARE_TAG_VALUE: &str = "true";
const MAX_BESPOKE_TAGS: usize = MAX_ENTITY_TAGS;

pub fn extract_request_tags(headers: &HeaderMap) -> Result<RequestTags, GatewayError> {
    let service = extract_single_header_value(headers, HEADER_SERVICE)?;
    let component = extract_single_header_value(headers, HEADER_COMPONENT)?;
    let env = extract_single_header_value(headers, HEADER_ENV)?;
    let bespoke = merge_alias_tags(
        extract_bespoke_tags(headers)?,
        extract_litellm_tags(headers)?,
    )?;

    Ok(RequestTags {
        service,
        component,
        env,
        bespoke,
    })
}

pub fn build_bespoke_tag_filter(key: &str, value: &str) -> Result<RequestTag, GatewayError> {
    Ok(RequestTag {
        key: validate_tag_key(key, "request log tag key")?,
        value: validate_tag_value(value, "request log tag value")?,
    })
}

fn extract_single_header_value(
    headers: &HeaderMap,
    header_name: &str,
) -> Result<Option<String>, GatewayError> {
    let values = headers.get_all(header_name);
    let mut iter = values.iter();
    let Some(first) = iter.next() else {
        return Ok(None);
    };
    if iter.next().is_some() {
        return Err(GatewayError::InvalidRequest(format!(
            "header `{header_name}` may only be sent once"
        )));
    }

    let value = first.to_str().map_err(|_| {
        GatewayError::InvalidRequest(format!("header `{header_name}` must be valid UTF-8 text"))
    })?;

    Ok(Some(validate_tag_value(value, header_name)?))
}

fn single_header_text<'a>(
    headers: &'a HeaderMap,
    header_name: &str,
) -> Result<Option<&'a str>, GatewayError> {
    let values = headers.get_all(header_name);
    let mut iter = values.iter();
    let Some(first) = iter.next() else {
        return Ok(None);
    };
    if iter.next().is_some() {
        return Err(GatewayError::InvalidRequest(format!(
            "header `{header_name}` may only be sent once"
        )));
    }
    first.to_str().map(Some).map_err(|_| {
        GatewayError::InvalidRequest(format!("header `{header_name}` must be valid UTF-8 text"))
    })
}

fn extract_bespoke_tags(headers: &HeaderMap) -> Result<Vec<RequestTag>, GatewayError> {
    let Some(raw) = single_header_text(headers, HEADER_TAGS)? else {
        return Ok(Vec::new());
    };

    let mut tags = Vec::new();
    for entry in raw.split(';') {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return Err(GatewayError::InvalidRequest(format!(
                "header `{HEADER_TAGS}` contains an empty tag entry"
            )));
        }
        tags.push(parse_tag_pair(trimmed, HEADER_TAGS)?);
    }

    if tags.len() > MAX_BESPOKE_TAGS {
        return Err(GatewayError::InvalidRequest(format!(
            "header `{HEADER_TAGS}` supports at most {MAX_BESPOKE_TAGS} bespoke tags"
        )));
    }

    gateway_core::validate_entity_tags(&tags, &format!("header `{HEADER_TAGS}`"))
}

/// Maps the comma-separated `x-litellm-tags` header onto bespoke tags. `key=value` and
/// `key:value` entries keep their key; a bare entry `name` becomes `name=true`. Entries are
/// trimmed, empty entries are dropped and repeated identical entries collapse.
fn extract_litellm_tags(headers: &HeaderMap) -> Result<Vec<RequestTag>, GatewayError> {
    let Some(raw) = single_header_text(headers, HEADER_LITELLM_TAGS)? else {
        return Ok(Vec::new());
    };
    let mut tags = Vec::new();
    for entry in raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (key, value) = entry
            .split_once('=')
            .or_else(|| entry.split_once(':'))
            .unwrap_or((entry, BARE_TAG_VALUE));
        tags.push(RequestTag {
            key: key.trim().to_string(),
            value: value.trim().to_string(),
        });
    }
    let context = format!("header `{HEADER_LITELLM_TAGS}`");
    let mut unique: Vec<RequestTag> = Vec::with_capacity(tags.len());
    for tag in tags {
        if !unique.contains(&tag) {
            unique.push(tag);
        }
    }
    if unique.len() > MAX_BESPOKE_TAGS {
        return Err(GatewayError::InvalidRequest(format!(
            "{context} supports at most {MAX_BESPOKE_TAGS} bespoke tags"
        )));
    }
    gateway_core::validate_entity_tags(&unique, &context)
}

/// `x-oceans-tags` and `x-litellm-tags` form one set: a tag sent on both collapses, while one
/// key with two different values is rejected rather than silently picking a winner.
fn merge_alias_tags(
    mut tags: Vec<RequestTag>,
    alias_tags: Vec<RequestTag>,
) -> Result<Vec<RequestTag>, GatewayError> {
    for tag in alias_tags {
        match tags.iter().find(|existing| existing.key == tag.key) {
            Some(existing) if existing.value == tag.value => {}
            Some(_) => {
                return Err(GatewayError::InvalidRequest(format!(
                    "headers `{HEADER_TAGS}` and `{HEADER_LITELLM_TAGS}` set different values for tag key `{}`",
                    tag.key
                )));
            }
            None => tags.push(tag),
        }
    }
    if tags.len() > MAX_BESPOKE_TAGS {
        return Err(GatewayError::InvalidRequest(format!(
            "headers `{HEADER_TAGS}` and `{HEADER_LITELLM_TAGS}` support at most {MAX_BESPOKE_TAGS} bespoke tags combined"
        )));
    }
    Ok(tags)
}

fn parse_tag_pair(value: &str, context: &str) -> Result<RequestTag, GatewayError> {
    let Some((raw_key, raw_value)) = value.split_once('=') else {
        return Err(GatewayError::InvalidRequest(format!(
            "{context} must use `key=value` formatting"
        )));
    };

    Ok(RequestTag {
        key: raw_key.to_string(),
        value: raw_value.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{
        HEADER_COMPONENT, HEADER_ENV, HEADER_LITELLM_TAGS, HEADER_SERVICE, HEADER_TAGS,
        build_bespoke_tag_filter, extract_request_tags,
    };

    fn bespoke(headers: &HeaderMap) -> Vec<(String, String)> {
        extract_request_tags(headers)
            .expect("request tags")
            .bespoke
            .into_iter()
            .map(|tag| (tag.key, tag.value))
            .collect()
    }

    fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn maps_litellm_tags_onto_bespoke_tags() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HEADER_LITELLM_TAGS,
            HeaderValue::from_static(" team-a, nightly,,feature=checkout , cohort:beta, nightly"),
        );
        assert_eq!(
            bespoke(&headers),
            pairs(&[
                ("team-a", "true"),
                ("nightly", "true"),
                ("feature", "checkout"),
                ("cohort", "beta"),
            ])
        );
    }

    #[test]
    fn merges_litellm_tags_with_oceans_tags() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HEADER_TAGS,
            HeaderValue::from_static("feature=checkout; cohort=beta"),
        );
        headers.insert(
            HEADER_LITELLM_TAGS,
            HeaderValue::from_static("cohort=beta, nightly"),
        );
        assert_eq!(
            bespoke(&headers),
            pairs(&[
                ("feature", "checkout"),
                ("cohort", "beta"),
                ("nightly", "true"),
            ])
        );

        headers.insert(
            HEADER_LITELLM_TAGS,
            HeaderValue::from_static("cohort=alpha"),
        );
        let error = extract_request_tags(&headers).expect_err("conflicting values must fail");
        assert!(
            error
                .to_string()
                .contains("different values for tag key `cohort`")
        );

        headers.insert(HEADER_LITELLM_TAGS, HeaderValue::from_static("a, b, c, d"));
        let error = extract_request_tags(&headers).expect_err("merged limit applies");
        assert!(
            error
                .to_string()
                .contains("at most 5 bespoke tags combined")
        );
    }

    #[test]
    fn rejects_invalid_litellm_tags_with_tag_validation_errors() {
        for (raw, message) in [
            ("Team-A", "must start with a lowercase ASCII letter"),
            ("team=Prod", "may only contain lowercase ASCII letters"),
            ("env:prod", "may not redefine reserved tag key `env`"),
            ("a=1, a=2", "contains duplicate tag key `a`"),
            ("a, b, c, d, e, f", "at most 5 bespoke tags"),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(HEADER_LITELLM_TAGS, HeaderValue::from_str(raw).unwrap());
            let error = extract_request_tags(&headers).expect_err(raw);
            assert!(error.to_string().contains(message), "{raw}: {error}");
        }
    }

    #[test]
    fn parses_universal_and_bespoke_request_tags() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_SERVICE, HeaderValue::from_static("checkout"));
        headers.insert(HEADER_COMPONENT, HeaderValue::from_static("pricing_api"));
        headers.insert(HEADER_ENV, HeaderValue::from_static("prod-eu"));
        headers.insert(
            HEADER_TAGS,
            HeaderValue::from_static("feature=guest_checkout; cohort=beta"),
        );

        let tags = extract_request_tags(&headers).expect("request tags");
        assert_eq!(tags.service.as_deref(), Some("checkout"));
        assert_eq!(tags.component.as_deref(), Some("pricing_api"));
        assert_eq!(tags.env.as_deref(), Some("prod-eu"));
        assert_eq!(tags.bespoke.len(), 2);
        assert_eq!(tags.bespoke[0].key, "feature");
        assert_eq!(tags.bespoke[0].value, "guest_checkout");
    }

    #[test]
    fn rejects_duplicate_bespoke_keys() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HEADER_TAGS,
            HeaderValue::from_static("feature=a; feature=b"),
        );

        let error = extract_request_tags(&headers).expect_err("duplicate keys must fail");
        assert!(
            error
                .to_string()
                .contains("contains duplicate tag key `feature`")
        );
    }

    #[test]
    fn rejects_reserved_bespoke_keys() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_TAGS, HeaderValue::from_static("service=checkout"));

        let error = extract_request_tags(&headers).expect_err("reserved keys must fail");
        assert!(
            error
                .to_string()
                .contains("may not redefine reserved tag key")
        );
    }

    #[test]
    fn rejects_invalid_universal_value_shape() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_SERVICE, HeaderValue::from_static("Checkout"));

        let error = extract_request_tags(&headers).expect_err("invalid universal value must fail");
        assert!(error.to_string().contains("lowercase ASCII letters"));
    }

    #[test]
    fn builds_explicit_bespoke_tag_filter() {
        let tag = build_bespoke_tag_filter("feature", "guest_checkout").expect("valid filter");
        assert_eq!(tag.key, "feature");
        assert_eq!(tag.value, "guest_checkout");
    }

    #[test]
    fn rejects_too_many_bespoke_tags() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HEADER_TAGS,
            HeaderValue::from_static("a=one; b=two; c=three; d=four; e=five; f=six"),
        );

        let error = extract_request_tags(&headers).expect_err("too many rejected");
        assert!(error.to_string().contains("at most 5 bespoke tags"));
    }
}
