use super::*;

#[test]
fn parses_github_copilot_provider_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: copilot-org
    type: github_copilot
    base_url: https://api.githubcopilot.com
    pricing_provider_id: openai
    auth:
      mode: github_app
      app_id: 12345
      installation_id: 67890
      private_key: literal.test-private-key-content
      repository_id: 112233
    editor_version: vscode/1.126.0
    integration_id: vscode-chat
models:
  - id: copilot-gpt-4o
    routes:
      - provider: copilot-org
        upstream_model: gpt-4o
        compatibility:
          github_copilot:
            chat_api: chat_completions
            supports_responses: true
            upstream_supports:
              streaming: true
              tool_calls: true
              vision: true
              structured_outputs: true
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let providers = config.seed_providers().expect("seed providers");
    assert_eq!(providers[0].provider_type, "github_copilot");
    assert_eq!(
        providers[0].config["base_url"],
        "https://api.githubcopilot.com"
    );
    assert_eq!(providers[0].config["editor_version"], "vscode/1.126.0");
    assert_eq!(providers[0].config["integration_id"], "vscode-chat");
    assert_eq!(providers[0].config["pricing_provider_id"], "openai");

    let runtime_configs = config
        .copilot_provider_configs()
        .expect("copilot runtime provider configs");
    assert_eq!(runtime_configs.len(), 1);
    assert_eq!(runtime_configs[0].provider_key, "copilot-org");
    assert_eq!(runtime_configs[0].base_url, "https://api.githubcopilot.com");
    assert_eq!(runtime_configs[0].editor_version, "vscode/1.126.0");
    assert_eq!(runtime_configs[0].integration_id, "vscode-chat");

    let models = config.seed_models().expect("seed models");
    let compatibility = models[0].routes[0]
        .compatibility
        .github_copilot
        .as_ref()
        .expect("Copilot route compatibility");
    assert_eq!(
        compatibility.chat_api,
        Some(GitHubCopilotChatApi::ChatCompletions)
    );
    assert!(compatibility.supports_responses);
    assert!(!compatibility.supports_embeddings);
    assert!(compatibility.upstream_supports.streaming);
    assert!(compatibility.upstream_supports.tool_calls);
    assert!(compatibility.upstream_supports.vision);
    assert!(compatibility.upstream_supports.structured_outputs);
}

#[test]
fn rejects_github_copilot_route_compatibility_for_other_providers() {
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
  - id: gpt-4o
    routes:
      - provider: openai-prod
        upstream_model: gpt-4o
        compatibility:
          github_copilot:
            chat_api: chat_completions
"#,
    );

    let error = GatewayConfig::from_path(&config_path)
        .expect_err("Copilot compatibility on another provider should fail");
    assert!(
        format!("{error:#}")
            .contains("compatibility.github_copilot but requires a github_copilot provider")
    );
}

#[test]
fn github_copilot_github_app_requires_repository_id() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: copilot-org
    type: github_copilot
    auth:
      mode: github_app
      app_id: 12345
      installation_id: 67890
      private_key: literal.test-private-key-content
"#,
    );

    let error =
        GatewayConfig::from_path(&config_path).expect_err("missing repository ID should fail");

    assert!(format!("{error:#}").contains("missing field `repository_id`"));
}

#[test]
fn github_copilot_github_app_rejects_zero_repository_id() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: copilot-org
    type: github_copilot
    auth:
      mode: github_app
      app_id: 12345
      installation_id: 67890
      private_key: literal.test-private-key-content
      repository_id: 0
"#,
    );

    let error = GatewayConfig::from_path(&config_path).expect_err("zero repository ID should fail");

    assert!(format!("{error:#}").contains("auth.repository_id cannot be 0"));
}

#[test]
fn github_copilot_github_app_accepts_mounted_private_key_path() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");
    let private_key_path = tmp.path().join("copilot-private-key.pem");
    write_config(&private_key_path, "test-private-key-content");
    write_config(
        &config_path,
        &format!(
            r#"
providers:
  - id: copilot-org
    type: github_copilot
    auth:
      mode: github_app
      app_id: 12345
      installation_id: 67890
      private_key: {}
      repository_id: 112233
"#,
            private_key_path.display()
        ),
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let runtime_configs = config
        .copilot_provider_configs()
        .expect("Copilot runtime provider configs");

    match &runtime_configs[0].auth {
        CopilotAuthConfig::GitHubAppKeyFile {
            private_key_path: configured_path,
            repository_id,
            ..
        } => {
            assert_eq!(configured_path, &private_key_path);
            assert_eq!(*repository_id, 112233);
        }
        other => panic!("expected GitHub App key file auth, got {other:?}"),
    }
}

#[test]
fn parses_github_copilot_bearer_auth_config() {
    let tmp = tempdir().expect("tempdir");
    let config_path = tmp.path().join("gateway.yaml");

    write_config(
        &config_path,
        r#"
providers:
  - id: copilot-bearer
    type: github_copilot
    auth:
      mode: bearer
      token: literal.ghs_test_token
models:
  - id: copilot-claude
    routes:
      - provider: copilot-bearer
        upstream_model: claude-3-7-sonnet
"#,
    );

    let config = GatewayConfig::from_path(&config_path).expect("config should parse");
    let runtime_configs = config
        .copilot_provider_configs()
        .expect("copilot runtime provider configs");
    assert_eq!(runtime_configs.len(), 1);
    assert_eq!(runtime_configs[0].provider_key, "copilot-bearer");
    assert_eq!(runtime_configs[0].base_url, "https://api.githubcopilot.com");
}
