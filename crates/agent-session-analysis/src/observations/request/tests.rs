use super::*;
use crate::observations::response::tool_inventory_limitations;
use crate::{LimitationCode, identity::MAX_EXTERNAL_IDENTIFIER_BYTES};
use serde_json::json;
#[test]
fn metadata_keeps_only_policy_permitted_bounded_dimensions() {
    let request = json!({
        "messages": [{"role": "user", "content": "secret"}],
        "metadata": {"session_id": "unverified-body-session", "execution_id": "turn-1"},
        "tools": [{"type": "function", "function": {"name": "read", "description": "read"}}]
    });
    let headers = BTreeMap::from([("X-Session-Id".to_string(), "  header-session\t".to_string())]);
    let metadata = extract_request_metadata(&request, &headers, true, "opencode");
    assert_eq!(
        metadata.external_session_id.as_deref(),
        Some("header-session")
    );
    assert_eq!(
        metadata.session_source.as_deref(),
        Some("header:x-session-id")
    );
    assert_eq!(metadata.session_limitation, None);
    assert_eq!(metadata.adapter_version, "opencode-v2");
    assert_eq!(metadata.execution_id, None);
    assert_eq!(metadata.message_count, Some(1));
    assert_eq!(metadata.supplied_tool_count, Some(1));
    assert_eq!(metadata.supplied_tools.len(), 1);
    assert_eq!(metadata.supplied_tools[0].name, "read");
    assert!(metadata.supplied_tools[0].token_estimate > 0);

    let unavailable = extract_request_metadata(
        &request,
        &BTreeMap::from([("X-Session-Id".to_string(), "header-session".to_string())]),
        false,
        "opencode",
    );
    assert_eq!(
        unavailable.external_session_id.as_deref(),
        Some("header-session")
    );
    assert_eq!(unavailable.message_count, None);
    assert_eq!(unavailable.supplied_tool_count, None);
    assert!(unavailable.supplied_tools.is_empty());
}
#[test]
fn metadata_accepts_direct_and_nested_tool_name_shapes() {
    let request = json!({
        "tools": [
            {"name": "search", "input_schema": {"type": "object"}},
            {"type": "function", "function": {"name": "edit", "parameters": {}}}
        ]
    });

    let metadata = extract_request_metadata(&request, &BTreeMap::new(), true, "opencode");

    assert_eq!(
        metadata
            .supplied_tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["search", "edit"]
    );
    assert!(
        metadata
            .supplied_tools
            .iter()
            .all(|tool| tool.token_estimate > 0)
    );
    assert_eq!(metadata.supplied_tool_count, Some(2));
    assert!(tool_inventory_limitations(&metadata).is_empty());

    let partial = extract_request_metadata(
        &json!({"tools": [{"name": "search"}, {"type": "unknown"}]}),
        &BTreeMap::new(),
        true,
        "opencode",
    );
    assert_eq!(partial.supplied_tool_count, Some(2));
    assert_eq!(
        tool_inventory_limitations(&partial),
        vec![LimitationCode::ToolInventoryPotentialOnly]
    );

    let no_tools = extract_request_metadata(
        &json!({"messages": [{"role": "user", "content": "hello"}]}),
        &BTreeMap::new(),
        true,
        "opencode",
    );
    assert_eq!(no_tools.supplied_tool_count, None);
    assert!(tool_inventory_limitations(&no_tools).is_empty());
}

#[test]
fn metadata_size_facts_match_serialized_json_bytes() {
    let request = json!({
        "messages": [{
            "role": "user",
            "content": "quote: \" newline:\n multibyte: 🙂"
        }],
        "tools": [
            {
                "name": "search",
                "description": "line one\nline two 🙂",
                "input_schema": {"type": "object"}
            },
            {
                "type": "function",
                "function": {
                    "name": "edit",
                    "parameters": {"type": "object", "description": "use \\ safely"}
                }
            }
        ]
    });

    let metadata = extract_request_metadata(&request, &BTreeMap::new(), true, "opencode");
    let expected_prompt_bytes =
        u64::try_from(serde_json::to_vec(&request["messages"]).unwrap().len()).unwrap();
    let expected_tool_schema_bytes =
        u64::try_from(serde_json::to_vec(&request["tools"]).unwrap().len()).unwrap();
    let expected_tool_tokens = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| {
            u64::try_from(serde_json::to_vec(tool).unwrap().len())
                .unwrap()
                .div_ceil(4)
        })
        .collect::<Vec<_>>();

    assert_eq!(metadata.prompt_bytes, Some(expected_prompt_bytes));
    assert_eq!(metadata.tool_schema_bytes, Some(expected_tool_schema_bytes));
    assert_eq!(
        metadata
            .supplied_tools
            .iter()
            .map(|tool| tool.token_estimate)
            .collect::<Vec<_>>(),
        expected_tool_tokens
    );
}

#[test]
fn metadata_captures_bounded_skill_file_cache_and_reasoning_facts() {
    let request = json!({
        "reasoning": {"effort": "high"},
        "cache_control": {"type": "ephemeral"},
        "metadata": {
            "agent_analysis": {
                "skills": [
                    {
                        "name": "review",
                        "description_tokens": 64,
                        "body_tokens": 1200,
                        "resource_tokens": 80,
                        "used": true,
                        "abandoned": false
                    }
                ],
                "file_interactions": [
                    {
                        "opaque_file_id": "file-1",
                        "operation": "edit",
                        "tool_name": "edit",
                        "succeeded": false,
                        "error_code": "conflict"
                    }
                ]
            }
        }
    });

    let metadata = extract_request_metadata(&request, &BTreeMap::new(), true, "opencode");

    assert_eq!(metadata.supplied_skills.len(), 1);
    assert_eq!(metadata.supplied_skills[0].name, "review");
    assert_eq!(metadata.supplied_skills[0].body_token_estimate, Some(1200));
    assert_eq!(metadata.file_interactions.len(), 1);
    assert_eq!(metadata.file_interactions[0].opaque_file_id, "file-1");
    assert_eq!(
        metadata.file_interactions[0].error_signature.as_deref(),
        Some("conflict")
    );
    assert!(metadata.reasoning_config_hash.is_some());
    assert_eq!(metadata.cache_requested, Some(true));
}
#[test]
fn lineage_candidates_are_hashed_before_persistence() {
    let execution = hash_lineage_candidate("user:a", "codex", "raw-thread");
    let parent = hash_lineage_candidate("user:a", "codex", "raw-thread");
    assert_eq!(execution, parent);
    assert!(execution.starts_with("sha256:"));
    assert!(!execution.contains("raw-thread"));
    assert_ne!(
        execution,
        hash_lineage_candidate("user:b", "codex", "raw-thread")
    );
    assert_ne!(
        execution,
        hash_lineage_candidate("user:a", "claude_code", "raw-thread")
    );
}

#[test]
fn stored_hashes_use_the_session_and_adapter_namespaces() {
    let headers = |canonical: Option<&str>| {
        let mut headers = BTreeMap::from([
            (
                "x-claude-code-session-id".to_string(),
                "session-a".to_string(),
            ),
            ("x-claude-code-agent-id".to_string(), "agent-a".to_string()),
        ]);
        if let Some(canonical) = canonical {
            headers.insert("x-oceans-session-id".to_string(), canonical.to_string());
        }
        headers
    };
    let canonical =
        extract_request_metadata(&json!({}), &headers(Some("session-a")), true, "claude_cli");
    assert_eq!(
        canonical.normalized_session_id("user:a"),
        Some(hash_lineage_candidate("user:a", "oceans", "session-a"))
    );
    assert_eq!(
        canonical.hashed_lineage("user:a"),
        (
            Some(hash_lineage_candidate("user:a", "claude_code", "agent-a")),
            None
        )
    );

    let harness = extract_request_metadata(&json!({}), &headers(None), true, "claude_cli");
    assert_eq!(
        harness.normalized_session_id("user:a"),
        Some(hash_lineage_candidate("user:a", "claude_code", "session-a"))
    );

    let unknown = extract_request_metadata(
        &json!({}),
        &BTreeMap::from([("x-oceans-session-id".to_string(), "session-a".to_string())]),
        false,
        "unknown",
    );
    assert_eq!(
        unknown.normalized_session_id("user:a"),
        canonical.normalized_session_id("user:a")
    );
    assert_eq!(unknown.hashed_lineage("user:a"), (None, None));
}
#[test]
fn metadata_accepts_each_verified_session_alias_with_exact_provenance() {
    let fixtures = vec![
        (
            "claude session header",
            "claude_code",
            json!({}),
            BTreeMap::from([(
                "X-Claude-Code-Session-Id".to_string(),
                "known-session".to_string(),
            )]),
            "header:x-claude-code-session-id",
        ),
        (
            "claude cli session header",
            "claude_cli",
            json!({}),
            BTreeMap::from([(
                "x-claude-code-session-id".to_string(),
                "known-session".to_string(),
            )]),
            "header:x-claude-code-session-id",
        ),
        (
            "canonical header without a harness adapter",
            "unknown",
            json!({}),
            BTreeMap::from([(
                "X-Oceans-Session-Id".to_string(),
                "known-session".to_string(),
            )]),
            "header:x-oceans-session-id",
        ),
        (
            "codex session header",
            "codex",
            json!({}),
            BTreeMap::from([("Session-Id".to_string(), "known-session".to_string())]),
            "header:session-id",
        ),
        (
            "codex client metadata",
            "codex",
            json!({"client_metadata": {"session_id": "known-session"}}),
            BTreeMap::new(),
            "body:client_metadata.session_id",
        ),
        (
            "codex turn metadata",
            "codex",
            json!({
                "client_metadata": {
                    "x-codex-turn-metadata": "{\"session_id\":\"known-session\"}"
                }
            }),
            BTreeMap::new(),
            "body:client_metadata.x-codex-turn-metadata.session_id",
        ),
        (
            "opencode session id",
            "opencode",
            json!({}),
            BTreeMap::from([("X-Session-Id".to_string(), "known-session".to_string())]),
            "header:x-session-id",
        ),
        (
            "opencode affinity",
            "opencode",
            json!({}),
            BTreeMap::from([(
                "x-session-affinity".to_string(),
                "known-session".to_string(),
            )]),
            "header:x-session-affinity",
        ),
        (
            "opencode managed provider",
            "opencode",
            json!({}),
            BTreeMap::from([(
                "x-opencode-session".to_string(),
                "known-session".to_string(),
            )]),
            "header:x-opencode-session",
        ),
        (
            "opencode explicit conversation",
            "opencode",
            json!({}),
            BTreeMap::from([(
                "x-opencode-session-id".to_string(),
                "known-session".to_string(),
            )]),
            "header:x-opencode-session-id",
        ),
        (
            "pi responses header",
            "pi",
            json!({}),
            BTreeMap::from([("Session-Id".to_string(), "known-session".to_string())]),
            "header:session-id",
        ),
        (
            "pi openrouter header",
            "pi",
            json!({}),
            BTreeMap::from([("X-Session-Id".to_string(), "known-session".to_string())]),
            "header:x-session-id",
        ),
        (
            "claude body-only metadata",
            "claude_cli",
            json!({"metadata": {"user_id": "{\"device_id\":\"device\",\"session_id\":\"known-session\"}"}}),
            BTreeMap::new(),
            "body:metadata.user_id.session_id",
        ),
        (
            "pi session header",
            "pi",
            json!({}),
            BTreeMap::from([("Session_Id".to_string(), "known-session".to_string())]),
            "header:session_id",
        ),
        (
            "omp claude-compatible header",
            "oh_my_pi",
            json!({}),
            BTreeMap::from([(
                "x-claude-code-session-id".to_string(),
                "known-session".to_string(),
            )]),
            "header:x-claude-code-session-id",
        ),
        (
            "omp official openai header",
            "oh_my_pi",
            json!({}),
            BTreeMap::from([("session_id".to_string(), "known-session".to_string())]),
            "header:session_id",
        ),
        (
            "omp anthropic metadata",
            "oh_my_pi",
            json!({
                "metadata": {
                    "user_id": "{\"device_id\":\"device\",\"session_id\":\"known-session\"}"
                }
            }),
            BTreeMap::new(),
            "body:metadata.user_id.session_id",
        ),
        (
            "omp openrouter body",
            "oh_my_pi",
            json!({"session_id": "known-session"}),
            BTreeMap::new(),
            "body:session_id",
        ),
    ];

    for (name, harness, body, headers, source) in fixtures {
        let metadata = extract_request_metadata(&body, &headers, true, harness);
        assert_eq!(
            metadata.external_session_id.as_deref(),
            Some("known-session"),
            "{name}"
        );
        assert_eq!(metadata.session_source.as_deref(), Some(source), "{name}");
        assert_eq!(metadata.session_limitation, None, "{name}");
        assert!(
            !metadata
                .session_source
                .as_deref()
                .unwrap_or_default()
                .contains("known-session"),
            "{name}"
        );
    }
}

#[test]
fn matching_equivalent_aliases_preserve_every_source() {
    let opencode = extract_request_metadata(
        &json!({}),
        &BTreeMap::from([
            ("x-session-id".to_string(), "ses_123".to_string()),
            ("x-session-affinity".to_string(), "ses_123".to_string()),
            ("x-opencode-session".to_string(), "ses_123".to_string()),
        ]),
        true,
        "opencode",
    );
    assert_eq!(opencode.external_session_id.as_deref(), Some("ses_123"));
    assert_eq!(
        opencode.session_source.as_deref(),
        Some("header:x-session-id+header:x-session-affinity+header:x-opencode-session")
    );

    let pi = extract_request_metadata(
        &json!({}),
        &BTreeMap::from([
            ("session_id".to_string(), "session-123".to_string()),
            ("x-client-request-id".to_string(), "session-123".to_string()),
        ]),
        true,
        "pi",
    );
    assert_eq!(pi.external_session_id.as_deref(), Some("session-123"));
    assert_eq!(
        pi.session_source.as_deref(),
        Some("header:session_id+header:x-client-request-id")
    );

    let codex = extract_request_metadata(
        &json!({"client_metadata": {"session_id": "session-123"}}),
        &BTreeMap::from([("session-id".to_string(), "session-123".to_string())]),
        true,
        "codex",
    );
    assert_eq!(codex.external_session_id.as_deref(), Some("session-123"));
    assert_eq!(
        codex.session_source.as_deref(),
        Some("header:session-id+body:client_metadata.session_id")
    );
}

#[test]
fn canonical_aliases_take_precedence_over_unrecognized_spoof_fields() {
    let fixtures = [
        (
            "claude",
            "claude_code",
            json!({"metadata": {"session_id": "spoof-body"}}),
            BTreeMap::from([
                (
                    "x-claude-code-session-id".to_string(),
                    "canonical".to_string(),
                ),
                ("session-id".to_string(), "spoof-header".to_string()),
            ]),
            "header:x-claude-code-session-id",
        ),
        (
            "codex",
            "codex",
            json!({"session_id": "spoof-body"}),
            BTreeMap::from([
                ("session-id".to_string(), "canonical".to_string()),
                ("session_id".to_string(), "spoof-header".to_string()),
            ]),
            "header:session-id",
        ),
        (
            "opencode",
            "opencode",
            json!({"metadata": {"session_id": "spoof-body"}}),
            BTreeMap::from([
                (
                    "x-opencode-session".to_string(),
                    "cache-affinity".to_string(),
                ),
                ("x-opencode-session-id".to_string(), "canonical".to_string()),
            ]),
            "header:x-opencode-session-id",
        ),
        (
            "pi",
            "pi",
            json!({"session_id": "spoof-body"}),
            BTreeMap::from([
                ("session_id".to_string(), "canonical".to_string()),
                ("x-agent-session-id".to_string(), "spoof-header".to_string()),
            ]),
            "header:session_id",
        ),
        (
            "oh my pi",
            "oh_my_pi",
            json!({"metadata": {"session_id": "spoof-body"}}),
            BTreeMap::from([
                (
                    "x-claude-code-session-id".to_string(),
                    "canonical".to_string(),
                ),
                ("session-id".to_string(), "spoof-header".to_string()),
            ]),
            "header:x-claude-code-session-id",
        ),
    ];

    for (name, harness, body, headers, source) in fixtures {
        let metadata = extract_request_metadata(&body, &headers, true, harness);
        assert_eq!(
            metadata.external_session_id.as_deref(),
            Some("canonical"),
            "{name}"
        );
        assert_eq!(metadata.session_source.as_deref(), Some(source), "{name}");
        assert_eq!(metadata.session_limitation, None, "{name}");
    }

    let blocked_body = extract_request_metadata(
        &json!({"session_id": "body-session"}),
        &BTreeMap::new(),
        false,
        "oh_my_pi",
    );
    assert_eq!(blocked_body.external_session_id, None);
    assert_eq!(blocked_body.session_limitation, None);
}

#[test]
fn conflicting_session_aliases_decline_correlation_explicitly() {
    let fixtures = [
        (
            "opencode equivalent aliases",
            "opencode",
            json!({}),
            BTreeMap::from([
                ("x-session-id".to_string(), "session-a".to_string()),
                ("x-session-affinity".to_string(), "session-b".to_string()),
            ]),
        ),
        (
            "opencode provider branches",
            "opencode",
            json!({}),
            BTreeMap::from([
                ("x-session-id".to_string(), "session-a".to_string()),
                ("x-opencode-session".to_string(), "session-b".to_string()),
            ]),
        ),
        (
            "codex header and body",
            "codex",
            json!({"client_metadata": {"session_id": "session-b"}}),
            BTreeMap::from([("session-id".to_string(), "session-a".to_string())]),
        ),
        (
            "pi failed corroboration",
            "pi",
            json!({}),
            BTreeMap::from([
                ("session_id".to_string(), "session-a".to_string()),
                (
                    "x-client-request-id".to_string(),
                    "request-not-session".to_string(),
                ),
            ]),
        ),
        (
            "omp wire branches",
            "oh_my_pi",
            json!({"session_id": "session-b"}),
            BTreeMap::from([("session_id".to_string(), "session-a".to_string())]),
        ),
        (
            "canonical header and harness ID",
            "oh_my_pi",
            json!({}),
            BTreeMap::from([
                ("x-oceans-session-id".to_string(), "session-a".to_string()),
                (
                    "x-claude-code-session-id".to_string(),
                    "session-b".to_string(),
                ),
            ]),
        ),
    ];

    for (name, harness, body, headers) in fixtures {
        let metadata = extract_request_metadata(&body, &headers, true, harness);
        assert_eq!(metadata.external_session_id, None, "{name}");
        assert_eq!(metadata.session_source, None, "{name}");
        assert_eq!(metadata.session_namespace, None, "{name}");
        assert_eq!(
            metadata.session_limitation,
            Some(SessionCorrelationLimitation::ConflictingAliases),
            "{name}"
        );
    }
}

#[test]
fn malformed_session_candidates_are_rejected_instead_of_truncated() {
    let oversized = "a".repeat(MAX_EXTERNAL_IDENTIFIER_BYTES + 1);
    let fixtures = [
        (
            "illegal header character",
            "claude_code",
            json!({}),
            BTreeMap::from([(
                "x-claude-code-session-id".to_string(),
                "session/../../../other".to_string(),
            )]),
        ),
        (
            "oversized header",
            "codex",
            json!({}),
            BTreeMap::from([("session-id".to_string(), oversized)]),
        ),
        (
            "non-string body",
            "codex",
            json!({"client_metadata": {"session_id": 42}}),
            BTreeMap::new(),
        ),
        (
            "body whitespace",
            "oh_my_pi",
            json!({"session_id": " padded "}),
            BTreeMap::new(),
        ),
        (
            "malformed nested metadata",
            "oh_my_pi",
            json!({"metadata": {"user_id": "{not-json"}}),
            BTreeMap::new(),
        ),
    ];

    for (name, harness, body, headers) in fixtures {
        let metadata = extract_request_metadata(&body, &headers, true, harness);
        assert_eq!(metadata.external_session_id, None, "{name}");
        assert_eq!(
            metadata.session_limitation,
            Some(SessionCorrelationLimitation::MalformedCandidate),
            "{name}"
        );
    }
}

#[test]
fn spoofed_or_noncanonical_aliases_never_fabricate_sessions() {
    let fixtures = [
        (
            "unrecognized opencode alias",
            "opencode",
            json!({}),
            BTreeMap::from([(
                "x-opencode-conversation-id".to_string(),
                "fake-session".to_string(),
            )]),
        ),
        (
            "unrecognized pi alias",
            "pi",
            json!({}),
            BTreeMap::from([("x-agent-session-id".to_string(), "fake-session".to_string())]),
        ),
        (
            "pi request id alone",
            "pi",
            json!({}),
            BTreeMap::from([(
                "x-client-request-id".to_string(),
                "fake-session".to_string(),
            )]),
        ),
        (
            "codex prompt cache key",
            "codex",
            json!({"prompt_cache_key": "fake-session"}),
            BTreeMap::new(),
        ),
        (
            "unknown harness",
            "unknown",
            json!({"client_metadata": {"session_id": "fake-session"}}),
            BTreeMap::from([("session-id".to_string(), "fake-session".to_string())]),
        ),
    ];

    for (name, harness, body, headers) in fixtures {
        let metadata = extract_request_metadata(&body, &headers, true, harness);
        assert_eq!(metadata.external_session_id, None, "{name}");
        assert_eq!(metadata.session_source, None, "{name}");
        assert_eq!(metadata.session_limitation, None, "{name}");
    }
}

#[test]
fn verified_lineage_fields_are_bounded_and_adapter_specific() {
    let claude = extract_request_metadata(
        &json!({}),
        &BTreeMap::from([
            (
                "x-claude-code-session-id".to_string(),
                "session-a".to_string(),
            ),
            ("x-claude-code-agent-id".to_string(), "agent-a".to_string()),
            (
                "x-claude-code-parent-agent-id".to_string(),
                "agent-parent".to_string(),
            ),
        ]),
        true,
        "claude_code",
    );
    assert_eq!(claude.execution_id.as_deref(), Some("agent-a"));
    assert_eq!(claude.parent_execution_id.as_deref(), Some("agent-parent"));

    let opencode = extract_request_metadata(
        &json!({}),
        &BTreeMap::from([
            ("x-session-id".to_string(), "session-a".to_string()),
            (
                "x-parent-session-id".to_string(),
                "session-parent".to_string(),
            ),
        ]),
        true,
        "opencode",
    );
    assert_eq!(opencode.execution_id, None);
    assert_eq!(
        opencode.parent_execution_id.as_deref(),
        Some("session-parent")
    );

    let codex = extract_request_metadata(
            &json!({}),
            &BTreeMap::from([
                ("session-id".to_string(), "session-a".to_string()),
                ("thread-id".to_string(), "thread-a".to_string()),
                (
                    "x-client-request-id".to_string(),
                    "thread-a".to_string(),
                ),
                (
                    "x-codex-turn-metadata".to_string(),
                    "{\"session_id\":\"session-a\",\"thread_id\":\"thread-a\",\"turn_id\":\"turn-a\",\"parent_thread_id\":\"thread-parent\"}".to_string(),
                ),
            ]),
            true,
            "codex",
        );
    assert_eq!(codex.execution_id.as_deref(), Some("thread-a"));
    assert_eq!(codex.parent_execution_id.as_deref(), Some("thread-parent"));

    let turn_only = extract_request_metadata(
        &json!({
            "client_metadata": {
                "session_id": "session-a",
                "turn_id": "turn-a",
                "x-codex-turn-metadata": "{\"turn_id\":\"turn-a\"}"
            }
        }),
        &BTreeMap::new(),
        true,
        "codex",
    );
    assert_eq!(turn_only.execution_id.as_deref(), Some("turn-a"));
}

#[test]
fn conflicting_or_malformed_lineage_is_not_persisted_as_execution_evidence() {
    let conflicted = extract_request_metadata(
        &json!({
            "client_metadata": {
                "session_id": "session-a",
                "turn_id": "must-not-replace-conflicted-thread"
            }
        }),
        &BTreeMap::from([
            ("thread-id".to_string(), "thread-a".to_string()),
            (
                "x-client-request-id".to_string(),
                "different-thread".to_string(),
            ),
        ]),
        true,
        "codex",
    );
    assert_eq!(conflicted.external_session_id.as_deref(), Some("session-a"));
    assert_eq!(conflicted.execution_id, None);

    let malformed = extract_request_metadata(
        &json!({}),
        &BTreeMap::from([
            (
                "x-claude-code-session-id".to_string(),
                "session-a".to_string(),
            ),
            (
                "x-claude-code-agent-id".to_string(),
                "not/an/opaque-id".to_string(),
            ),
            (
                "x-claude-code-parent-agent-id".to_string(),
                "parent-a".repeat(MAX_EXTERNAL_IDENTIFIER_BYTES),
            ),
        ]),
        true,
        "claude_code",
    );
    assert_eq!(malformed.execution_id, None);
    assert_eq!(malformed.parent_execution_id, None);
}

#[test]
fn request_fact_budgets_remain_bounded_without_exposing_uninspected_body() {
    let name = "界".repeat(MAX_TOOL_NAME_CHARS + 1);
    let request = json!({
        "session_id": "body-session",
        "tools": vec![json!({"name": name}); MAX_SUPPLIED_TOOL_FACTS + 1],
        "reasoning": {"effort": "high"},
        "cache_control": {"type": "ephemeral"},
        "metadata": {"agent_analysis": {
            "skills": vec![json!({"name": name, "body_tokens": 10_000_001}); MAX_SKILL_FACTS + 1],
            "file_interactions": vec![
                json!({"opaque_file_id": name, "operation": "EDIT"});
                MAX_FILE_INTERACTION_FACTS + 1
            ],
        }},
    });
    let metadata = extract_request_metadata(&request, &BTreeMap::new(), true, "oh_my_pi");
    assert_eq!(metadata.supplied_tool_count, Some(257));
    assert_eq!(metadata.supplied_tools.len(), MAX_SUPPLIED_TOOL_FACTS);
    assert_eq!(metadata.supplied_skills.len(), MAX_SKILL_FACTS);
    assert_eq!(metadata.file_interactions.len(), MAX_FILE_INTERACTION_FACTS);
    assert_eq!(
        metadata.supplied_tools[0].name.chars().count(),
        MAX_TOOL_NAME_CHARS
    );
    assert_eq!(metadata.supplied_skills[0].body_token_estimate, None);
    assert_eq!(metadata.file_interactions[0].operation, FileOperation::Edit);
    assert_eq!(
        metadata.file_interactions[0].opaque_file_id.chars().count(),
        MAX_TOOL_NAME_CHARS,
    );
    assert_eq!(
        tool_inventory_limitations(&metadata),
        vec![LimitationCode::ToolInventoryPotentialOnly]
    );

    let hidden = extract_request_metadata(&request, &BTreeMap::new(), false, "oh_my_pi");
    assert!(!hidden.body_inspected);
    assert_eq!(hidden.external_session_id, None);
    assert!(hidden.supplied_tools.is_empty());
    assert!(hidden.supplied_skills.is_empty());
    assert!(hidden.file_interactions.is_empty());
    assert_eq!(hidden.reasoning_config_hash, None);
    assert_eq!(hidden.cache_requested, None);
}
