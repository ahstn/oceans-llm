use std::time::Duration;

use gateway_core::{RateLimitMetric, RateLimitOperation, RateLimitPeriod, RateLimitScopeKind};
use gateway_service::RateLimitFailureMode;

use super::*;

const MODELS: &str = r#"
models:
  - id: fast
    routes:
      - provider: openai
        upstream_model: fast
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
"#;

fn parse(yaml: &str) -> anyhow::Result<GatewayConfig> {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    write_config(&config_path, &format!("{yaml}{MODELS}"));
    GatewayConfig::from_path(&config_path)
}

fn error_chain(yaml: &str) -> String {
    format!("{:#}", parse(yaml).expect_err("config should fail"))
}

#[test]
fn rate_limits_default_to_no_policies_and_fail_open() {
    let config = parse("").expect("config should parse");
    let settings = config.rate_limits.settings();
    assert!(settings.config_policies.is_empty());
    assert_eq!(settings.failure_mode, RateLimitFailureMode::Open);
    assert_eq!(settings.policy_refresh, Duration::from_secs(10));
    assert!(config.server.trusted_proxy_networks().unwrap().is_empty());
}

#[test]
fn parses_rate_limit_policies_and_settings() {
    let config = parse(
        r#"
server:
  trusted_proxies: ["10.0.0.0/8", "fd00::/8", "192.0.2.10"]
rate_limits:
  failure_mode: closed
  policy_refresh_seconds: 30
  policies:
    - name: " per key "
      operation: inference
      scope: api_key
      period: minute
      limit: 60
    - name: team tokens
      operation: inference
      scope: team
      model: " fast "
      metric: tokens
      period: hour
      limit: 100000
    - name: notes calls
      operation: mcp_tool_call
      scope: user
      mcp_server: notes
      period: month
      limit: 1000
"#,
    )
    .expect("config should parse");

    let settings = config.rate_limits.settings();
    assert_eq!(settings.failure_mode, RateLimitFailureMode::Closed);
    assert_eq!(settings.policy_refresh, Duration::from_secs(30));
    let [per_key, tokens, notes] = settings.config_policies.as_slice() else {
        panic!("expected three policies");
    };
    assert_eq!(per_key.name, "per key");
    assert_eq!(per_key.operation, RateLimitOperation::Inference);
    assert_eq!(per_key.scope_kind, RateLimitScopeKind::ApiKey);
    assert_eq!(per_key.metric, RateLimitMetric::Requests);
    assert_eq!(per_key.period, RateLimitPeriod::Minute);
    assert_eq!(tokens.model_key.as_deref(), Some("fast"));
    assert_eq!(tokens.metric, RateLimitMetric::Tokens);
    assert_eq!(notes.mcp_server_key.as_deref(), Some("notes"));
    assert_eq!(notes.period, RateLimitPeriod::Month);
    assert_eq!(
        config
            .rate_limits
            .mcp_server_keys()
            .into_iter()
            .collect::<Vec<_>>(),
        vec!["notes"]
    );
    assert_eq!(config.server.trusted_proxy_networks().unwrap().len(), 3);
}

#[test]
fn rejects_invalid_rate_limit_policies() {
    let policy = |body: &str| format!("rate_limits:\n  policies:\n    - {body}\n");
    let cases = [
        (
            policy("{name: a, operation: inference, scope: user, period: day, limit: 0}"),
            "limit must be greater than 0",
        ),
        (
            policy(
                "{name: a, operation: count_tokens, scope: user, metric: tokens, period: day, limit: 5}",
            ),
            "token limits apply only to the `inference` operation",
        ),
        (
            policy(
                "{name: a, operation: inference, scope: user, mcp_server: notes, period: day, limit: 5}",
            ),
            "an MCP server filter applies only to `mcp_tool_call`",
        ),
        (
            policy(
                "{name: a, operation: mcp_tool_call, scope: user, model: fast, period: day, limit: 5}",
            ),
            "a model filter does not apply to `mcp_tool_call`",
        ),
        (
            policy("{name: a, operation: inference, scope: user, model: slow, period: day, limit: 5}"),
            "references unknown model `slow`",
        ),
        (
            policy(
                "{name: a, operation: mcp_tool_call, scope: user, mcp_server: ' ', period: day, limit: 5}",
            ),
            "mcp_server must not be empty",
        ),
        (
            policy("{name: ' ', operation: inference, scope: user, period: day, limit: 5}"),
            "name must be 1 to 100 characters",
        ),
        (
            "rate_limits:\n  policies:\n    - {name: a, operation: inference, scope: user, period: day, limit: 5}\n    - {name: ' a', operation: inference, scope: team, period: day, limit: 5}\n".to_string(),
            "duplicate rate_limits.policies `a`",
        ),
        (
            "rate_limits:\n  policy_refresh_seconds: 0\n".to_string(),
            "policy_refresh_seconds must be between 1 and 3600",
        ),
        (
            policy("{name: a, operation: inference, scope: user, period: week, limit: 5}"),
            "unknown variant `week`",
        ),
        (
            "rate_limits:\n  failure_mode: maybe\n".to_string(),
            "unknown variant `maybe`",
        ),
        (
            "server:\n  trusted_proxies: [\"10.0.0.0/33\"]\n".to_string(),
            "server.trusted_proxies entry `10.0.0.0/33` is not an IP address or CIDR",
        ),
    ];
    for (yaml, expected) in cases {
        let error = error_chain(&yaml);
        assert!(error.contains(expected), "{yaml}\n=> {error}");
    }
}

#[test]
fn rejects_more_policies_per_operation_than_the_cap() {
    let cap = gateway_core::MAX_ENABLED_RATE_LIMIT_POLICIES_PER_OPERATION;
    let policies: String = (0..=cap)
        .map(|n| {
            format!("    - {{name: p{n}, operation: inference, scope: api_key, period: minute, limit: 5}}\n")
        })
        .collect();
    let error = error_chain(&format!("rate_limits:\n  policies:\n{policies}"));
    assert!(
        error.contains(&format!(
            "rate_limits.policies has {} `inference` policies",
            cap + 1
        )),
        "{error}"
    );
}
