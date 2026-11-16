use super::*;

fn config_with_provider(fields: &str) -> anyhow::Result<GatewayConfig> {
    let tmp = tempdir().expect("tempdir");
    let path = tmp.path().join("gateway.yaml");
    let credentials = if fields.contains("credentials_file:") {
        ""
    } else {
        "    credentials_file: ~/.config/oceans/chatgpt.json\n"
    };
    write_config(
        &path,
        &format!(
            "providers:\n  - id: chatgpt\n    type: chatgpt_plan\n{credentials}{fields}\nmodels:\n  - id: chatgpt/gpt-6-luna\n    routes:\n      - provider: chatgpt\n        upstream_model: gpt-6-luna\n        capabilities:\n          responses: true\n          stream: true\n"
        ),
    );
    GatewayConfig::from_path(&path)
}

#[test]
fn chatgpt_plan_defaults_seed_mutable_path_without_secrets() {
    let config = config_with_provider("").expect("config");
    let providers = config.seed_providers().expect("seed");
    assert_eq!(providers[0].provider_type, "chatgpt_plan");
    assert_eq!(providers[0].config["pricing_provider_id"], "openai");
    assert_eq!(providers[0].config["base_url"], "https://api.openai.com/v1");
    assert_eq!(providers[0].config["strip_unsupported_fields"], true);
    assert_eq!(
        providers[0].config["credentials_file"],
        "~/.config/oceans/chatgpt.json"
    );
    assert!(providers[0].secrets.is_none());
    let runtime = config.chatgpt_plan_provider_configs().expect("runtime");
    assert_eq!(runtime.len(), 1);
    assert_eq!(runtime[0].provider_key, "chatgpt");
    let home = std::env::var_os("HOME").expect("HOME");
    assert_eq!(
        runtime[0].credentials_file,
        std::path::PathBuf::from(home).join(".config/oceans/chatgpt.json")
    );
    assert_eq!(runtime[0].request_timeout_ms, 300_000);
    assert!(
        config
            .openai_compatible_provider_configs()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn chatgpt_plan_projects_explicit_options() {
    let config = config_with_provider(
        "    base_url: http://127.0.0.1:38992/v1/\n    strip_unsupported_fields: false\n    pricing_provider_id: openai\n    default_headers:\n      x-test: mock\n    timeouts:\n      total_ms: 15000\n    display:\n      label: My ChatGPT subscription\n      icon_key: openai",
    )
    .expect("config");
    let runtime = config.chatgpt_plan_provider_configs().expect("runtime");
    assert_eq!(runtime[0].base_url, "http://127.0.0.1:38992/v1");
    assert!(!runtime[0].strip_unsupported_fields);
    assert_eq!(runtime[0].default_headers["x-test"], "mock");
    assert_eq!(runtime[0].request_timeout_ms, 15_000);
}

#[test]
fn chatgpt_plan_rejects_invalid_configuration() {
    for (fields, expected) in [
        ("    base_url: http://api.openai.com/v1", "must use https"),
        (
            "    base_url: https://api.openai.com/v1?key=x",
            "query parameters",
        ),
        ("    base_url: https://api.openai.com/v1#x", "fragments"),
        ("    pricing_provider_id: unsupported", "is not supported"),
        (
            "    pricing_provider_id: ''",
            "pricing_provider_id cannot be empty",
        ),
        (
            "    default_headers:\n      Authorization: Bearer other",
            "cannot set authorization",
        ),
        (
            "    default_headers:\n      AUTHORIZATION: Bearer other",
            "cannot set authorization",
        ),
        (
            "    credentials_file: ''",
            "credentials_file cannot be empty",
        ),
    ] {
        let error = config_with_provider(fields).expect_err("invalid config");
        assert!(format!("{error:#}").contains(expected), "{error:#}");
    }
}

#[test]
fn chatgpt_plan_credentials_path_is_not_a_secret_reference() {
    let mut config = config_with_provider("").expect("config");
    let super::super::ProviderConfig::ChatGptPlan(provider) = &mut config.providers[0] else {
        panic!("chatgpt_plan provider");
    };
    provider.credentials_file = "env.NOT_A_SECRET_REFERENCE".into();
    let runtime = config.chatgpt_plan_provider_configs().expect("plain path");
    assert_eq!(
        runtime[0].credentials_file,
        std::path::PathBuf::from("env.NOT_A_SECRET_REFERENCE")
    );
}
