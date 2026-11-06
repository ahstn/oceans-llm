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

struct RecordingManagedEvaluator {
    inner: StubManagedEvaluator,
    inputs: Arc<std::sync::Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl ManagedEvaluator for RecordingManagedEvaluator {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn service(&self) -> ManagedService {
        self.inner.service()
    }

    async fn evaluate(
        &self,
        input: &EvaluationInput,
    ) -> Result<ManagedOutcome, gateway_guardrails::EvaluationError> {
        self.inputs.lock().expect("recorded inputs").push(
            input
                .payload
                .text_content()
                .expect("text input")
                .to_string(),
        );
        self.inner.evaluate(input).await
    }
}

async fn recorded_response_state(
    outcomes: impl IntoIterator<Item = ManagedOutcome>,
) -> (
    tempfile::TempDir,
    AppState,
    Arc<std::sync::Mutex<Vec<String>>>,
) {
    let (directory, mut state) = managed_state(GuardPhase::ModelResponse, []).await;
    let inputs = Arc::new(std::sync::Mutex::new(Vec::new()));
    state.guardrail_engine = Arc::new(GuardrailEngine::new(
        Vec::new(),
        BTreeMap::from([(
            "regression".into(),
            Arc::new(RecordingManagedEvaluator {
                inner: StubManagedEvaluator::new(
                    "regression",
                    ManagedService::GoogleModelArmor,
                    outcomes.into_iter().map(Ok),
                ),
                inputs: inputs.clone(),
            }) as Arc<dyn ManagedEvaluator>,
        )]),
    ));
    (directory, state, inputs)
}

fn chat_replay_events(metadata_first: bool) -> Vec<Value> {
    let mut events = Vec::new();
    if metadata_first {
        events.push(
            json!({"choices":[{"delta":{"provider_metadata":{"anthropic":{
                "native_event":{"type":"content_block_start","index":0,
                    "content_block":{"type":"text","text":""}}
            }}}}]}),
        );
    }
    for text in ["private ", "answer"] {
        events.push(json!({"choices":[{"delta":{
            "content":text,
            "provider_metadata":{"anthropic":{"native_event":{
                "type":"content_block_delta","index":0,
                "delta":{"type":"text_delta","text":text}
            }}}
        }}]}));
    }
    events.push(
        json!({"choices":[{"delta":{"provider_metadata":{"anthropic":{
            "native_content":{"content":[{"type":"text","text":"private answer"}]}
        }}}}]}),
    );
    events.push(
        json!({"choices":[{"delta":{"provider_metadata":{"anthropic":{
        "native_content":{"content":[{"type":"text","text":"private answer"}]},
        "stop_sequence":"END"
    }}},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}),
    );
    events
}

fn chat_replay_stream(events: &[Value]) -> String {
    format!(
        ": heartbeat\r\n\r\n{}data: [DONE]\n\n",
        events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>()
    )
}

async fn assert_chat_replay_transform(metadata_first: bool) {
    let (_directory, state, inputs) = recorded_response_state([transform("masked answer")]).await;
    let source = chat_replay_stream(&chat_replay_events(metadata_first));

    let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
        .await
        .expect("public text transformation");
    let rendered = String::from_utf8(guarded).expect("UTF-8");
    let frames = sse_frames(&rendered)
        .filter_map(|frame| {
            let data = sse_lines(frame)
                .filter_map(|(line, _)| sse_data(line))
                .collect::<Vec<_>>()
                .join("\n");
            serde_json::from_str::<Value>(&data).ok()
        })
        .collect::<Vec<_>>();
    let public_text = frames
        .iter()
        .filter_map(|frame| frame["choices"][0]["delta"]["content"].as_str())
        .collect::<String>();

    assert_eq!(public_text, "masked answer");
    assert_eq!(*inputs.lock().expect("inputs"), ["private answer"]);
    assert!(!rendered.contains("private"));
    assert!(!rendered.contains("native_event"));
    assert!(!rendered.contains("native_content"));
    assert_eq!(
        frames.last().expect("terminal")["usage"]["total_tokens"],
        13
    );
    assert_eq!(
        frames.last().expect("terminal")["choices"][0]["delta"]["provider_metadata"]["anthropic"]["stop_sequence"],
        "END"
    );
    assert!(rendered.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn chat_replay_metadata_first_preserves_transformed_public_answer() {
    assert_chat_replay_transform(true).await;
}

#[tokio::test]
async fn chat_replay_public_first_does_not_duplicate_transform_input() {
    assert_chat_replay_transform(false).await;
}

#[tokio::test]
async fn chat_replay_unchanged_stream_is_byte_preserved_and_inspected_once() {
    let (_directory, state, inputs) = recorded_response_state([]).await;
    let source = chat_replay_stream(&chat_replay_events(true));

    let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
        .await
        .expect("unchanged stream");

    assert_eq!(guarded, source.as_bytes());
    assert_eq!(*inputs.lock().expect("inputs"), ["private answer"]);
}

#[tokio::test]
async fn chat_replay_unprojected_text_remains_subject_to_denial() {
    for field in ["native_event", "native_content", "custom"] {
        let (_directory, state) = managed_state(GuardPhase::ModelResponse, [deny()]).await;
        let events = [
            json!({"choices":[{"delta":{"provider_metadata":{"anthropic":{
            field: {"text":"unprojected forbidden answer"}
        }}}}]} ),
        ];
        let source = chat_replay_stream(&events);

        let error = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
            .await
            .expect_err("unprojected provider text must still be inspected");

        assert_denied(error);
    }
}

#[tokio::test]
async fn chat_replay_conflicting_text_is_inspected_independently() {
    for field in ["native_event", "native_content"] {
        let (_directory, state, inputs) = recorded_response_state([
            ManagedOutcome::Allow {
                reason_code: ReasonCode::new("regression.allowed").expect("reason"),
                metadata: Default::default(),
            },
            deny(),
        ])
        .await;
        let events = [json!({"choices":[{"delta":{
            "content":"public answer",
            "provider_metadata":{"anthropic":{
                field: {"text":"unprojected forbidden answer"}
            }}
        }}]})];
        let source = chat_replay_stream(&events);

        let error = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
            .await
            .expect_err("a replay copy that differs from public text must be inspected");

        assert_denied(error);
        assert_eq!(
            *inputs.lock().expect("inputs"),
            ["public answer", "unprojected forbidden answer"]
        );
    }
}

#[tokio::test]
async fn chat_replay_unprojected_native_tools_remain_subject_to_denial() {
    let (_directory, mut state) = stream_state(PolicyMode::Deny).await;
    Arc::make_mut(&mut state.guardrail_config).default.packs =
        vec![gateway_guardrails::PackId::new("core.filesystem").expect("pack")];
    let events = [
        json!({"choices":[{"delta":{"provider_metadata":{"anthropic":{
        "native_event": {
            "type":"content_block_start","index":0,
            "content_block":{"type":"tool_use","id":"tool-1","name":"bash",
                "input":{"command":"rm -rf /tmp/work"}}
        }
    }}}}]} ),
    ];
    let source = chat_replay_stream(&events);

    let error = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
        .await
        .expect_err("native executable input must still be inspected");

    let GatewayError::GuardrailDenied { reason_code, .. } = error else {
        panic!("expected destructive command denial, got {error:?}");
    };
    assert_eq!(reason_code, "filesystem.recursive_force_remove");
}

#[tokio::test]
async fn chat_replay_fix_preserves_independent_responses_snapshot_inspection() {
    let (_directory, state, inputs) = recorded_response_state([
        ManagedOutcome::Allow {
            reason_code: ReasonCode::new("regression.allowed").expect("reason"),
            metadata: Default::default(),
        },
        deny(),
    ])
    .await;
    let events = [
        json!({"type":"response.output_text.delta","output_index":0,"delta":"public answer"}),
        json!({"type":"response.completed","response":{"output":[{
            "content":[{"type":"output_text","text":"forbidden snapshot"}]
        }]}}),
    ];
    let source = chat_replay_stream(&events);

    let error = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
        .await
        .expect_err("Responses terminal text remains independently inspected");

    assert_denied(error);
    assert_eq!(
        *inputs.lock().expect("inputs"),
        ["public answer", "forbidden snapshot"]
    );
}

struct PhraseManagedEvaluator {
    outcome: ManagedOutcome,
    inputs: Arc<std::sync::Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl ManagedEvaluator for PhraseManagedEvaluator {
    fn id(&self) -> &str {
        "regression"
    }

    fn service(&self) -> ManagedService {
        ManagedService::GoogleModelArmor
    }

    async fn evaluate(
        &self,
        input: &EvaluationInput,
    ) -> Result<ManagedOutcome, gateway_guardrails::EvaluationError> {
        let text = input.payload.text_content().expect("text input");
        self.inputs.lock().expect("inputs").push(text.to_string());
        Ok(if text.contains("private answer") {
            self.outcome.clone()
        } else {
            ManagedOutcome::Allow {
                reason_code: ReasonCode::new("regression.allowed").expect("reason"),
                metadata: Default::default(),
            }
        })
    }
}

async fn phrase_response_state(
    outcome: ManagedOutcome,
) -> (
    tempfile::TempDir,
    AppState,
    Arc<std::sync::Mutex<Vec<String>>>,
) {
    let (directory, mut state) = managed_state(GuardPhase::ModelResponse, []).await;
    let inputs = Arc::new(std::sync::Mutex::new(Vec::new()));
    state.guardrail_engine = Arc::new(GuardrailEngine::new(
        Vec::new(),
        BTreeMap::from([(
            "regression".into(),
            Arc::new(PhraseManagedEvaluator {
                outcome,
                inputs: inputs.clone(),
            }) as Arc<dyn ManagedEvaluator>,
        )]),
    ));
    (directory, state, inputs)
}

fn bedrock_reasoning_delta(text: &str) -> Value {
    json!({"choices":[{"delta":{"provider_metadata":{"aws_bedrock":{
        "reasoning":{"source":"bedrock_converse_stream",
            "blocks":[{"type":"reasoning_text","text":text}]}
    }}},"finish_reason":null}]})
}

fn interrupted_reasoning_stream(with_error: bool) -> String {
    let mut source = ["private ", "answer"]
        .map(|text| format!("data: {}\n\n", bedrock_reasoning_delta(text)))
        .concat();
    if with_error {
        source.push_str(
            "data: {\"error\":{\"code\":\"upstream_failed\",\"message\":\"interrupted\"}}\n\n",
        );
    }
    source
}

#[tokio::test]
async fn chat_replay_bedrock_reasoning_interruptions_reconstruct_cross_fragment_denial() {
    for with_error in [false, true] {
        let (_directory, state, inputs) = phrase_response_state(deny()).await;
        let source = interrupted_reasoning_stream(with_error);

        let error = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
            .await
            .expect_err("a phrase split across interrupted reasoning deltas must be denied");

        assert_denied(error);
        assert!(
            inputs
                .lock()
                .expect("inputs")
                .iter()
                .any(|text| text == "private answer"),
            "the evaluator must receive the reconstructed phrase"
        );
    }
}

#[tokio::test]
async fn chat_replay_bedrock_reasoning_interruptions_transform_cross_fragment_phrase() {
    for with_error in [false, true] {
        let (_directory, state, inputs) =
            phrase_response_state(transform("masked reasoning")).await;
        let source = interrupted_reasoning_stream(with_error);

        let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
            .await
            .expect("interrupted reasoning is inspected");
        let rendered = String::from_utf8(guarded).expect("UTF-8");

        assert!(!rendered.contains("private"));
        assert!(!rendered.contains("\"text\":\"answer\""));
        assert!(
            inputs
                .lock()
                .expect("inputs")
                .iter()
                .any(|text| text == "private answer")
        );
        assert_eq!(rendered.contains("upstream_failed"), with_error);
        assert!(!rendered.contains("[DONE]"));
    }
}

#[tokio::test]
async fn chat_replay_bedrock_reasoning_terminal_copies_do_not_duplicate_transform_input() {
    for finish_reason in [json!("stop"), Value::Null] {
        let (_directory, state, inputs) =
            phrase_response_state(transform("masked reasoning")).await;
        let terminal = json!({"choices":[{"delta":{"provider_metadata":{"aws_bedrock":{
            "stop_reason":"end_turn",
            "reasoning":{"source":"bedrock_converse_stream",
                "blocks":[{"type":"reasoning_text","text":"private answer"}]},
            "native_content":{"source":"bedrock_converse","blocks":[
                {"reasoningContent":{"reasoningText":{"text":"private answer","signature":"opaque"}}},
                {"text":"public answer"}
            ]}
        }}},"finish_reason":finish_reason}],
            "usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}});
        let public = json!({"choices":[{"delta":{"content":"public answer"}}]});
        let source = format!(
            "data: {public}\n\n{}data: {terminal}\n\n",
            interrupted_reasoning_stream(false)
        );

        let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
            .await
            .expect("complete reasoning copies are inspected once");
        let rendered = String::from_utf8(guarded).expect("UTF-8");

        assert_eq!(
            *inputs.lock().expect("inputs"),
            ["public answer", "private answer"]
        );
        assert!(!rendered.contains("private"));
        assert!(!rendered.contains("opaque"));
        assert!(rendered.contains("\"content\":\"public answer\""));
        assert!(rendered.contains("\"total_tokens\":13"));
    }
}

#[tokio::test]
async fn chat_replay_bedrock_reasoning_unchanged_interruptions_are_byte_preserved() {
    for with_error in [false, true] {
        let (_directory, state, inputs) = phrase_response_state(ManagedOutcome::Allow {
            reason_code: ReasonCode::new("regression.allowed").expect("reason"),
            metadata: Default::default(),
        })
        .await;
        let source = interrupted_reasoning_stream(with_error);

        let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
            .await
            .expect("allowed reasoning stream");

        assert_eq!(guarded, source.as_bytes());
        assert!(
            inputs
                .lock()
                .expect("inputs")
                .iter()
                .any(|text| text == "private answer")
        );
    }
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

fn native_complete_tool_events(input: Value) -> [Value; 5] {
    [
        json!({
            "type": "message_start",
            "message": {
                "id": "msg-1", "type": "message", "role": "assistant",
                "model": "claude", "content": [], "stop_reason": null,
                "stop_sequence": null, "usage": {"input_tokens": 10, "output_tokens": 0},
            },
        }),
        json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "tool_use", "id": "tool-1", "name": "bash", "input": input},
        }),
        json!({"type": "content_block_stop", "index": 0}),
        json!({
            "type": "message_delta",
            "delta": {"stop_reason": "tool_use", "stop_sequence": null},
            "usage": {"output_tokens": 3},
        }),
        json!({"type": "message_stop"}),
    ]
}

#[tokio::test]
async fn native_boundary_denies_complete_tool_start_input_without_json_deltas() {
    let (_directory, mut state) = stream_state(PolicyMode::Deny).await;
    Arc::make_mut(&mut state.guardrail_config).default.packs =
        vec![gateway_guardrails::PackId::new("core.filesystem").expect("pack")];
    let events = native_complete_tool_events(json!({"command": "rm -rf /tmp/work"}));
    let source = native_stream(&events, ("\n", "\n", "\n"));
    let upstream: ProviderStream = Box::pin(stream::iter(
        source
            .bytes()
            .map(|byte| Ok(Bytes::from(vec![byte])))
            .collect::<Vec<_>>(),
    ));

    let Err(failure) = guard_stream(&state, &stream_guard_context(1 << 20), upstream).await else {
        panic!("complete native tool input bypassed generated-tool-call guardrails");
    };
    let GatewayError::GuardrailDenied { reason_code, .. } = failure.error else {
        panic!(
            "expected destructive command denial, got {:?}",
            failure.error
        );
    };
    assert_eq!(reason_code, "filesystem.recursive_force_remove");
}

#[tokio::test]
async fn native_boundary_transforms_complete_tool_start_input_as_object_preserving_terminal_frames()
{
    let (_directory, state) =
        managed_state(GuardPhase::GeneratedToolCall, [transform("masked command")]).await;
    let events = native_complete_tool_events(json!({
        "command": "private command",
        "description": "retain this argument",
    }));
    let endings = ("\r\n", "\r\n", "\r\n");
    let source = native_stream(&events, endings);
    let upstream: ProviderStream = Box::pin(stream::iter(
        source
            .bytes()
            .map(|byte| Ok(Bytes::from(vec![byte])))
            .collect::<Vec<_>>(),
    ));

    let mut guarded = guard_stream(&state, &stream_guard_context(1 << 20), upstream)
        .await
        .unwrap_or_else(|failure| panic!("tool transformation failed: {:?}", failure.error));
    let mut rendered = Vec::new();
    while let Some(chunk) = guarded.next().await {
        rendered.extend_from_slice(&chunk.expect("guarded stream chunk"));
    }
    let mut expected = events;
    expected[1]["content_block"]["input"]["command"] = json!("masked command");

    assert_eq!(
        String::from_utf8(rendered).expect("UTF-8"),
        native_stream(&expected, endings),
        "rewrite the native input object while retaining tool identity, other arguments, usage, and terminal frames"
    );
}

#[tokio::test]
async fn native_boundary_denies_conflicting_initial_or_delta_tool_input_independently() {
    for (initial, delta) in [
        ("rm -rf /tmp/work", "echo safe"),
        ("echo safe", "rm -rf /tmp/work"),
    ] {
        let (_directory, mut state) = stream_state(PolicyMode::Deny).await;
        Arc::make_mut(&mut state.guardrail_config).default.packs =
            vec![gateway_guardrails::PackId::new("core.filesystem").expect("pack")];
        let mut events = native_tool_events();
        events[0]["content_block"]["input"] = json!({"command": initial});
        events[1]["delta"]["partial_json"] = json!(json!({"command": delta}).to_string());
        events[2]["delta"]["partial_json"] = json!("");
        let source = native_stream(&events, ("\n", "\n", "\n"));

        let error = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
            .await
            .expect_err("each executable input must be inspected without concatenating copies");

        let GatewayError::GuardrailDenied { reason_code, .. } = error else {
            panic!("expected destructive command denial, got {error:?}");
        };
        assert_eq!(reason_code, "filesystem.recursive_force_remove");
    }
}

#[tokio::test]
async fn native_boundary_transforms_conflicting_initial_and_delta_tool_inputs_independently() {
    let (_directory, state) = managed_state(
        GuardPhase::GeneratedToolCall,
        [transform("masked delta"), transform("masked initial")],
    )
    .await;
    let mut events = native_tool_events();
    events[0]["content_block"]["input"] = json!({
        "command": "private initial",
        "description": "initial argument",
    });
    let delta = json!({"command": "private delta", "description": "delta argument"});
    events[1]["delta"]["partial_json"] = json!(delta.to_string());
    events[2]["delta"]["partial_json"] = json!("");
    let endings = ("\r\n", "\n", "\r");
    let source = native_stream(&events, endings);

    let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
        .await
        .expect("both executable inputs are transformed independently");
    events[0]["content_block"]["input"]["command"] = json!("masked initial");
    events[1]["delta"]["partial_json"] =
        json!(json!({"command": "masked delta", "description": "delta argument"}).to_string());

    assert_eq!(
        String::from_utf8(guarded).expect("UTF-8"),
        native_stream(&events, endings),
        "retain the object input, delta encoding, independent arguments, and framing"
    );
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

#[tokio::test]
async fn response_transforms_invalidate_ordered_replay_metadata() {
    for (namespace, native_content) in [
        (
            "gcp_vertex",
            json!({
                "parts": [{"functionCall": {"name": "bash", "args": {"command": "private command"}}}],
                "tool_call_ids": ["call-1"]
            }),
        ),
        (
            "aws_bedrock",
            json!({
                "source": "bedrock_converse",
                "blocks": [{"toolUse": {"toolUseId": "call-1", "name": "bash", "input": {"command": "private command"}}}]
            }),
        ),
    ] {
        let (_directory, state) =
            managed_state(GuardPhase::GeneratedToolCall, [transform("masked command")]).await;
        let mut response = json!({
            "choices": [{"message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call-1", "type": "function",
                    "function": {"name": "bash", "arguments": "{\"command\":\"private command\"}"}
                }],
                "provider_metadata": {
                    namespace: {
                        "native_content": native_content,
                        "additionalModelResponseFields": {"stop_sequence": "END"}
                    }
                }
            }}]
        });

        guard_model_response(&state, &stream_guard_context(1 << 20), &mut response)
            .await
            .expect("response tool transformation");

        let message = &response["choices"][0]["message"];
        let arguments: Value = serde_json::from_str(
            message["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .expect("serialized arguments"),
        )
        .expect("arguments");
        assert_eq!(arguments["command"], "masked command");
        assert!(
            message["provider_metadata"][namespace]
                .get("native_content")
                .is_none()
        );
        assert_eq!(
            message["provider_metadata"][namespace]["additionalModelResponseFields"]["stop_sequence"],
            "END"
        );
        assert!(!response.to_string().contains("private command"));
    }
}

#[tokio::test]
async fn stream_transforms_invalidate_later_replay_snapshots_without_losing_usage() {
    let (_directory, state) =
        managed_state(GuardPhase::GeneratedToolCall, [transform("masked command")]).await;
    let events = [
        json!({"choices":[{"delta":{"tool_calls":[{
            "index":0,"id":"call-1","type":"function",
            "function":{"name":"bash","arguments":"{\"command\":\"private "}
        }]}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{
            "index":0,"function":{"arguments":"command\"}"}
        }]}}]}),
        json!({"choices":[{"delta":{"provider_metadata":{"gcp_vertex":{
            "native_content":{
                "parts":[{"functionCall":{"name":"bash","args":{"command":"private command"}}}],
                "tool_call_ids":["call-1"]
            }
        }}},"finish_reason":"tool_calls"}],
        "usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}),
    ];
    let source = format!(
        "{}data: [DONE]\n\n",
        events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>()
    );

    let guarded = guard_sse_payload(&state, &stream_guard_context(1 << 20), source.as_bytes())
        .await
        .expect("stream tool transformation");
    let rendered = String::from_utf8(guarded).expect("UTF-8");

    assert!(rendered.contains("masked command"));
    assert!(!rendered.contains("private command"));
    assert!(!rendered.contains("native_content"));
    assert!(rendered.contains("\"total_tokens\":13"));
    assert!(rendered.ends_with("data: [DONE]\n\n"));
}
