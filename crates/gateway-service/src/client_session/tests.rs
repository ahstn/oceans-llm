use super::*;
use serde_json::json;

fn routing_session(
    body: Value,
    headers: &[(&str, &str)],
    harness: &str,
) -> Result<Option<RoutingSession>, GatewayError> {
    let extra = serde_json::from_value(body).expect("request extensions");
    let headers = headers
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect();
    extract_routing_session(&extra, &headers, harness)
}

#[test]
fn canonical_id_works_without_a_recognized_harness() {
    let session = routing_session(
        json!({}),
        &[("X-Oceans-Session-Id", " \tproject_a:session-123.4\t ")],
        "unknown",
    )
    .expect("valid canonical ID")
    .expect("session");
    assert_eq!(session.namespace, "oceans");
    assert_eq!(session.value, "project_a:session-123.4");
}

#[test]
fn canonical_id_must_agree_with_harness_id() {
    let headers = [
        ("x-oceans-session-id", "session-a"),
        ("session-id", "session-a"),
    ];
    let session = routing_session(json!({}), &headers, "codex")
        .expect("matching IDs")
        .expect("session");
    assert_eq!(session.namespace, "oceans");
    assert!(matches!(
        routing_session(
            json!({"client_metadata": {"session_id": "session-b"}}),
            &headers,
            "codex"
        ),
        Err(GatewayError::InvalidRequest(message)) if message.contains("conflicting")
    ));
}

#[test]
fn routing_preserves_recognized_harness_namespaces() {
    let cases = [
        ("claude_code", "x-claude-code-session-id"),
        ("codex", "session-id"),
        ("opencode", "x-session-id"),
        ("opencode", "x-session-affinity"),
        ("opencode", "x-opencode-session"),
        ("pi", "session_id"),
        ("oh_my_pi", "session_id"),
        ("oh_my_pi", "x-claude-code-session-id"),
    ];
    for (harness, header) in cases {
        let session = routing_session(json!({}), &[(header, "session-a")], harness)
            .expect("verified harness header")
            .expect("session");
        assert_eq!(session.namespace, harness);
        assert_eq!(session.value, "session-a");
    }
}

#[test]
fn routing_reads_only_supported_body_metadata() {
    let cases = [
        (
            "codex",
            json!({"client_metadata": {"session_id": "session-a"}}),
        ),
        (
            "codex",
            json!({"client_metadata": {"x-codex-turn-metadata": "{\"session_id\":\"session-a\"}"}}),
        ),
        ("oh_my_pi", json!({"session_id": "session-a"})),
        (
            "oh_my_pi",
            json!({"metadata": {"user_id": "{\"session_id\":\"session-a\"}"}}),
        ),
    ];
    for (harness, body) in cases {
        let session = routing_session(body, &[], harness)
            .expect("verified metadata path")
            .expect("session");
        assert_eq!(session.namespace, harness);
        assert_eq!(session.value, "session-a");
    }
    let codex_header = routing_session(
        json!({}),
        &[("x-codex-turn-metadata", "{\"session_id\":\"session-a\"}")],
        "codex",
    )
    .expect("Codex header metadata")
    .expect("session");
    assert_eq!(codex_header.value, "session-a");
}

#[test]
fn routing_does_not_infer_sessions_from_prompt_cache_keys_or_lineage() {
    let body = json!({
        "prompt_cache_key": "session-a",
        "metadata": {"session_id": "session-a"},
        "messages": [{"session_id": "session-a", "content": "session-a"}],
        "input": [{"session_id": "session-a"}],
        "client_metadata": {"thread_id": "session-a", "turn_id": "turn-a"},
    });
    assert_eq!(
        routing_session(body, &[("thread-id", "session-a")], "codex")
            .expect("no recognized session"),
        None
    );
    assert_eq!(
        routing_session(json!({}), &[("x-client-request-id", "request-a")], "pi")
            .expect("request ID alone is not a session"),
        None
    );
    assert_eq!(
        routing_session(json!({}), &[("session-id", "session-a")], "unknown")
            .expect("unsupported harness"),
        None
    );
}

#[test]
fn routing_rejects_malformed_ids_without_echoing_them() {
    let oversized = "a".repeat(MAX_EXTERNAL_IDENTIFIER_BYTES + 1);
    for value in [
        "",
        "bad/session",
        "with space",
        "snowman-☃",
        REDACTED_VALUE,
        &oversized,
    ] {
        let error = routing_session(json!({}), &[("x-oceans-session-id", value)], "codex")
            .expect_err("invalid canonical ID");
        assert!(matches!(error, GatewayError::InvalidRequest(_)));
        if !value.is_empty() {
            assert!(!error.to_string().contains(value));
        }
    }
    assert!(
        routing_session(
            json!({}),
            &[(
                "x-oceans-session-id",
                &"a".repeat(MAX_EXTERNAL_IDENTIFIER_BYTES)
            )],
            "unknown"
        )
        .is_ok()
    );
}

#[test]
fn routing_rejects_case_variant_header_conflicts() {
    assert!(
        routing_session(
            json!({}),
            &[
                ("X-Oceans-Session-Id", "session-a"),
                ("x-oceans-session-id", "session-b")
            ],
            "codex"
        )
        .is_err()
    );
}

#[test]
fn malformed_recognized_metadata_cannot_be_hidden_by_a_valid_canonical_id() {
    let cases = [
        ("codex", json!({"client_metadata": {"session_id": 42}})),
        (
            "codex",
            json!({"client_metadata": {"session_id": " padded "}}),
        ),
        (
            "codex",
            json!({"client_metadata": {"x-codex-turn-metadata": "not-json"}}),
        ),
        (
            "codex",
            json!({"client_metadata": {"x-codex-turn-metadata": "[]"}}),
        ),
        ("oh_my_pi", json!({"metadata": {"user_id": "not-json"}})),
        ("oh_my_pi", json!({"session_id": REDACTED_VALUE})),
    ];
    for (harness, body) in cases {
        assert!(routing_session(body, &[("x-oceans-session-id", "session-a")], harness).is_err());
    }
    let metadata = format!("{{\"unused\":\"{}\"}}", "x".repeat(MAX_TURN_METADATA_BYTES));
    assert!(routing_session(json!({}), &[("x-codex-turn-metadata", &metadata)], "codex").is_err());
}

#[test]
fn routing_keeps_harness_alias_conflict_rules() {
    let cases = [
        (
            "pi",
            [
                ("session_id", "session-a"),
                ("x-client-request-id", "session-b"),
            ],
        ),
        (
            "opencode",
            [
                ("x-session-id", "session-a"),
                ("x-session-affinity", "session-b"),
            ],
        ),
        (
            "opencode",
            [
                ("x-session-id", "session-a"),
                ("x-opencode-session", "session-a"),
            ],
        ),
    ];
    for (harness, headers) in cases {
        assert!(routing_session(json!({}), &headers, harness).is_err());
    }
}

#[test]
fn passive_metadata_keeps_existing_redaction_and_lineage_behavior() {
    let body = json!({"client_metadata": {"session_id": "session-a", "thread_id": "thread-a"}});
    let headers = BTreeMap::from([
        ("session-id".to_string(), REDACTED_VALUE.to_string()),
        ("x-codex-turn-metadata".to_string(), "malformed".to_string()),
        (
            "x-oceans-session-id".to_string(),
            "different-session".to_string(),
        ),
    ]);
    let passive = extract_client_session(&body, &headers, "codex");
    assert_eq!(passive.session.value.as_deref(), Some("session-a"));
    assert_eq!(
        passive.session.source.as_deref(),
        Some("body:client_metadata.session_id")
    );
    assert_eq!(passive.execution_id.as_deref(), Some("thread-a"));
    assert_eq!(passive.adapter_version, "codex-v1");
}
