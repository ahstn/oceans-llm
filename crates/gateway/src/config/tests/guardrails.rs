use super::*;
use gateway_guardrails::{
    DecisionAction, EvaluationInput, EvaluationPayload, GuardPhase, PolicyResolver, PolicyTarget,
};
use tempfile::tempdir;

fn managed_config() -> GatewayConfig {
    serde_yaml::from_str(
        r#"
guardrails:
  default:
    enabled: true
    mode: deny
    managed_checks: [armor]
  managed_checks:
    armor:
      kind: google_model_armor
      phases: [prompt]
      failure_disposition: fail_closed
      model_armor:
        project: project
        location: us-central1
        prompt_template: projects/project/locations/us-central1/templates/prompt
        auth:
          kind: bearer_token
          token: literal.test-token
    bedrock:
      kind: amazon_bedrock
      phases: [prompt]
      bedrock:
        region: us-east-1
        guardrail_identifier: guardrail-id
        guardrail_version: '1'
        auth:
          kind: static_credentials
          access_key_id: literal.access-key
          secret_access_key: literal.secret-key
          session_token: literal.session-token
"#,
    )
    .expect("managed configuration")
}

#[test]
fn constructs_managed_engines_with_static_and_default_aws_credentials() {
    let mut config = managed_config();
    config.guardrail_engine().expect("static credentials");
    config
        .guardrails
        .managed_checks
        .get_mut("bedrock")
        .unwrap()
        .bedrock
        .as_mut()
        .unwrap()
        .auth = BedrockManagedAuthConfig::DefaultChain;
    config
        .guardrail_engine()
        .expect("default credentials are deferred until evaluation");
}

#[test]
fn engine_construction_rejects_missing_adapter_settings() {
    for (name, expected) in [
        (
            "bedrock",
            "managed guardrail check `bedrock` is missing bedrock config",
        ),
        (
            "armor",
            "managed guardrail check `armor` is missing model_armor config",
        ),
    ] {
        let mut config = managed_config();
        let check = config.guardrails.managed_checks.get_mut(name).unwrap();
        check.bedrock = None;
        check.model_armor = None;
        let error = config
            .guardrail_engine()
            .err()
            .expect("missing adapter must fail");
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn engine_construction_checks_secret_references_and_adapter_invariants() {
    let temporary = tempdir().expect("tempdir");
    let missing_reference = format!("file.{}", temporary.path().join("missing").display());
    for (token, expected) in [
        ("literal.   ", "Model Armor bearer token cannot be empty"),
        ("unqualified-token", "unsupported secret reference"),
        (missing_reference.as_str(), "failed to read secret file"),
    ] {
        let mut config = managed_config();
        config
            .guardrails
            .managed_checks
            .get_mut("armor")
            .unwrap()
            .model_armor
            .as_mut()
            .unwrap()
            .auth = ModelArmorAuthConfig::BearerToken {
            token: token.into(),
        };
        let error = config
            .guardrail_engine()
            .err()
            .expect("invalid token must fail");
        assert!(error.to_string().contains(expected), "{error:#}");
    }
    let mut config = managed_config();
    config
        .guardrails
        .managed_checks
        .get_mut("bedrock")
        .unwrap()
        .bedrock
        .as_mut()
        .unwrap()
        .guardrail_version = "DRAFT".into();
    let error = config
        .guardrail_engine()
        .err()
        .expect("invalid version must fail");
    assert!(error.to_string().contains("numbered production version"));
}

#[tokio::test]
async fn configured_engine_uses_rotated_tokens_and_fails_closed_if_token_disappears() {
    use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
    let requests = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let app =
        Router::new()
            .route(
                "/v1/projects/project/locations/us-central1/templates/prompt:sanitizeUserPrompt",
                post(
                    |State(requests): State<Arc<std::sync::Mutex<Vec<String>>>>,
                     headers: HeaderMap| async move {
                        requests
                            .lock()
                            .unwrap()
                            .push(headers["authorization"].to_str().unwrap().into());
                        Json(serde_json::json!({"sanitizationResult": {
                            "invocationResult": "SUCCESS", "filterMatchState": "NO_MATCH_FOUND"
                        }}))
                    },
                ),
            )
            .with_state(requests.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let temporary = tempdir().unwrap();
    let token_path = temporary.path().join("token");
    std::fs::write(&token_path, "first-token\n").unwrap();
    let mut config = managed_config();
    let armor = config
        .guardrails
        .managed_checks
        .get_mut("armor")
        .unwrap()
        .model_armor
        .as_mut()
        .unwrap();
    armor.endpoint_url = Some(format!("http://{address}"));
    armor.auth = ModelArmorAuthConfig::BearerToken {
        token: format!("file.{}", token_path.display()),
    };
    let engine = config.guardrail_engine().expect("configured engine");
    let policy = PolicyResolver::new(&config.guardrails).resolve(PolicyTarget::Global);
    for token in ["first-token", "second-token"] {
        std::fs::write(&token_path, format!(" {token}\n")).unwrap();
        let evaluation = engine
            .evaluate(
                &policy,
                &config.guardrails,
                EvaluationInput::new(
                    GuardPhase::Prompt,
                    EvaluationPayload::Text {
                        text: "hello".into(),
                    },
                ),
            )
            .await;
        assert_eq!(evaluation.action, DecisionAction::Allow);
        assert!(
            evaluation
                .decisions
                .iter()
                .any(|decision| decision.reason_code.as_str() == "model_armor.allow")
        );
    }
    for empty_file in [true, false] {
        if empty_file {
            std::fs::write(&token_path, " \n").unwrap();
        } else {
            std::fs::remove_file(&token_path).unwrap();
        }
        let evaluation = engine
            .evaluate(
                &policy,
                &config.guardrails,
                EvaluationInput::new(
                    GuardPhase::Prompt,
                    EvaluationPayload::Text {
                        text: "hello".into(),
                    },
                ),
            )
            .await;
        assert_eq!(evaluation.action, DecisionAction::Deny);
        assert!(
            evaluation
                .decisions
                .iter()
                .any(|decision| decision.reason_code.as_str() == "managed.unavailable")
        );
    }
    assert_eq!(
        *requests.lock().unwrap(),
        ["Bearer first-token", "Bearer second-token"]
    );
    server.abort();
}

#[test]
fn route_validation_uses_configured_routes_and_defers_mcp_registry_validation() {
    let temporary = tempdir().unwrap();
    let config_path = temporary.path().join("gateway.yaml");
    let yaml = r#"
providers:
  - id: upstream
    type: openai_compat
    base_url: https://example.com/v1
    pricing_provider_id: openai
models:
  - id: public
    routes:
      - provider: upstream
        upstream_model: vendor/model
guardrails:
  model_routes:
    public/upstream/vendor/model: { enabled: true }
  mcp_servers:
    registered-server: { enabled: true }
"#;
    std::fs::write(&config_path, yaml).unwrap();
    let config =
        GatewayConfig::from_path(&config_path).expect("MCP registry validation is deferred");
    config
        .validate_guardrail_mcp_server_keys(&["registered-server".into()].into())
        .unwrap();
    let error = config
        .validate_guardrail_mcp_server_keys(&Default::default())
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "unknown guardrail MCP-server override `registered-server`"
    );
    std::fs::write(
        &config_path,
        yaml.replace("public/upstream/vendor/model:", "public/upstream/unknown:"),
    )
    .unwrap();
    let error = GatewayConfig::from_path(&config_path).unwrap_err();
    assert!(
        format!("{error:#}")
            .contains("unknown guardrail model-route override `public/upstream/unknown`")
    );
}
