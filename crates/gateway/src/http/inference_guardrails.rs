use std::collections::BTreeMap;

use axum::body::Bytes;
use futures_util::{StreamExt, stream};
use gateway_core::{GatewayError, ProviderStream};
use gateway_guardrails::{
    DecisionAction, DecisionRecord, EvaluationInput, EvaluationPayload, GuardPhase,
    GuardrailEvaluation, PolicyResolver, PolicyTarget, UninspectedContent, redact_prompt_secrets,
    uninspected_content_decision,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::http::{
    guardrail_events::{
        record_guardrail_decisions_in_background, record_guardrail_evaluation,
        uninspected_content_hash,
    },
    state::AppState,
};
const PROMPT_TEXT_FIELDS: &[&str] = &[
    "content",
    "description",
    "input",
    "instructions",
    "name",
    "prompt",
    "summary",
    "text",
    "title",
];

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
    // Redact before any other evaluator, so managed services never see the secrets either.
    let redactions = redact_prompt_secrets(&policy, request);
    record_guardrail_decisions_in_background(state, vec![(request_id.to_string(), redactions)]);
    let associated_prompt = inspect_text_fields(
        state,
        &route_key,
        request_id,
        GuardPhase::Prompt,
        request,
        None,
        PROMPT_TEXT_FIELDS,
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

/// Redacts prompt secrets and records their decisions without running the other
/// prompt evaluators.
pub async fn redact_prompt(
    state: &AppState,
    request_id: &str,
    route_key: &str,
    request: &mut Value,
) {
    let redactions = redact_prompt_body(state, route_key, request);
    record_guardrail_decisions_in_background(state, vec![(request_id.to_string(), redactions)]);
}

/// Redacts secrets in `request` in place and returns the decisions, leaving
/// persistence to the caller.
pub fn redact_prompt_body(
    state: &AppState,
    route_key: &str,
    request: &mut Value,
) -> Vec<DecisionRecord> {
    let policy =
        PolicyResolver::new(&state.guardrail_config).resolve(PolicyTarget::ModelRoute(route_key));
    redact_prompt_secrets(&policy, request)
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
    collect_string_pointers(request, "", PROMPT_TEXT_FIELDS, &mut pointers);
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
) -> Result<ProviderStream, Box<GuardStreamError>> {
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
                return Err(Box::new(GuardStreamError {
                    error: GatewayError::Provider(error),
                    collector: Some(collector),
                }));
            }
        };
        collector.observe_chunk(&chunk);
        if buffered.len().saturating_add(chunk.len()) > context.stream_buffer_bytes {
            if release_uninspected_stream(
                state,
                context,
                UninspectedContent::BufferExceeded,
                &[&buffered, &chunk],
            ) {
                let inspected = stream::iter([Ok(Bytes::from(buffered)), Ok(chunk)]);
                return Ok(Box::pin(inspected.chain(upstream)));
            }
            collector.finish();
            return Err(Box::new(GuardStreamError {
                error: GatewayError::PayloadTooLarge {
                    limit_bytes: context.stream_buffer_bytes,
                },
                collector: Some(collector),
            }));
        }
        buffered.extend_from_slice(&chunk);
    }
    collector.finish();
    let guarded = guard_sse_payload(state, context, &buffered)
        .await
        .map_err(|error| {
            Box::new(GuardStreamError {
                error,
                collector: Some(collector),
            })
        })?;
    Ok(Box::pin(stream::once(
        async move { Ok(Bytes::from(guarded)) },
    )))
}

/// Records the guardrail decision for a provider stream the guard cannot inspect, whether it
/// releases or rejects the stream, and returns whether the stream is released unchanged
/// instead of failing the request. `gateway-guardrails` owns the mode semantics: audit mode
/// releases it.
fn release_uninspected_stream(
    state: &AppState,
    context: &InferenceGuardContext,
    reason: UninspectedContent,
    content: &[&[u8]],
) -> bool {
    let policy = PolicyResolver::new(&state.guardrail_config)
        .resolve(PolicyTarget::ModelRoute(&context.route_key));
    let decision = uninspected_content_decision(
        &policy,
        GuardPhase::ModelResponse,
        reason,
        uninspected_content_hash(content),
    );
    let released = decision.action != DecisionAction::Deny;
    record_guardrail_decisions_in_background(
        state,
        vec![(context.request_id.clone(), vec![decision])],
    );
    released
}

async fn guard_sse_payload(
    state: &AppState,
    context: &InferenceGuardContext,
    payload: &[u8],
) -> Result<Vec<u8>, GatewayError> {
    let source = match std::str::from_utf8(payload) {
        Ok(source) => source,
        Err(_)
            if release_uninspected_stream(
                state,
                context,
                UninspectedContent::Unparseable,
                &[payload],
            ) =>
        {
            return Ok(payload.to_vec());
        }
        Err(error) => {
            return Err(GatewayError::Internal(format!(
                "provider stream was not UTF-8: {error}"
            )));
        }
    };
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
        let mut raw_data = Vec::new();
        let mut data_index = None;
        for line in block.split(separator) {
            let Some(payload) = line.strip_prefix("data:") else {
                lines.push(StreamLine::Other(line.to_string()));
                continue;
            };
            raw_data.push(line.to_string());
            data_index.get_or_insert(lines.len());
            data.push(payload.strip_prefix(' ').unwrap_or(payload));
        }
        if let Some(data_index) = data_index {
            let payload = data.join("\n");
            let stream_line = if payload.trim().is_empty() {
                StreamLine::Other(raw_data.join(separator))
            } else if payload == "[DONE]" {
                StreamLine::Done
            } else {
                let value = match parse_guarded_sse_json(&payload) {
                    Ok(value) => value,
                    Err(_)
                        if release_uninspected_stream(
                            state,
                            context,
                            UninspectedContent::Unparseable,
                            &[source.as_bytes()],
                        ) =>
                    {
                        return Ok(source.as_bytes().to_vec());
                    }
                    Err(error) => return Err(error),
                };
                let mut pointers = Vec::new();
                collect_string_pointers(&value, "", MODEL_RESPONSE_TEXT_FIELDS, &mut pointers);
                if let Some(error) = value.get("error") {
                    collect_all_string_pointers(error, "/error", &mut pointers);
                }
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

    let owners = call_id_owners(&tool_fragments);
    let mut evaluated_by_call = BTreeMap::<&str, EvaluatedToolArguments>::new();
    for (key, fragments) in &tool_fragments {
        let call = owners.get(key.as_str()).copied().unwrap_or(key);
        let evaluated = evaluated_by_call.entry(call).or_default();
        if !fragments.name.is_empty() && !fragments.arguments.is_empty() {
            let replacement = guard_stream_tool_arguments(
                state,
                context,
                evaluated,
                &fragments.name,
                fragments.shell_command_locations,
                &fragments.arguments,
            )
            .await?;
            if let Some(replacement) = &replacement {
                replace_stream_arguments(
                    &mut blocks,
                    &fragments.argument_locations,
                    replacement,
                    fragments.shell_command_locations,
                    true,
                );
            }
        }
        for terminal in &fragments.terminal_arguments {
            let name = terminal.name.as_deref().unwrap_or(&fragments.name);
            if name.is_empty() {
                continue;
            }
            let replacement = guard_stream_tool_arguments(
                state,
                context,
                evaluated,
                name,
                terminal.shell_command,
                &terminal.arguments,
            )
            .await?;
            if let Some(replacement) = replacement {
                replace_stream_arguments(
                    &mut blocks,
                    std::slice::from_ref(&terminal.location),
                    &replacement,
                    terminal.shell_command,
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
        let combined_text = stream_text_at_locations(&blocks, &locations);
        if combined_text.is_empty() {
            continue;
        }
        let replacement = guard_stream_text(state, context, &combined_text).await?;
        if let Some(transformed) = &replacement {
            replace_stream_text(&mut blocks, &locations, transformed, true);
        }
        let Some(snapshot_locations) = snapshot_locations_by_group.remove(&group) else {
            continue;
        };
        let snapshot_text = stream_text_at_locations(&blocks, &snapshot_locations);
        if snapshot_text == combined_text {
            if let Some(transformed) = &replacement {
                replace_stream_text(&mut blocks, &snapshot_locations, transformed, true);
            }
        } else if let Some(transformed) = guard_stream_text(state, context, &snapshot_text).await? {
            replace_stream_text(&mut blocks, &snapshot_locations, &transformed, true);
        }
    }
    for locations in snapshot_locations_by_group.values() {
        let snapshot_text = stream_text_at_locations(&blocks, locations);
        if let Some(transformed) = guard_stream_text(state, context, &snapshot_text).await? {
            replace_stream_text(&mut blocks, locations, &transformed, true);
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

/// Replacements already decided for one tool call, keyed by tool name, shell-command shape,
/// and a digest of the canonical arguments. Providers repeat a call in its deltas and up to
/// three terminal snapshots, and each call should cost one evaluation per distinct argument
/// set. Distinct calls never share an entry, even with identical arguments.
type EvaluatedToolArguments = BTreeMap<(String, bool, [u8; 32]), Option<String>>;

async fn guard_stream_tool_arguments(
    state: &AppState,
    context: &InferenceGuardContext,
    evaluated: &mut EvaluatedToolArguments,
    name: &str,
    shell_command_locations: bool,
    argument_text: &str,
) -> Result<Option<String>, GatewayError> {
    let parsed = serde_json::from_str::<Value>(argument_text);
    let canonical = match &parsed {
        Ok(value) => Sha256::digest(value.to_string()),
        Err(_) => Sha256::digest(argument_text),
    };
    let key = (name.to_string(), shell_command_locations, canonical.into());
    if let Some(replacement) = evaluated.get(&key) {
        return Ok(replacement.clone());
    }
    let arguments = parsed.unwrap_or_else(|_| Value::String(argument_text.to_string()));
    let replacement =
        evaluate_stream_tool_arguments(state, context, name, shell_command_locations, arguments)
            .await?;
    evaluated.insert(key, replacement.clone());
    Ok(replacement)
}

async fn evaluate_stream_tool_arguments(
    state: &AppState,
    context: &InferenceGuardContext,
    name: &str,
    shell_command_locations: bool,
    arguments: Value,
) -> Result<Option<String>, GatewayError> {
    let payload = shell_command(name, &arguments)
        .map(|command| EvaluationPayload::ShellCommand { command })
        .unwrap_or_else(|| EvaluationPayload::ToolCall {
            name: name.to_string(),
            arguments: arguments.clone(),
        });
    let mut input = EvaluationInput::new(GuardPhase::GeneratedToolCall, payload);
    if let Some(prompt) = &context.associated_prompt {
        input = input.with_associated_prompt(prompt);
    }
    let evaluation = evaluate(state, &context.route_key, Some(&context.request_id), input).await;
    ensure_allowed(&evaluation)?;
    if !evaluation
        .decisions
        .iter()
        .any(|decision| decision.transformed)
    {
        return Ok(None);
    }
    let Some(arguments) = transformed_tool_arguments(&evaluation.output, &arguments) else {
        return Ok(None);
    };
    let replacement = if shell_command_locations {
        arguments
            .get("command")
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        serde_json::to_string(&arguments).ok()
    };
    Ok(replacement)
}

fn stream_text_at_locations(blocks: &[Vec<StreamLine>], locations: &[(usize, String)]) -> String {
    locations
        .iter()
        .filter_map(|(event_index, pointer)| {
            blocks[*event_index].iter().find_map(|line| match line {
                StreamLine::Json(value) => value.pointer(pointer).and_then(Value::as_str),
                _ => None,
            })
        })
        .collect()
}

async fn guard_stream_text(
    state: &AppState,
    context: &InferenceGuardContext,
    text: &str,
) -> Result<Option<String>, GatewayError> {
    if text.is_empty() {
        return Ok(None);
    }
    let mut input = EvaluationInput::new(
        GuardPhase::ModelResponse,
        EvaluationPayload::Text {
            text: text.to_string(),
        },
    );
    if let Some(prompt) = &context.associated_prompt {
        input = input.with_associated_prompt(prompt);
    }
    let evaluation = evaluate(state, &context.route_key, Some(&context.request_id), input).await;
    ensure_allowed(&evaluation)?;
    Ok(evaluation
        .output
        .text_content()
        .filter(|transformed| *transformed != text)
        .map(str::to_string))
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
    let kind = stream_text_kind(value, &segments);
    if segments.first() == Some(&"choices")
        && let Some(choice) = segments.get(1)
    {
        return format!("choice:{choice}:{kind}");
    }
    if let Some(output_index) = value.get("output_index").and_then(Value::as_u64) {
        return format!("output:{output_index}:{kind}");
    }
    if let Some(position) = segments.iter().position(|segment| *segment == "output")
        && let Some(output_index) = segments.get(position + 1)
    {
        return format!("output:{output_index}:{kind}");
    }
    if let Some(item_id) = value.get("item_id").and_then(Value::as_str) {
        return format!("item:{item_id}:{kind}");
    }
    if let Some(content_block_index) = value.get("index").and_then(Value::as_u64) {
        return format!("content-block:{content_block_index}:{kind}");
    }
    format!("default:{kind}")
}

fn stream_text_kind(value: &Value, segments: &[&str]) -> &'static str {
    let field = segments.last().copied().unwrap_or_default();
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if matches!(
        field,
        "thinking" | "reasoning" | "reasoning_content" | "reasoning_text"
    ) || event_type.contains("reasoning")
    {
        "reasoning"
    } else if field == "refusal" {
        "refusal"
    } else {
        "content"
    }
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

/// The tool calls in one guarded stream, keyed by call identity. `name`, `arguments`, and
/// `shell_command_locations` describe the streamed deltas. Each terminal snapshot keeps the
/// name and argument encoding from its own event, so a snapshot can never change how the
/// deltas, or another snapshot, are evaluated and rewritten.
#[derive(Default)]
struct ToolCallFragments {
    name: String,
    arguments: String,
    terminal_arguments: Vec<TerminalArguments>,
    argument_locations: Vec<(usize, String)>,
    shell_command_locations: bool,
    /// The item's `call_id` when the call is keyed by its item `id`.
    call_id: Option<String>,
}

/// Responses providers can key some of a call's events by `call_id` alone while others
/// use the item `id`. Maps each such `call_id` key to the `id` key of the call it belongs
/// to, so both share one evaluation cache. Each keeps its own locations and encoding.
fn call_id_owners(calls: &BTreeMap<String, ToolCallFragments>) -> BTreeMap<&str, &str> {
    let mut owners = BTreeMap::new();
    for (key, call) in calls {
        if let Some(call_id) = call.call_id.as_deref()
            && call_id != key
            && calls.contains_key(call_id)
        {
            owners.entry(call_id).or_insert(key.as_str());
        }
    }
    owners
}

struct TerminalArguments {
    /// `None` when the event carries no name, such as `function_call_arguments.done`.
    name: Option<String>,
    arguments: String,
    location: (usize, String),
    /// Whether `location` holds a shell action command rather than serialized JSON arguments.
    shell_command: bool,
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
                if fragments.name.is_empty() {
                    fragments.name.clone_from(&name);
                }
                record_call_id(fragments, item);
                let arguments = serialized_arguments(&arguments);
                let shell_call = is_responses_shell_call(item);
                let argument_pointer = if shell_call {
                    shell_argument_pointer(item)
                        .map(|suffix| format!("{pointer}/item/action/{suffix}"))
                        .unwrap_or_else(|| format!("{pointer}/item/action/command"))
                } else {
                    format!("{pointer}/item/arguments")
                };
                if event_type == Some("response.output_item.done") {
                    fragments.terminal_arguments.push(TerminalArguments {
                        name: Some(name),
                        arguments,
                        location: (event_index, argument_pointer),
                        shell_command: shell_call,
                    });
                } else {
                    fragments.shell_command_locations |= shell_call;
                    // An added item usually opens an empty call that deltas fill in. When it
                    // already carries the whole call, as a shell call's command does, it is
                    // released with those arguments, so a rewrite must reach it too.
                    if fragments.arguments.is_empty() && !arguments.is_empty() {
                        fragments.arguments = arguments;
                        fragments
                            .argument_locations
                            .push((event_index, argument_pointer));
                    }
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
                let key = object
                    .get("index")
                    .or_else(|| object.get("id"))
                    .map(identity_key)
                    .map_or_else(
                        || format!("{pointer}:default"),
                        |identity| format!("{}:{identity}", tool_call_container(pointer)),
                    );
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
                    fragments.terminal_arguments.push(TerminalArguments {
                        name: None,
                        arguments: arguments.to_string(),
                        location: (event_index, format!("{pointer}/arguments")),
                        shell_command: false,
                    });
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
                // A whole tool call inside an event (such as `response.completed`) is a
                // snapshot of the call, not a delta, so it is guarded on its own.
                let fragments = output.entry(key).or_default();
                if fragments.name.is_empty() {
                    fragments.name.clone_from(&name);
                }
                record_call_id(fragments, object);
                let arguments = serialized_arguments(&arguments);
                let shell_call = is_responses_shell_call(object);
                let argument_pointer = if shell_call {
                    shell_argument_pointer(object)
                        .map(|suffix| format!("{pointer}/action/{suffix}"))
                        .unwrap_or_else(|| format!("{pointer}/action/command"))
                } else {
                    format!("{pointer}/arguments")
                };
                fragments.terminal_arguments.push(TerminalArguments {
                    name: Some(name),
                    arguments,
                    location: (event_index, argument_pointer),
                    shell_command: shell_call,
                });
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

fn record_call_id(fragments: &mut ToolCallFragments, item: &Map<String, Value>) {
    if item.contains_key("id")
        && let Some(call_id) = item.get("call_id")
    {
        fragments
            .call_id
            .get_or_insert_with(|| identity_key(call_id));
    }
}

fn tool_call_container(pointer: &str) -> &str {
    pointer
        .rsplit_once('/')
        .filter(|(_, position)| {
            !position.is_empty() && position.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map_or(pointer, |(container, _)| container)
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
    collect_string_pointers_inner(value, pointer, field_names, false, output);
}

fn collect_string_pointers_inner(
    value: &Value,
    pointer: &str,
    field_names: &[&str],
    matched_array: bool,
    output: &mut Vec<String>,
) {
    match value {
        Value::String(_) if matched_array => output.push(pointer.to_string()),
        Value::Object(object) => {
            for (key, child) in object {
                let child_pointer = format!("{pointer}/{}", escape_pointer(key));
                let child_matches =
                    field_names.contains(&key.as_str()) && (child.is_string() || child.is_array());
                collect_string_pointers_inner(
                    child,
                    &child_pointer,
                    field_names,
                    child_matches,
                    output,
                );
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                collect_string_pointers_inner(
                    child,
                    &format!("{pointer}/{index}"),
                    field_names,
                    matched_array && !child.is_object(),
                    output,
                );
            }
        }
        _ => {}
    }
}

fn collect_all_string_pointers(value: &Value, pointer: &str, output: &mut Vec<String>) {
    match value {
        Value::String(_) => {
            if !output.iter().any(|existing| existing == pointer) {
                output.push(pointer.to_string());
            }
        }
        Value::Object(object) => {
            for (key, child) in object {
                collect_all_string_pointers(
                    child,
                    &format!("{pointer}/{}", escape_pointer(key)),
                    output,
                );
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                collect_all_string_pointers(child, &format!("{pointer}/{index}"), output);
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
    use std::sync::Arc;

    use gateway_core::{GuardrailDecisionQuery, GuardrailDecisionRepository};
    use serde_json::json;

    #[tokio::test]
    async fn prompt_guard_redacts_secrets_and_records_decisions() {
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        let policy = &mut Arc::make_mut(&mut state.guardrail_config).default;
        policy.enabled = true;
        policy.secret_redaction.enabled = true;
        // Assembled at runtime so no literal credential lands in the repository.
        let key = format!(
            "sk-ant-api03-{}AA",
            &"aZ3kQ9mB7xR2tW5nL8pJ4vC6yH1dF0gS".repeat(3)[..93]
        );
        let mut request = json!({
            "model": "gpt",
            "messages": [
                {"role": "user", "content": format!("deploy with {key}")},
                {"role": "assistant", "tool_calls": [{"function": {
                    "name": "deploy",
                    "arguments": json!({"value": key}).to_string(),
                }}]},
            ],
        });

        super::guard_prompt(&state, "request-1", "openai/gpt".into(), &mut request)
            .await
            .expect("redaction never denies");

        assert!(!request.to_string().contains(&key));
        assert_eq!(
            request["messages"][0]["content"],
            "deploy with [REDACTED:anthropic-api-key]"
        );
        // Decisions are written in the background, so wait for the write.
        let mut items = Vec::new();
        for _ in 0..100 {
            items = state
                .store
                .list_guardrail_decisions(&GuardrailDecisionQuery {
                    page: 1,
                    page_size: 10,
                    request_id: Some("request-1".into()),
                    evaluator: Some("secret_redaction".into()),
                    ..Default::default()
                })
                .await
                .expect("decisions")
                .items;
            if !items.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let [decision] = items.as_slice() else {
            panic!("expected one decision per rule, got {items:?}");
        };
        assert_eq!(decision.rule_id.as_deref(), Some("anthropic-api-key"));
        assert!(decision.transformed);
    }

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
    fn groups_stream_text_by_independent_output_and_kind() {
        assert_eq!(
            stream_text_group(&json!({}), "/choices/0/delta/content"),
            "choice:0:content"
        );
        assert_eq!(
            stream_text_group(&json!({}), "/choices/0/delta/reasoning_content"),
            "choice:0:reasoning"
        );
        assert_eq!(
            stream_text_group(&json!({}), "/choices/0/delta/refusal"),
            "choice:0:refusal"
        );
        assert_eq!(
            stream_text_group(&json!({"item_id": "item-a", "output_index": 1}), "/delta"),
            "output:1:content"
        );
        assert_eq!(
            stream_text_group(&json!({}), "/response/output/1/content/0/text"),
            "output:1:content"
        );
        assert_eq!(
            stream_text_group(
                &json!({"type": "content_block_delta", "index": 2}),
                "/delta/text"
            ),
            "content-block:2:content"
        );
        assert_eq!(
            stream_text_group(&json!({"type": "response.reasoning_text.delta"}), "/delta"),
            "default:reasoning"
        );
        assert_eq!(stream_text_group(&json!({}), "/delta"), "default:content");
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
        let first = &fragments["/choices/0/delta/tool_calls:0"];
        let second = &fragments["/choices/0/delta/tool_calls:1"];
        assert_eq!(first.name, "bash");
        assert_eq!(first.arguments, "{\"command\":\"rm -rf /tmp/x\"}");
        assert_eq!(second.arguments, "{\"value\":1}");
        assert_eq!(first.argument_locations.len(), 2);
    }

    #[test]
    fn joins_stream_tool_deltas_that_change_array_position() {
        let mut fragments = BTreeMap::new();
        for (event_index, event) in [
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": "call-1", "function": {"name": "bash", "arguments": "{\"command\":\"rm "}},
                {"index": 1, "id": "call-2", "function": {"name": "safe", "arguments": "{\"value\":"}}
            ]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 1, "function": {"arguments": "1}"}}
            ]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": "-rf /tmp/x\"}"}}
            ]}}]}),
        ]
        .iter()
        .enumerate()
        {
            collect_stream_tool_fragments(event, "", event_index, &mut fragments);
        }

        assert_eq!(fragments.len(), 2);
        assert_eq!(
            fragments["/choices/0/delta/tool_calls:0"].arguments,
            "{\"command\":\"rm -rf /tmp/x\"}"
        );
        assert_eq!(
            fragments["/choices/0/delta/tool_calls:1"].arguments,
            "{\"value\":1}"
        );
    }

    #[test]
    fn keeps_positional_keys_for_tool_deltas_without_identity() {
        let mut fragments = BTreeMap::new();
        let event = json!({"choices": [{"delta": {"tool_calls": [
            {"function": {"name": "bash", "arguments": "{}"}},
            {"function": {"name": "safe", "arguments": "{}"}}
        ]}}]});
        collect_stream_tool_fragments(&event, "", 0, &mut fragments);

        assert_eq!(fragments.len(), 2);
        assert_eq!(
            fragments["/choices/0/delta/tool_calls/0:default"].name,
            "bash"
        );
        assert_eq!(
            fragments["/choices/0/delta/tool_calls/1:default"].name,
            "safe"
        );
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
        assert_eq!(fragments["item-1"].terminal_arguments.len(), 2);
        assert!(
            fragments["item-1"]
                .terminal_arguments
                .iter()
                .all(|terminal| terminal.arguments == "{\"cmd\":\"git reset --hard\"}")
        );
    }

    #[test]
    fn keeps_streamed_and_terminal_tool_arguments_separate() {
        let mut fragments = BTreeMap::new();
        for (event_index, event) in [
            json!({
                "type": "response.output_item.added",
                "item": {"type": "function_call", "id": "item-1", "name": "shell", "arguments": ""}
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "item-1",
                "delta": "{\"cmd\":\"rm -rf /tmp/work\"}"
            }),
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": "item-1",
                "arguments": "{\"cmd\":\"echo safe\"}"
            }),
        ]
        .iter()
        .enumerate()
        {
            collect_stream_tool_fragments(event, "", event_index, &mut fragments);
        }

        assert_eq!(
            fragments["item-1"].arguments,
            "{\"cmd\":\"rm -rf /tmp/work\"}"
        );
        assert_eq!(fragments["item-1"].terminal_arguments.len(), 1);
        assert_eq!(
            fragments["item-1"].terminal_arguments[0].arguments,
            "{\"cmd\":\"echo safe\"}"
        );
    }

    fn responses_function_call_stream(name: &str, completed_arguments: &str) -> String {
        responses_tool_call_stream(name, "{\"query\":\"oceans\"}", name, completed_arguments)
    }

    fn responses_tool_call_stream(
        name: &str,
        streamed_arguments: &str,
        completed_name: &str,
        completed_arguments: &str,
    ) -> String {
        let item = |name: &str, arguments: &str| {
            json!({
                "id": "fc_1",
                "type": "function_call",
                "call_id": "call_1",
                "name": name,
                "arguments": arguments,
            })
        };
        let (first_delta, second_delta) = streamed_arguments.split_at(streamed_arguments.len() / 2);
        [
            json!({"type": "response.created", "response": {"id": "resp_1", "output": []}}),
            json!({"type": "response.output_item.added", "output_index": 0, "item": item(name, "")}),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_1",
                "output_index": 0,
                "delta": first_delta,
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_1",
                "output_index": 0,
                "delta": second_delta,
            }),
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": "fc_1",
                "output_index": 0,
                "arguments": streamed_arguments,
            }),
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": item(name, streamed_arguments),
            }),
            json!({
                "type": "response.completed",
                "response": {"id": "resp_1", "output": [item(completed_name, completed_arguments)]},
            }),
        ]
        .iter()
        .map(|event| format!("event: {}\ndata: {event}", event["type"].as_str().unwrap()))
        .collect::<Vec<_>>()
        .join("\n\n")
    }

    async fn built_in_guardrail_state() -> (tempfile::TempDir, crate::http::state::AppState) {
        let (directory, mut state) = crate::http::test_support::app_state().await;
        state.guardrail_engine = Arc::new(gateway_guardrails::GuardrailEngine::deterministic_only(
            Arc::new(gateway_guardrails::BuiltInEvaluator),
        ));
        (directory, state)
    }

    #[test]
    fn keeps_responses_completed_snapshot_out_of_streamed_arguments() {
        let stream = responses_function_call_stream("wiki_search", "{\"query\":\"oceans\"}");
        let mut fragments = BTreeMap::new();
        for (event_index, block) in stream.split("\n\n").enumerate() {
            let data = block.split_once("data: ").expect("data line").1;
            let event = serde_json::from_str(data).expect("event JSON");
            collect_stream_tool_fragments(&event, "", event_index, &mut fragments);
        }

        let call = &fragments["fc_1"];
        assert_eq!(call.name, "wiki_search");
        assert_eq!(call.arguments, "{\"query\":\"oceans\"}");
        assert_eq!(
            call.terminal_arguments.len(),
            3,
            "done, item done, completed"
        );
    }

    #[tokio::test]
    async fn audit_mode_allows_streamed_responses_function_call() {
        let (_directory, mut state) = built_in_guardrail_state().await;
        let policy = &mut Arc::make_mut(&mut state.guardrail_config).default;
        policy.enabled = true;
        policy.packs = vec![gateway_guardrails::PackId::new("core.shell").expect("pack id")];
        let context = super::InferenceGuardContext {
            route_key: "openai/gpt".into(),
            request_id: "request-1".into(),
            associated_prompt: None,
            enabled: true,
            stream_buffer_bytes: 1 << 20,
        };
        let stream = responses_function_call_stream("wiki_search", "{\"query\":\"oceans\"}");

        let guarded = super::guard_sse_payload(&state, &context, stream.as_bytes())
            .await
            .expect("a well-formed streamed tool call is allowed");

        assert_eq!(String::from_utf8(guarded).expect("UTF-8"), stream);
    }

    #[tokio::test]
    async fn audit_mode_releases_uninspectable_streamed_tool_call() {
        let (_directory, mut state) = built_in_guardrail_state().await;
        let policy = &mut Arc::make_mut(&mut state.guardrail_config).default;
        policy.enabled = true;
        let context = super::InferenceGuardContext {
            route_key: "openai/gpt".into(),
            request_id: "request-1".into(),
            associated_prompt: None,
            enabled: true,
            stream_buffer_bytes: 1 << 20,
        };
        let stream = responses_function_call_stream("wiki_search", "{\"query\":");

        let guarded = super::guard_sse_payload(&state, &context, stream.as_bytes())
            .await
            .expect("audit mode records malformed arguments without blocking");

        assert_eq!(String::from_utf8(guarded).expect("UTF-8"), stream);
    }

    fn stream_guard_context(stream_buffer_bytes: usize) -> super::InferenceGuardContext {
        super::InferenceGuardContext {
            route_key: "openai/gpt".into(),
            request_id: "request-1".into(),
            associated_prompt: None,
            enabled: true,
            stream_buffer_bytes,
        }
    }

    async fn stream_state(mode: gateway_guardrails::PolicyMode) -> (tempfile::TempDir, AppState) {
        let (directory, mut state) = built_in_guardrail_state().await;
        let policy = &mut Arc::make_mut(&mut state.guardrail_config).default;
        policy.enabled = true;
        policy.mode = mode;
        (directory, state)
    }

    async fn recorded_stream_decisions(state: &AppState) -> Vec<(String, String)> {
        // Stream failure decisions are written in the background, so wait for the write.
        for _ in 0..100 {
            let items = state
                .store
                .list_guardrail_decisions(&GuardrailDecisionQuery {
                    page: 1,
                    page_size: 10,
                    request_id: Some("request-1".into()),
                    evaluator: Some("stream_buffer".into()),
                    ..Default::default()
                })
                .await
                .expect("decisions")
                .items;
            if !items.is_empty() {
                return items
                    .iter()
                    .map(|item| {
                        (
                            format!("{}:{}", item.action, item.reason_code),
                            item.content_hash.clone(),
                        )
                    })
                    .collect();
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        Vec::new()
    }

    async fn stream_decision_actions(state: &AppState) -> Vec<String> {
        recorded_stream_decisions(state)
            .await
            .into_iter()
            .map(|(decision, _)| decision)
            .collect()
    }

    fn chunked_stream(chunks: &[&'static str]) -> gateway_core::ProviderStream {
        Box::pin(futures_util::stream::iter(
            chunks
                .iter()
                .map(|chunk| Ok(axum::body::Bytes::from_static(chunk.as_bytes())))
                .collect::<Vec<_>>(),
        ))
    }

    async fn collect_stream(stream: gateway_core::ProviderStream) -> String {
        use futures_util::StreamExt;
        let chunks = stream
            .map(|chunk| chunk.expect("stream chunk"))
            .collect::<Vec<_>>()
            .await;
        String::from_utf8(chunks.concat()).expect("UTF-8")
    }

    #[tokio::test]
    async fn audit_mode_passes_oversized_stream_through_unchanged() {
        let (_directory, state) = stream_state(gateway_guardrails::PolicyMode::Audit).await;
        let chunks = [
            "data: {\"a\":1}\n\n",
            "data: {\"b\":2}\n\n",
            "data: [DONE]\n\n",
        ];

        let Ok(stream) =
            super::guard_stream(&state, &stream_guard_context(20), chunked_stream(&chunks)).await
        else {
            panic!("audit mode releases an oversized stream");
        };

        assert_eq!(collect_stream(stream).await, chunks.concat());
        let consumed = chunks[..2].concat();
        assert_eq!(
            recorded_stream_decisions(&state).await,
            [(
                "audit:stream.buffer_exceeded".to_string(),
                format!("sha256:{:x}", Sha256::digest(consumed.as_bytes())),
            )],
            "the hash covers the chunk that overflowed the buffer"
        );
    }

    #[tokio::test]
    async fn deny_mode_rejects_oversized_stream() {
        let (_directory, state) = stream_state(gateway_guardrails::PolicyMode::Deny).await;
        let chunks = ["data: {\"a\":1}\n\n", "data: {\"b\":2}\n\n"];

        let Err(failure) =
            super::guard_stream(&state, &stream_guard_context(20), chunked_stream(&chunks)).await
        else {
            panic!("deny mode rejects an oversized stream");
        };

        assert!(matches!(
            failure.error,
            GatewayError::PayloadTooLarge { .. }
        ));
        assert_eq!(
            stream_decision_actions(&state).await,
            ["deny:stream.buffer_exceeded"],
            "the rejection is recorded"
        );
    }

    #[tokio::test]
    async fn audit_mode_passes_unparseable_stream_through_unchanged() {
        let (_directory, state) = stream_state(gateway_guardrails::PolicyMode::Audit).await;
        let stream = "data: {\"a\":1}\n\ndata: not-json\n\ndata: [DONE]\n\n";

        let guarded =
            super::guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
                .await
                .expect("audit mode releases an unparseable stream");

        assert_eq!(String::from_utf8(guarded).expect("UTF-8"), stream);
        let decisions = recorded_stream_decisions(&state).await;
        assert_eq!(
            decisions
                .iter()
                .map(|(decision, _)| decision.as_str())
                .collect::<Vec<_>>(),
            ["audit:stream.unparseable"]
        );
    }

    #[tokio::test]
    async fn audit_mode_passes_non_utf8_stream_through_unchanged() {
        let (_directory, state) = stream_state(gateway_guardrails::PolicyMode::Audit).await;
        let stream = b"data: \xff\n\n".as_slice();

        let guarded = super::guard_sse_payload(&state, &stream_guard_context(1 << 20), stream)
            .await
            .expect("audit mode releases a non-UTF-8 stream");

        assert_eq!(guarded, stream);
    }

    #[tokio::test]
    async fn deny_mode_rejects_unparseable_stream() {
        let (_directory, state) = stream_state(gateway_guardrails::PolicyMode::Deny).await;

        let error = super::guard_sse_payload(
            &state,
            &stream_guard_context(1 << 20),
            b"data: not-json\n\n",
        )
        .await
        .expect_err("deny mode rejects an unparseable stream");

        assert!(error.to_string().contains("invalid SSE JSON"));
        assert_eq!(
            stream_decision_actions(&state).await,
            ["deny:stream.unparseable"],
            "the rejection is recorded"
        );
    }

    fn sse_events(events: &[Value]) -> String {
        events
            .iter()
            .map(|event| format!("data: {event}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Guards `stream` in audit mode and returns how many tool-call evaluations it recorded.
    async fn tool_call_evaluation_count(stream: &str) -> usize {
        let (_directory, state) = stream_state(gateway_guardrails::PolicyMode::Audit).await;
        super::guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
            .await
            .expect("allowed");
        state
            .store
            .list_guardrail_decisions(&GuardrailDecisionQuery {
                page: 1,
                page_size: 10,
                request_id: Some("request-1".into()),
                evaluator: Some("built_in".into()),
                ..Default::default()
            })
            .await
            .expect("decisions")
            .items
            .len()
    }

    fn wiki_search_item(id: Option<&str>, call_id: &str, arguments: &str) -> Value {
        let mut item = json!({
            "type": "function_call",
            "call_id": call_id,
            "name": "wiki_search",
            "arguments": arguments,
        });
        if let Some(id) = id {
            item["id"] = json!(id);
        }
        item
    }

    #[tokio::test]
    async fn evaluates_repeated_terminal_snapshots_once() {
        let item = |arguments: &str| wiki_search_item(Some("fc_1"), "call_1", arguments);
        let stream = sse_events(&[
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": "fc_1",
                "arguments": "{\"query\": \"oceans\"}",
            }),
            json!({"type": "response.output_item.done", "item": item("{\"query\":\"oceans\"}")}),
            json!({
                "type": "response.completed",
                "response": {"output": [item("{\"query\": \"oceans\"}")]},
            }),
        ]);

        assert_eq!(tool_call_evaluation_count(&stream).await, 1, "one call");
    }

    #[tokio::test]
    async fn links_snapshots_that_only_carry_the_call_id() {
        let arguments = "{\"query\":\"oceans\"}";
        let stream = sse_events(&[
            json!({
                "type": "response.output_item.added",
                "item": wiki_search_item(Some("fc_1"), "call_1", ""),
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_1",
                "delta": arguments,
            }),
            json!({
                "type": "response.output_item.done",
                "item": wiki_search_item(None, "call_1", arguments),
            }),
            json!({
                "type": "response.completed",
                "response": {"output": [wiki_search_item(None, "call_1", arguments)]},
            }),
        ]);

        assert_eq!(
            tool_call_evaluation_count(&stream).await,
            1,
            "snapshots keyed by call_id belong to the call streamed under its item id"
        );
    }

    fn shell_call_added(id: Option<&str>, command: &str) -> Value {
        let mut item = json!({
            "type": "shell_call",
            "call_id": "call_1",
            "action": {"command": [command]},
        });
        if let Some(id) = id {
            item["id"] = json!(id);
        }
        json!({"type": "response.output_item.added", "item": item})
    }

    #[tokio::test]
    async fn shares_evaluations_between_complete_call_id_aliases() {
        let same = sse_events(&[
            shell_call_added(Some("sc_1"), "ls"),
            shell_call_added(None, "ls"),
        ]);
        let different = sse_events(&[
            shell_call_added(Some("sc_1"), "ls"),
            shell_call_added(None, "pwd"),
        ]);

        assert_eq!(
            tool_call_evaluation_count(&same).await,
            1,
            "one call carried under its id and its call_id"
        );
        assert_eq!(
            tool_call_evaluation_count(&different).await,
            2,
            "distinct arguments of one call are each evaluated"
        );
    }

    #[tokio::test]
    async fn evaluates_identical_parallel_calls_separately() {
        let arguments = "{\"query\":\"oceans\"}";
        let stream = sse_events(&[
            json!({
                "type": "response.output_item.done",
                "item": wiki_search_item(Some("fc_1"), "call_1", arguments),
            }),
            json!({
                "type": "response.output_item.done",
                "item": wiki_search_item(Some("fc_2"), "call_2", arguments),
            }),
        ]);

        assert_eq!(tool_call_evaluation_count(&stream).await, 2, "two calls");
    }

    #[tokio::test]
    async fn rewrites_each_snapshot_in_its_own_argument_encoding() {
        use gateway_guardrails::{
            ContentTransformation, GuardPhase, GuardrailEngine, ManagedCheckConfig,
            ManagedCheckKind, ManagedEvaluator, ManagedOutcome, ManagedService,
            test_utils::StubManagedEvaluator,
        };
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        let masked = || {
            Ok(ManagedOutcome::Transformed {
                transformation: ContentTransformation::new("masked-cmd".into()),
                reason_code: gateway_guardrails::ReasonCode::new("stub.masked").unwrap(),
                metadata: Default::default(),
            })
        };
        let stub = Arc::new(StubManagedEvaluator::new(
            "mask",
            ManagedService::AmazonBedrock,
            [masked(), masked()],
        ));
        state.guardrail_engine = Arc::new(GuardrailEngine::new(
            Vec::new(),
            BTreeMap::from([("mask".to_string(), stub as Arc<dyn ManagedEvaluator>)]),
        ));
        let config = Arc::make_mut(&mut state.guardrail_config);
        config.default.enabled = true;
        config.default.managed_checks = vec!["mask".into()];
        config.managed_checks.insert(
            "mask".into(),
            ManagedCheckConfig {
                kind: ManagedCheckKind::AmazonBedrock,
                phases: [GuardPhase::GeneratedToolCall].into(),
                timeout_ms: 1_000,
                failure_disposition: Default::default(),
                max_content_bytes: 1 << 20,
                bedrock: None,
                model_armor: None,
            },
        );
        let stream = [
            json!({
                "type": "response.output_item.added",
                "item": {"id": "call_1", "type": "shell_call", "action": {"command": ["secret-cmd"]}},
            }),
            json!({
                "type": "response.completed",
                "response": {"output": [{
                    "id": "call_1",
                    "type": "function_call",
                    "name": "bash",
                    "arguments": "{\"command\":\"secret-cmd\"}",
                }]},
            }),
        ]
        .iter()
        .map(|event| format!("data: {event}"))
        .collect::<Vec<_>>()
        .join("\n\n");

        let guarded =
            super::guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
                .await
                .expect("masked");

        let events = String::from_utf8(guarded)
            .expect("UTF-8")
            .split("\n\n")
            .map(|block| {
                let data = block.strip_prefix("data: ").expect("data line");
                serde_json::from_str::<Value>(data).expect("JSON")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            events[0]["item"]["action"]["command"],
            json!(["masked-cmd"]),
            "the added item carrying the command is rewritten too"
        );
        assert_eq!(
            events[1]["response"]["output"][0]["arguments"], "{\"command\":\"masked-cmd\"}",
            "a function-call snapshot keeps JSON arguments after a shell-call delta"
        );
    }

    #[tokio::test]
    async fn snapshot_tool_name_does_not_rename_streamed_arguments() {
        let (_directory, mut state) = stream_state(gateway_guardrails::PolicyMode::Deny).await;
        Arc::make_mut(&mut state.guardrail_config).default.packs =
            vec![gateway_guardrails::PackId::new("core.filesystem").expect("pack id")];
        let command = "{\"command\":\"rm -rf /tmp/work\"}";
        let stream = responses_tool_call_stream("bash", command, "wiki_search", command);

        let error =
            super::guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
                .await
                .expect_err("streamed bash arguments are evaluated as bash");

        let GatewayError::GuardrailDenied { reason_code, .. } = error else {
            panic!("expected a guardrail denial, got {error:?}");
        };
        assert_eq!(reason_code, "filesystem.recursive_force_remove");
    }

    #[tokio::test]
    async fn guards_each_terminal_snapshot_under_its_own_tool_name() {
        let (_directory, mut state) = built_in_guardrail_state().await;
        let policy = &mut Arc::make_mut(&mut state.guardrail_config).default;
        policy.enabled = true;
        policy.mode = gateway_guardrails::PolicyMode::Deny;
        policy.packs = vec![gateway_guardrails::PackId::new("core.filesystem").expect("pack id")];
        let item = |name: &str, arguments: &str| json!({"id": "fc_1", "type": "function_call", "call_id": "call_1", "name": name, "arguments": arguments});
        // A later snapshot with a different tool name must not rename the destructive call.
        let stream = [
            json!({"type": "response.output_item.added", "output_index": 0, "item": item("bash", "")}),
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": item("bash", "{\"command\":\"rm -rf /tmp/work\"}"),
            }),
            json!({
                "type": "response.completed",
                "response": {"id": "resp_1", "output": [item("wiki_search", "{\"query\":\"oceans\"}")]},
            }),
        ]
        .iter()
        .map(|event| format!("event: {}\ndata: {event}", event["type"].as_str().unwrap()))
        .collect::<Vec<_>>()
        .join("\n\n");

        let error =
            super::guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
                .await
                .expect_err("the bash snapshot is guarded as bash");

        let GatewayError::GuardrailDenied { reason_code, .. } = error else {
            panic!("expected a guardrail denial, got {error:?}");
        };
        assert_eq!(reason_code, "filesystem.recursive_force_remove");
    }

    #[tokio::test]
    async fn rewrites_each_terminal_snapshot_in_its_own_call_shape() {
        use gateway_guardrails::{
            ContentTransformation, FailureDisposition, GuardrailEngine, ManagedCheckConfig,
            ManagedCheckKind, ManagedEvaluator, ManagedOutcome, ManagedService, ReasonCode,
            test_utils::StubManagedEvaluator,
        };
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        let config = Arc::make_mut(&mut state.guardrail_config);
        config.default.enabled = true;
        config.default.managed_checks = vec!["mask".into()];
        config.managed_checks.insert(
            "mask".into(),
            ManagedCheckConfig {
                kind: ManagedCheckKind::GoogleModelArmor,
                phases: [GuardPhase::GeneratedToolCall].into(),
                timeout_ms: 1_000,
                failure_disposition: FailureDisposition::FailOpen,
                max_content_bytes: 4_096,
                bedrock: None,
                model_armor: None,
            },
        );
        // The shell snapshot is allowed; the function snapshot's arguments are masked.
        state.guardrail_engine = Arc::new(GuardrailEngine::new(
            Vec::new(),
            BTreeMap::from([(
                "mask".into(),
                Arc::new(StubManagedEvaluator::new(
                    "mask",
                    ManagedService::GoogleModelArmor,
                    [
                        Ok(ManagedOutcome::Allow {
                            reason_code: ReasonCode::new("test.allow").unwrap(),
                            metadata: Default::default(),
                        }),
                        Ok(ManagedOutcome::Transformed {
                            transformation: ContentTransformation::new(
                                "{\"query\":\"[masked]\"}".into(),
                            ),
                            reason_code: ReasonCode::new("test.masked").unwrap(),
                            metadata: Default::default(),
                        }),
                    ],
                )) as Arc<dyn ManagedEvaluator>,
            )]),
        ));
        let stream = [
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": {"id": "fc_1", "type": "shell_call", "action": {"command": ["echo hi"]}},
            }),
            json!({
                "type": "response.completed",
                "response": {"id": "resp_1", "output": [{
                    "id": "fc_1",
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "wiki_search",
                    "arguments": "{\"query\":\"private-query\"}",
                }]},
            }),
        ]
        .iter()
        .map(|event| format!("event: {}\ndata: {event}", event["type"].as_str().unwrap()))
        .collect::<Vec<_>>()
        .join("\n\n");

        let guarded =
            super::guard_sse_payload(&state, &stream_guard_context(1 << 20), stream.as_bytes())
                .await
                .expect("the masked stream is released");

        let guarded = String::from_utf8(guarded).expect("UTF-8");
        assert!(!guarded.contains("private-query"), "{guarded}");
        assert!(guarded.contains("[masked]"), "{guarded}");
        assert!(guarded.contains("echo hi"), "{guarded}");
    }

    #[tokio::test]
    async fn denies_completed_snapshot_that_differs_from_streamed_arguments() {
        let (_directory, mut state) = built_in_guardrail_state().await;
        let policy = &mut Arc::make_mut(&mut state.guardrail_config).default;
        policy.enabled = true;
        policy.mode = gateway_guardrails::PolicyMode::Deny;
        policy.packs = vec![gateway_guardrails::PackId::new("core.filesystem").expect("pack id")];
        let context = super::InferenceGuardContext {
            route_key: "openai/gpt".into(),
            request_id: "request-1".into(),
            associated_prompt: None,
            enabled: true,
            stream_buffer_bytes: 1 << 20,
        };
        let stream = responses_function_call_stream("bash", "{\"command\":\"rm -rf /tmp/work\"}");

        let error = super::guard_sse_payload(&state, &context, stream.as_bytes())
            .await
            .expect_err("the terminal snapshot is evaluated on its own");

        let GatewayError::GuardrailDenied { reason_code, .. } = error else {
            panic!("expected a guardrail denial, got {error:?}");
        };
        assert_ne!(reason_code, "tool_call.malformed_arguments");
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

    #[test]
    fn prompt_pointers_include_tool_definitions_without_request_identifiers() {
        let value = json!({
            "model": "keep",
            "tools": [{
                "type": "function",
                "function": {
                    "name": "inspect_name",
                    "description": "inspect description",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string", "description": "inspect parameter"}
                        }
                    }
                }
            }]
        });
        let mut pointers = Vec::new();
        collect_string_pointers(&value, "", PROMPT_TEXT_FIELDS, &mut pointers);

        assert_eq!(
            pointers,
            vec![
                "/tools/0/function/description",
                "/tools/0/function/name",
                "/tools/0/function/parameters/properties/query/description",
            ]
        );
        assert!(!pointers.iter().any(|pointer| pointer == "/model"));
    }

    #[test]
    fn prompt_pointers_keep_matched_string_arrays_scoped() {
        let value = json!({
            "input": ["first", ["second"]],
            "metadata": ["ignore"],
            "messages": [{"content": "third", "ids": ["ignore"]}]
        });
        let mut pointers = Vec::new();
        collect_string_pointers(&value, "", PROMPT_TEXT_FIELDS, &mut pointers);

        assert_eq!(
            pointers,
            vec!["/input/0", "/input/1/0", "/messages/0/content"]
        );
    }

    #[test]
    fn response_error_pointers_include_all_nested_strings() {
        let value = json!({
            "error": {
                "code": "private-code",
                "data": ["private-data"],
                "message": "private-message"
            }
        });
        let mut pointers = Vec::new();
        collect_all_string_pointers(&value["error"], "/error", &mut pointers);

        assert_eq!(
            pointers,
            vec!["/error/code", "/error/data/0", "/error/message",]
        );
    }
}
