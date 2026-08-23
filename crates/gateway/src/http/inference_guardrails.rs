use std::collections::BTreeMap;

use axum::body::Bytes;
use futures_util::{StreamExt, stream};
use gateway_core::{GatewayError, ProviderStream};
use gateway_guardrails::{
    DecisionAction, EvaluationInput, EvaluationPayload, GuardPhase, GuardrailEvaluation,
    PolicyResolver, PolicyTarget,
};
use serde_json::{Map, Value};

use crate::http::{guardrail_events::record_guardrail_evaluation, state::AppState};
const MODEL_RESPONSE_TEXT_FIELDS: &[&str] = &[
    "content",
    "output_text",
    "refusal",
    "text",
    "thinking",
    "reasoning",
    "reasoning_content",
    "reasoning_text",
];

#[derive(Debug, Clone)]
pub struct InferenceGuardContext {
    pub route_key: String,
    pub request_id: String,
    pub associated_prompt: Option<String>,
    pub enabled: bool,
    pub stream_buffer_bytes: usize,
}
pub async fn guard_prompt(
    state: &AppState,
    request_id: &str,
    route_key: String,
    request: &mut Value,
) -> Result<InferenceGuardContext, GatewayError> {
    let policy =
        PolicyResolver::new(&state.guardrail_config).resolve(PolicyTarget::ModelRoute(&route_key));
    if !policy.enabled {
        return Ok(InferenceGuardContext {
            route_key,
            request_id: request_id.to_string(),
            associated_prompt: None,
            enabled: false,
            stream_buffer_bytes: policy.stream_buffer_bytes,
        });
    }
    let associated_prompt = inspect_text_fields(
        state,
        &route_key,
        request_id,
        GuardPhase::Prompt,
        request,
        None,
        &["content", "input", "instructions", "prompt", "text"],
    )
    .await?;
    Ok(InferenceGuardContext {
        route_key,
        request_id: request_id.to_string(),
        associated_prompt,
        enabled: true,
        stream_buffer_bytes: policy.stream_buffer_bytes,
    })
}

pub fn batch_guard_context(
    state: &AppState,
    request_id: String,
    route_key: String,
    request: &Value,
) -> InferenceGuardContext {
    let policy =
        PolicyResolver::new(&state.guardrail_config).resolve(PolicyTarget::ModelRoute(&route_key));
    let associated_prompt = batch_associated_prompt(request);
    InferenceGuardContext {
        route_key,
        request_id,
        associated_prompt,
        enabled: policy.enabled,
        stream_buffer_bytes: policy.stream_buffer_bytes,
    }
}

fn batch_associated_prompt(request: &Value) -> Option<String> {
    let mut pointers = Vec::new();
    collect_string_pointers(
        request,
        "",
        &["content", "input", "instructions", "prompt", "text"],
        &mut pointers,
    );
    let associated_prompt = pointers
        .iter()
        .filter_map(|pointer| request.pointer(pointer).and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!associated_prompt.is_empty()).then_some(associated_prompt)
}

pub async fn guard_model_response(
    state: &AppState,
    context: &InferenceGuardContext,
    response: &mut Value,
) -> Result<(), GatewayError> {
    inspect_generated_tool_calls(state, context, response).await?;
    inspect_text_fields(
        state,
        &context.route_key,
        &context.request_id,
        GuardPhase::ModelResponse,
        response,
        context.associated_prompt.as_deref(),
        MODEL_RESPONSE_TEXT_FIELDS,
    )
    .await?;
    Ok(())
}

pub struct GuardStreamError {
    pub error: GatewayError,
    pub collector: Option<gateway_service::StreamResponseCollector>,
}

pub async fn guard_stream(
    state: &AppState,
    context: &InferenceGuardContext,
    mut upstream: ProviderStream,
) -> Result<ProviderStream, GuardStreamError> {
    if !context.enabled {
        return Ok(upstream);
    }
    let mut buffered = Vec::new();
    let mut collector = state.service.new_stream_response_collector();
    while let Some(chunk) = upstream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                collector.finish();
                return Err(GuardStreamError {
                    error: GatewayError::Provider(error),
                    collector: Some(collector),
                });
            }
        };
        collector.observe_chunk(&chunk);
        if buffered.len().saturating_add(chunk.len()) > context.stream_buffer_bytes {
            collector.finish();
            return Err(GuardStreamError {
                error: GatewayError::PayloadTooLarge {
                    limit_bytes: context.stream_buffer_bytes,
                },
                collector: Some(collector),
            });
        }
        buffered.extend_from_slice(&chunk);
    }
    collector.finish();
    let guarded = guard_sse_payload(state, context, &buffered)
        .await
        .map_err(|error| GuardStreamError {
            error,
            collector: Some(collector),
        })?;
    Ok(Box::pin(stream::once(
        async move { Ok(Bytes::from(guarded)) },
    )))
}

async fn guard_sse_payload(
    state: &AppState,
    context: &InferenceGuardContext,
    payload: &[u8],
) -> Result<Vec<u8>, GatewayError> {
    let source = std::str::from_utf8(payload).map_err(|error| {
        GatewayError::Internal(format!("provider stream was not UTF-8: {error}"))
    })?;
    let separator = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let event_separator = format!("{separator}{separator}");
    let mut blocks = Vec::new();
    let mut tool_fragments = BTreeMap::<String, ToolCallFragments>::new();
    let mut text_locations = Vec::<(String, usize, String)>::new();
    let mut snapshot_text_locations = Vec::<(String, usize, String)>::new();

    for block in source.split(&event_separator) {
        let event_index = blocks.len();
        let mut lines = Vec::new();
        let mut data = Vec::new();
        let mut data_index = None;
        for line in block.split(separator) {
            let Some(payload) = line.strip_prefix("data:") else {
                lines.push(StreamLine::Other(line.to_string()));
                continue;
            };
            data_index.get_or_insert(lines.len());
            data.push(payload.strip_prefix(' ').unwrap_or(payload));
        }
        if let Some(data_index) = data_index {
            let payload = data.join("\n");
            let stream_line = if payload == "[DONE]" {
                StreamLine::Done
            } else {
                let value = parse_guarded_sse_json(&payload)?;
                let mut pointers = Vec::new();
                collect_string_pointers(&value, "", MODEL_RESPONSE_TEXT_FIELDS, &mut pointers);
                let event_type = value.get("type").and_then(Value::as_str);
                if event_type.is_some_and(|kind| {
                    kind == "response.completed"
                        || kind == "response.done"
                        || kind == "response.output_text.done"
                }) {
                    snapshot_text_locations.extend(pointers.into_iter().map(|pointer| {
                        (stream_text_group(&value, &pointer), event_index, pointer)
                    }));
                } else {
                    if event_type.is_some_and(|kind| {
                        kind.ends_with("output_text.delta")
                            || kind.ends_with("reasoning_text.delta")
                    }) && value.get("delta").is_some_and(Value::is_string)
                    {
                        pointers.push("/delta".to_string());
                    }
                    text_locations.extend(pointers.into_iter().map(|pointer| {
                        (stream_text_group(&value, &pointer), event_index, pointer)
                    }));
                }
                collect_stream_tool_fragments(&value, "", event_index, &mut tool_fragments);
                StreamLine::Json(value)
            };
            lines.insert(data_index, stream_line);
        }
        blocks.push(lines);
    }

    if text_locations.is_empty() {
        text_locations.clone_from(&snapshot_text_locations);
    }
    for fragments in tool_fragments.values() {
        if fragments.name.is_empty() {
            continue;
        }
        let arguments = serde_json::from_str(&fragments.arguments)
            .unwrap_or_else(|_| Value::String(fragments.arguments.clone()));
        let payload = shell_command(&fragments.name, &arguments)
            .map(|command| EvaluationPayload::ShellCommand { command })
            .unwrap_or_else(|| EvaluationPayload::ToolCall {
                name: fragments.name.clone(),
                arguments: arguments.clone(),
            });
        let mut input = EvaluationInput::new(GuardPhase::GeneratedToolCall, payload);
        if let Some(prompt) = &context.associated_prompt {
            input = input.with_associated_prompt(prompt);
        }
        let evaluation =
            evaluate(state, &context.route_key, Some(&context.request_id), input).await;
        ensure_allowed(&evaluation)?;
        if evaluation
            .decisions
            .iter()
            .any(|decision| decision.transformed)
            && let Some(arguments) = transformed_tool_arguments(&evaluation.output, &arguments)
        {
            let replacement = if fragments.shell_command_locations {
                arguments
                    .get("command")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                serde_json::to_string(&arguments).ok()
            };
            if let Some(replacement) = replacement {
                replace_stream_arguments(
                    &mut blocks,
                    &fragments.argument_locations,
                    &replacement,
                    fragments.shell_command_locations,
                    true,
                );
                replace_stream_arguments(
                    &mut blocks,
                    &fragments.terminal_argument_locations,
                    &replacement,
                    fragments.shell_command_locations,
                    false,
                );
            }
        }
    }

    let mut text_locations_by_group = BTreeMap::<String, Vec<(usize, String)>>::new();
    for (group, event_index, pointer) in text_locations {
        text_locations_by_group
            .entry(group)
            .or_default()
            .push((event_index, pointer));
    }
    let mut snapshot_locations_by_group = BTreeMap::<String, Vec<(usize, String)>>::new();
    for (group, event_index, pointer) in snapshot_text_locations {
        snapshot_locations_by_group
            .entry(group)
            .or_default()
            .push((event_index, pointer));
    }
    for (group, locations) in text_locations_by_group {
        let combined_text = locations
            .iter()
            .filter_map(|(event_index, pointer)| {
                blocks[*event_index].iter().find_map(|line| match line {
                    StreamLine::Json(value) => value.pointer(pointer).and_then(Value::as_str),
                    _ => None,
                })
            })
            .collect::<String>();
        if combined_text.is_empty() {
            continue;
        }
        let mut input = EvaluationInput::new(
            GuardPhase::ModelResponse,
            EvaluationPayload::Text {
                text: combined_text.clone(),
            },
        );
        if let Some(prompt) = &context.associated_prompt {
            input = input.with_associated_prompt(prompt);
        }
        let evaluation =
            evaluate(state, &context.route_key, Some(&context.request_id), input).await;
        ensure_allowed(&evaluation)?;
        if let Some(transformed) = evaluation.output.text_content()
            && transformed != combined_text
        {
            replace_stream_text(&mut blocks, &locations, transformed, true);
            if let Some(snapshot_locations) = snapshot_locations_by_group.get(&group)
                && locations != *snapshot_locations
            {
                replace_stream_text(&mut blocks, snapshot_locations, transformed, true);
            }
        }
    }

    let rendered = blocks
        .into_iter()
        .map(|lines| {
            lines
                .into_iter()
                .map(|line| match line {
                    StreamLine::Json(value) => format!(
                        "data: {}",
                        serde_json::to_string(&value).expect("JSON stream event serialization")
                    ),
                    StreamLine::Done => "data: [DONE]".to_string(),
                    StreamLine::Other(line) => line,
                })
                .collect::<Vec<_>>()
                .join(separator)
        })
        .collect::<Vec<_>>()
        .join(&event_separator);
    Ok(rendered.into_bytes())
}
fn parse_guarded_sse_json(payload: &str) -> Result<Value, GatewayError> {
    serde_json::from_str(payload).map_err(|error| {
        GatewayError::Internal(format!(
            "provider stream contained invalid SSE JSON: {error}"
        ))
    })
}

fn stream_text_group(value: &Value, pointer: &str) -> String {
    let segments = pointer
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.first() == Some(&"choices")
        && let Some(choice) = segments.get(1)
    {
        return format!("choice:{choice}");
    }
    if let Some(output_index) = value.get("output_index").and_then(Value::as_u64) {
        return format!("output:{output_index}");
    }
    if let Some(position) = segments.iter().position(|segment| *segment == "output")
        && let Some(output_index) = segments.get(position + 1)
    {
        return format!("output:{output_index}");
    }
    if let Some(item_id) = value.get("item_id").and_then(Value::as_str) {
        return format!("item:{item_id}");
    }
    if let Some(content_block_index) = value.get("index").and_then(Value::as_u64) {
        return format!("content-block:{content_block_index}");
    }
    "default".to_string()
}

fn replace_stream_text(
    blocks: &mut [Vec<StreamLine>],
    locations: &[(usize, String)],
    replacement: &str,
    clear_after_first: bool,
) {
    let mut first = true;
    for (event_index, pointer) in locations {
        for line in &mut blocks[*event_index] {
            if let StreamLine::Json(value) = line
                && let Some(slot) = value.pointer_mut(pointer)
            {
                *slot = Value::String(if first || !clear_after_first {
                    replacement.to_string()
                } else {
                    String::new()
                });
                first = false;
                break;
            }
        }
    }
}

fn replace_stream_arguments(
    blocks: &mut [Vec<StreamLine>],
    locations: &[(usize, String)],
    replacement: &str,
    shell_command: bool,
    clear_after_first: bool,
) {
    let mut first = true;
    for (event_index, pointer) in locations {
        for line in &mut blocks[*event_index] {
            if let StreamLine::Json(value) = line
                && let Some(slot) = value.pointer_mut(pointer)
            {
                let value = if first || !clear_after_first {
                    replacement
                } else {
                    ""
                };
                replace_stream_argument(slot, value, shell_command);
                first = false;
                break;
            }
        }
    }
}

enum StreamLine {
    Json(Value),
    Done,
    Other(String),
}

#[derive(Default)]
struct ToolCallFragments {
    name: String,
    arguments: String,
    argument_locations: Vec<(usize, String)>,
    terminal_argument_locations: Vec<(usize, String)>,
    shell_command_locations: bool,
}

fn collect_stream_tool_fragments(
    value: &Value,
    pointer: &str,
    event_index: usize,
    output: &mut BTreeMap<String, ToolCallFragments>,
) {
    match value {
        Value::Object(object) => {
            let response_item_event = matches!(
                object.get("type").and_then(Value::as_str),
                Some("response.output_item.added" | "response.output_item.done")
            );
            if response_item_event
                && let Some(item) = object.get("item").and_then(Value::as_object)
                && let Some((name, arguments)) = parse_tool_call(item)
            {
                let key = item
                    .get("id")
                    .or_else(|| item.get("call_id"))
                    .map(identity_key)
                    .unwrap_or_else(|| "default".to_string());
                let event_type = object.get("type").and_then(Value::as_str);
                let fragments = output.entry(key).or_default();
                fragments.name = name;
                let arguments = serialized_arguments(&arguments);
                if fragments.arguments.is_empty() || event_type == Some("response.output_item.done")
                {
                    fragments.arguments = arguments;
                }
                let shell_call = is_responses_shell_call(item);
                fragments.shell_command_locations |= shell_call;
                let argument_pointer = if shell_call {
                    shell_argument_pointer(item)
                        .map(|suffix| format!("{pointer}/item/action/{suffix}"))
                        .unwrap_or_else(|| format!("{pointer}/item/action/command"))
                } else {
                    format!("{pointer}/item/arguments")
                };
                if event_type == Some("response.output_item.done") {
                    fragments
                        .terminal_argument_locations
                        .push((event_index, argument_pointer));
                } else if event_type != Some("response.output_item.added") {
                    fragments
                        .argument_locations
                        .push((event_index, argument_pointer));
                }
            }

            let anthropic_index = (object
                .get("content_block")
                .and_then(Value::as_object)
                .and_then(|block| block.get("type"))
                .and_then(Value::as_str)
                == Some("tool_use")
                || object
                    .get("delta")
                    .and_then(Value::as_object)
                    .and_then(|delta| delta.get("type"))
                    .and_then(Value::as_str)
                    == Some("input_json_delta"))
            .then(|| object.get("index").map(Value::to_string))
            .flatten();
            if let Some(key) = anthropic_index
                .as_ref()
                .map(|index| format!("anthropic:{index}"))
            {
                if let Some(block) = object.get("content_block").and_then(Value::as_object)
                    && block.get("type").and_then(Value::as_str) == Some("tool_use")
                {
                    let fragments = output.entry(key.clone()).or_default();
                    if let Some(name) = block.get("name").and_then(Value::as_str) {
                        fragments.name.push_str(name);
                    }
                }
                if let Some(delta) = object.get("delta").and_then(Value::as_object)
                    && delta.get("type").and_then(Value::as_str) == Some("input_json_delta")
                    && let Some(arguments) = delta.get("partial_json").and_then(Value::as_str)
                {
                    let fragments = output.entry(key).or_default();
                    fragments.arguments.push_str(arguments);
                    fragments
                        .argument_locations
                        .push((event_index, format!("{pointer}/delta/partial_json")));
                }
            }

            let function_field = if object.contains_key("function") {
                "function"
            } else {
                "function_call"
            };
            if let Some(function) = object.get(function_field).and_then(Value::as_object) {
                let identity = object
                    .get("index")
                    .or_else(|| object.get("id"))
                    .map(identity_key)
                    .unwrap_or_else(|| "default".to_string());
                let key = format!("{pointer}:{identity}");
                let fragments = output.entry(key).or_default();
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    fragments.name.push_str(name);
                }
                if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                    fragments.arguments.push_str(arguments);
                    fragments
                        .argument_locations
                        .push((event_index, format!("{pointer}/{function_field}/arguments")));
                }
            }

            let event_type = object.get("type").and_then(Value::as_str);
            if event_type.is_some_and(|kind| kind.ends_with("function_call_arguments.delta")) {
                let key = object
                    .get("item_id")
                    .map(identity_key)
                    .unwrap_or_else(|| "default".to_string());
                if let Some(delta) = object.get("delta").and_then(Value::as_str) {
                    let fragments = output.entry(key).or_default();
                    fragments.arguments.push_str(delta);
                    fragments
                        .argument_locations
                        .push((event_index, format!("{pointer}/delta")));
                }
            } else if event_type.is_some_and(|kind| kind.ends_with("function_call_arguments.done"))
            {
                let key = object
                    .get("item_id")
                    .map(identity_key)
                    .unwrap_or_else(|| "default".to_string());
                if let Some(arguments) = object.get("arguments").and_then(Value::as_str) {
                    let fragments = output.entry(key).or_default();
                    fragments.arguments = arguments.to_string();
                    fragments
                        .terminal_argument_locations
                        .push((event_index, format!("{pointer}/arguments")));
                }
            }

            if !response_item_event
                && object.get("function").is_none()
                && let Some((name, arguments)) = parse_tool_call(object)
            {
                let key = object
                    .get("id")
                    .or_else(|| object.get("call_id"))
                    .map(identity_key)
                    .unwrap_or_else(|| "default".to_string());
                let fragments = output.entry(key).or_default();
                fragments.name.push_str(&name);
                fragments
                    .arguments
                    .push_str(&serialized_arguments(&arguments));
                let shell_call = is_responses_shell_call(object);
                fragments.shell_command_locations |= shell_call;
                let argument_pointer = if shell_call {
                    shell_argument_pointer(object)
                        .map(|suffix| format!("{pointer}/action/{suffix}"))
                        .unwrap_or_else(|| format!("{pointer}/action/command"))
                } else {
                    format!("{pointer}/arguments")
                };
                fragments
                    .argument_locations
                    .push((event_index, argument_pointer));
                return;
            }

            for (key, child) in object {
                if anthropic_index.is_some() && matches!(key.as_str(), "content_block" | "delta") {
                    continue;
                }
                if response_item_event && key == "item" {
                    continue;
                }
                collect_stream_tool_fragments(
                    child,
                    &format!("{pointer}/{}", escape_pointer(key)),
                    event_index,
                    output,
                );
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                collect_stream_tool_fragments(
                    child,
                    &format!("{pointer}/{index}"),
                    event_index,
                    output,
                );
            }
        }
        _ => {}
    }
}

fn identity_key(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn serialized_arguments(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(value).expect("tool arguments are valid JSON"))
}
pub fn model_route_key(model: &str, provider: &str, upstream_model: &str) -> String {
    format!("{model}/{provider}/{upstream_model}")
}

async fn inspect_text_fields(
    state: &AppState,
    route_key: &str,
    request_id: &str,
    phase: GuardPhase,
    value: &mut Value,
    associated_prompt: Option<&str>,
    field_names: &[&str],
) -> Result<Option<String>, GatewayError> {
    debug_assert!(matches!(
        phase,
        GuardPhase::Prompt | GuardPhase::ModelResponse
    ));
    let mut pointers = Vec::new();
    collect_string_pointers(value, "", field_names, &mut pointers);
    let inspected = pointers
        .iter()
        .filter_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    if inspected.is_empty() {
        return Ok(None);
    }

    let mut input = EvaluationInput::new(
        phase,
        EvaluationPayload::TextSegments {
            segments: inspected.clone(),
        },
    );
    if let Some(prompt) = associated_prompt {
        input = input.with_associated_prompt(prompt);
    }
    let evaluation = evaluate(state, route_key, Some(request_id), input).await;
    ensure_allowed(&evaluation)?;
    let transformed = evaluation
        .decisions
        .iter()
        .any(|decision| decision.transformed);
    let final_segments = if transformed {
        let EvaluationPayload::TextSegments { segments } = &evaluation.output else {
            return Err(GatewayError::Internal(
                "guardrail transformation changed the prompt payload kind".to_string(),
            ));
        };
        if segments.len() != pointers.len() {
            return Err(GatewayError::Internal(
                "guardrail transformation returned a mismatched segment count".to_string(),
            ));
        }
        for (pointer, transformed) in pointers.iter().zip(segments) {
            if let Some(slot) = value.pointer_mut(pointer) {
                *slot = Value::String(transformed.clone());
            }
        }
        segments
    } else {
        &inspected
    };
    Ok(Some(final_segments.join("\n")))
}

async fn inspect_generated_tool_calls(
    state: &AppState,
    context: &InferenceGuardContext,
    response: &mut Value,
) -> Result<(), GatewayError> {
    let mut pointers = Vec::new();
    collect_tool_call_pointers(response, "", &mut pointers);
    for pointer in pointers {
        let Some((name, arguments)) = response
            .pointer(&pointer)
            .and_then(Value::as_object)
            .and_then(parse_tool_call)
        else {
            continue;
        };
        let payload = shell_command(&name, &arguments)
            .map(|command| EvaluationPayload::ShellCommand { command })
            .unwrap_or_else(|| EvaluationPayload::ToolCall {
                name,
                arguments: arguments.clone(),
            });
        let mut input = EvaluationInput::new(GuardPhase::GeneratedToolCall, payload);
        if let Some(prompt) = &context.associated_prompt {
            input = input.with_associated_prompt(prompt);
        }
        let evaluation =
            evaluate(state, &context.route_key, Some(&context.request_id), input).await;
        ensure_allowed(&evaluation)?;
        if evaluation
            .decisions
            .iter()
            .any(|decision| decision.transformed)
            && let Some(arguments) = transformed_tool_arguments(&evaluation.output, &arguments)
            && let Some(object) = response
                .pointer_mut(&pointer)
                .and_then(Value::as_object_mut)
        {
            replace_tool_call_arguments(object, arguments);
        }
    }
    Ok(())
}

async fn evaluate(
    state: &AppState,
    route_key: &str,
    request_id: Option<&str>,
    input: EvaluationInput,
) -> GuardrailEvaluation {
    let policy =
        PolicyResolver::new(&state.guardrail_config).resolve(PolicyTarget::ModelRoute(route_key));
    let evaluation = state
        .guardrail_engine
        .evaluate(&policy, &state.guardrail_config, input)
        .await;
    record_guardrail_evaluation(state, request_id, None, &evaluation).await;
    evaluation
}

fn ensure_allowed(evaluation: &GuardrailEvaluation) -> Result<(), GatewayError> {
    if !evaluation.denied() {
        return Ok(());
    }
    let decision = evaluation
        .decisions
        .iter()
        .rev()
        .find(|decision| decision.action == DecisionAction::Deny)
        .expect("denied evaluation has a deny decision");
    Err(GatewayError::GuardrailDenied {
        decision_id: decision.decision_id.to_string(),
        reason_code: decision.reason_code.to_string(),
    })
}

fn collect_string_pointers(
    value: &Value,
    pointer: &str,
    field_names: &[&str],
    output: &mut Vec<String>,
) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let child_pointer = format!("{pointer}/{}", escape_pointer(key));
                if child.is_string() && field_names.contains(&key.as_str()) {
                    output.push(child_pointer.clone());
                } else {
                    collect_string_pointers(child, &child_pointer, field_names, output);
                }
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                collect_string_pointers(child, &format!("{pointer}/{index}"), field_names, output);
            }
        }
        _ => {}
    }
}

fn collect_tool_call_pointers(value: &Value, pointer: &str, output: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if parse_tool_call(object).is_some() {
                output.push(pointer.to_string());
                return;
            }
            for (key, child) in object {
                collect_tool_call_pointers(
                    child,
                    &format!("{pointer}/{}", escape_pointer(key)),
                    output,
                );
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                collect_tool_call_pointers(child, &format!("{pointer}/{index}"), output);
            }
        }
        _ => {}
    }
}

fn parse_tool_call(object: &Map<String, Value>) -> Option<(String, Value)> {
    if let Some(function) = object
        .get("function")
        .or_else(|| object.get("function_call"))
        .and_then(Value::as_object)
    {
        let name = function.get("name")?.as_str()?.to_string();
        let arguments = parse_arguments(function.get("arguments")?);
        return Some((name, arguments));
    }
    let call_type = object.get("type").and_then(Value::as_str);
    if matches!(call_type, Some("function_call" | "tool_use")) {
        let name = object.get("name")?.as_str()?.to_string();
        let arguments = object
            .get("arguments")
            .or_else(|| object.get("input"))
            .map(parse_arguments)
            .unwrap_or_else(|| Value::Object(Map::new()));
        return Some((name, arguments));
    }
    if matches!(call_type, Some("shell_call" | "local_shell_call")) {
        let action = object.get("action")?.as_object()?;
        let command = action
            .get("command")
            .or_else(|| action.get("commands"))
            .and_then(shell_action_command)?;
        return Some((
            call_type?.to_string(),
            serde_json::json!({"command": command}),
        ));
    }
    None
}

fn is_responses_shell_call(object: &Map<String, Value>) -> bool {
    matches!(
        object.get("type").and_then(Value::as_str),
        Some("shell_call" | "local_shell_call")
    )
}
fn shell_argument_pointer(object: &Map<String, Value>) -> Option<&'static str> {
    let action = object.get("action")?.as_object()?;
    if action.contains_key("command") {
        Some("command")
    } else if action.contains_key("commands") {
        Some("commands")
    } else {
        None
    }
}

fn shell_action_command(value: &Value) -> Option<String> {
    match value {
        Value::String(command) => Some(command.clone()),
        Value::Array(commands) => commands
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()
            .map(|commands| commands.join("\n")),
        _ => None,
    }
}

fn parse_arguments(value: &Value) -> Value {
    value
        .as_str()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_else(|| value.clone())
}

fn shell_command(name: &str, arguments: &Value) -> Option<String> {
    const SHELL_TOOL_NAMES: &[&str] = &[
        "bash",
        "shell",
        "shell_call",
        "local_shell_call",
        "shell_command",
        "terminal.exec",
    ];
    if !SHELL_TOOL_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
        return None;
    }
    arguments
        .get("command")
        .or_else(|| arguments.get("cmd"))
        .or_else(|| arguments.get("script"))
        .and_then(shell_action_command)
}

fn transformed_tool_arguments(output: &EvaluationPayload, original: &Value) -> Option<Value> {
    match output {
        EvaluationPayload::ToolCall { arguments, .. } => Some(arguments.clone()),
        EvaluationPayload::ShellCommand { command } => {
            let mut arguments = original.clone();
            replace_shell_command(&mut arguments, command).then_some(arguments)
        }
        _ => None,
    }
}

fn replace_shell_command(arguments: &mut Value, replacement: &str) -> bool {
    for field in ["command", "cmd", "script"] {
        if let Some(slot) = arguments.get_mut(field)
            && (slot.is_string() || slot.is_array())
        {
            let array = slot.is_array();
            replace_stream_argument(slot, replacement, array);
            return true;
        }
    }
    false
}
fn replace_stream_argument(slot: &mut Value, replacement: &str, shell_command: bool) {
    if shell_command && slot.is_array() {
        *slot = Value::Array(
            replacement
                .lines()
                .map(|line| Value::String(line.to_string()))
                .collect(),
        );
    } else {
        *slot = Value::String(replacement.to_string());
    }
}

fn replace_tool_call_arguments(object: &mut Map<String, Value>, arguments: Value) {
    let function_field = if object.contains_key("function") {
        "function"
    } else {
        "function_call"
    };
    if let Some(function) = object
        .get_mut(function_field)
        .and_then(Value::as_object_mut)
        && let Some(slot) = function.get_mut("arguments")
    {
        replace_json_value(slot, arguments);
        return;
    }
    if is_responses_shell_call(object)
        && let Some(command) = arguments.get("command").and_then(Value::as_str)
        && let Some(action) = object.get_mut("action").and_then(Value::as_object_mut)
    {
        let field = if action.contains_key("command") {
            "command"
        } else {
            "commands"
        };
        if let Some(slot) = action.get_mut(field) {
            replace_stream_argument(slot, command, true);
            return;
        }
    }
    for field in ["arguments", "input"] {
        if let Some(slot) = object.get_mut(field) {
            replace_json_value(slot, arguments);
            return;
        }
    }
}

fn replace_json_value(slot: &mut Value, replacement: Value) {
    if slot.is_string() {
        *slot = Value::String(
            serde_json::to_string(&replacement).expect("guardrail tool arguments are valid JSON"),
        );
    } else {
        *slot = replacement;
    }
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn extracts_batch_prompt_without_request_identifiers() {
        let request = json!({
            "id": "private-id",
            "messages": [{"role": "user", "content": "protect this"}],
            "input": [{"type": "input_text", "text": "and this"}],
        });

        assert_eq!(
            batch_associated_prompt(&request).as_deref(),
            Some("and this\nprotect this")
        );
    }

    use super::*;

    #[test]
    fn rejects_non_json_guarded_sse_data() {
        let error = parse_guarded_sse_json("not-json").unwrap_err();

        assert!(error.to_string().contains("invalid SSE JSON"));
        assert!(!error.to_string().contains("not-json"));
    }

    #[test]
    fn groups_stream_text_by_independent_output() {
        assert_eq!(
            stream_text_group(&json!({}), "/choices/0/delta/content"),
            "choice:0"
        );
        assert_eq!(
            stream_text_group(&json!({"item_id": "item-a", "output_index": 1}), "/delta"),
            "output:1"
        );
        assert_eq!(
            stream_text_group(&json!({}), "/response/output/1/content/0/text"),
            "output:1"
        );
        assert_eq!(
            stream_text_group(
                &json!({"type": "content_block_delta", "index": 2}),
                "/delta/text"
            ),
            "content-block:2"
        );
        assert_eq!(stream_text_group(&json!({}), "/delta"), "default");
    }

    #[test]
    fn extracts_parallel_and_protocol_specific_tool_calls() {
        let response = json!({
            "choices": [{"message": {"tool_calls": [
                {"function": {"name": "bash", "arguments": "{\"command\":\"rm -rf /\"}"}},
                {"function": {"name": "safe", "arguments": {"value": 1}}}
            ]}}],
            "content": [{"type": "tool_use", "name": "shell", "input": {"cmd": "git reset --hard"}}],
            "output": [{"type": "function_call", "name": "terminal.exec", "arguments": "{\"script\":\"kill 1\"}"}]
        });
        let mut pointers = Vec::new();
        collect_tool_call_pointers(&response, "", &mut pointers);
        let calls = pointers
            .iter()
            .filter_map(|pointer| {
                response
                    .pointer(pointer)
                    .and_then(Value::as_object)
                    .and_then(parse_tool_call)
            })
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 4);
        assert_eq!(
            shell_command(&calls[0].0, &calls[0].1).as_deref(),
            Some("rm -rf /")
        );
        assert_eq!(
            shell_command(&calls[2].0, &calls[2].1).as_deref(),
            Some("git reset --hard")
        );
        assert_eq!(
            shell_command(&calls[3].0, &calls[3].1).as_deref(),
            Some("kill 1")
        );
    }

    #[test]
    fn parses_and_replaces_legacy_function_call_envelopes() {
        let mut response = json!({
            "choices": [{
                "message": {
                    "function_call": {
                        "name": "bash",
                        "arguments": "{\"command\":\"rm -rf /tmp/work\"}"
                    }
                }
            }]
        });
        let mut pointers = Vec::new();
        collect_tool_call_pointers(&response, "", &mut pointers);

        assert_eq!(pointers, ["/choices/0/message"]);
        let call = response
            .pointer(&pointers[0])
            .and_then(Value::as_object)
            .and_then(parse_tool_call)
            .expect("legacy function call");
        assert_eq!(
            shell_command(&call.0, &call.1).as_deref(),
            Some("rm -rf /tmp/work")
        );
        replace_tool_call_arguments(
            response
                .pointer_mut(&pointers[0])
                .and_then(Value::as_object_mut)
                .expect("legacy envelope"),
            json!({"command": "[masked]"}),
        );
        assert_eq!(
            response["choices"][0]["message"]["function_call"]["arguments"],
            "{\"command\":\"[masked]\"}"
        );
    }

    #[test]
    fn reconstructs_split_parallel_stream_tool_calls() {
        let mut fragments = BTreeMap::new();
        for (event_index, event) in [
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": "call-1", "function": {"name": "bash", "arguments": "{\"command\":\"rm "}},
                {"index": 1, "id": "call-2", "function": {"name": "safe", "arguments": "{\"value\":"}}
            ]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "-rf /tmp/x\"}"}},
                {"index": 1, "function": {"arguments": "1}"}}
            ]}}]}),
        ]
        .iter()
        .enumerate()
        {
            collect_stream_tool_fragments(event, "", event_index, &mut fragments);
        }

        assert_eq!(fragments.len(), 2);
        let first = &fragments["/choices/0/delta/tool_calls/0:0"];
        let second = &fragments["/choices/0/delta/tool_calls/1:1"];
        assert_eq!(first.name, "bash");
        assert_eq!(first.arguments, "{\"command\":\"rm -rf /tmp/x\"}");
        assert_eq!(second.arguments, "{\"value\":1}");
        assert_eq!(first.argument_locations.len(), 2);
    }

    #[test]
    fn reconstructs_anthropic_and_responses_api_argument_deltas() {
        let mut fragments = BTreeMap::new();
        for (event_index, event) in [
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "tool_use", "id": "tool-1", "name": "bash", "input": {}}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "{\"command\":\"rm "}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": "-rf /tmp/x\"}"}
            }),
            json!({
                "type": "response.output_item.added",
                "item": {"type": "function_call", "id": "item-1", "name": "shell", "arguments": ""}
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "item-1",
                "delta": "{\"cmd\":\"git reset --hard\"}"
            }),
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": "item-1",
                "arguments": "{\"cmd\":\"git reset --hard\"}"
            }),
            json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "function_call",
                    "id": "item-1",
                    "name": "shell",
                    "arguments": "{\"cmd\":\"git reset --hard\"}"
                }
            }),
        ]
        .iter()
        .enumerate()
        {
            collect_stream_tool_fragments(event, "", event_index, &mut fragments);
        }

        assert_eq!(
            fragments["anthropic:0"].arguments,
            "{\"command\":\"rm -rf /tmp/x\"}"
        );
        assert_eq!(fragments["anthropic:0"].name, "bash");
        assert_eq!(
            fragments["item-1"].arguments,
            "{\"cmd\":\"git reset --hard\"}"
        );
        assert_eq!(fragments["item-1"].name, "shell");
        assert_eq!(fragments["item-1"].terminal_argument_locations.len(), 2);
    }

    #[test]
    fn preserves_tool_call_shape_when_replacing_transformed_arguments() {
        let mut openai = json!({
            "function": {
                "name": "bash",
                "arguments": "{\"command\":\"rm -rf /\"}"
            }
        });
        replace_tool_call_arguments(
            openai.as_object_mut().expect("tool call"),
            json!({"command": "[masked]"}),
        );
        assert_eq!(
            openai["function"]["arguments"],
            Value::String("{\"command\":\"[masked]\"}".to_string())
        );

        let mut anthropic = json!({
            "type": "tool_use",
            "name": "shell",
            "input": {"cmd": "git reset --hard"}
        });
        replace_tool_call_arguments(
            anthropic.as_object_mut().expect("tool call"),
            json!({"cmd": "[masked]"}),
        );
        assert_eq!(anthropic["input"], json!({"cmd": "[masked]"}));
        let mut responses = json!({
            "type": "shell_call",
            "action": {"command": ["rm -rf /tmp/a", "printf safe"]}
        });

        replace_tool_call_arguments(
            responses.as_object_mut().expect("tool call"),
            json!({"command": "[masked]\nprintf safe"}),
        );
        assert_eq!(
            responses["action"]["command"],
            json!(["[masked]", "printf safe"])
        );

        let mut streamed_commands = json!(["rm -rf /tmp/a", "printf safe"]);
        replace_stream_argument(&mut streamed_commands, "[masked]\nprintf safe", true);
        assert_eq!(streamed_commands, json!(["[masked]", "printf safe"]));
    }

    #[test]
    fn preserves_responses_delta_and_terminal_snapshot_semantics() {
        let mut blocks = vec![
            vec![StreamLine::Json(json!({
                "type": "response.output_item.added",
                "item": {"arguments": ""}
            }))],
            vec![StreamLine::Json(json!({
                "type": "response.function_call_arguments.delta",
                "delta": "{\"command\":\"old\"}"
            }))],
            vec![StreamLine::Json(json!({
                "type": "response.output_item.done",
                "item": {"arguments": "{\"command\":\"old\"}"}
            }))],
        ];
        replace_stream_arguments(
            &mut blocks,
            &[(1, "/delta".into())],
            "{\"command\":\"[masked]\"}",
            false,
            true,
        );
        replace_stream_arguments(
            &mut blocks,
            &[(2, "/item/arguments".into())],
            "{\"command\":\"[masked]\"}",
            false,
            false,
        );

        let json_at = |index: usize| match &blocks[index][0] {
            StreamLine::Json(value) => value,
            _ => panic!("expected JSON"),
        };
        assert_eq!(json_at(0)["item"]["arguments"], "");
        assert_eq!(json_at(1)["delta"], "{\"command\":\"[masked]\"}");
        assert_eq!(
            json_at(2)["item"]["arguments"],
            "{\"command\":\"[masked]\"}"
        );
    }

    #[test]
    fn rewrites_responses_text_deltas_and_completion_snapshot() {
        let mut blocks = vec![
            vec![StreamLine::Json(json!({
                "type": "response.output_text.delta",
                "delta": "private"
            }))],
            vec![StreamLine::Json(json!({
                "type": "response.completed",
                "response": {"output": [{"content": [{"text": "private"}]}]}
            }))],
        ];
        replace_stream_text(&mut blocks, &[(0, "/delta".into())], "[masked]", true);
        replace_stream_text(
            &mut blocks,
            &[(1, "/response/output/0/content/0/text".into())],
            "[masked]",
            true,
        );

        let json_at = |index: usize| match &blocks[index][0] {
            StreamLine::Json(value) => value,
            _ => panic!("expected JSON"),
        };
        assert_eq!(json_at(0)["delta"], "[masked]");
        assert_eq!(
            json_at(1)["response"]["output"][0]["content"][0]["text"],
            "[masked]"
        );
    }

    #[test]
    fn text_pointers_target_response_content_without_touching_identifiers() {
        let value = json!({
            "id": "keep",
            "choices": [
                {"message": {"content": "inspect", "refusal": null, "reasoning_content": "inspect reasoning"}},
                {"message": {"content": null, "refusal": "inspect refusal"}},
                {"delta": {"refusal": "stream refusal", "reasoning": "stream reasoning"}}
            ],
            "content": [{"type": "thinking", "thinking": "inspect thinking"}]
        });
        let mut pointers = Vec::new();
        collect_string_pointers(&value, "", MODEL_RESPONSE_TEXT_FIELDS, &mut pointers);
        assert_eq!(
            pointers,
            vec![
                "/choices/0/message/content",
                "/choices/0/message/reasoning_content",
                "/choices/1/message/refusal",
                "/choices/2/delta/reasoning",
                "/choices/2/delta/refusal",
                "/content/0/thinking",
            ]
        );
        assert!(!pointers.iter().any(|pointer| pointer.ends_with("/id")));
    }
}
