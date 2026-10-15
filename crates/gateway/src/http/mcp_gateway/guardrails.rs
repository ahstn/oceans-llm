use std::{borrow::Cow, time::Duration};

use axum::{
    body::{Body, Bytes},
    http::{Response, StatusCode},
};
use futures_util::{
    StreamExt,
    stream::{self, BoxStream},
};
use gateway_core::{GatewayError, ProviderError};
use gateway_guardrails::{
    DecisionAction, EffectivePolicy, EvaluationError, EvaluationInput, EvaluationPayload,
    GuardPhase, GuardrailEvaluation, McpCall, PolicyResolver, PolicyTarget,
    uninspected_content_decision,
};
use serde_json::{Map, Value, json};
use tokio::time::{Instant, timeout_at};
use uuid::Uuid;

use super::{
    GUARDRAIL_POLICY_DENIED_CODE,
    json_rpc::{mcp_jsonrpc_error_response, mcp_request_id},
};
use crate::http::{
    guardrail_events::{
        record_guardrail_decisions, record_guardrail_evaluation, uninspected_content_hash,
    },
    state::AppState,
};

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
                GuardPhase::McpCall,
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
                GuardPhase::McpResult,
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
) -> Result<GuardedMcpResult, GatewayError> {
    let policy =
        PolicyResolver::new(&state.guardrail_config).resolve(PolicyTarget::McpServer(server));
    if !policy.enabled {
        return Ok((response, None, false));
    }
    let (mut parts, body) = response.into_parts();
    let request_id = mcp_request_id(&id.cloned());
    let bytes = match buffer_mcp_result(body, &policy).await? {
        BufferedMcpResult::Complete(bytes) => bytes,
        BufferedMcpResult::Partial {
            failure,
            prefix,
            rest,
        } => {
            let released = release_uninspected_result(
                state,
                &policy,
                &request_id,
                mcp_tool_invocation_id,
                failure.error(),
                &prefix,
            );
            let Some(evaluation) = released else {
                return Ok((failure.response(id), None, true));
            };
            // The consumed prefix is replayed ahead of the unread remainder, so the caller
            // receives the upstream result unchanged.
            let body = stream::iter([Ok(Bytes::from(prefix))]).chain(rest);
            return Ok((
                Response::from_parts(parts, Body::from_stream(body)),
                Some(evaluation),
                false,
            ));
        }
    };
    let event_stream = parts
        .headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"));
    let guarded = if event_stream {
        guard_mcp_sse_result(
            state,
            &request_id,
            mcp_tool_invocation_id,
            server,
            tool,
            &bytes,
        )
        .await
    } else {
        guard_mcp_json_result(
            state,
            &request_id,
            mcp_tool_invocation_id,
            server,
            tool,
            &bytes,
        )
        .await
    };
    Ok(match guarded {
        Ok((output, evaluation)) => {
            parts.headers.remove(axum::http::header::CONTENT_LENGTH);
            (
                Response::from_parts(parts, Body::from(output)),
                evaluation,
                false,
            )
        }
        Err(Some(evaluation)) => (
            guardrail_denied_response(id, &evaluation),
            Some(evaluation),
            true,
        ),
        Err(None) => {
            let released = release_uninspected_result(
                state,
                &policy,
                &request_id,
                mcp_tool_invocation_id,
                EvaluationError::UnparseableStream,
                &bytes,
            );
            match released {
                Some(evaluation) => (
                    Response::from_parts(parts, Body::from(bytes)),
                    Some(evaluation),
                    false,
                ),
                None => (
                    mcp_jsonrpc_error_response(
                        StatusCode::FORBIDDEN,
                        id,
                        GUARDRAIL_POLICY_DENIED_CODE,
                        if event_stream {
                            "MCP result was not valid guarded SSE"
                        } else {
                            "MCP result was not a valid guarded response"
                        },
                    ),
                    None,
                    true,
                ),
            }
        }
    })
}

/// The guarded response, the result evaluation when one ran, and whether the result was denied.
pub(super) type GuardedMcpResult = (Response<Body>, Option<GuardrailEvaluation>, bool);

/// An upstream MCP result read into memory for inspection.
enum BufferedMcpResult {
    Complete(Bytes),
    /// Reading stopped before the end. `prefix` holds every byte consumed so far and `rest`
    /// the remainder, starting with any chunk that was read but not buffered.
    Partial {
        failure: BufferFailure,
        prefix: Vec<u8>,
        rest: BoxStream<'static, Result<Bytes, axum::Error>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BufferFailure {
    Exceeded,
    TimedOut,
}

impl BufferFailure {
    fn error(self) -> EvaluationError {
        match self {
            Self::Exceeded => EvaluationError::StreamBufferExceeded,
            Self::TimedOut => EvaluationError::StreamBufferTimeout,
        }
    }

    fn response(self, id: Option<&Value>) -> Response<Body> {
        let (status, message) = match self {
            Self::Exceeded => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "MCP result exceeded the configured guardrail buffer",
            ),
            Self::TimedOut => (
                StatusCode::GATEWAY_TIMEOUT,
                "MCP result did not complete within the guardrail buffer timeout",
            ),
        };
        mcp_jsonrpc_error_response(status, id, GUARDRAIL_POLICY_DENIED_CODE, message)
    }
}

/// Reads the result within the policy's buffer size and timeout. Unlike `to_bytes`, a result
/// that stops short keeps what was consumed, so audit mode can still release all of it. A body
/// that fails mid-read is an upstream failure, not an uninspectable result, so it is an error.
async fn buffer_mcp_result(
    body: Body,
    policy: &EffectivePolicy,
) -> Result<BufferedMcpResult, GatewayError> {
    let deadline = Instant::now() + Duration::from_millis(policy.stream_buffer_timeout_ms);
    let mut upstream = body.into_data_stream();
    let mut buffered = Vec::new();
    let (failure, unbuffered) = loop {
        match timeout_at(deadline, upstream.next()).await {
            Ok(None) => return Ok(BufferedMcpResult::Complete(Bytes::from(buffered))),
            Ok(Some(Ok(chunk)))
                if buffered.len().saturating_add(chunk.len()) <= policy.stream_buffer_bytes =>
            {
                buffered.extend_from_slice(&chunk);
            }
            Ok(Some(Ok(chunk))) => break (BufferFailure::Exceeded, Some(Ok(chunk))),
            // The upstream error can carry its URL, so it is not passed through.
            Ok(Some(Err(_))) => {
                return Err(ProviderError::Transport(
                    "MCP upstream response body failed".to_string(),
                )
                .into());
            }
            Err(_) => break (BufferFailure::TimedOut, None),
        }
    };
    Ok(BufferedMcpResult::Partial {
        failure,
        prefix: buffered,
        rest: stream::iter(unbuffered).chain(upstream).boxed(),
    })
}

/// Records an audit decision for an MCP result the guard cannot inspect. Returns the
/// evaluation when the policy is in audit mode, which releases the result unchanged instead
/// of failing the call; deny mode returns `None` and the caller rejects the result.
fn release_uninspected_result(
    state: &AppState,
    policy: &EffectivePolicy,
    request_id: &str,
    mcp_tool_invocation_id: Uuid,
    error: EvaluationError,
    content: &[u8],
) -> Option<GuardrailEvaluation> {
    let decision = uninspected_content_decision(
        policy,
        GuardPhase::McpResult,
        "stream_buffer",
        error,
        uninspected_content_hash(&[content]),
    );
    if decision.action == DecisionAction::Deny {
        return None;
    }
    let evaluation = GuardrailEvaluation {
        action: DecisionAction::Audit,
        decisions: vec![decision],
        output: EvaluationPayload::McpResult {
            server: String::new(),
            tool: String::new(),
            result: Value::Null,
        },
    };
    // Persisting on a background task keeps the released result from waiting on the store.
    let (state, request_id, decisions) = (
        state.clone(),
        request_id.to_string(),
        evaluation.decisions.clone(),
    );
    tokio::spawn(async move {
        record_guardrail_decisions(
            &state,
            Some(&request_id),
            Some(mcp_tool_invocation_id),
            &decisions,
        )
        .await;
    });
    Some(evaluation)
}

async fn guard_mcp_json_result(
    state: &AppState,
    request_id: &str,
    mcp_tool_invocation_id: Uuid,
    server: &str,
    tool: &str,
    bytes: &Bytes,
) -> Result<(Bytes, Option<GuardrailEvaluation>), Option<GuardrailEvaluation>> {
    let mut parsed = serde_json::from_slice::<Value>(bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(bytes).into_owned()));
    let (payload_field, result_payload) = guarded_mcp_payload(&parsed).map_err(|()| None)?;
    let evaluation = evaluate_mcp_result(
        state,
        Some(request_id),
        Some(mcp_tool_invocation_id),
        server,
        tool,
        result_payload,
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
    let output = match &evaluation.output {
        EvaluationPayload::McpResult { result, .. } => if let Some(payload_field) = payload_field
            && let Some(slot) = parsed.get_mut(payload_field)
        {
            *slot = result.clone();
            serde_json::to_vec(&parsed).map(Bytes::from)
        } else {
            serde_json::to_vec(result).map(Bytes::from)
        }
        .unwrap_or_else(|_| bytes.clone()),
        _ => bytes.clone(),
    };
    Ok((output, Some(evaluation)))
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
