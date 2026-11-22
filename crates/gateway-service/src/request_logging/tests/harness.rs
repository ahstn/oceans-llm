use std::collections::BTreeMap;

use super::super::{normalized_user_agent, request_user_agent};

#[test]
fn normalizes_user_agent_with_length_cap() {
    let value = "a".repeat(600);
    let normalized = normalized_user_agent(Some(&value)).expect("normalized user agent");

    assert_eq!(normalized.len(), 512);
    assert!(normalized.chars().all(|value| value == 'a'));
}

#[test]
fn reads_user_agent_header_case_insensitively() {
    let headers = BTreeMap::from([("User-Agent".to_string(), "opencode/1.2.3".to_string())]);

    assert_eq!(
        request_user_agent(&headers).map(String::as_str),
        Some("opencode/1.2.3")
    );
}
