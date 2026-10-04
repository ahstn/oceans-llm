use serde_json::{Value, json};

use super::*;

fn dynamic_providers() -> Vec<Value> {
    let mut providers = vec![json!({
        "id": "cloud", "type": "aws_bedrock", "region": "us-east-1",
        "endpoint_kind": "bedrock_runtime", "auth": {"mode": "default_chain"}
    })];
    for auth in [
        json!({"mode": "adc"}),
        json!({"mode": "service_account", "credentials_path": "/tmp/does-not-need-to-exist.json"}),
    ] {
        providers.push(json!({
            "id": "cloud", "type": "gcp_vertex", "project_id": "test-project", "auth": auth
        }));
        providers.push(json!({
            "id": "cloud", "type": "gcp_cloud_run_openai_compat",
            "base_url": "https://cloud.example.com/v1", "pricing_provider_id": "google-vertex",
            "auth": auth
        }));
    }
    providers
}

fn config_for_provider(provider: Value) -> GatewayConfig {
    let mut route = json!({"id": "primary", "provider": "cloud", "upstream_model": "google/model"});
    if provider["type"] == "aws_bedrock" {
        route["compatibility"] = json!({"aws_bedrock": {"api_style": "runtime_converse"}});
        route["capabilities"] = json!({"responses": false, "json_schema": false});
    }
    serde_yaml::from_str(
        &json!({
            "providers": [provider],
            "models": [{"id": "pooled", "routing": {}, "routes": [route]}]
        })
        .to_string(),
    )
    .unwrap()
}

#[test]
fn dynamic_credentials_require_an_account_scope_only_in_routing_pools() {
    for mut provider in dynamic_providers() {
        let mut config = config_for_provider(provider.clone());
        let error = config.validate().expect_err("pool needs account scope");
        assert!(format!("{error:#}").contains("requires routing_account_scope"));

        config.models[0].routing = None;
        config
            .validate()
            .expect("legacy provider does not need account scope");
        assert!(!config.provider_routing_identity_enabled("cloud"));

        provider["routing_account_scope"] = json!("principal@example.com");
        let config = config_for_provider(provider);
        config
            .validate()
            .expect("explicit scope permits routed provider");
        assert!(config.provider_routing_identity_enabled("cloud"));
        assert_eq!(
            config.provider_routing_account_scope("cloud"),
            Some("principal@example.com")
        );
        assert_eq!(config.provider_routing_account_scope("unknown"), None);
    }
}

#[test]
fn resolved_bearer_and_static_credentials_do_not_require_account_scopes() {
    for mut provider in dynamic_providers() {
        provider["auth"] = json!({"mode": "bearer", "token": "literal.test-token"});
        config_for_provider(provider)
            .validate()
            .expect("resolved bearer identifies the credential");
    }
    let mut provider = dynamic_providers().remove(0);
    provider["auth"] = json!({
        "mode": "static_credentials", "access_key_id": "literal.test-key",
        "secret_access_key": "literal.test-secret"
    });
    config_for_provider(provider)
        .validate()
        .expect("resolved AWS keys identify the credential");
}

#[test]
fn configured_account_scopes_are_bounded_without_silently_trimming() {
    for scope in [
        "".to_string(),
        " a".into(),
        "a ".into(),
        "a\nb".into(),
        "a".repeat(257),
    ] {
        let mut provider = dynamic_providers().remove(0);
        provider["routing_account_scope"] = json!(scope);
        let error = config_for_provider(provider)
            .validate()
            .expect_err("invalid scope");
        assert!(format!("{error:#}").contains("routing_account_scope must contain 1 to 256 bytes"));
    }
    let mut provider = dynamic_providers().remove(0);
    provider["routing_account_scope"] = json!("a".repeat(256));
    config_for_provider(provider)
        .validate()
        .expect("maximum scope length");
}

#[test]
#[serial_test::serial]
fn runtime_identity_uses_resolved_credentials_even_when_the_reference_is_unchanged() {
    const KEY: &str = "OCEANS_TEST_ROUTING_IDENTITY_TOKEN";
    let _environment = super::environment::TestEnvironment::capture(&[KEY]);
    let config: GatewayConfig = serde_yaml::from_str(
        r#"
providers:
  - id: openai
    type: openai_compat
    base_url: https://example.com/v1
    pricing_provider_id: openai
    auth:
      kind: bearer
      token: env.OCEANS_TEST_ROUTING_IDENTITY_TOKEN
"#,
    )
    .unwrap();
    // The serial environment guard restores this test-only variable on exit.
    unsafe { std::env::set_var(KEY, "first-resolved-token") };
    let first = config
        .openai_compatible_provider_configs()
        .unwrap()
        .remove(0);
    let mut metadata_changed = config.clone();
    let crate::config::ProviderConfig::OpenAiCompat(provider) = &mut metadata_changed.providers[0]
    else {
        panic!("OpenAI fixture");
    };
    provider.pricing_provider_id = "openrouter".into();
    provider.display = Some(crate::config::ProviderDisplayConfig {
        label: Some("New display label".into()),
        icon_key: None,
    });
    let metadata_only = metadata_changed
        .openai_compatible_provider_configs()
        .unwrap()
        .remove(0);
    assert_eq!(
        crate::provider_routing_identity::openai_compat(&first).unwrap(),
        crate::provider_routing_identity::openai_compat(&metadata_only).unwrap()
    );
    unsafe { std::env::set_var(KEY, "second-resolved-token") };
    let second = config
        .openai_compatible_provider_configs()
        .unwrap()
        .remove(0);
    assert_ne!(
        crate::provider_routing_identity::openai_compat(&first).unwrap(),
        crate::provider_routing_identity::openai_compat(&second).unwrap()
    );
}
