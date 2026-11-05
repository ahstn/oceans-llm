use std::sync::Arc;

use gateway_guardrails::{
    ContentTransformation, FailureDisposition, GuardrailEngine, ManagedCheckConfig,
    ManagedCheckKind, ManagedEvaluator, ManagedOutcome, ManagedService, PolicyMode, ReasonCode,
    test_utils::StubManagedEvaluator,
};
use serde_json::json;

use super::{
    tests::{stream_guard_context, stream_state},
    *,
};

async fn managed_state(
    phase: GuardPhase,
    outcomes: impl IntoIterator<Item = ManagedOutcome>,
) -> (tempfile::TempDir, AppState) {
    let (directory, mut state) = stream_state(PolicyMode::Deny).await;
    let config = Arc::make_mut(&mut state.guardrail_config);
    config.default.managed_checks = vec!["regression".into()];
    config.managed_checks.insert(
        "regression".into(),
        ManagedCheckConfig {
            kind: ManagedCheckKind::GoogleModelArmor,
            phases: [phase].into(),
            timeout_ms: 1_000,
            failure_disposition: FailureDisposition::FailClosed,
            max_content_bytes: 1 << 20,
            bedrock: None,
            model_armor: None,
        },
    );
    state.guardrail_engine = Arc::new(GuardrailEngine::new(
        Vec::new(),
        BTreeMap::from([(
            "regression".into(),
            Arc::new(StubManagedEvaluator::new(
                "regression",
                ManagedService::GoogleModelArmor,
                outcomes.into_iter().map(Ok),
            )) as Arc<dyn ManagedEvaluator>,
        )]),
    ));
    (directory, state)
}

fn deny() -> ManagedOutcome {
    ManagedOutcome::Intervention {
        reason_code: ReasonCode::new("regression.denied").expect("reason"),
        metadata: Default::default(),
    }
}

fn transform(text: &str) -> ManagedOutcome {
    ManagedOutcome::Transformed {
        transformation: ContentTransformation::new(text.into()),
        reason_code: ReasonCode::new("regression.masked").expect("reason"),
        metadata: Default::default(),
    }
}

fn assert_denied(error: GatewayError) {
    let GatewayError::GuardrailDenied { reason_code, .. } = error else {
        panic!("expected guardrail denial, got {error:?}");
    };
    assert_eq!(reason_code, "regression.denied");
}

#[tokio::test]
async fn denies_plain_system_prompt_without_other_text_fields() {
    let (_directory, state) = managed_state(GuardPhase::Prompt, [deny()]).await;
    let mut request = json!({"system": "forbidden instruction", "messages": []});

    let error = guard_prompt(&state, "request-1", "anthropic/claude".into(), &mut request)
        .await
        .expect_err("the top-level system string is inspected");

    assert_denied(error);
    assert_eq!(request["system"], "forbidden instruction");
}

#[tokio::test]
async fn transforms_plain_system_prompt_in_place_and_retains_it_as_context() {
    let (_directory, state) =
        managed_state(GuardPhase::Prompt, [transform(r#"["masked system"]"#)]).await;
    let mut request = json!({"system": "private instruction", "messages": []});

    let context = guard_prompt(&state, "request-1", "anthropic/claude".into(), &mut request)
        .await
        .expect("system transformation");

    assert_eq!(request, json!({"system": "masked system", "messages": []}));
    assert_eq!(context.associated_prompt.as_deref(), Some("masked system"));
    assert_eq!(
        batch_associated_prompt(&request).as_deref(),
        Some("masked system")
    );
}

#[tokio::test]
async fn retains_plain_system_and_user_text_in_associated_prompt() {
    let (_directory, state) = stream_state(PolicyMode::Deny).await;
    let mut request = json!({
        "system": "system instruction",
        "messages": [{"role": "user", "content": "user instruction"}],
    });

    let context = guard_prompt(&state, "request-1", "anthropic/claude".into(), &mut request)
        .await
        .expect("allowed prompt");

    assert_eq!(
        context.associated_prompt.as_deref(),
        Some("user instruction\nsystem instruction")
    );
    assert_eq!(context.associated_prompt, batch_associated_prompt(&request));
    assert_eq!(request["system"], "system instruction");
}

// Field, data, and blank-line endings. The last profile mixes all three
// legal forms within every event, not merely between adjacent events.
const ENDINGS: [(&str, &str, &str); 4] = [
    ("\r", "\r", "\r"),
    ("\n", "\n", "\n"),
    ("\r\n", "\r\n", "\r\n"),
    ("\r\n", "\n", "\r"),
];

fn native_frame(event: &Value, endings: (&str, &str, &str)) -> String {
    let (field, data, blank) = endings;
    let kind = event["type"].as_str().expect("native event type");
    format!(
        "event: {kind}{field}: preserve comment{field}id: frame-id{field}retry: 123{field}future-field: retain{field}data: {event}{data}{blank}"
    )
}

fn native_tool_events() -> [Value; 3] {
    [
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "tool_use", "id": "tool-1", "name": "bash", "input": {}},
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "input_json_delta", "partial_json": "{\"command\":\"rm "},
        }),
        json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "input_json_delta", "partial_json": "-rf /tmp/work\"}"},
        }),
    ]
}

fn native_text_events() -> [Value; 2] {
    [
        json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "text_delta", "text": "private "},
        }),
        json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "text_delta", "text": "response"},
        }),
    ]
}

fn native_stream(events: &[Value], endings: (&str, &str, &str)) -> String {
    events
        .iter()
        .map(|event| native_frame(event, endings))
        .collect()
}

#[tokio::test]
async fn denies_generated_tools_for_every_sse_line_ending() {
    for endings in ENDINGS {
        let (_directory, mut state) = stream_state(PolicyMode::Deny).await;
        Arc::make_mut(&mut state.guardrail_config).default.packs =
            vec![gateway_guardrails::PackId::new("core.filesystem").expect("pack")];
        let stream = native_stream(&native_tool_events(), endings);

        let error = guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
            .await
            .expect_err("native tool argument deltas must be inspected");

        let GatewayError::GuardrailDenied { reason_code, .. } = error else {
            panic!("expected destructive command denial, got {error:?}");
        };
        assert_eq!(reason_code, "filesystem.recursive_force_remove");
    }
}

#[tokio::test]
async fn denies_response_text_for_every_sse_line_ending() {
    for endings in ENDINGS {
        let (_directory, state) = managed_state(GuardPhase::ModelResponse, [deny()]).await;
        let stream = native_stream(&native_text_events(), endings);

        let error = guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
            .await
            .expect_err("native response text deltas must be inspected");

        assert_denied(error);
    }
}

#[tokio::test]
async fn transforms_generated_tools_without_rewriting_native_metadata_or_other_frames() {
    for endings in ENDINGS {
        let (_directory, state) =
            managed_state(GuardPhase::GeneratedToolCall, [transform("masked command")]).await;
        let events = native_tool_events();
        let untouched =
            ": heartbeat\r\n\r\nevent: future\rdata: { \"type\": \"future\", \"value\": 1 }\r\r";
        let stream = format!("{untouched}{}{untouched}", native_stream(&events, endings));
        let mut expected = events.clone();
        expected[1]["delta"]["partial_json"] = json!(r#"{"command":"masked command"}"#);
        expected[2]["delta"]["partial_json"] = json!("");

        let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
            .await
            .expect("transformed tool stream");

        assert_eq!(
            String::from_utf8(guarded).expect("UTF-8"),
            format!(
                "{untouched}{}{untouched}",
                native_stream(&expected, endings)
            ),
            "endings {endings:?}"
        );
    }
}

#[tokio::test]
async fn transforms_response_text_without_rewriting_native_metadata_or_other_frames() {
    for endings in ENDINGS {
        let (_directory, state) =
            managed_state(GuardPhase::ModelResponse, [transform("masked response")]).await;
        let events = native_text_events();
        let untouched = "event: future\ndata: { \"type\": \"future\", \"value\": 1 }\n\n";
        let stream = format!("{untouched}{}{untouched}", native_stream(&events, endings));
        let mut expected = events.clone();
        expected[0]["delta"]["text"] = json!("masked response");
        expected[1]["delta"]["text"] = json!("");

        let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
            .await
            .expect("transformed response stream");

        assert_eq!(
            String::from_utf8(guarded).expect("UTF-8"),
            format!(
                "{untouched}{}{untouched}",
                native_stream(&expected, endings)
            ),
            "endings {endings:?}"
        );
    }
}

#[tokio::test]
async fn preserves_multiline_data_and_all_untouched_frame_bytes() {
    let (_directory, state) = stream_state(PolicyMode::Deny).await;
    let stream = concat!(
        ": heartbeat\r\r",
        "event: future\r\nid: 42\nretry: 123\r",
        "data: { \"type\": \"future\",\n",
        ": interleaved comment\r\n",
        "data: \"value\": 1 }\r\n\r",
        "event: message_stop\ndata: { \"type\": \"message_stop\" }\n\n",
    );

    let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
        .await
        .expect("valid mixed-ending multiline events");

    assert_eq!(guarded, stream.as_bytes());
}

#[tokio::test]
async fn transforms_multiline_data_without_dropping_interleaved_comments() {
    let (_directory, state) = managed_state(GuardPhase::ModelResponse, [transform("masked")]).await;
    let stream = concat!(
        "event: content_block_delta\r",
        "data: {\"type\":\"content_block_delta\",\r",
        ": interleaved comment\n",
        "data: \"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"private\"}}\r\n\r\n",
    );
    let expected = format!(
        "event: content_block_delta\rdata: {}\r: interleaved comment\ndata:\r\n\r\n",
        json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "text_delta", "text": "masked"},
        })
    );

    let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
        .await
        .expect("multiline JSON is inspected and transformed");

    assert_eq!(String::from_utf8(guarded).expect("UTF-8"), expected);
}

#[tokio::test]
async fn multiline_transform_does_not_merge_mixed_ending_events() {
    let (_directory, state) = managed_state(GuardPhase::ModelResponse, [transform("masked")]).await;
    let future = "event: future\ndata: { \"type\": \"future\" }\n\n";
    let stream = format!(
        "event: content_block_delta\rdata: {{\"type\":\"content_block_delta\",\r\
         data: \"index\":1,\"delta\":{{\"type\":\"text_delta\",\"text\":\"private\"}}}}\n\n{future}"
    );
    let expected = format!(
        "event: content_block_delta\rdata: {}\rdata:\n\n{future}",
        json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "text_delta", "text": "masked"},
        })
    );

    let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
        .await
        .expect("event delimiter survives multiline transformation");

    assert_eq!(String::from_utf8(guarded).expect("UTF-8"), expected);
}

#[tokio::test]
async fn fragmented_native_streams_still_enforce_both_guardrail_phases() {
    for phase in [GuardPhase::GeneratedToolCall, GuardPhase::ModelResponse] {
        for endings in ENDINGS {
            let (_directory, state) = managed_state(phase, [deny()]).await;
            let events = if phase == GuardPhase::GeneratedToolCall {
                native_tool_events().to_vec()
            } else {
                native_text_events().to_vec()
            };
            let source = native_stream(&events, endings);
            // Single-byte chunks split every CRLF and every JSON/data boundary.
            let upstream: ProviderStream = Box::pin(stream::iter(
                source
                    .bytes()
                    .map(|byte| Ok(Bytes::from(vec![byte])))
                    .collect::<Vec<_>>(),
            ));

            let Err(failure) = guard_stream(&state, &stream_guard_context(1 << 20), upstream).await
            else {
                panic!("fragmented {phase:?} stream bypassed guardrails");
            };
            assert_denied(failure.error);
        }
    }
}
