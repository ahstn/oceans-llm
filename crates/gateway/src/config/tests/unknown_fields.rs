use super::*;

/// One valid config that reaches every provider and route struct. Each `@slot@` marks the
/// line after which a misspelled key is inserted, as a sibling of that line.
const CONFIG: &str = r#"
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai@openai_compat_provider@
    auth:
      kind: bearer
      token: literal.test-openai-key@openai_compat_auth@
    timeouts:
      total_ms: 1000@provider_timeouts@
    display:
      label: OpenAI@provider_display@
    batch:
      dialect: open_ai@openai_batch@
  - id: typesafe
    type: typesafe
    pricing_provider_id: openrouter@typesafe_provider@
  - id: anthropic
    type: anthropic_compat
    base_url: https://api.anthropic.com
    pricing_provider_id: opencode@anthropic_compat_provider@
    auth:
      kind: x_api_key
      token: literal.test-anthropic-key@anthropic_compat_auth@
  - id: vertex
    type: gcp_vertex
    project_id: test-project@gcp_vertex_provider@
    auth:
      mode: bearer
      token: literal.test-vertex-token@gcp_vertex_auth@
    batch:
      dataset: batches@gcp_vertex_batch@
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime@aws_bedrock_provider@
  - id: copilot
    type: github_copilot
    pricing_provider_id: github-copilot@github_copilot_provider@
    auth:
      mode: github_user
models:
  - id: gpt
    routes:
      - provider: openai
        upstream_model: gpt-6@model_route@
        capabilities:
          tools: true@route_capabilities@
        compatibility:
          openai_compat:
            supports_store: false@openai_compat_route_compatibility@
  - id: nova
    routes:
      - provider: bedrock
        upstream_model: amazon.nova-lite
        compatibility:
          aws_bedrock:
            api_style: runtime_converse@aws_bedrock_route_compatibility@
"#;

const SLOTS: &[&str] = &[
    "openai_compat_provider",
    "openai_compat_auth",
    "provider_timeouts",
    "provider_display",
    "openai_batch",
    "typesafe_provider",
    "anthropic_compat_provider",
    "anthropic_compat_auth",
    "gcp_vertex_provider",
    "gcp_vertex_auth",
    "gcp_vertex_batch",
    "aws_bedrock_provider",
    "github_copilot_provider",
    "model_route",
    "route_capabilities",
    "openai_compat_route_compatibility",
    "aws_bedrock_route_compatibility",
];

/// Strips the markers, then adds `typo_<slot>: true` after the line marked with `typo_slot`.
fn render(typo_slot: Option<&str>) -> String {
    let mut rendered = String::new();
    for line in CONFIG.lines() {
        let (content, slot) = match line.split_once('@') {
            Some((content, slot)) => (content, slot.strip_suffix('@')),
            None => (line, None),
        };
        rendered.push_str(content);
        rendered.push('\n');
        if let Some(slot) = slot
            && Some(slot) == typo_slot
        {
            let indent = content.len() - content.trim_start().len();
            rendered.push_str(&" ".repeat(indent));
            rendered.push_str("typo_");
            rendered.push_str(slot);
            rendered.push_str(": true\n");
        }
    }
    rendered
}

#[test]
fn provider_and_route_structs_reject_unknown_keys() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(&config_path, &render(None));
    GatewayConfig::from_path(&config_path)
        .unwrap_or_else(|error| panic!("the base config must load: {error:#}"));

    for &slot in SLOTS {
        write_config(&config_path, &render(Some(slot)));
        let error = GatewayConfig::from_path(&config_path)
            .expect_err("an unknown key must fail config load");
        let expected = format!("unknown field `typo_{slot}`");
        assert!(
            format!("{error:#}").contains(&expected),
            "{slot}: unexpected error: {error:#}"
        );
    }
}

#[test]
fn audit_examples_of_ignored_route_keys_now_fail() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    for (route_yaml, expected) in [
        ("service_teir: priority", "unknown field `service_teir`"),
        (
            "capabilities:\n          messages: true",
            "unknown field `messages`",
        ),
        (
            "compatibility:\n          openai_compat:\n            store: false",
            "unknown field `store`",
        ),
        (
            "compatibility:\n          openai_compatible: {}",
            "unknown field `openai_compatible`",
        ),
    ] {
        write_config(
            &config_path,
            &format!(
                "providers:\n  - id: openai\n    type: openai_compat\n    base_url: https://api.openai.com/v1\n    pricing_provider_id: openai\nmodels:\n  - id: gpt\n    routes:\n      - provider: openai\n        upstream_model: gpt-6\n        {route_yaml}\n"
            ),
        );
        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        assert!(
            format!("{error:#}").contains(expected),
            "unexpected error for {route_yaml}: {error:#}"
        );
    }
}
