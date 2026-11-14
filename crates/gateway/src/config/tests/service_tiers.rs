use gateway_core::ServiceTier;

use super::*;

const PROVIDERS: &str = r#"
providers:
  - id: openai-prod
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
  - id: claude-api
    type: anthropic_compat
    base_url: https://api.anthropic.com
    pricing_provider_id: opencode
    auth:
      kind: x_api_key
      token: literal.test-anthropic-key
  - id: vertex-us
    type: gcp_vertex
    project_id: test-proj
    location: us
    auth:
      mode: bearer
      token: literal.test-vertex-token
  - id: vertex-global
    type: gcp_vertex
    project_id: test-proj
    location: global
    auth:
      mode: bearer
      token: literal.test-vertex-token
  - id: openrouter
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openrouter
"#;

fn load(models: &str) -> anyhow::Result<GatewayConfig> {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(&config_path, &format!("{PROVIDERS}models:\n{models}"));
    GatewayConfig::from_path(&config_path)
}

fn load_error(models: &str) -> String {
    format!("{:#}", load(models).expect_err("config should fail"))
}

/// One model with one route to `upstream_model` on `provider` that forces `tier`. `extra` adds
/// route settings, indented as route fields.
fn forced_tier_route(provider: &str, upstream_model: &str, tier: &str, extra: &str) -> String {
    format!(
        "  - id: m\n    routes:\n      - provider: {provider}\n        upstream_model: {upstream_model}\n        service_tier: {tier}\n{extra}"
    )
}

const CHAT_ONLY: &str = "        capabilities:\n          responses: false\n";

#[test]
fn rejects_a_forced_tier_the_provider_cannot_send() {
    for (provider, upstream_model, tier, extra, expected) in [
        (
            "claude-api",
            "claude-opus-5-5",
            "flex",
            "",
            "model `m` route `claude-opus-5-5` for provider `claude-api` forces service_tier `flex`, but anthropic_compat cannot send it for this model",
        ),
        (
            "claude-api",
            "claude-sonnet-5-5",
            "fast",
            "",
            "forces service_tier `priority`, but anthropic_compat cannot send it for this model",
        ),
        (
            "openai-prod",
            "gpt-6-luna",
            "ultrafast",
            CHAT_ONLY,
            "route `gpt-6-luna` for provider `openai-prod` forces service_tier `ultrafast`, but the route cannot send it on chat_completions",
        ),
        (
            "vertex-us",
            "google/gemini-3-pro",
            "flex",
            "",
            "for provider `vertex-us` forces service_tier `flex`, but gcp_vertex location `us` cannot send it for this model",
        ),
        (
            "vertex-global",
            "anthropic/claude-opus-5-5",
            "priority",
            "",
            "gcp_vertex location `global` cannot send it for this model",
        ),
        (
            "openai-prod",
            "gpt-6-luna",
            "priority",
            "        capabilities:\n          chat_completions: false\n          responses: false\n",
            "the route serves neither chat_completions nor responses",
        ),
    ] {
        let error = load_error(&forced_tier_route(provider, upstream_model, tier, extra));
        assert!(
            error.contains(expected),
            "unexpected error for `{tier}` on `{provider}`: {error}"
        );
    }
}

#[test]
fn accepts_a_forced_tier_some_served_api_can_send() {
    for (provider, upstream_model, tier, extra) in [
        ("claude-api", "claude-opus-5-5", "priority", ""),
        ("openai-prod", "gpt-6-luna", "ultrafast", ""),
        ("openai-prod", "gpt-6-luna", "flex", CHAT_ONLY),
        ("vertex-us", "google/gemini-3-pro", "priority", ""),
        ("vertex-global", "google/gemini-3-pro", "flex", ""),
        (
            "vertex-global",
            "anthropic/claude-fable-5-2",
            "priority",
            "",
        ),
        ("openrouter", "openai/gpt-6-luna", "ultrafast", CHAT_ONLY),
        ("claude-api", "claude-sonnet-5-5", "standard", ""),
    ] {
        load(&forced_tier_route(provider, upstream_model, tier, extra)).unwrap_or_else(|error| {
            panic!("`{tier}` on `{provider}` route `{upstream_model}` should load: {error:#}")
        });
    }
}

#[test]
fn seeds_route_and_model_service_tiers() {
    let config = load(
        r#"
  - id: gpt-fast
    service_tiers: [flex, fast]
    routes:
      - provider: openai-prod
        upstream_model: gpt-6-luna
        service_tier: fast
        pricing_override:
          input_usd_per_million_tokens: "2.5000"
          output_usd_per_million_tokens: "20.0000"
          service_tiers:
            - service_tier: flex
              input_usd_per_million_tokens: "0.6250"
              output_usd_per_million_tokens: "5.0000"
              cache_read_usd_per_million_tokens: "0.0625"
            - service_tier: standard
              input_usd_per_million_tokens: "1.2500"
              output_usd_per_million_tokens: "10.0000"
"#,
    )
    .expect("config should parse");

    let models = config.seed_models().expect("seed models");
    let route = &models[0].routes[0];
    let pricing = route.pricing_override.as_ref().expect("pricing override");
    assert_eq!(
        models[0].service_tiers,
        Some(vec![ServiceTier::Flex, ServiceTier::Priority])
    );
    assert_eq!(route.service_tier, Some(ServiceTier::Priority));
    let tiers: Vec<_> = pricing
        .service_tiers
        .iter()
        .map(|rates| {
            (
                rates.service_tier,
                rates.input_cost_per_million_tokens,
                rates.cache_read_cost_per_million_tokens,
            )
        })
        .collect();
    assert_eq!(
        tiers,
        [
            (ServiceTier::Default, Money4::from_scaled(12_500), None),
            (
                ServiceTier::Flex,
                Money4::from_scaled(6_250),
                Some(Money4::from_scaled(625))
            ),
        ]
    );
}

#[test]
fn rejects_unknown_service_tier_names() {
    for models in [
        "  - id: m\n    routes:\n      - provider: openai-prod\n        upstream_model: u\n        service_tier: turbo\n",
        "  - id: m\n    service_tiers: [turbo]\n    routes:\n      - provider: openai-prod\n        upstream_model: u\n",
    ] {
        let error = load_error(models);
        assert!(
            error.contains("unknown service tier `turbo`"),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn rejects_default_or_repeated_model_service_tiers() {
    for (tiers, expected) in [
        ("[standard]", "service_tiers cannot list `default`"),
        ("[flex, priority, fast]", "lists `priority` more than once"),
    ] {
        let error = load_error(&format!(
            "  - id: m\n    service_tiers: {tiers}\n    routes:\n      - provider: openai-prod\n        upstream_model: u\n"
        ));
        assert!(error.contains(expected), "unexpected error: {error}");
    }
}

#[test]
fn rejects_tier_selection_through_route_passthrough() {
    for (passthrough, field) in [
        ("extra_body:\n          service_tier: flex", "service_tier"),
        ("extra_body:\n          serviceTier: flex", "serviceTier"),
        ("extra_body:\n          speed: fast", "speed"),
        (
            "extra_headers:\n          x-amzn-bedrock-service-tier: priority",
            "x-amzn-bedrock-service-tier",
        ),
        (
            "extra_headers:\n          X-Vertex-AI-LLM-Shared-Request-Type: priority",
            "X-Vertex-AI-LLM-Shared-Request-Type",
        ),
    ] {
        let error = load_error(&format!(
            "  - id: m\n    routes:\n      - provider: openai-prod\n        upstream_model: u\n        {passthrough}\n"
        ));
        assert!(
            error.contains(&format!(
                "cannot set `{field}`; set the route's `service_tier` instead"
            )),
            "unexpected error for `{field}`: {error}"
        );
    }
}

#[test]
fn rejects_tier_selection_through_provider_default_headers() {
    for (provider, header) in [
        (
            "type: gcp_vertex\n    project_id: test-proj\n    auth:\n      mode: adc",
            "X-Vertex-AI-LLM-Shared-Request-Type",
        ),
        (
            "type: aws_bedrock\n    region: us-east-1\n    endpoint_kind: bedrock_runtime",
            "x-amzn-bedrock-service-tier",
        ),
    ] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");
        write_config(
            &config_path,
            &format!(
                "providers:\n  - id: tiered\n    {provider}\n    default_headers:\n      {header}: priority\n"
            ),
        );
        let error = format!(
            "{:#}",
            GatewayConfig::from_path(&config_path).expect_err("config should fail")
        );
        assert!(
            error.contains(&format!(
                "provider `tiered` default_headers cannot set `{header}`"
            )),
            "unexpected error for `{header}`: {error}"
        );
    }
}

#[test]
fn rejects_pricing_tiers_that_repeat_a_priced_tier() {
    for (route_tier, listed, expected) in [
        (
            "",
            &["default"][..],
            "cannot list `default`, the tier the route serves",
        ),
        (
            "service_tier: priority",
            &["fast"],
            "cannot list `priority`, the tier the route serves",
        ),
        ("", &["flex", "flex"], "lists `flex` more than once"),
    ] {
        let entries: String = listed
            .iter()
            .map(|tier| {
                format!(
                    "            - service_tier: {tier}\n              input_usd_per_million_tokens: \"2.0000\"\n              output_usd_per_million_tokens: \"4.0000\"\n"
                )
            })
            .collect();
        let error = load_error(&format!(
            r#"  - id: m
    routes:
      - provider: openai-prod
        upstream_model: u
        {route_tier}
        pricing_override:
          input_usd_per_million_tokens: "1.0000"
          output_usd_per_million_tokens: "2.0000"
          service_tiers:
{entries}"#
        ));
        assert!(error.contains(expected), "unexpected error: {error}");
    }
}
