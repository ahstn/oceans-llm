use super::*;

#[test]
fn accepts_valid_vertex_auth_modes() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: vertex-adc
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: adc
  - id: vertex-sa
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: service_account
      credentials_path: /tmp/sa.json
  - id: vertex-bearer
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: bearer
      token: literal.test-token
models:
  - id: fast
    routes:
      - provider: vertex-adc
        upstream_model: google/gemini-2.0-flash
"#,
    );

    GatewayConfig::from_path(&config_path).expect("config should parse");
}

#[test]
fn rejects_missing_vertex_bearer_token() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: vertex-bearer
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: bearer
"#,
    );

    GatewayConfig::from_path(&config_path).expect_err("config should fail");
}

#[test]
fn parses_cloud_run_openai_compat_provider_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: gemma-cloud-run
    type: gcp_cloud_run_openai_compat
    base_url: https://gemma-service-abc-uc.a.run.app/v1
    pricing_provider_id: google-vertex
    auth:
      mode: bearer
      token: literal.debug-id-token
    auth_header: x_serverless_authorization
models:
  - id: gemma-cloud-run
    routes:
      - provider: gemma-cloud-run
        upstream_model: google/gemma-4-12b-it
        extra_body:
          chat_template_kwargs:
            enable_thinking: true
          skip_special_tokens: false
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let providers = config.seed_providers().expect("seed providers");
    assert_eq!(providers[0].provider_type, "gcp_cloud_run_openai_compat");
    assert_eq!(
        providers[0].config["audience"],
        "https://gemma-service-abc-uc.a.run.app/"
    );
    assert_eq!(
        providers[0].config["auth_header"],
        "x_serverless_authorization"
    );

    let runtime_configs = config
        .openai_compatible_provider_configs()
        .expect("runtime provider configs");
    assert_eq!(
        runtime_configs[0].provider_type,
        "gcp_cloud_run_openai_compat"
    );
    assert_eq!(
        runtime_configs[0].bearer_auth_header,
        BearerAuthHeader::XServerlessAuthorization
    );
    assert_eq!(
        runtime_configs[0].bearer_token.as_deref(),
        Some("debug-id-token")
    );

    let models = config.seed_models().expect("seed models");
    assert_eq!(
        models[0].routes[0].extra_body["chat_template_kwargs"]["enable_thinking"],
        true
    );
    assert_eq!(models[0].routes[0].extra_body["skip_special_tokens"], false);
}

#[test]
fn accepts_cloud_run_openai_compat_custom_audience() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: gemma-cloud-run
    type: gcp_cloud_run_openai_compat
    base_url: https://gemma.example.com/v1
    audience: https://custom-audience.example.com
    pricing_provider_id: google-vertex
    auth:
      mode: bearer
      token: literal.debug-id-token
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let providers = config.seed_providers().expect("seed providers");
    assert_eq!(
        providers[0].config["audience"],
        "https://custom-audience.example.com"
    );
}

#[test]
fn rejects_cloud_run_openai_compat_non_https_base_url() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: gemma-cloud-run
    type: gcp_cloud_run_openai_compat
    base_url: http://gemma-service.run.app/v1
    pricing_provider_id: google-vertex
    auth:
      mode: adc
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("base_url must use https"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_cloud_run_openai_compat_whitespace_padded_base_url() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: gemma-cloud-run
    type: gcp_cloud_run_openai_compat
    base_url: " https://gemma-service.run.app/v1 "
    pricing_provider_id: google-vertex
    auth:
      mode: adc
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("base_url cannot include leading or trailing whitespace"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn accepts_supported_provider_display_icon_keys() {
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
    display:
      icon_key: openai
  - id: router-prod
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    pricing_provider_id: openai
    display:
      icon_key: openrouter
  - id: bedrock-prod
    type: openai_compat
    base_url: https://bedrock-runtime.us-east-1.amazonaws.com/openai/v1
    pricing_provider_id: openai
    display:
      icon_key: aws
"#,
    );

    GatewayConfig::from_path(&config_path).expect("config should parse");
}

#[test]
fn projects_every_provider_variant_into_seed_and_runtime_configs() -> anyhow::Result<()> {
    let tmp = tempdir()?;
    let config_path = tmp.path().join("gateway.yaml");
    write_config(
        &config_path,
        r#"
providers:
  - id: openai
    type: openai_compat
    base_url: https://api.openai.com/v1
    pricing_provider_id: openai
    auth:
      kind: bearer
      token: literal.openai-token
    timeouts:
      total_ms: 1000
  - id: cloud-run
    type: gcp_cloud_run_openai_compat
    base_url: https://service.example.com/v1
    pricing_provider_id: google-vertex
    auth:
      mode: service_account
      credentials_path: literal./tmp/cloud-run.json
  - id: vertex-adc
    type: gcp_vertex
    project_id: project
    auth:
      mode: adc
  - id: vertex-service-account
    type: gcp_vertex
    project_id: project
    auth:
      mode: service_account
      credentials_path: literal./tmp/vertex.json
  - id: vertex-bearer
    type: gcp_vertex
    project_id: project
    auth:
      mode: bearer
      token: literal.vertex-token
  - id: bedrock-default
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
  - id: bedrock-static
    type: aws_bedrock
    region: us-west-2
    endpoint_kind: bedrock_runtime
    auth:
      mode: static_credentials
      access_key_id: literal.access-key
      secret_access_key: literal.secret-key
      session_token: literal.session-token
  - id: bedrock-bearer
    type: aws_bedrock
    region: us-east-2
    endpoint_kind: bedrock_runtime
    auth:
      mode: bearer
      token: literal.bedrock-token
  - id: copilot-app
    type: github_copilot
    auth:
      mode: github_app
      app_id: 1
      installation_id: 2
      repository_id: 3
      private_key: "literal.-----BEGIN PRIVATE KEY-----"
  - id: copilot-bearer
    type: github_copilot
    auth:
      mode: bearer
      token: literal.copilot-token
"#,
    );

    let config = GatewayConfig::from_path(&config_path)?;

    let seed_providers = config.seed_providers()?;
    assert_eq!(seed_providers.len(), 10);
    assert_eq!(config.openai_compatible_provider_configs()?.len(), 2);
    assert_eq!(config.vertex_provider_configs()?.len(), 3);
    assert_eq!(config.bedrock_provider_configs()?.len(), 3);
    assert_eq!(config.copilot_provider_configs()?.len(), 2);

    Ok(())
}

#[test]
fn rejects_invalid_provider_fields_before_runtime_startup() -> anyhow::Result<()> {
    let cases = [
        (
            "providers:\n  - id: ''\n    type: openai_compat\n    base_url: https://api.openai.com/v1\n    pricing_provider_id: openai\n",
            "openai_compat provider id cannot be empty",
        ),
        (
            "providers:\n  - id: openai\n    type: openai_compat\n    base_url: ''\n    pricing_provider_id: openai\n",
            "base_url cannot be empty",
        ),
        (
            "providers:\n  - id: cloud-run\n    type: gcp_cloud_run_openai_compat\n    base_url: https://service.example.com\n    pricing_provider_id: unsupported\n    auth:\n      mode: adc\n",
            "pricing_provider_id `unsupported` is not supported",
        ),
        (
            "providers:\n  - id: cloud-run\n    type: gcp_cloud_run_openai_compat\n    base_url: https://service.example.com\n    audience: ''\n    pricing_provider_id: google-vertex\n    auth:\n      mode: adc\n",
            "audience cannot be empty",
        ),
        (
            "providers:\n  - id: vertex\n    type: gcp_vertex\n    project_id: ''\n    auth:\n      mode: adc\n",
            "project_id cannot be empty",
        ),
        (
            "providers:\n  - id: vertex\n    type: gcp_vertex\n    project_id: project\n    location: ''\n    auth:\n      mode: adc\n",
            "location cannot be empty",
        ),
        (
            "providers:\n  - id: vertex\n    type: gcp_vertex\n    project_id: project\n    api_host: ''\n    auth:\n      mode: adc\n",
            "api_host cannot be empty",
        ),
        (
            "providers:\n  - id: copilot\n    type: github_copilot\n    editor_version: ''\n    auth:\n      mode: bearer\n      token: literal.token\n",
            "editor_version cannot be empty",
        ),
        (
            "providers:\n  - id: copilot\n    type: github_copilot\n    integration_id: ''\n    auth:\n      mode: bearer\n      token: literal.token\n",
            "integration_id cannot be empty",
        ),
        (
            "providers:\n  - id: copilot\n    type: github_copilot\n    auth:\n      mode: github_app\n      app_id: 0\n      installation_id: 2\n      repository_id: 3\n      private_key: literal.key\n",
            "auth.app_id cannot be 0",
        ),
        (
            "providers:\n  - id: copilot\n    type: github_copilot\n    auth:\n      mode: github_app\n      app_id: 1\n      installation_id: 0\n      repository_id: 3\n      private_key: literal.key\n",
            "auth.installation_id cannot be 0",
        ),
        (
            "providers:\n  - id: copilot\n    type: github_copilot\n    auth:\n      mode: github_app\n      app_id: 1\n      installation_id: 2\n      repository_id: 3\n      private_key: ''\n",
            "auth.private_key cannot be empty",
        ),
    ];

    for (yaml, expected) in cases {
        let tmp = tempdir()?;
        let config_path = tmp.path().join("gateway.yaml");
        write_config(&config_path, yaml);
        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        assert!(
            format!("{error:#}").contains(expected),
            "expected error containing `{expected}`, got `{error:#}`"
        );
    }

    Ok(())
}
#[test]
fn rejects_missing_vertex_service_account_path() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: vertex-sa
    type: gcp_vertex
    project_id: test-proj
    auth:
      mode: service_account
"#,
    );

    GatewayConfig::from_path(&config_path).expect_err("config should fail");
}

#[test]
fn rejects_cloud_run_openai_compat_empty_service_account_path() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: gemma-cloud-run
    type: gcp_cloud_run_openai_compat
    base_url: https://gemma-service.run.app/v1
    pricing_provider_id: google-vertex
    auth:
      mode: service_account
      credentials_path: ""
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("service_account.credentials_path cannot be empty"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn parses_anthropic_compat_provider_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: opencode-zen
    type: anthropic_compat
    base_url: https://opencode.ai/zen
    pricing_provider_id: opencode
    auth:
      kind: x_api_key
      token: literal.test_opencode_key
    default_headers:
      anthropic-version: "2023-06-01"
    display:
      label: OpenCode Zen
      icon_key: anthropic

models:
  - id: claude-fable-5-1
    description: Claude Fable 5.1 through OpenCode Zen
    max_reasoning_effort: max
    routes:
      - provider: opencode-zen
        upstream_model: claude-fable-5-1
        context_window_tokens: 1000000
        pricing_override:
          input_usd_per_million_tokens: "10.0000"
          output_usd_per_million_tokens: "50.0000"
          cache_read_usd_per_million_tokens: "0.2500"
          cache_write_usd_per_million_tokens: "12.5000"
        capabilities:
          chat_completions: true
          responses: false
          embeddings: false
          stream: true
          tools: true
          vision: true
          json_schema: false
          developer_role: false
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let runtime_configs = config
        .anthropic_compatible_provider_configs()
        .expect("anthropic_compat runtime provider configs");
    assert_eq!(runtime_configs.len(), 1);
    assert_eq!(runtime_configs[0].provider_key, "opencode-zen");
    assert_eq!(runtime_configs[0].base_url, "https://opencode.ai/zen");
    assert_eq!(
        runtime_configs[0].auth,
        Some(gateway_providers::AnthropicCompatAuth {
            kind: gateway_providers::AnthropicCompatAuthKind::XApiKey,
            token: "test_opencode_key".to_string(),
        })
    );
    assert_eq!(
        runtime_configs[0].default_headers.get("anthropic-version"),
        Some(&"2023-06-01".to_string())
    );

    let seed_providers = config.seed_providers().expect("seed providers");
    assert_eq!(seed_providers[0].provider_key, "opencode-zen");
    assert_eq!(seed_providers[0].provider_type, "anthropic_compat");

    let seed_models = config.seed_models().expect("seed models");
    assert_eq!(seed_models[0].model_key, "claude-fable-5-1");
    assert_eq!(
        seed_models[0].max_reasoning_effort,
        Some(gateway_core::ReasoningEffort::Max)
    );
}

#[test]
fn rejects_anthropic_compat_with_unsupported_pricing_provider() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: opencode-zen
    type: anthropic_compat
    base_url: https://opencode.ai/zen
    pricing_provider_id: unknown_provider
models:
  - id: test
    routes:
      - provider: opencode-zen
        upstream_model: test
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("pricing_provider_id `unknown_provider` is not supported"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_anthropic_compat_with_query_or_fragment_in_base_url() {
    for invalid_base_url in [
        "https://opencode.ai/zen?query=1",
        "https://opencode.ai/zen#fragment",
        "https://opencode.ai/zen?query=1#fragment",
    ] {
        let tmp = tempdir().expect("tempdir");
        let config_path = tmp.path().join("gateway.yaml");

        write_config(
            &config_path,
            &format!(
                r#"
providers:
  - id: opencode-zen
    type: anthropic_compat
    base_url: {invalid_base_url}
    pricing_provider_id: opencode
models:
  - id: test
    routes:
      - provider: opencode-zen
        upstream_model: test
"#
            ),
        );

        let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
        let error_text = format!("{error:#}");
        assert!(
            error_text.contains("base_url cannot include query parameters or fragments"),
            "unexpected error for {invalid_base_url}: {error_text}"
        );
    }
}

#[test]
fn rejects_duplicate_provider_ids_across_provider_types() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    for second_type in ["openai_compat", "anthropic_compat"] {
        write_config(
            &path,
            &format!(
                r#"
providers:
  - id: upstream
    type: openai_compat
    base_url: https://api.example.com/v1
    pricing_provider_id: openai
  - id: upstream
    type: {second_type}
    base_url: https://other.example.com/v1
    pricing_provider_id: openai
"#
            ),
        );
        let error = GatewayConfig::from_path(&path).expect_err("duplicate provider");
        assert!(format!("{error:#}").contains("duplicate provider id `upstream`"));
    }
}

#[test]
fn validates_http_provider_urls_at_config_load() {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    for (provider_type, field) in [
        ("openai_compat", "base_url"),
        ("github_copilot", "base_url"),
        ("github_copilot", "github_api_url"),
    ] {
        for (url, accepted) in [
            ("/relative/path", false),
            ("file:///tmp/upstream", false),
            ("ftp://example.com/api", false),
            ("https://", false),
            ("http://127.0.0.1:8080/v1", true),
            ("https://api.example.com/v1", true),
        ] {
            let base_url = if field == "base_url" {
                url
            } else {
                "https://api.example.com"
            };
            let github_api = if field == "github_api_url" {
                format!("    github_api_url: '{url}'\n")
            } else {
                String::new()
            };
            let auth = if provider_type == "github_copilot" {
                "    auth:\n      mode: github_user\n"
            } else {
                ""
            };
            write_config(
                &path,
                &format!(
                    "providers:\n  - id: upstream\n    type: {provider_type}\n    base_url: '{base_url}'\n    pricing_provider_id: openai\n{github_api}{auth}"
                ),
            );
            let result = GatewayConfig::from_path(&path);
            if accepted {
                result.unwrap_or_else(|error| {
                    panic!("{provider_type}.{field} rejected {url}: {error:#}")
                });
            } else {
                let error = result.expect_err("invalid provider URL");
                assert!(
                    format!("{error:#}")
                        .contains(&format!("{provider_type} provider `upstream` {field}")),
                    "{error:#}"
                );
            }
        }
    }
}
