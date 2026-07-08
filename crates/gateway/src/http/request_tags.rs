use axum::http::HeaderMap;
use gateway_core::{
    GatewayError, MAX_ENTITY_TAGS, RequestTag, RequestTags, validate_tag_key, validate_tag_value,
};

const HEADER_SERVICE: &str = "x-oceans-service";
const HEADER_COMPONENT: &str = "x-oceans-component";
const HEADER_ENV: &str = "x-oceans-env";
const HEADER_TAGS: &str = "x-oceans-tags";
const MAX_BESPOKE_TAGS: usize = MAX_ENTITY_TAGS;

pub fn extract_request_tags(headers: &HeaderMap) -> Result<RequestTags, GatewayError> {
    let service = extract_single_header_value(headers, HEADER_SERVICE)?;
    let component = extract_single_header_value(headers, HEADER_COMPONENT)?;
    let env = extract_single_header_value(headers, HEADER_ENV)?;
    let bespoke = extract_bespoke_tags(headers)?;

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

fn extract_bespoke_tags(headers: &HeaderMap) -> Result<Vec<RequestTag>, GatewayError> {
    let values = headers.get_all(HEADER_TAGS);
    let mut iter = values.iter();
    let Some(first) = iter.next() else {
        return Ok(Vec::new());
    };
    if iter.next().is_some() {
        return Err(GatewayError::InvalidRequest(format!(
            "header `{HEADER_TAGS}` may only be sent once"
        )));
    }

    let raw = first.to_str().map_err(|_| {
        GatewayError::InvalidRequest(format!("header `{HEADER_TAGS}` must be valid UTF-8 text"))
    })?;

    gateway_core::validate_entity_tags(
        &raw.split(';')
            .map(|entry| {
                let trimmed = entry.trim();
                if trimmed.is_empty() {
                    return Err(GatewayError::InvalidRequest(format!(
                        "header `{HEADER_TAGS}` contains an empty tag entry"
                    )));
                }
                parse_tag_pair(trimmed, HEADER_TAGS)
            })
            .collect::<Result<Vec<_>, _>>()?,
        &format!("header `{HEADER_TAGS}`"),
    )
    .map_err(|error| match error {
        GatewayError::InvalidRequest(message) if message.contains("supports at most") => {
            GatewayError::InvalidRequest(format!(
                "header `{HEADER_TAGS}` supports at most {MAX_BESPOKE_TAGS} bespoke tags"
            ))
        }
        other => other,
    })
}

fn parse_tag_pair(value: &str, context: &str) -> Result<RequestTag, GatewayError> {
    let Some((raw_key, raw_value)) = value.split_once('=') else {
        return Err(GatewayError::InvalidRequest(format!(
            "{context} must use `key=value` formatting"
        )));
    };

    Ok(RequestTag {
        key: validate_tag_key(raw_key, context)?,
        value: validate_tag_value(raw_value, context)?,
    })
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{
        HEADER_COMPONENT, HEADER_ENV, HEADER_SERVICE, HEADER_TAGS, build_bespoke_tag_filter,
        extract_request_tags,
    };

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
