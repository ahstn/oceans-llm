//! Bounded, privacy-preserving facts inferred from policy-permitted request and response inputs.

use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::identity::hashing::{hash_identifier, stable_uuid};
use crate::{
    InferredObservation, InferredObservationKind, LimitationCode, OBSERVATION_PARSER_VERSION,
};

pub mod request;
pub mod response;

const MAX_TOOL_NAME_CHARS: usize = 128;
const OBSERVATION_SET_ID_NAMESPACE: Uuid = Uuid::from_u128(0x373f2ed6_0734_4af4_bfb3_ea4ad2b890a4);
const OBSERVATION_ID_NAMESPACE: Uuid = Uuid::from_u128(0xbdfc8775_f822_425d_8d0f_9a553961fc58);

pub fn observation_set_id(session_id: Uuid, request_id: &str, watermark: OffsetDateTime) -> Uuid {
    stable_uuid(
        OBSERVATION_SET_ID_NAMESPACE,
        &json!({
            "session": session_id,
            "request": request_id,
            "parser": OBSERVATION_PARSER_VERSION,
            "watermark": watermark.unix_timestamp_nanos(),
        })
        .to_string(),
    )
}

pub(crate) fn tool_inventory_is_estimated(
    supplied_tool_count: Option<u32>,
    retained_tool_count: usize,
) -> Option<bool> {
    let retained_tool_count = u32::try_from(retained_tool_count).unwrap_or(u32::MAX);
    supplied_tool_count.map(|supplied_count| supplied_count > retained_tool_count)
}

pub fn normalize_legacy_tool_inventory_limitations(observations: &mut [InferredObservation]) {
    for observation in observations {
        if observation.kind == InferredObservationKind::SessionMetadataClassified
            && tool_inventory_is_estimated(
                observation.facts.supplied_tool_count,
                observation.facts.supplied_tools.len(),
            ) == Some(false)
        {
            observation
                .limitations
                .retain(|code| *code != LimitationCode::ToolInventoryPotentialOnly);
        }
    }
}

#[cfg(test)]
mod tests;
pub fn scope_file_identifiers(
    observations: &mut [InferredObservation],
    ownership_scope: Option<&str>,
) {
    for observation in observations {
        if let Some(ownership_scope) = ownership_scope {
            for interaction in &mut observation.facts.file_interactions {
                interaction.opaque_file_id = hash_identifier(
                    &json!({
                        "ownership_scope": ownership_scope,
                        "file_identifier": interaction.opaque_file_id,
                    })
                    .to_string(),
                );
            }
        } else {
            observation.facts.file_interactions.clear();
        }
    }
}
pub fn truncate_nested_facts(observations: &mut [InferredObservation]) {
    for observation in observations {
        observation.facts.supplied_tools.clear();
        observation.facts.supplied_skills.clear();
        observation.facts.file_interactions.clear();
        if !observation
            .limitations
            .contains(&LimitationCode::PayloadTruncated)
        {
            observation
                .limitations
                .push(LimitationCode::PayloadTruncated);
        }
    }
}

pub fn assign_observation_ids(agent_session_id: Uuid, observations: &mut [InferredObservation]) {
    for (index, observation) in observations.iter_mut().enumerate() {
        observation.observation_id = stable_uuid(
            OBSERVATION_ID_NAMESPACE,
            &json!({
                "session": agent_session_id,
                "request": observation.source_request_id,
                "parser": OBSERVATION_PARSER_VERSION,
                "index": index,
                "kind": observation.kind,
                "facts": observation.facts,
            })
            .to_string(),
        );
    }
}
