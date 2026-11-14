use super::*;
use serde_json::json;

use crate::request_logging::classify_agent_harness;

fn header_map(headers: &[(&str, &str)]) -> BTreeMap<String, String> {
    headers
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn routing_session(
    body: Value,
    headers: &[(&str, &str)],
    harness: &str,
) -> Result<Option<RoutingSession>, GatewayError> {
    let extra = serde_json::from_value(body).expect("request extensions");
    extract_routing_session(&extra, &header_map(headers), harness)
}

fn passive_session(body: Value, headers: &[(&str, &str)], harness: &str) -> ClientSessionMetadata {
    extract_client_session(&body, &header_map(headers), harness)
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
        ("claude_code", "x-claude-code-session-id", "claude_code"),
        ("claude_cli", "x-claude-code-session-id", "claude_code"),
        ("codex", "session-id", "codex"),
        ("opencode", "x-session-id", "opencode"),
        ("opencode", "x-session-affinity", "opencode"),
        ("opencode", "x-opencode-session", "opencode"),
        ("pi", "session_id", "pi"),
        ("oh_my_pi", "session_id", "oh_my_pi"),
        ("oh_my_pi", "x-claude-code-session-id", "oh_my_pi"),
    ];
    for (harness, header, namespace) in cases {
        let session = routing_session(json!({}), &[(header, "session-a")], harness)
            .expect("verified harness header")
            .expect("session");
        assert_eq!(session.namespace, namespace, "{harness}");
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
    let headers = [
        ("session-id", REDACTED_VALUE),
        ("x-codex-turn-metadata", "malformed"),
    ];
    let passive = passive_session(body, &headers, "codex");
    assert_eq!(passive.session.value.as_deref(), Some("session-a"));
    assert_eq!(
        passive.session.source.as_deref(),
        Some("body:client_metadata.session_id")
    );
    assert_eq!(passive.session_namespace, Some("codex"));
    assert_eq!(passive.execution_id.as_deref(), Some("thread-a"));
    assert_eq!(passive.lineage_namespace, Some("codex"));
    assert_eq!(passive.adapter_version, "codex-v1");
}

#[test]
fn passive_reads_the_canonical_header_for_every_harness() {
    for harness in ["unknown", "oh_my_pi", "pi", "claude_cli"] {
        let passive = passive_session(
            json!({}),
            &[("X-Oceans-Session-Id", " session-a\t")],
            harness,
        );
        assert_eq!(
            passive.session.value.as_deref(),
            Some("session-a"),
            "{harness}"
        );
        assert_eq!(
            passive.session.source.as_deref(),
            Some("header:x-oceans-session-id"),
            "{harness}"
        );
        assert_eq!(passive.session.limitation, None, "{harness}");
        assert_eq!(
            passive.session_namespace,
            Some(OCEANS_SESSION_NAMESPACE),
            "{harness}"
        );
    }
    let unknown = passive_session(
        json!({}),
        &[("x-oceans-session-id", "session-a")],
        "unknown",
    );
    assert_eq!(unknown.lineage_namespace, None);
    assert_eq!(unknown.adapter_version, "unsupported-v1");
}

#[test]
fn passive_canonical_id_must_agree_with_harness_id() {
    let agreeing = passive_session(
        json!({}),
        &[
            ("x-oceans-session-id", "session-a"),
            ("x-claude-code-session-id", "session-a"),
            ("x-claude-code-agent-id", "agent-a"),
        ],
        "claude_cli",
    );
    assert_eq!(agreeing.session.value.as_deref(), Some("session-a"));
    assert_eq!(
        agreeing.session.source.as_deref(),
        Some("header:x-oceans-session-id+header:x-claude-code-session-id")
    );
    assert_eq!(agreeing.session_namespace, Some(OCEANS_SESSION_NAMESPACE));
    assert_eq!(agreeing.execution_id.as_deref(), Some("agent-a"));
    assert_eq!(agreeing.lineage_namespace, Some("claude_code"));

    let cases = [
        (
            "harness ID differs",
            "claude_cli",
            [
                ("x-oceans-session-id", "session-a"),
                ("x-claude-code-session-id", "session-b"),
            ],
            SessionCorrelationLimitation::ConflictingAliases,
        ),
        (
            "canonical case variants differ",
            "unknown",
            [
                ("X-Oceans-Session-Id", "session-a"),
                ("x-oceans-session-id", "session-b"),
            ],
            SessionCorrelationLimitation::ConflictingAliases,
        ),
        (
            "malformed canonical ID",
            "codex",
            [
                ("x-oceans-session-id", "bad/session"),
                ("session-id", "session-a"),
            ],
            SessionCorrelationLimitation::MalformedCandidate,
        ),
        (
            "malformed harness ID",
            "claude_cli",
            [
                ("x-oceans-session-id", "session-a"),
                ("x-claude-code-session-id", "bad/session"),
            ],
            SessionCorrelationLimitation::MalformedCandidate,
        ),
    ];
    for (name, harness, headers, limitation) in cases {
        let passive = passive_session(json!({}), &headers, harness);
        assert_eq!(passive.session.value, None, "{name}");
        assert_eq!(passive.session.source, None, "{name}");
        assert_eq!(passive.session.limitation, Some(limitation), "{name}");
        assert_eq!(passive.session_namespace, None, "{name}");
    }
}

#[test]
fn claude_cli_uses_the_claude_code_adapter() {
    let passive = passive_session(
        json!({}),
        &[
            ("x-claude-code-session-id", "session-a"),
            ("x-claude-code-agent-id", "agent-a"),
            ("x-claude-code-parent-agent-id", "agent-parent"),
        ],
        "claude_cli",
    );
    assert_eq!(passive.session.value.as_deref(), Some("session-a"));
    assert_eq!(
        passive.session.source.as_deref(),
        Some("header:x-claude-code-session-id")
    );
    assert_eq!(passive.session_namespace, Some("claude_code"));
    assert_eq!(passive.execution_id.as_deref(), Some("agent-a"));
    assert_eq!(passive.parent_execution_id.as_deref(), Some("agent-parent"));
    assert_eq!(passive.lineage_namespace, Some("claude_code"));
    assert_eq!(passive.adapter_version, "claude-code-v1");
}

#[test]
fn codex_exec_requests_are_read_as_codex_sessions() {
    let harness = classify_agent_harness(Some(
        "codex_exec/0.166.0 (Ubuntu 24.4.0; x86_64) xterm-256color (codex_exec; 0.166.0)",
    ));
    let session = "019a5c1e-7d2b-7c40-9a51-3f1e2d4c5b6a";
    let thread = "019a5c1e-8e3c-7d51-8b62-4a2f3e5d6c7b";
    let metadata = format!("{{\"session_id\":\"{session}\",\"thread_id\":\"{thread}\"}}");
    let passive = passive_session(
        json!({}),
        &[
            ("session-id", session),
            ("x-client-request-id", thread),
            ("x-codex-turn-metadata", &metadata),
        ],
        harness.key,
    );
    assert_eq!(passive.session.value.as_deref(), Some(session));
    assert_eq!(
        passive.session.source.as_deref(),
        Some("header:session-id+header:x-codex-turn-metadata.session_id")
    );
    assert_eq!(passive.session_namespace, Some("codex"));
    assert_eq!(passive.execution_id.as_deref(), Some(thread));
    assert_eq!(passive.lineage_namespace, Some("codex"));
}
