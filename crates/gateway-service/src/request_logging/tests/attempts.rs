use crate::redaction::RequestLogPayloadCaptureMode;

use super::super::truncate_attempt_error_detail;
use super::{policy, policy_with_redaction_paths};

#[test]
fn attempt_error_detail_redacts_structured_payloads_with_active_policy() {
    let policy = policy_with_redaction_paths(&["message"]);
    let (detail, truncated) = truncate_attempt_error_detail(
        r#"{"message":"secret prompt","api_key":"sk-test"}"#,
        &policy,
    );

    assert!(!truncated);
    assert!(!detail.contains("secret prompt"));
    assert!(!detail.contains("sk-test"));
    assert!(detail.contains("[REDACTED]"));
}

#[test]
fn attempt_error_detail_suppresses_raw_text_when_payload_capture_is_disabled() {
    let policy = policy(RequestLogPayloadCaptureMode::SummaryOnly, 4096, 4096, 4);
    let (detail, truncated) =
        truncate_attempt_error_detail("provider leaked token sk-test", &policy);

    assert!(!truncated);
    assert!(!detail.contains("sk-test"));
    assert!(detail.starts_with("[redacted error detail; "));
    assert!(detail.ends_with(" bytes]"));
}
