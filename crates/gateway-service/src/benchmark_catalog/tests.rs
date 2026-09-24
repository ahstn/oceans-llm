use super::*;

fn at(rfc3339: &str) -> OffsetDateTime {
    OffsetDateTime::parse(rfc3339, &time::format_description::well_known::Rfc3339)
        .expect("valid timestamp")
}

fn indices(
    intelligence: Option<f64>,
    coding: Option<f64>,
    agentic: Option<f64>,
) -> ArtificialAnalysisIndices {
    ArtificialAnalysisIndices {
        intelligence_index: intelligence,
        coding_index: coding,
        agentic_index: agentic,
    }
}

fn fetched(id: &str, indices: ArtificialAnalysisIndices) -> OpenRouterBenchmarkModel {
    OpenRouterBenchmarkModel {
        id: id.to_string(),
        name: format!("Name {id}"),
        canonical_slug: format!("{id}-20260101"),
        indices,
    }
}

#[test]
fn vendored_snapshot_parses_with_attribution() {
    let snapshot: BenchmarkSnapshot =
        serde_json::from_str(VENDORED_BENCHMARKS_JSON).expect("vendored benchmarks parse");

    assert_eq!(snapshot.metadata.attribution, BENCHMARK_ATTRIBUTION);
    assert_eq!(snapshot.metadata.source_url, DEFAULT_BENCHMARK_SOURCE_URL);
    assert!(!snapshot.models.is_empty());
    for (id, entry) in &snapshot.models {
        assert!(!id.contains(VARIANT_SEPARATOR), "{id} is a variant");
        assert!(!entry.artificial_analysis.is_empty(), "{id} has no indices");
        entry
            .artificial_analysis
            .validate(id)
            .expect("vendored indices are valid");
    }
}

#[test]
fn parse_skips_variants_and_models_without_indices() {
    let body = r#"{
        "data": [
            {
                "id": "openai/gpt-6",
                "name": "OpenAI: GPT-6",
                "canonical_slug": "openai/gpt-6-20260801",
                "benchmarks": {
                    "artificial_analysis": {
                        "intelligence_index": 61.2,
                        "coding_index": 70.1,
                        "agentic_index": null
                    }
                }
            },
            {
                "id": "openai/gpt-6:batch",
                "name": "OpenAI: GPT-6 (batch)",
                "canonical_slug": "openai/gpt-6-20260801",
                "benchmarks": {
                    "artificial_analysis": {
                        "intelligence_index": 61.2,
                        "coding_index": 70.1,
                        "agentic_index": null
                    }
                }
            },
            {
                "id": "openai/gpt-6:free",
                "name": "OpenAI: GPT-6 (free)",
                "canonical_slug": "openai/gpt-6-20260801",
                "benchmarks": {
                    "artificial_analysis": {"intelligence_index": 61.2}
                }
            },
            {
                "id": "acme/no-benchmarks",
                "name": "Acme",
                "canonical_slug": "acme/no-benchmarks"
            },
            {
                "id": "acme/null-benchmarks",
                "name": "Acme",
                "canonical_slug": "acme/null-benchmarks",
                "benchmarks": {
                    "artificial_analysis": {
                        "intelligence_index": null,
                        "coding_index": null,
                        "agentic_index": null
                    }
                }
            }
        ]
    }"#;

    let models = parse_openrouter_models(body).expect("parse");

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "openai/gpt-6");
    assert_eq!(models[0].indices.intelligence_index, Some(61.2));
}

#[test]
fn parse_rejects_out_of_range_scores_and_empty_lists() {
    let out_of_range = r#"{"data": [{
        "id": "acme/model",
        "name": "Acme",
        "canonical_slug": "acme/model",
        "benchmarks": {"artificial_analysis": {"intelligence_index": 101.0}}
    }]}"#;

    assert!(parse_openrouter_models(out_of_range).is_err());
    assert!(parse_openrouter_models(r#"{"data": []}"#).is_err());
}

#[test]
fn merge_upserts_without_deleting_or_dropping_known_values() {
    let first = at("2026-09-01T00:00:00Z");
    let second = at("2026-09-24T00:00:00Z");
    let mut snapshot = empty_benchmark_snapshot(DEFAULT_BENCHMARK_SOURCE_URL, first);

    assert!(merge_benchmark_models(
        &mut snapshot,
        vec![
            fetched("acme/kept", indices(Some(40.0), Some(50.0), Some(30.0))),
            fetched("acme/updated", indices(Some(20.0), None, None)),
        ],
        DEFAULT_BENCHMARK_SOURCE_URL,
        first,
    ));

    assert!(merge_benchmark_models(
        &mut snapshot,
        vec![
            fetched("acme/updated", indices(None, Some(25.0), None)),
            fetched("acme/new", indices(Some(10.0), None, None)),
        ],
        DEFAULT_BENCHMARK_SOURCE_URL,
        second,
    ));

    assert_eq!(snapshot.models.len(), 3);
    let kept = &snapshot.models["acme/kept"];
    assert_eq!(kept.artificial_analysis.intelligence_index, Some(40.0));
    assert_eq!(kept.updated_at, first);

    let updated = &snapshot.models["acme/updated"];
    assert_eq!(updated.artificial_analysis.intelligence_index, Some(20.0));
    assert_eq!(updated.artificial_analysis.coding_index, Some(25.0));
    assert_eq!(updated.updated_at, second);

    assert_eq!(snapshot.models["acme/new"].updated_at, second);
    assert_eq!(snapshot.metadata.updated_at, second);
    assert_eq!(snapshot.metadata.attribution, BENCHMARK_ATTRIBUTION);
}

#[test]
fn merge_is_a_no_op_when_nothing_changed() {
    let first = at("2026-09-01T00:00:00Z");
    let later = at("2026-09-24T00:00:00Z");
    let mut snapshot = empty_benchmark_snapshot(DEFAULT_BENCHMARK_SOURCE_URL, first);
    let model = || fetched("acme/model", indices(Some(40.0), None, None));
    merge_benchmark_models(
        &mut snapshot,
        vec![model()],
        DEFAULT_BENCHMARK_SOURCE_URL,
        first,
    );

    assert!(!merge_benchmark_models(
        &mut snapshot,
        vec![model()],
        DEFAULT_BENCHMARK_SOURCE_URL,
        later,
    ));
    assert_eq!(snapshot.models["acme/model"].updated_at, first);
    assert_eq!(snapshot.metadata.updated_at, first);
}

#[test]
fn candidates_normalize_provider_model_ids() {
    let cases = [
        (
            "us.anthropic.claude-sonnet-4-6-v1:0",
            "anthropic/claude-sonnet-4.6",
        ),
        (
            "arn:aws:bedrock:us-east-1:123456789012:inference-profile/us.anthropic.claude-sonnet-4-6-v1:0",
            "anthropic/claude-sonnet-4.6",
        ),
        ("openai.gpt-oss-120b", "openai/gpt-oss-120b"),
        ("gpt-6-astra", "openai/gpt-6-astra"),
        ("claude-sonnet-4-6@20260101", "anthropic/claude-sonnet-4.6"),
        ("anthropic/claude-fable-5-1", "anthropic/claude-fable-5.1"),
        ("qwen/qwen3.6-27b", "qwen/qwen3.6-27b"),
        ("gpt-5.6-luna", "openai/gpt-5.6-luna"),
        ("amazon/nova-2-lite-v1", "amazon/nova-2-lite-v1"),
    ];

    for (upstream, expected) in cases {
        let candidates = benchmark_model_id_candidates(upstream);
        assert!(
            candidates.iter().any(|candidate| candidate == expected),
            "{upstream} produced {candidates:?}, expected {expected}"
        );
    }
}

#[test]
fn derived_lookup_only_matches_exact_ids() {
    let now = at("2026-09-24T00:00:00Z");
    let mut snapshot = empty_benchmark_snapshot(DEFAULT_BENCHMARK_SOURCE_URL, now);
    merge_benchmark_models(
        &mut snapshot,
        vec![fetched(
            "deepseek/deepseek-v4-pro",
            indices(Some(30.4), None, None),
        )],
        DEFAULT_BENCHMARK_SOURCE_URL,
        now,
    );

    assert_eq!(
        derive_from_snapshot(&snapshot, "deepseek/deepseek-v4-pro"),
        Some("deepseek/deepseek-v4-pro".to_string())
    );
    assert_eq!(
        derive_from_snapshot(&snapshot, "deepseek/deepseek-v4-pro-0813"),
        None
    );
}

#[test]
fn scores_include_only_present_indices_with_openrouter_source() {
    let now = at("2026-09-24T00:00:00Z");
    let mut snapshot = empty_benchmark_snapshot(DEFAULT_BENCHMARK_SOURCE_URL, now);
    merge_benchmark_models(
        &mut snapshot,
        vec![fetched(
            "anthropic/claude-opus-4.7",
            indices(None, Some(60.0), Some(55.0)),
        )],
        DEFAULT_BENCHMARK_SOURCE_URL,
        now,
    );

    let scores = scores_from_snapshot(
        &snapshot,
        "anthropic/claude-opus-4.7",
        BenchmarkMatchKind::Explicit,
    );

    assert_eq!(
        scores
            .iter()
            .map(|score| score.metric_key)
            .collect::<Vec<_>>(),
        [
            "artificial_analysis_coding_index",
            "artificial_analysis_agentic_index"
        ]
    );
    assert_eq!(
        scores[0].source_url,
        "https://openrouter.ai/anthropic/claude-opus-4.7"
    );
    assert_eq!(scores[0].match_kind, BenchmarkMatchKind::Explicit);
    assert!(
        scores_from_snapshot(&snapshot, "acme/missing", BenchmarkMatchKind::Derived).is_empty()
    );
}
