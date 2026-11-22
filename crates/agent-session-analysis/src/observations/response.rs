use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{MAX_TOOL_NAME_CHARS, request::PassiveRequestMetadata, tool_inventory_is_estimated};
use crate::{
    BoundedObservationFacts, EvidenceQuality, InferredObservation, InferredObservationKind,
    LimitationCode, OBSERVATION_PARSER_VERSION,
    tool_calls::{ToolCall, ToolCallKind, distinct_named_tool_calls},
};

const MAX_INFERRED_TOOL_CALLS: usize = 128;

/// Inputs required to classify observations, independent of authentication and persistence.
pub struct ResponseObservationInput<'a> {
    pub request_id: &'a str,
    pub metadata: &'a PassiveRequestMetadata,
    pub response_body: Option<&'a Value>,
    pub occurred_at: OffsetDateTime,
    pub completed_at: OffsetDateTime,
}

#[cfg(test)]
mod tests;
pub fn observations_for_response(input: &ResponseObservationInput<'_>) -> Vec<InferredObservation> {
    let mut observations = Vec::new();
    if input.metadata.message_count.is_some()
        || input.metadata.prompt_bytes.is_some()
        || input.metadata.supplied_tool_count.is_some()
        || !input.metadata.supplied_skills.is_empty()
        || !input.metadata.file_interactions.is_empty()
        || input.metadata.reasoning_config_hash.is_some()
        || input.metadata.cache_requested.is_some()
    {
        observations.push(InferredObservation {
            observation_id: Uuid::nil(),
            kind: InferredObservationKind::SessionMetadataClassified,
            source_request_id: input.request_id.to_string(),
            parser_version: OBSERVATION_PARSER_VERSION.to_string(),
            evidence: EvidenceQuality::Direct,
            occurred_at: input.occurred_at,
            facts: BoundedObservationFacts {
                message_count: input.metadata.message_count,
                prompt_bytes: input.metadata.prompt_bytes,
                supplied_tool_count: input.metadata.supplied_tool_count,
                tool_schema_bytes: input.metadata.tool_schema_bytes,
                supplied_tools: input.metadata.supplied_tools.clone(),
                supplied_skills: input.metadata.supplied_skills.clone(),
                file_interactions: input.metadata.file_interactions.clone(),
                reasoning_config_hash: input.metadata.reasoning_config_hash.clone(),
                cache_requested: input.metadata.cache_requested,
                ..Default::default()
            },
            limitations: tool_inventory_limitations(input.metadata),
        });
    }
    if let Some(response) = input.response_body {
        let (finish_reason, incomplete_reason) = response_finish_reasons(response);
        if finish_reason.is_some() || incomplete_reason.is_some() {
            observations.push(InferredObservation {
                observation_id: Uuid::nil(),
                kind: InferredObservationKind::ResponseFinishClassified,
                source_request_id: input.request_id.to_string(),
                parser_version: OBSERVATION_PARSER_VERSION.to_string(),
                evidence: EvidenceQuality::Direct,
                occurred_at: input.completed_at,
                facts: BoundedObservationFacts {
                    finish_reason,
                    incomplete_reason,
                    ..Default::default()
                },
                limitations: Vec::new(),
            });
        }
        let (calls, scan_truncated) = distinct_named_tool_calls(response, MAX_INFERRED_TOOL_CALLS);
        let first_observation = observations.len();
        observations.extend(
            calls
                .into_iter()
                .map(|call| classify_tool_call(input, call)),
        );
        if scan_truncated {
            if let Some(observation) = observations.get_mut(first_observation) {
                observation
                    .limitations
                    .push(LimitationCode::PayloadTruncated);
            } else {
                observations.push(InferredObservation {
                    observation_id: Uuid::nil(),
                    kind: InferredObservationKind::ToolCallClassified,
                    source_request_id: input.request_id.to_string(),
                    parser_version: OBSERVATION_PARSER_VERSION.to_string(),
                    evidence: EvidenceQuality::Unavailable,
                    occurred_at: input.completed_at,
                    facts: BoundedObservationFacts::default(),
                    limitations: vec![LimitationCode::PayloadTruncated],
                });
            }
        }
    }
    observations
}

pub(super) fn tool_inventory_limitations(metadata: &PassiveRequestMetadata) -> Vec<LimitationCode> {
    if tool_inventory_is_estimated(metadata.supplied_tool_count, metadata.supplied_tools.len())
        == Some(true)
    {
        vec![LimitationCode::ToolInventoryPotentialOnly]
    } else {
        Vec::new()
    }
}

fn response_finish_reasons(response: &Value) -> (Option<String>, Option<String>) {
    let finish_reason = [
        "/choices/0/finish_reason",
        "/stop_reason",
        "/candidates/0/finishReason",
        "/response/choices/0/finish_reason",
        "/response/stop_reason",
    ]
    .into_iter()
    .find_map(|path| response.pointer(path).and_then(Value::as_str))
    .map(|value| value.chars().take(64).collect());
    let incomplete_reason = [
        "/incomplete_details/reason",
        "/response/incomplete_details/reason",
        "/incompleteDetails/reason",
    ]
    .into_iter()
    .find_map(|path| response.pointer(path).and_then(Value::as_str))
    .map(|value| value.chars().take(64).collect());
    (finish_reason, incomplete_reason)
}

fn classify_tool_call(
    input: &ResponseObservationInput<'_>,
    call: ToolCall<'_>,
) -> InferredObservation {
    let name = call.name.unwrap_or_default();
    let normalized = name.to_ascii_lowercase();
    // Tools the provider runs, hosted ones such as `web_search` and remote MCP servers, never
    // touch the caller's files. Client-run `mcp__<server>__<tool>` calls still may.
    let kind = if call.kind != ToolCallKind::Client {
        InferredObservationKind::ToolCallClassified
    } else if normalized.contains("search") || normalized.contains("grep") {
        InferredObservationKind::FileSearchSuspected
    } else if normalized.contains("read") {
        InferredObservationKind::FileReadSuspected
    } else if normalized.contains("overwrite") {
        InferredObservationKind::FileOverwriteSuspected
    } else if normalized.contains("edit") || normalized.contains("patch") {
        InferredObservationKind::FileEditSuspected
    } else if normalized.contains("create") || normalized.contains("write") {
        InferredObservationKind::FileCreateSuspected
    } else if normalized.contains("test")
        || normalized.contains("check")
        || normalized.contains("lint")
        || normalized.contains("build")
    {
        InferredObservationKind::VerificationResultClassified
    } else {
        InferredObservationKind::ToolCallClassified
    };
    // Tool arguments may contain sensitive paths, so classification reads only
    // the tool name and never persists a path-derived identifier.
    InferredObservation {
        observation_id: Uuid::nil(),
        kind,
        source_request_id: input.request_id.to_string(),
        parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        evidence: EvidenceQuality::InferredHigh,
        occurred_at: input.completed_at,
        facts: BoundedObservationFacts {
            tool_name: Some(name.chars().take(MAX_TOOL_NAME_CHARS).collect()),
            ..Default::default()
        },
        limitations: vec![LimitationCode::SemanticVerificationUnavailable],
    }
}
