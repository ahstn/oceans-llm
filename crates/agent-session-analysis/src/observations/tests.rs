use super::*;
use crate::{
    BoundedFileInteractionFact, BoundedObservationFacts, BoundedToolDefinitionFact,
    EvidenceQuality, FileOperation,
};

fn metadata_observation(supplied_tool_count: u32, retained_tools: usize) -> InferredObservation {
    InferredObservation {
        observation_id: Uuid::nil(),
        kind: InferredObservationKind::SessionMetadataClassified,
        source_request_id: "request".to_string(),
        parser_version: "passive-observations-v3".to_string(),
        evidence: EvidenceQuality::Direct,
        occurred_at: OffsetDateTime::UNIX_EPOCH,
        facts: BoundedObservationFacts {
            supplied_tool_count: Some(supplied_tool_count),
            supplied_tools: (0..retained_tools)
                .map(|index| BoundedToolDefinitionFact {
                    name: format!("tool-{index}"),
                    server_key: None,
                    token_estimate: 1,
                })
                .collect(),
            ..BoundedObservationFacts::default()
        },
        limitations: vec![LimitationCode::ToolInventoryPotentialOnly],
    }
}

#[test]
fn legacy_tool_inventory_notice_is_kept_only_for_incomplete_inventory() {
    let complete = metadata_observation(1, 1);
    let incomplete = metadata_observation(2, 1);
    let mut unknown = metadata_observation(0, 0);
    unknown.facts.supplied_tool_count = None;
    let mut observations = vec![complete, incomplete, unknown];
    normalize_legacy_tool_inventory_limitations(&mut observations);
    assert!(observations[0].limitations.is_empty());
    assert_eq!(
        observations[1].limitations,
        vec![LimitationCode::ToolInventoryPotentialOnly]
    );
    assert_eq!(
        observations[2].limitations,
        vec![LimitationCode::ToolInventoryPotentialOnly]
    );
}
#[test]
fn file_identifiers_are_owner_scoped_before_persistence() {
    let observation = || InferredObservation {
        observation_id: Uuid::nil(),
        kind: InferredObservationKind::SessionMetadataClassified,
        source_request_id: "request".to_string(),
        parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        evidence: EvidenceQuality::Direct,
        occurred_at: OffsetDateTime::UNIX_EPOCH,
        facts: BoundedObservationFacts {
            file_interactions: vec![BoundedFileInteractionFact {
                opaque_file_id: "/Users/alice/private/source.rs".to_string(),
                operation: FileOperation::Read,
                tool_name: None,
                succeeded: None,
                error_signature: None,
            }],
            ..BoundedObservationFacts::default()
        },
        limitations: Vec::new(),
    };
    let mut first = vec![observation()];
    scope_file_identifiers(&mut first, Some("user:a"));
    let first_id = &first[0].facts.file_interactions[0].opaque_file_id;
    assert_eq!(
        first_id,
        "sha256:ac01d28a00330b51f5b9591338409c7d9018a530f556558d41d474865438fb5a"
    );
    assert!(!first_id.contains("/Users/alice"));

    let mut second = vec![observation()];
    scope_file_identifiers(&mut second, Some("user:b"));
    assert_ne!(
        first_id,
        &second[0].facts.file_interactions[0].opaque_file_id
    );
    let mut unscoped = vec![observation()];
    scope_file_identifiers(&mut unscoped, None);
    assert!(unscoped[0].facts.file_interactions.is_empty());
}

#[test]
fn persisted_observation_ids_keep_the_existing_canonical_bytes() {
    let session_id = Uuid::from_u128(0x00112233_4455_6677_8899_aabbccddeeff);
    assert_eq!(
        observation_set_id(session_id, "req-1", OffsetDateTime::UNIX_EPOCH),
        Uuid::from_u128(0x255c35c5_b515_5b89_ba01_78d493f6522d),
    );
    let mut observations = vec![InferredObservation {
        observation_id: Uuid::nil(),
        kind: InferredObservationKind::FileReadSuspected,
        source_request_id: "req-1".to_string(),
        parser_version: OBSERVATION_PARSER_VERSION.to_string(),
        evidence: EvidenceQuality::InferredHigh,
        occurred_at: OffsetDateTime::UNIX_EPOCH,
        facts: BoundedObservationFacts {
            tool_name: Some("read_file".to_string()),
            ..Default::default()
        },
        limitations: vec![LimitationCode::SemanticVerificationUnavailable],
    }];
    assign_observation_ids(session_id, &mut observations);
    assert_eq!(
        observations[0].observation_id,
        Uuid::from_u128(0xe697af17_2d76_58e2_90f3_f74c25dd8925)
    );
}
