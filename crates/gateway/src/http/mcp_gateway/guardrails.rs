use std::{borrow::Cow, time::Duration};

use axum::{
    body::{Body, Bytes, to_bytes},
    http::{Response, StatusCode},
};
use gateway_guardrails::{
    DecisionAction, EvaluationInput, EvaluationPayload, GuardrailEvaluation, McpCall,
    PolicyResolver, PolicyTarget,
};
use serde_json::{Map, Value, json};
use tokio::time::timeout;
use uuid::Uuid;

use super::{
    GUARDRAIL_POLICY_DENIED_CODE,
    json_rpc::{mcp_jsonrpc_error_response, mcp_request_id},
};
use crate::http::{guardrail_events::record_guardrail_evaluation, state::AppState};

pub(super) async fn evaluate_mcp_call(
    state: &AppState,
    request_id: Option<&str>,
    mcp_tool_invocation_id: Option<Uuid>,
    server: &str,
    tool: &str,
    arguments: Value,
) -> GuardrailEvaluation {
    let policy =
        PolicyResolver::new(&state.guardrail_config).resolve(PolicyTarget::McpServer(server));
    let evaluation = state
        .guardrail_engine
        .evaluate(
            &policy,
            &state.guardrail_config,
            EvaluationInput::new(
                gateway_guardrails::GuardPhase::McpCall,
                EvaluationPayload::McpCall {
                    call: McpCall {
                        server: server.to_string(),
                        tool: tool.to_string(),
                        arguments,
                    },
                },
            ),
        )
        .await;
    record_guardrail_evaluation(state, request_id, mcp_tool_invocation_id, &evaluation).await;
    evaluation
}

pub(super) async fn evaluate_mcp_result(
    state: &AppState,
    request_id: Option<&str>,
    mcp_tool_invocation_id: Option<Uuid>,
    server: &str,
    tool: &str,
    result: Value,
) -> GuardrailEvaluation {
    let policy =
        PolicyResolver::new(&state.guardrail_config).resolve(PolicyTarget::McpServer(server));
    let evaluation = state
        .guardrail_engine
        .evaluate(
            &policy,
            &state.guardrail_config,
            EvaluationInput::new(
                gateway_guardrails::GuardPhase::McpResult,
                EvaluationPayload::McpResult {
                    server: server.to_string(),
                    tool: tool.to_string(),
                    result,
                },
            ),
        )
        .await;
    record_guardrail_evaluation(state, request_id, mcp_tool_invocation_id, &evaluation).await;
    evaluation
}

pub(super) fn guardrail_decision_metadata(evaluation: &GuardrailEvaluation) -> Map<String, Value> {
    let relevant = evaluation
        .decisions
        .iter()
        .rev()
        .find(|decision| decision.action != DecisionAction::Allow);
    let action = match evaluation.action {
        DecisionAction::Allow => "allowed",
        DecisionAction::Audit => "audit",
        DecisionAction::Deny => "denied",
        DecisionAction::Transformed => "transformed",
    };
    Map::from_iter([
        ("guardrail_decision".to_string(), json!(action)),
        (
            "guardrail_decision_id".to_string(),
            relevant
                .map(|decision| json!(decision.decision_id.to_string()))
                .unwrap_or(Value::Null),
        ),
        (
            "guardrail_reason_code".to_string(),
            relevant
                .map(|decision| json!(decision.reason_code.as_str()))
                .unwrap_or(Value::Null),
        ),
    ])
}

pub(super) fn guardrail_denied_response(
    id: Option<&Value>,
    evaluation: &GuardrailEvaluation,
) -> Response<Body> {
    let reason_code = evaluation
        .decisions
        .iter()
        .find(|decision| decision.action == DecisionAction::Deny)
        .map(|decision| decision.reason_code.as_str())
        .unwrap_or("guardrail.policy_denied");
    mcp_jsonrpc_error_response(
        StatusCode::FORBIDDEN,
        id,
        GUARDRAIL_POLICY_DENIED_CODE,
        &format!("MCP operation denied by guardrail policy ({reason_code})"),
    )
}

pub(super) async fn enforce_direct_mcp_result(
    state: &AppState,
    server: &str,
    tool: &str,
    id: Option<&Value>,
    mcp_tool_invocation_id: Uuid,
    response: Response<Body>,
) -> (Response<Body>, Option<GuardrailEvaluation>, bool) {
    let policy =
        PolicyResolver::new(&state.guardrail_config).resolve(PolicyTarget::McpServer(server));
    if !policy.enabled {
        return (response, None, false);
    }
    let (mut parts, body) = response.into_parts();
    let buffer_timeout = Duration::from_millis(policy.stream_buffer_timeout_ms);
    let bytes = match timeout(buffer_timeout, to_bytes(body, policy.stream_buffer_bytes)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => {
            return (
                mcp_jsonrpc_error_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    id,
                    GUARDRAIL_POLICY_DENIED_CODE,
                    "MCP result exceeded the configured guardrail buffer",
                ),
                None,
                true,
            );
        }
        Err(_) => {
            return (
                mcp_jsonrpc_error_response(
                    StatusCode::GATEWAY_TIMEOUT,
                    id,
                    GUARDRAIL_POLICY_DENIED_CODE,
                    "MCP result did not complete within the guardrail buffer timeout",
                ),
                None,
                true,
            );
        }
    };
    let request_id = mcp_request_id(&id.cloned());
    let output = if parts
        .headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"))
    {
        match guard_mcp_sse_result(
            state,
            &request_id,
            mcp_tool_invocation_id,
            server,
            tool,
            &bytes,
        )
        .await
        {
            Ok((output, evaluation)) => (output, evaluation),
            Err(evaluation) => {
                let response = evaluation.as_ref().map_or_else(
                    || {
                        mcp_jsonrpc_error_response(
                            StatusCode::FORBIDDEN,
                            id,
                            GUARDRAIL_POLICY_DENIED_CODE,
                            "MCP result was not valid guarded SSE",
                        )
                    },
                    |evaluation| guardrail_denied_response(id, evaluation),
                );
                return (response, evaluation, true);
            }
        }
    } else {
        let mut parsed = serde_json::from_slice::<Value>(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        let Ok((payload_field, result_payload)) = guarded_mcp_payload(&parsed) else {
            return (
                mcp_jsonrpc_error_response(
                    StatusCode::FORBIDDEN,
                    id,
                    GUARDRAIL_POLICY_DENIED_CODE,
                    "MCP result was not a valid guarded response",
                ),
                None,
                true,
            );
        };
        let evaluation = evaluate_mcp_result(
            state,
            Some(&request_id),
            Some(mcp_tool_invocation_id),
            server,
            tool,
            result_payload,
        )
        .await;
        if evaluation.denied() {
            return (
                guardrail_denied_response(id, &evaluation),
                Some(evaluation),
                true,
            );
        }
        let output = if evaluation
            .decisions
            .iter()
            .any(|decision| decision.transformed)
        {
            match &evaluation.output {
                EvaluationPayload::McpResult { result, .. } => {
                    if let Some(payload_field) = payload_field
                        && let Some(slot) = parsed.get_mut(payload_field)
                    {
                        *slot = result.clone();
                        serde_json::to_vec(&parsed)
                            .map(Bytes::from)
                            .unwrap_or(bytes)
                    } else {
                        serde_json::to_vec(result).map(Bytes::from).unwrap_or(bytes)
                    }
                }
                _ => bytes,
            }
        } else {
            bytes
        };
        (output, Some(evaluation))
    };
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    (
        Response::from_parts(parts, Body::from(output.0)),
        output.1,
        false,
    )
}

async fn guard_mcp_sse_result(
    state: &AppState,
    request_id: &str,
    mcp_tool_invocation_id: Uuid,
    server: &str,
    tool: &str,
    bytes: &Bytes,
) -> Result<(Bytes, Option<GuardrailEvaluation>), Option<GuardrailEvaluation>> {
    let source = std::str::from_utf8(bytes).map_err(|_| None)?;
    // SSE permits mixed CR, LF, and CRLF line endings and an initial UTF-8 BOM.
    let normalized = source
        .strip_prefix('\u{feff}')
        .unwrap_or(source)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut blocks = normalized
        .split("\n\n")
        .map(Cow::Borrowed)
        .collect::<Vec<_>>();
    let mut event_values = Vec::new();
    let mut events = Vec::new();
    for (block_index, block) in blocks.iter().enumerate() {
        let data = block
            .split('\n')
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|data| data.strip_prefix(' ').unwrap_or(data))
            .collect::<Vec<_>>();
        if data.is_empty() {
            continue;
        }
        let data = data.join("\n");
        if data == "[DONE]" {
            continue;
        }
        let parsed = serde_json::from_str::<Value>(&data).map_err(|_| None)?;
        let (payload_field, payload) = guarded_mcp_payload(&parsed).map_err(|()| None)?;
        event_values.push(payload);
        events.push(GuardedSseEvent {
            block_index,
            envelope: parsed,
            payload_field,
        });
    }
    if event_values.is_empty() {
        return Ok((bytes.clone(), None));
    }

    let event_count = event_values.len();
    let evaluation = evaluate_mcp_result(
        state,
        Some(request_id),
        Some(mcp_tool_invocation_id),
        server,
        tool,
        Value::Array(event_values),
    )
    .await;
    if evaluation.denied() {
        return Err(Some(evaluation));
    }
    if !evaluation
        .decisions
        .iter()
        .any(|decision| decision.transformed)
    {
        return Ok((bytes.clone(), Some(evaluation)));
    }
    let replacements = match &evaluation.output {
        EvaluationPayload::McpResult { result, .. } => result
            .as_array()
            .filter(|result| result.len() == event_count),
        _ => None,
    }
    .ok_or_else(|| Some(evaluation.clone()))?;

    for (event, replacement) in events.into_iter().zip(replacements) {
        let mut envelope = event.envelope;
        if let Some(field) = event.payload_field {
            envelope[field] = replacement.clone();
        } else {
            envelope = replacement.clone();
        }
        blocks[event.block_index] =
            Cow::Owned(rewrite_sse_event(&blocks[event.block_index], &envelope));
    }
    Ok((Bytes::from(blocks.join("\n\n")), Some(evaluation)))
}

struct GuardedSseEvent {
    block_index: usize,
    envelope: Value,
    payload_field: Option<&'static str>,
}

fn rewrite_sse_event(block: &str, envelope: &Value) -> String {
    let data = serde_json::to_string(envelope).expect("guardrail MCP envelope is valid JSON");
    let mut replacement = Some(format!("data: {data}"));
    block
        .split('\n')
        .filter_map(|line| {
            if line.starts_with("data:") {
                replacement.take()
            } else {
                Some(line.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn guarded_mcp_payload(value: &Value) -> Result<(Option<&'static str>, Value), ()> {
    // A response cannot carry both fields: selecting one would leave the other unguarded.
    if value.get("result").is_some() && value.get("error").is_some() {
        return Err(());
    }
    Ok(value
        .get("result")
        .map(|result| (Some("result"), result.clone()))
        .or_else(|| {
            value
                .get("error")
                .map(|error| (Some("error"), error.clone()))
        })
        .unwrap_or_else(|| (None, value.clone())))
}

#[cfg(test)]
mod tests;
