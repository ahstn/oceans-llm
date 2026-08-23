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
        &["content", "input", "prompt", "text"],
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
        &["content", "output_text", "text"],
    )
    .await?;
    Ok(())
}

pub async fn guard_stream(
    state: &AppState,
    context: &InferenceGuardContext,
    mut upstream: ProviderStream,
) -> Result<ProviderStream, GatewayError> {
    if !context.enabled {
        return Ok(upstream);
    }
    let mut buffered = Vec::new();
    while let Some(chunk) = upstream.next().await {
        let chunk = chunk.map_err(GatewayError::Provider)?;
        if buffered.len().saturating_add(chunk.len()) > context.stream_buffer_bytes {
            return Err(GatewayError::PayloadTooLarge {
                limit_bytes: context.stream_buffer_bytes,
            });
        }
        buffered.extend_from_slice(&chunk);
    }
    let guarded = guard_sse_payload(state, context, &buffered).await?;
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
    let mut text_locations = Vec::<(usize, String)>::new();

    for block in source.split(&event_separator) {
        let mut lines = Vec::new();
        for line in block.split(separator) {
            let Some(data) = line.strip_prefix("data:") else {
                lines.push(StreamLine::Other(line.to_string()));
                continue;
            };
            let data = data.strip_prefix(' ').unwrap_or(data);
            if data == "[DONE]" {
                lines.push(StreamLine::Done);
                continue;
            }
            let mut value: Value = serde_json::from_str(data).map_err(|error| {
                GatewayError::Internal(format!("provider stream contained invalid JSON: {error}"))
            })?;
            let event_index = blocks.len();
            let mut pointers = Vec::new();
            collect_string_pointers(
                &value,
                "",
                &["content", "output_text", "text"],
                &mut pointers,
            );
            if value
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.ends_with("output_text.delta"))
                && value.get("delta").is_some_and(Value::is_string)
            {
                pointers.push("/delta".to_string());
            }
            text_locations.extend(pointers.into_iter().map(|pointer| (event_index, pointer)));
            collect_stream_tool_fragments(&value, "", event_index, &mut tool_fragments);
            lines.push(StreamLine::Json(std::mem::take(&mut value)));
        }
        blocks.push(lines);
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
            && let Ok(arguments) = serde_json::to_string(&arguments)
        {
            let mut replacement = Some(arguments);
            for (event_index, pointer) in &fragments.argument_locations {
                for line in &mut blocks[*event_index] {
                    if let StreamLine::Json(value) = line
                        && let Some(slot) = value.pointer_mut(pointer)
                    {
                        *slot = Value::String(replacement.take().unwrap_or_default());
                        break;
                    }
                }
            }
        }
    }

    let combined_text = text_locations
        .iter()
        .filter_map(|(event_index, pointer)| {
            blocks[*event_index].iter().find_map(|line| match line {
                StreamLine::Json(value) => value.pointer(pointer).and_then(Value::as_str),
                _ => None,
            })
        })
        .collect::<String>();
    if !combined_text.is_empty() {
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
            let mut replacement = Some(transformed.to_string());
            for (event_index, pointer) in &text_locations {
                for line in &mut blocks[*event_index] {
                    if let StreamLine::Json(value) = line
                        && let Some(slot) = value.pointer_mut(pointer)
                    {
                        *slot = Value::String(replacement.take().unwrap_or_default());
                        break;
                    }
                }
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
}

fn collect_stream_tool_fragments(
    value: &Value,
    pointer: &str,
    event_index: usize,
    output: &mut BTreeMap<String, ToolCallFragments>,
) {
    match value {
        Value::Object(object) => {
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
            if let Some(function) = object.get("function").and_then(Value::as_object) {
                let key = object
                    .get("index")
                    .or_else(|| object.get("id"))
                    .map(Value::to_string)
                    .unwrap_or_else(|| "default".to_string());
                let fragments = output.entry(key).or_default();
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    fragments.name.push_str(name);
                }
                if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                    fragments.arguments.push_str(arguments);
                    fragments
                        .argument_locations
                        .push((event_index, format!("{pointer}/function/arguments")));
                }
            }
            if object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.ends_with("function_call_arguments.delta"))
            {
                let key = object
                    .get("item_id")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "default".to_string());
                if let Some(delta) = object.get("delta").and_then(Value::as_str) {
                    let fragments = output.entry(key).or_default();
                    fragments.arguments.push_str(delta);
                    fragments
                        .argument_locations
                        .push((event_index, format!("{pointer}/delta")));
                }
            }
            if matches!(
                object.get("type").and_then(Value::as_str),
                Some("function_call" | "tool_use")
            ) {
                let key = object
                    .get("id")
                    .or_else(|| object.get("call_id"))
                    .map(Value::to_string)
                    .unwrap_or_else(|| "default".to_string());
                let fragments = output.entry(key).or_default();
                if let Some(name) = object.get("name").and_then(Value::as_str) {
                    fragments.name.push_str(name);
                }
                if let Some((field, arguments)) =
                    ["arguments", "input"].into_iter().find_map(|field| {
                        object
                            .get(field)
                            .and_then(Value::as_str)
                            .map(|arguments| (field, arguments))
                    })
                {
                    fragments.arguments.push_str(arguments);
                    fragments
                        .argument_locations
                        .push((event_index, format!("{pointer}/{field}")));
                }
            }
            for (key, child) in object {
                if anthropic_index.is_some() && matches!(key.as_str(), "content_block" | "delta") {
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
    if evaluation
        .decisions
        .iter()
        .any(|decision| decision.transformed)
        && let EvaluationPayload::TextSegments { segments } = &evaluation.output
    {
        for (pointer, transformed) in pointers.iter().zip(segments) {
            if let Some(slot) = value.pointer_mut(pointer) {
                *slot = Value::String(transformed.clone());
            }
        }
    }
    Ok(Some(inspected.join("\n")))
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
    if let Some(function) = object.get("function").and_then(Value::as_object) {
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
    None
}

fn parse_arguments(value: &Value) -> Value {
    value
        .as_str()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_else(|| value.clone())
}

fn shell_command(name: &str, arguments: &Value) -> Option<String> {
    const SHELL_TOOL_NAMES: &[&str] = &["bash", "shell", "shell_command", "terminal.exec"];
    if !SHELL_TOOL_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
        return None;
    }
    arguments
        .get("command")
        .or_else(|| arguments.get("cmd"))
        .or_else(|| arguments.get("script"))
        .and_then(Value::as_str)
        .map(str::to_string)
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
            && slot.is_string()
        {
            *slot = Value::String(replacement.to_string());
            return true;
        }
    }
    false
}

fn replace_tool_call_arguments(object: &mut Map<String, Value>, arguments: Value) {
    if let Some(function) = object.get_mut("function").and_then(Value::as_object_mut)
        && let Some(slot) = function.get_mut("arguments")
    {
        replace_json_value(slot, arguments);
        return;
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

    use super::*;

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
        assert_eq!(fragments["0"].name, "bash");
        assert_eq!(fragments["0"].arguments, "{\"command\":\"rm -rf /tmp/x\"}");
        assert_eq!(fragments["1"].arguments, "{\"value\":1}");
        assert_eq!(fragments["0"].argument_locations.len(), 2);
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
            fragments["\"item-1\""].arguments,
            "{\"cmd\":\"git reset --hard\"}"
        );
        assert_eq!(fragments["\"item-1\""].name, "shell");
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
    }

    #[test]
    fn text_pointers_target_content_without_touching_identifiers() {
        let value = json!({"id": "keep", "choices": [{"message": {"content": "inspect"}}]});
        let mut pointers = Vec::new();
        collect_string_pointers(&value, "", &["content"], &mut pointers);
        assert_eq!(pointers, vec!["/choices/0/message/content"]);
    }
}
