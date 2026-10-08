use super::*;
use gateway_core::RoutingStrategy;

fn routing_model_config(routing_yaml: &str) -> String {
    format!(
        r#"
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: pooled
    {routing_yaml}
    routes:
      - id: openai-primary
        provider: openai
        upstream_model: gpt-5
  - id: plain
    routes:
      - provider: openai
        upstream_model: gpt-5
"#
    )
}

#[test]
fn seeds_each_routing_strategy_and_optional_affinity() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    for (strategy, expected) in [
        ("preferred", RoutingStrategy::Preferred),
        ("weighted_random", RoutingStrategy::WeightedRandom),
        ("round_robin", RoutingStrategy::RoundRobin),
    ] {
        write_config(
            &config_path,
            &routing_model_config(&format!(
                "routing:\n      strategy: {strategy}\n      affinity:\n        idle_timeout_seconds: 7200"
            )),
        );

        let config = GatewayConfig::from_path(&config_path).expect("config should parse");
        let seeds = config.seed_models().expect("seed models");
        let routing = seeds[0].routing.as_ref().expect("routing policy");
        assert_eq!(routing.strategy, expected);
        assert_eq!(
            routing
                .affinity
                .as_ref()
                .expect("affinity policy")
                .idle_timeout_seconds,
            7200
        );
        assert!(
            seeds[1].routing.is_none(),
            "omitted policy must remain unset"
        );
        assert_eq!(
            seeds[0].routes[0].route_key.as_deref(),
            Some("openai-primary")
        );
        assert!(seeds[1].routes[0].route_key.is_none());
    }
}

#[test]
fn routing_defaults_preserve_weighted_selection_and_use_one_hour_affinity() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    for (routing_yaml, expected_timeout) in [
        ("routing: {}", None),
        ("routing:\n      affinity: {}", Some(3600)),
    ] {
        write_config(&config_path, &routing_model_config(routing_yaml));

        let config = GatewayConfig::from_path(&config_path).expect("config should parse");
        let seeds = config.seed_models().expect("seed models");
        let routing = seeds[0].routing.as_ref().expect("routing policy");
        assert_eq!(routing.strategy, RoutingStrategy::WeightedRandom);
        assert!(routing.failover.is_none());
        assert_eq!(
            routing
                .affinity
                .as_ref()
                .map(|affinity| affinity.idle_timeout_seconds),
            expected_timeout
        );
    }
}

#[test]
fn failover_is_opt_in_and_seeded_with_validated_limits() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    write_config(&path, &routing_model_config("routing:\n      failover: {}"));
    let config = GatewayConfig::from_path(&path).unwrap();
    let models = config.seed_models().unwrap();
    assert_eq!(
        models[0].routing.as_ref().unwrap().failover,
        Some(gateway_core::ProviderFailoverPolicy::default())
    );
    for policy in [
        "max_attempts: 0",
        "max_retries_per_route: 6",
        "retry_limit: 2",
    ] {
        write_config(
            &path,
            &routing_model_config(&format!("routing:\n      failover:\n        {policy}")),
        );
        assert!(GatewayConfig::from_path(&path).is_err(), "{policy}");
    }
}

#[test]
fn rejects_invalid_routing_policy() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    for (routing_yaml, expected_error) in [
        (
            "routing:\n      strategy: unknown",
            "unknown variant `unknown`",
        ),
        (
            "routing:\n      preference: openai",
            "unknown field `preference`",
        ),
        (
            "routing:\n      affinity:\n        timeout: 3600",
            "unknown field `timeout`",
        ),
        (
            "routing:\n      affinity:\n        idle_timeout_seconds: 0",
            "model `pooled` routing.affinity.idle_timeout_seconds must be positive",
        ),
    ] {
        write_config(&config_path, &routing_model_config(routing_yaml));

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains(expected_error),
            "unexpected error for `{routing_yaml}`: {error_text}"
        );
    }
}

#[test]
fn rejects_alias_with_its_own_routing_policy() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    let mut yaml = routing_model_config("routing:\n      strategy: round_robin");
    yaml.push_str("  - id: alias\n    alias_of: pooled\n    routing:\n      strategy: preferred\n");
    write_config(&config_path, &yaml);

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("model `alias` cannot define routing with alias_of"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn routing_requires_explicit_unique_route_ids() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    let yaml = routing_model_config("routing: {}");
    for (yaml, expected_error) in [
        (
            yaml.replace("      - id: openai-primary\n        provider:", "      - provider:"),
            "requires an explicit id for every route when routing is configured",
        ),
        (
            yaml.replace(
                "  - id: plain",
                "      - id: openai-primary\n        provider: openai\n        upstream_model: gpt-5\n  - id: plain",
            ),
            "defines duplicate route id `openai-primary`",
        ),
    ] {
        write_config(&config_path, &yaml);
        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains(expected_error),
            "unexpected error: {error_text}"
        );
    }
}

#[test]
fn rejects_invalid_explicit_route_ids() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    for id in [
        "",
        "with space",
        "with/slash",
        "with:colon",
        "日本語",
        &"a".repeat(129),
    ] {
        let yaml = routing_model_config("routing: {}")
            .replace("id: openai-primary", &format!("id: '{id}'"));
        write_config(&config_path, &yaml);
        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("route id must contain 1 to 128 ASCII letters"),
            "unexpected error for `{id}`: {error_text}"
        );
    }
}

#[test]
fn accepts_alias_backed_model_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: fast-v2
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
  - id: fast
    alias_of: fast-v2
"#,
    );

    GatewayConfig::from_path(&config_path).expect("config should parse");
}

#[test]
fn parses_model_allowlist_normalizes_refs_and_preserves_omitted_policy() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: unrestricted
    routes:
      - provider: openai-prod
        upstream_model: gpt-4o-mini
  - id: restricted
    allowlist:
      users:
        - " Alice@Example.COM "
        - "alice@example.com"
        - "Zoe@Example.com"
      teams:
        - " platform "
        - research
        - platform
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let models = config.seed_models().expect("seed models");
    let unrestricted = models
        .iter()
        .find(|model| model.model_key == "unrestricted")
        .expect("unrestricted model");
    let restricted = models
        .iter()
        .find(|model| model.model_key == "restricted")
        .expect("restricted model");

    assert_eq!(unrestricted.allowlist, None);
    let allowlist = restricted.allowlist.as_ref().expect("restricted allowlist");
    assert_eq!(
        allowlist.users,
        vec![
            "alice@example.com".to_string(),
            "zoe@example.com".to_string()
        ]
    );
    assert_eq!(
        allowlist.teams,
        vec!["platform".to_string(), "research".to_string()]
    );
}

#[test]
fn rejects_unknown_model_allowlist_keys() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: restricted
    allowlist:
      users:
        - alice@example.com
      team:
        - platform
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("unknown field `team`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_explicit_empty_model_allowlists() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    for allowlist_yaml in [
        "allowlist: {}",
        "allowlist:\n      users: []\n      teams: []",
    ] {
        write_config(
            &config_path,
            &format!(
                r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: restricted
    {allowlist_yaml}
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
"#
            ),
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text
                .contains("model `restricted` allowlist must include at least one user or team"),
            "unexpected error: {error_text}"
        );
    }
}

#[test]
fn rejects_model_with_alias_and_routes() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: fast
    alias_of: fast-v2
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
  - id: fast-v2
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("cannot define both alias_of and routes"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_model_without_alias_or_routes() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
models:
  - id: fast
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("must define either alias_of or at least one route"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_alias_to_unknown_model() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
models:
  - id: fast
    alias_of: missing
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("aliases unknown model `missing`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_self_alias() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
models:
  - id: fast
    alias_of: fast
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("cannot alias itself"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_alias_cycles() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
models:
  - id: fast
    alias_of: fast-v2
  - id: fast-v2
    alias_of: fast
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("model alias cycle detected"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn loads_model_reasoning_effort_policy_into_seed_models() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: reasoning
    max_reasoning_effort: medium
    routes:
      - provider: openai
        upstream_model: gpt-5
        extra_body:
          reasoning_effort: low
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should load");
    assert_eq!(
        config.models[0].max_reasoning_effort,
        Some(ReasoningEffort::Medium)
    );
    assert_eq!(
        config.seed_models().expect("seed models")[0].max_reasoning_effort,
        Some(ReasoningEffort::Medium)
    );
}

#[test]
fn rejects_route_extra_body_above_model_reasoning_effort_policy() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: reasoning
    max_reasoning_effort: low
    routes:
      - provider: openai
        upstream_model: gpt-5
        extra_body:
          reasoning:
            effort: high
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("extra_body violates max_reasoning_effort"),
        "unexpected error: {error_text}"
    );
    assert!(
        error_text.contains("reasoning effort `high` exceeds the model maximum `low`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_target_route_above_alias_reasoning_effort_policy() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: reasoning-safe
    alias_of: reasoning
    max_reasoning_effort: medium
  - id: reasoning
    max_reasoning_effort: high
    routes:
      - provider: openai
        upstream_model: gpt-5
        extra_body:
          reasoning_effort: high
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
            error_text.contains(
                "model `reasoning-safe` effective route `gpt-5` extra_body violates max_reasoning_effort"
            ),
            "unexpected error: {error_text}"
        );
    assert!(
        error_text.contains("reasoning effort `high` exceeds the model maximum `medium`"),
        "unexpected error: {error_text}"
    );
}

fn benchmark_model_config(benchmark_model_id: &str) -> String {
    format!(
        r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
models:
  - id: fast
    benchmark_model_id: "{benchmark_model_id}"
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
  - id: plain
    routes:
      - provider: openai-prod
        upstream_model: gpt-5
"#
    )
}

#[test]
fn collects_explicit_benchmark_model_ids() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(&config_path, &benchmark_model_config("openai/gpt-5"));

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");

    assert_eq!(
        config.benchmark_model_ids(),
        [("fast".to_string(), "openai/gpt-5".to_string())].into()
    );
}

#[test]
fn rejects_invalid_benchmark_model_ids() {
    for invalid in [
        "gpt-5",
        "openai/gpt-5:batch",
        " openai/gpt-5",
        "/gpt-5",
        "openai/",
    ] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(&config_path, &benchmark_model_config(invalid));

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("benchmark_model_id must be an OpenRouter model ID"),
            "unexpected error for `{invalid}`: {error_text}"
        );
    }
}
