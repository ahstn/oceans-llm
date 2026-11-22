use super::*;
use crate::observations::request::extract_request_metadata;
use serde_json::json;
#[test]
fn response_finish_reason_supports_openai_anthropic_and_incomplete_shapes() {
    assert_eq!(
        response_finish_reasons(&json!({"choices": [{"finish_reason": "length"}]})),
        (Some("length".to_string()), None)
    );
    assert_eq!(
        response_finish_reasons(&json!({"stop_reason": "end_turn"})),
        (Some("end_turn".to_string()), None)
    );
    assert_eq!(
        response_finish_reasons(
            &json!({"response": {"incomplete_details": {"reason": "max_output_tokens"}}})
        ),
        (None, Some("max_output_tokens".to_string()))
    );
}
fn with_passive_record(
    response_body: Option<&Value>,
    check: impl FnOnce(&ResponseObservationInput<'_>),
) {
    let metadata = extract_request_metadata(&Value::Null, &Default::default(), false, "test");
    let input = ResponseObservationInput {
        request_id: "request",
        metadata: &metadata,
        response_body,
        occurred_at: OffsetDateTime::UNIX_EPOCH,
        completed_at: OffsetDateTime::UNIX_EPOCH,
    };
    check(&input);
}

fn client_call(name: &str) -> ToolCall<'_> {
    ToolCall {
        id: None,
        name: Some(name),
        kind: ToolCallKind::Client,
        mcp_server: None,
    }
}

#[test]
fn tool_classification_omits_file_identity_and_distinguishes_overwrites() {
    with_passive_record(None, |input| {
        let observation = classify_tool_call(input, client_call("edit_file"));
        assert_eq!(observation.kind, InferredObservationKind::FileEditSuspected);
        assert!(observation.facts.opaque_file_id.is_none());
        let overwrite = classify_tool_call(input, client_call("overwrite_file"));
        assert_eq!(
            overwrite.kind,
            InferredObservationKind::FileOverwriteSuspected
        );
    });
}

#[test]
fn provider_run_tools_are_not_classified_as_file_operations() {
    with_passive_record(None, |input| {
        for (kind, name) in [
            (ToolCallKind::ProviderHosted, "web_search"),
            (ToolCallKind::ProviderMcp, "create_issue"),
            (ToolCallKind::ProviderMcp, "search"),
        ] {
            let observation = classify_tool_call(
                input,
                ToolCall {
                    kind,
                    ..client_call(name)
                },
            );
            assert_eq!(
                observation.kind,
                InferredObservationKind::ToolCallClassified,
                "{name}"
            );
            assert_eq!(observation.facts.tool_name.as_deref(), Some(name));
        }
        let client_mcp = classify_tool_call(input, client_call("mcp__fs__read_file"));
        assert_eq!(client_mcp.kind, InferredObservationKind::FileReadSuspected);
        let apply_patch = classify_tool_call(input, client_call("apply_patch"));
        assert_eq!(apply_patch.kind, InferredObservationKind::FileEditSuspected);
    });
}

fn observed_tool_names(response: &Value) -> Vec<(InferredObservationKind, Option<String>)> {
    let mut observed = Vec::new();
    with_passive_record(Some(response), |input| {
        observed = observations_for_response(input)
            .into_iter()
            .filter(|observation| {
                observation.kind != InferredObservationKind::ResponseFinishClassified
            })
            .map(|observation| (observation.kind, observation.facts.tool_name))
            .collect();
    });
    observed
}

#[test]
fn response_observations_read_tool_calls_from_every_api_format() {
    let edit = (
        InferredObservationKind::FileEditSuspected,
        Some("edit_file".to_string()),
    );
    let chat = json!({
        "choices": [{"message": {"tool_calls": [
            {"id": "call_1", "type": "function", "function": {"name": "edit_file"}}
        ]}}]
    });
    let responses_stream = json!({"stream": true, "events": [
        {"type": "response.output_item.added",
         "item": {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "edit_file"}},
        {"type": "response.completed", "response": {"output": [
            {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "edit_file"}
        ]}}
    ]});
    let messages_stream = json!({"stream": true, "events": [
        {"type": "content_block_start", "index": 0,
         "content_block": {"type": "tool_use", "id": "toolu_1", "name": "edit_file", "input": {}}},
        {"type": "content_block_delta", "index": 0,
         "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}}
    ]});

    for response in [chat, responses_stream, messages_stream] {
        assert_eq!(
            observed_tool_names(&response),
            vec![edit.clone()],
            "{response}"
        );
    }
}

#[test]
fn response_observations_ignore_tool_shapes_inside_arguments() {
    let response = json!({
        "content": [{"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {
            "nested": {"type": "function_call", "name": "edit_file"},
            "function": {"name": "write_file"}
        }}]
    });

    assert_eq!(
        observed_tool_names(&response),
        vec![(
            InferredObservationKind::ToolCallClassified,
            Some("lookup".to_string())
        )]
    );
}

#[test]
fn response_observations_bound_tool_calls() {
    let response = json!({
        "choices": [{"message": {"tool_calls": vec![
            json!({"type": "function", "function": {"name": "read_file"}});
            MAX_INFERRED_TOOL_CALLS + 1
        ]}}]
    });
    with_passive_record(Some(&response), |input| {
        let observations = observations_for_response(input);
        assert_eq!(observations.len(), MAX_INFERRED_TOOL_CALLS);
        assert!(
            observations[0]
                .limitations
                .contains(&LimitationCode::PayloadTruncated)
        );
    });
}
