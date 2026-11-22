use super::*;
use crate::ScoreMaturity;

fn versions() -> AnalysisVersions<'static> {
    AnalysisVersions {
        report_schema_version: "schema",
        boundary_policy_version: "boundary",
        observation_parser_version: "parser",
        report_observation_parser_version: "parser",
        analyzer_version: "analyzer",
        score_policy_version: "score",
        pricing_policy_version: "pricing",
        cohort_version: COHORT_VERSION,
        configuration_version: "configuration",
        score_maturity: ScoreMaturity::Experimental,
        calibration_approval_id: None,
    }
}

fn current() -> CohortSession<'static> {
    CohortSession {
        session_id: Uuid::nil(),
        requested_model_key: "model",
        operation: "chat",
        caller_class: "human",
        boundary_group_key: "group",
    }
}

fn candidate(id: u128) -> CohortCandidate<'static> {
    CohortCandidate {
        session: CohortSession {
            session_id: Uuid::from_u128(id),
            ..current()
        },
        analysis_id: Uuid::from_u128(id),
        versions: versions(),
        gateway_outcome: GatewayOutcomeState::Succeeded,
        actual_cost_10000: Some(i64::try_from(id).unwrap() * 10),
        active_time_ms: i64::try_from(id).unwrap() * 100,
    }
}

#[test]
fn snapshot_preserves_uuid_sorted_json_tuple_bytes() {
    let mut selection = CohortSelection::new(current(), versions());
    for id in (1..=6).rev() {
        selection.consider(candidate(id));
    }
    let selected = selection.finish().unwrap().unwrap();
    let snapshot = concat!(
        "[[\"00000000-0000-0000-0000-000000000001\",10,100],",
        "[\"00000000-0000-0000-0000-000000000002\",20,200],",
        "[\"00000000-0000-0000-0000-000000000003\",30,300],",
        "[\"00000000-0000-0000-0000-000000000004\",40,400],",
        "[\"00000000-0000-0000-0000-000000000005\",50,500],",
        "[\"00000000-0000-0000-0000-000000000006\",60,600]]",
    );
    assert_eq!(
        selected.snapshot_digest,
        crate::identity::hashing::hash_identifier(snapshot)
    );
    assert_eq!(
        selected.reference.cohort_version,
        "successful-boundary-group-v2"
    );
    assert_eq!(selected.reference.fallback_level, 0);
    assert_eq!(
        selected.reference.successful_costs_10000,
        [10, 20, 30, 40, 50, 60]
    );
    assert_eq!(
        selected.reference.successful_active_time_ms,
        [100, 200, 300, 400, 500, 600]
    );
}

#[test]
fn chooses_the_narrowest_level_with_at_least_six_samples() {
    for expected_level in 0..=3 {
        let mut selection = CohortSelection::new(current(), versions());
        for id in 1..=6 {
            let mut value = candidate(id);
            match expected_level {
                0 => {}
                1 => value.session.boundary_group_key = "other-group",
                2 => value.session.caller_class = "other-caller",
                _ => value.session.requested_model_key = "other-model",
            }
            selection.consider(value);
        }
        let selected = selection.finish().unwrap().unwrap();
        assert_eq!(selected.reference.fallback_level, expected_level);
        assert_eq!(selected.reference.successful_costs_10000.len(), 6);
    }
    let mut selection = CohortSelection::new(current(), versions());
    for id in 1..=5 {
        selection.consider(candidate(id));
    }
    let mut broad = candidate(6);
    broad.session.boundary_group_key = "other-group";
    selection.consider(broad);
    assert_eq!(
        selection
            .finish()
            .unwrap()
            .unwrap()
            .reference
            .fallback_level,
        1
    );
}

#[test]
fn insufficient_samples_do_not_produce_a_cohort() {
    let mut selection = CohortSelection::new(current(), versions());
    for id in 1..=5 {
        selection.consider(candidate(id));
    }
    assert!(selection.finish().unwrap().is_none());
}

#[test]
fn filters_self_unsuccessful_unpriced_and_incompatible_candidates() {
    let mut selection = CohortSelection::new(current(), versions());
    for id in 1..=6 {
        for invalid in 0..6 {
            let mut value = candidate(id);
            match invalid {
                0 => value.session.session_id = current().session_id,
                1 => value.gateway_outcome = GatewayOutcomeState::Partial,
                2 => value.actual_cost_10000 = None,
                3 => value.versions.observation_parser_version = "old-parser",
                4 => value.versions.configuration_version = "old-config",
                _ => value.versions.pricing_policy_version = "old-pricing",
            }
            selection.consider(value);
        }
    }
    assert!(selection.finish().unwrap().is_none());
}

#[test]
fn historical_cohort_and_calibration_metadata_do_not_change_eligibility() {
    let mut selection = CohortSelection::new(current(), versions());
    for id in 1..=6 {
        let mut value = candidate(id);
        value.versions.cohort_version = "historical-cohort";
        value.versions.report_observation_parser_version = "historical-report-parser";
        value.versions.score_maturity = ScoreMaturity::Calibrated;
        value.versions.calibration_approval_id = Some("historical-approval");
        selection.consider(value);
    }
    assert_eq!(
        selection
            .finish()
            .unwrap()
            .unwrap()
            .reference
            .fallback_level,
        0
    );
}

#[test]
fn sample_caps_apply_in_scan_order_before_snapshot_sorting() {
    let mut selection = CohortSelection::new(current(), versions());
    for id in 1..=u128::try_from(MAX_COHORT_SAMPLES_PER_LEVEL + 1).unwrap() {
        let mut value = candidate(id);
        value.analysis_id = Uuid::from_u128(3_000 - id);
        selection.consider(value);
    }
    let selected = selection.finish().unwrap().unwrap();
    assert_eq!(selected.reference.successful_costs_10000.len(), 2_000);
    assert_eq!(
        selected.reference.successful_costs_10000.first(),
        Some(&20_000)
    );
    assert_eq!(selected.reference.successful_costs_10000.last(), Some(&10));
    assert!(!selected.reference.successful_costs_10000.contains(&20_010));
}

#[test]
fn broad_level_saturation_does_not_stop_later_exact_sampling() {
    let mut selection = CohortSelection::new(current(), versions());
    for id in 1..=2_000 {
        let mut value = candidate(id);
        value.session.requested_model_key = "other-model";
        selection.consider(value);
    }
    for id in 2_001..=2_006 {
        selection.consider(candidate(id));
    }
    let selected = selection.finish().unwrap().unwrap();
    assert_eq!(selected.reference.fallback_level, 0);
    assert_eq!(selected.reference.successful_costs_10000.len(), 6);
}
