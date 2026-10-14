use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::super::request_header_sources;

fn sources(headers: &[(&str, &str)]) -> Value {
    let headers = headers
        .iter()
        .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
        .collect::<BTreeMap<_, _>>();
    Value::Object(request_header_sources(&headers))
}

#[test]
fn records_alias_headers_without_their_values() {
    let recorded = sources(&[
        ("authorization", "Bearer unrelated-oauth-token"),
        ("x-litellm-api-key", "gwk_test.secret"),
        ("x-oceans-tags", "team=core"),
        ("x-litellm-tags", "batch"),
        ("x-litellm-session-id", "session-a"),
    ]);
    assert_eq!(
        recorded,
        json!({
            "credential": "x-litellm-api-key",
            "tags": "x-oceans-tags+x-litellm-tags",
            "session": "x-litellm-session-id",
        })
    );
    assert!(!recorded.to_string().contains("secret"));
    assert!(!recorded.to_string().contains("session-a"));
}

#[test]
fn records_native_headers_and_omits_absent_sources() {
    assert_eq!(
        sources(&[("authorization", "Bearer gwk_test.secret")]),
        json!({"credential": "authorization"})
    );
    assert_eq!(
        sources(&[
            ("x-api-key", "gwk_test.secret"),
            ("x-oceans-session-id", "session-a"),
        ]),
        json!({"credential": "x-api-key", "session": "x-oceans-session-id"})
    );
    assert_eq!(sources(&[]), json!({}));
}
