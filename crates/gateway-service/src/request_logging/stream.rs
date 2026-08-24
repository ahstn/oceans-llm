use std::collections::HashSet;

use gateway_core::{RequestAttemptRecord, SseEventParser};
use serde_json::{Map, Value, json};

use crate::{
    RequestLogIconMetadata,
    redaction::{
        RequestLogPayloadPolicy, redact_json_value_with_policy, truncate_large_payload_fields,
    },
};

use super::{
    tool_cardinality::{ToolCallIdentity, tool_call_identities_from_value},
    truncate_payload,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamFailureSummary {
    pub status_code: i64,
    pub error_code: String,
}

#[derive(Debug, Clone)]
pub struct StreamLogResultInput {
    pub provider_key: String,
    pub icon_metadata: RequestLogIconMetadata,
    pub latency_ms: i64,
    pub collector: StreamResponseCollector,
    pub failure: Option<StreamFailureSummary>,
    pub attempts: Vec<RequestAttemptRecord>,
}

#[derive(Debug, Clone, Default)]
pub struct StreamResponseCollector {
    parser: SseEventParser,
    payload_policy: RequestLogPayloadPolicy,
    events: Vec<Value>,
    usage: Option<Value>,
    failure: Option<StreamFailureSummary>,
    seen_tool_call_ids: HashSet<String>,
    anonymous_tool_call_count: i64,
    finished: bool,
    truncated: bool,
}

impl StreamResponseCollector {
    pub(super) fn with_payload_policy(payload_policy: RequestLogPayloadPolicy) -> Self {
        Self {
            payload_policy,
            ..Self::default()
        }
    }

    pub fn observe_chunk(&mut self, chunk: &[u8]) {
        if self.finished {
            return;
        }

        let events = match self.parser.push_bytes(chunk) {
            Ok(events) => events,
            Err(_) => {
                self.truncated = true;
                self.failure.get_or_insert_with(|| StreamFailureSummary {
                    status_code: 502,
                    error_code: "stream_parse_error".to_string(),
                });
                return;
            }
        };

        for event in events {
            let payload = event.data.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }

            let parsed = serde_json::from_str::<Value>(payload).ok();
            if let Some(usage) = parsed
                .as_ref()
                .and_then(usage_value_from_stream_event)
                .filter(|usage| !usage.is_null())
            {
                merge_usage_observation(&mut self.usage, usage);
            }
            if let Some(failure) = parsed.as_ref().and_then(stream_failure_from_value) {
                self.failure = Some(failure);
            }
            if let Some(parsed) = parsed.as_ref() {
                self.observe_tool_calls(parsed);
            }

            if self.events.len() >= self.payload_policy.stream_max_events {
                self.truncated = true;
                continue;
            }

            self.events
                .push(parsed.unwrap_or_else(|| json!({ "raw": payload })));
        }
    }

    pub fn finish(&mut self) {
        if self.finished {
            return;
        }

        self.finished = true;
        if self.parser.finish().is_err() {
            self.truncated = true;
            self.failure.get_or_insert_with(|| StreamFailureSummary {
                status_code: 502,
                error_code: "stream_parse_error".to_string(),
            });
        }
    }

    #[must_use]
    pub fn usage(&self) -> Option<&Value> {
        self.usage.as_ref()
    }

    #[must_use]
    pub fn failure(&self) -> Option<&StreamFailureSummary> {
        self.failure.as_ref()
    }

    #[must_use]
    pub fn analysis_payload(&self) -> Option<Value> {
        (!self.events.is_empty()).then(|| json!({ "events": self.events }))
    }

    #[must_use]
    pub fn invoked_tool_count(&self) -> i64 {
        i64::try_from(self.seen_tool_call_ids.len())
            .unwrap_or(i64::MAX)
            .saturating_add(self.anonymous_tool_call_count)
    }

    fn observe_tool_calls(&mut self, value: &Value) {
        for identity in tool_call_identities_from_value(value) {
            match identity {
                ToolCallIdentity::Known(id) => {
                    self.seen_tool_call_ids.insert(id);
                }
                ToolCallIdentity::Anonymous => {
                    self.anonymous_tool_call_count =
                        self.anonymous_tool_call_count.saturating_add(1);
                }
            }
        }
    }

    pub(super) fn into_payload(self, failure: Option<&StreamFailureSummary>) -> (Value, bool) {
        let payload = redact_json_value_with_policy(
            &json!({
                "stream": true,
                "events": self.events,
                "usage": self.usage,
                "error": failure.map(|failure| {
                    json!({
                        "status_code": failure.status_code,
                        "code": failure.error_code,
                    })
                }),
            }),
            &self.payload_policy,
        );
        truncate_payload(
            truncate_large_payload_fields(&payload),
            self.payload_policy.response_max_bytes,
        )
        .map_truncated(self.truncated)
    }
}

fn stream_failure_from_value(value: &Value) -> Option<StreamFailureSummary> {
    let error = value.get("error")?.as_object()?;
    Some(StreamFailureSummary {
        status_code: 502,
        error_code: error
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("stream_error")
            .to_string(),
    })
}

fn merge_usage_observation(current: &mut Option<Value>, incoming: &Value) {
    let Some(current_object) = current.as_mut().and_then(Value::as_object_mut) else {
        *current = Some(incoming.clone());
        return;
    };
    let Some(incoming_object) = incoming.as_object() else {
        *current = Some(incoming.clone());
        return;
    };
    merge_usage_objects(current_object, incoming_object);
}

fn merge_usage_objects(current: &mut Map<String, Value>, incoming: &Map<String, Value>) {
    for (key, incoming_value) in incoming {
        match (
            current.get_mut(key).and_then(Value::as_object_mut),
            incoming_value.as_object(),
        ) {
            (Some(current_nested), Some(incoming_nested)) => {
                merge_usage_objects(current_nested, incoming_nested);
            }
            _ => {
                current.insert(key.clone(), incoming_value.clone());
            }
        }
    }
}

fn usage_value_from_stream_event(value: &Value) -> Option<&Value> {
    let usage = value
        .get("usage")
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("usage"))
        })
        .or_else(|| {
            value
                .get("message")
                .and_then(|message| message.get("usage"))
        })?;
    if is_synthetic_anthropic_zero_usage(value, usage) {
        return None;
    }
    Some(usage)
}

fn is_synthetic_anthropic_zero_usage(value: &Value, usage: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("message_delta")
        && value
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .and_then(Value::as_str)
            .is_some()
        && usage.get("input_tokens").and_then(Value::as_i64) == Some(0)
        && usage.get("output_tokens").and_then(Value::as_i64) == Some(0)
        && usage
            .get("total_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            == 0
}

trait PayloadResultExt {
    fn map_truncated(self, additional_truncated: bool) -> (Value, bool);
}

impl PayloadResultExt for (Value, bool) {
    fn map_truncated(self, additional_truncated: bool) -> (Value, bool) {
        (self.0, self.1 || additional_truncated)
    }
}
