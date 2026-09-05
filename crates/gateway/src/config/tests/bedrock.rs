use super::*;

#[test]
fn accepts_bedrock_bearer_auth_and_seeds_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock-bearer
    type: aws_bedrock
    region: " us-west-2 "
    endpoint_kind: bedrock_runtime
    endpoint_url: "https://bedrock-runtime.us-west-2.amazonaws.com/"
    auth:
      mode: bearer
      token: literal.test-token
    default_headers:
      x-test: configured
    timeouts:
      total_ms: 30000
    display:
      label: Bedrock
      icon_key: aws
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let providers = config.seed_providers().expect("seed providers");

    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].provider_type, "aws_bedrock");
    assert_eq!(
        providers[0].config["endpoint_url"],
        "https://bedrock-runtime.us-west-2.amazonaws.com"
    );
    assert_eq!(providers[0].config["region"], "us-west-2");
    assert_eq!(providers[0].config["endpoint_kind"], "bedrock_runtime");
    assert!(providers[0].config.get("token").is_none());
    assert_eq!(providers[0].secrets.as_ref().unwrap()["mode"], "bearer");

    let runtime_configs = config
        .bedrock_provider_configs()
        .expect("runtime provider configs");
    assert_eq!(runtime_configs[0].request_timeout_ms, 30_000);
}

#[test]
fn rejects_invalid_bedrock_provider_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: ""
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("aws_bedrock provider id cannot be empty"),
        "unexpected error: {error_text}"
    );

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: ""
    endpoint_kind: bedrock_runtime
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("region cannot be empty"),
        "unexpected error: {error_text}"
    );

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
    endpoint_url: "not a url"
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("endpoint_url `not a url` is invalid"),
        "unexpected error: {error_text}"
    );

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: "us east 1"
    endpoint_kind: bedrock_runtime
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("aws_bedrock provider `bedrock` endpoint_url is invalid"),
        "unexpected error: {error_text}"
    );

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
    auth:
      mode: static_credentials
      access_key_id: literal.test-access-key
"#,
    );
    GatewayConfig::from_path(&config_path).expect_err("config should fail");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
    auth:
      mode: bearer
      token: literal.test-token
      access_key_id: literal.test-access-key
"#,
    );
    GatewayConfig::from_path(&config_path).expect_err("config should fail");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
    auth:
      mode: bearer
      token: raw-token
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("aws_bedrock provider `bedrock` bearer.token"),
        "unexpected error: {error_text}"
    );
    assert!(
        error_text.contains(
            "unsupported secret reference; use env.*, file.*, or literal.* for this phase"
        ),
        "unexpected error: {error_text}"
    );
    assert!(
        !error_text.contains("raw-token"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn accepts_bedrock_default_chain_and_static_credentials_auth() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock-default
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
    auth:
      mode: default_chain
  - id: bedrock-static
    type: aws_bedrock
    region: us-west-2
    endpoint_kind: bedrock_runtime
    auth:
      mode: static_credentials
      access_key_id: literal.test-access-key
      secret_access_key: literal.test-secret-key
      session_token: literal.test-session-token
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let runtime_configs = config
        .bedrock_provider_configs()
        .expect("runtime provider configs");

    assert_eq!(runtime_configs.len(), 2);
    assert!(matches!(
        runtime_configs[0].auth,
        BedrockAuthConfig::DefaultChain
    ));
    match &runtime_configs[1].auth {
        BedrockAuthConfig::StaticCredentials {
            access_key_id,
            secret_access_key,
            session_token,
        } => {
            assert_eq!(access_key_id, "test-access-key");
            assert_eq!(secret_access_key, "test-secret-key");
            assert_eq!(session_token.as_deref(), Some("test-session-token"));
        }
        other => panic!("unexpected auth config: {other:?}"),
    }
}

#[test]
fn rejects_empty_bedrock_static_credential_fields() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
    auth:
      mode: static_credentials
      access_key_id: literal.
      secret_access_key: literal.test-secret-key
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("static_credentials.access_key_id cannot be empty"),
        "unexpected error: {error_text}"
    );

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
    auth:
      mode: static_credentials
      access_key_id: literal.test-access-key
      secret_access_key: literal.
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("static_credentials.secret_access_key cannot be empty"),
        "unexpected error: {error_text}"
    );

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
    auth:
      mode: static_credentials
      access_key_id: literal.test-access-key
      secret_access_key: literal.test-secret-key
      session_token: literal.
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("static_credentials.session_token cannot be empty"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn maps_bedrock_strict_tools_compatibility_override() {
    let compatibility = AwsBedrockRouteCompatibilityConfig {
        api_style: AwsBedrockApiStyle::RuntimeConverse,
        openai_base_path: None,
        supports_strict_tools: Some(false),
    }
    .into_compatibility();

    assert_eq!(compatibility.supports_strict_tools, Some(false));
}

#[test]
fn parses_bedrock_mantle_route_compatibility_and_extra_headers() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock-mantle
    type: aws_bedrock
    region: us-east-2
    endpoint_kind: bedrock_mantle
    auth:
      mode: bearer
      token: literal.test-token
models:
  - id: gpt-55
    routes:
      - provider: bedrock-mantle
        upstream_model: openai.gpt-5.5
        capabilities:
          chat_completions: false
          responses: true
          stream: true
          embeddings: false
        extra_headers:
          OpenAI-Project: proj_123
        compatibility:
          aws_bedrock:
            api_style: mantle_openai_responses
            openai_base_path: /openai/v1
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let models = config.seed_models().expect("seed models");
    let route = &models[0].routes[0];
    let compatibility = route
        .compatibility
        .aws_bedrock
        .as_ref()
        .expect("aws bedrock compatibility");

    assert_eq!(
        compatibility.api_style,
        AwsBedrockApiStyle::MantleOpenaiResponses
    );
    assert_eq!(
        compatibility.openai_base_path.as_deref(),
        Some("/openai/v1")
    );
    assert_eq!(route.extra_headers["OpenAI-Project"], "proj_123");
}

#[test]
fn rejects_bedrock_routes_without_required_compatibility() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
models:
  - id: nova
    routes:
      - provider: bedrock
        upstream_model: amazon.nova-pro-v1:0
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("compatibility.aws_bedrock.api_style"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_incompatible_bedrock_endpoint_and_api_style_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock-mantle
    type: aws_bedrock
    region: us-east-2
    endpoint_kind: bedrock_mantle
models:
  - id: nova
    routes:
      - provider: bedrock-mantle
        upstream_model: amazon.nova-pro-v1:0
        compatibility:
          aws_bedrock:
            api_style: runtime_converse
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("incompatible with endpoint_kind `bedrock_mantle`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_openai_shaped_bedrock_route_without_base_path() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock-mantle
    type: aws_bedrock
    region: us-east-2
    endpoint_kind: bedrock_mantle
models:
  - id: gpt-55
    routes:
      - provider: bedrock-mantle
        upstream_model: openai.gpt-5.5
        compatibility:
          aws_bedrock:
            api_style: mantle_openai_responses
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("compatibility.aws_bedrock.openai_base_path"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_bedrock_responses_capability_for_non_responses_api_style() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
models:
  - id: nova
    routes:
      - provider: bedrock
        upstream_model: amazon.nova-pro-v1:0
        capabilities:
          responses: true
        compatibility:
          aws_bedrock:
            api_style: runtime_converse
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("responses require api_style `mantle_openai_responses`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_bedrock_json_schema_capability_for_non_responses_api_style() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock
    type: aws_bedrock
    region: us-east-1
    endpoint_kind: bedrock_runtime
models:
  - id: nova
    routes:
      - provider: bedrock
        upstream_model: amazon.nova-pro-v1:0
        capabilities:
          responses: false
          json_schema: true
        compatibility:
          aws_bedrock:
            api_style: runtime_converse
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("json_schema requires api_style `mantle_openai_responses`"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn rejects_bedrock_responses_api_style_with_chat_capability() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: bedrock-mantle
    type: aws_bedrock
    region: us-east-2
    endpoint_kind: bedrock_mantle
models:
  - id: gpt-55
    routes:
      - provider: bedrock-mantle
        upstream_model: openai.gpt-5.5
        capabilities:
          responses: true
          chat_completions: true
        compatibility:
          aws_bedrock:
            api_style: mantle_openai_responses
            openai_base_path: /openai/v1
"#,
    );
    let error = GatewayConfig::from_path(&config_path).expect_err("config should fail");
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("cannot enable chat_completions capability"),
        "unexpected error: {error_text}"
    );
}

#[test]
fn managed_bedrock_credentials_accept_file_references() {
    let temporary = tempdir().expect("tempdir");
    for (name, expected) in [
        ("access-key", "test-access-key"),
        ("secret-key", "test-secret-key"),
        ("session-token", "test-session-token"),
    ] {
        let path = temporary.path().join(name);
        std::fs::write(&path, format!("{expected}\n")).expect("write credential");
        assert_eq!(
            resolve_secret_reference(&format!("file.{}", path.display()))
                .expect("resolve file credential"),
            expected
        );
    }
}

#[test]
fn bedrock_api_styles_require_explicit_compatible_capabilities() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    for (api_style, endpoint_kind, capabilities) in [
        (
            "runtime_converse",
            "bedrock_runtime",
            "responses: false, json_schema: false",
        ),
        (
            "runtime_anthropic_invoke",
            "bedrock_runtime",
            "responses: false, json_schema: false, stream: false",
        ),
        (
            "runtime_openai_chat",
            "bedrock_runtime",
            "responses: false, json_schema: false",
        ),
        (
            "mantle_openai_chat",
            "bedrock_mantle",
            "responses: false, json_schema: false",
        ),
        (
            "mantle_anthropic_messages",
            "bedrock_mantle",
            "responses: false, json_schema: false",
        ),
        (
            "mantle_openai_responses",
            "bedrock_mantle",
            "chat_completions: false",
        ),
    ] {
        let base_path = if api_style.contains("openai") {
            "\n            openai_base_path: /openai/v1"
        } else {
            ""
        };
        let yaml = format!(
            "providers:\n  - id: bedrock\n    type: aws_bedrock\n    region: us-east-1\n    endpoint_kind: {endpoint_kind}\nmodels:\n  - id: model\n    routes:\n      - provider: bedrock\n        upstream_model: upstream\n        compatibility:\n          aws_bedrock:\n            api_style: {api_style}{base_path}\n"
        );
        write_config(&config_path, &yaml);
        let error = GatewayConfig::from_path(&config_path)
            .expect_err("permissive defaults must conflict with each Bedrock style");
        let expected = if api_style == "mantle_openai_responses" {
            "cannot enable chat_completions capability"
        } else {
            "cannot enable responses capability"
        };
        assert!(
            format!("{error:#}").contains(expected),
            "{api_style}: {error:#}"
        );

        write_config(
            &config_path,
            &format!("{yaml}        capabilities: {{ {capabilities}, embeddings: false }}\n"),
        );
        GatewayConfig::from_path(&config_path)
            .unwrap_or_else(|error| panic!("explicit capabilities for {api_style}: {error:#}"));
    }
}
