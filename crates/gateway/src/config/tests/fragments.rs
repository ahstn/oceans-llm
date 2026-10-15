use super::*;
use crate::config::{ConfigSources, render_config};

const MAIN: &str = r#"
providers:
  openai-prod:
    type: openai_compat
    base_url: https://api.openai.com/v1
    auth: { token: literal.test }
"#;

fn sources(dir: &Path) -> ConfigSources {
    ConfigSources {
        path: dir.join("gateway.yaml"),
        path_is_explicit: true,
        dir: Some(dir.join("conf.d")),
    }
}

fn write(dir: &Path, relative: &str, yaml: &str) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
    write_config(&path, yaml);
}

#[test]
fn merges_provider_and_model_fragments_from_nested_directories() {
    let tmp = tempdir().expect("tempdir");
    write(tmp.path(), "gateway.yaml", MAIN);
    write(
        tmp.path(),
        "conf.d/models/openai.yaml",
        "providers:\n  openai-secondary:\n    type: openai_compat\n    base_url: https://api.openai.com/v1\nmodels:\n  gpt-5:\n    route: { provider: openai-prod, upstream_model: gpt-5 }\n",
    );
    write(
        tmp.path(),
        "conf.d/models/multi-provider.yaml",
        "models:\n  gpt-5-pooled:\n    routes:\n      - { provider: openai-prod, upstream_model: gpt-5, weight: 9 }\n      - { provider: openai-secondary, upstream_model: gpt-5 }\n",
    );
    // Kubernetes ConfigMap mounts keep real files in dot directories; they must be skipped.
    write(
        tmp.path(),
        "conf.d/..2026_10_15/models/openai.yaml",
        "models:\n  gpt-5: { alias_of: gpt-5-pooled }\n",
    );
    write(tmp.path(), "conf.d/notes.txt", "not yaml");
    write(tmp.path(), "conf.d/empty.yaml", "# placeholder\n");

    let loaded = GatewayConfig::load(&sources(tmp.path())).expect("config should load");
    let model_ids = loaded
        .config
        .models
        .iter()
        .map(|model| model.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(model_ids, ["gpt-5-pooled", "gpt-5"], "lexical file order");
    let pooled = &loaded.config.seed_models().expect("seed models")[0];
    let route_keys = pooled
        .routes
        .iter()
        .map(|route| route.route_key.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(route_keys, [Some("openai-prod"), Some("openai-secondary")]);
    assert!(
        loaded
            .sources
            .iter()
            .any(|source| source.collection == "models"
                && source.id == "gpt-5"
                && source.path.ends_with("conf.d/models/openai.yaml"))
    );
}

#[test]
fn rejects_an_id_defined_in_two_files() {
    let tmp = tempdir().expect("tempdir");
    write(tmp.path(), "gateway.yaml", MAIN);
    write(
        tmp.path(),
        "conf.d/models/openai.yaml",
        "providers:\n  openai-prod:\n    type: openai_compat\n    base_url: https://api.openai.com/v1\n",
    );

    let error = GatewayConfig::load(&sources(tmp.path())).expect_err("duplicate provider");
    let message = format!("{error:#}");
    assert!(
        message.contains("provider `openai-prod` is defined in both")
            && message.contains("gateway.yaml")
            && message.contains("conf.d/models/openai.yaml"),
        "unexpected error: {message}"
    );
}

#[test]
fn fragments_may_only_contain_providers_and_models() {
    let tmp = tempdir().expect("tempdir");
    write(tmp.path(), "gateway.yaml", MAIN);
    write(
        tmp.path(),
        "conf.d/server.yaml",
        "server:\n  bind: 0.0.0.0:9000\n",
    );

    let error = GatewayConfig::load(&sources(tmp.path())).expect_err("server in a fragment");
    let message = format!("{error:#}");
    assert!(
        message.contains("conf.d/server.yaml") && message.contains("unknown field `server`"),
        "unexpected error: {message}"
    );
}

#[test]
fn fragment_references_resolve_after_merge() {
    let tmp = tempdir().expect("tempdir");
    write(tmp.path(), "gateway.yaml", MAIN);
    write(
        tmp.path(),
        "conf.d/models/multi-provider.yaml",
        "models:\n  pooled:\n    route: { provider: missing, upstream_model: gpt-5 }\n",
    );

    let error = GatewayConfig::load(&sources(tmp.path())).expect_err("unknown provider");
    let message = format!("{error:#}");
    assert!(
        message.contains("with fragments from")
            && message.contains("model `pooled` route references unknown provider `missing`"),
        "unexpected error: {message}"
    );
}

#[test]
fn explicit_paths_must_exist() {
    let tmp = tempdir().expect("tempdir");

    let error = GatewayConfig::load(&ConfigSources::file(tmp.path().join("missing.yaml")))
        .expect_err("missing explicit config");
    assert!(format!("{error:#}").contains("does not exist"));

    let implicit = GatewayConfig::from_path(&tmp.path().join("missing.yaml"))
        .expect("the implicit default path may be absent");
    assert!(implicit.models.is_empty());

    write(tmp.path(), "gateway.yaml", MAIN);
    let error = GatewayConfig::load(&sources(tmp.path())).expect_err("missing config dir");
    assert!(
        format!("{error:#}").contains("config directory"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn renders_merged_config_with_sources() {
    let tmp = tempdir().expect("tempdir");
    write(tmp.path(), "gateway.yaml", MAIN);
    write(
        tmp.path(),
        "conf.d/models/openai.yaml",
        "models:\n  - id: gpt-5\n    route: { provider: openai-prod, upstream_model: gpt-5 }\n",
    );

    let rendered = render_config(&sources(tmp.path()), true).expect("render");
    let value: serde_yaml::Value = serde_yaml::from_str(&rendered).expect("rendered yaml");
    assert!(value["models"]["gpt-5"]["route"]["provider"] == "openai-prod");
    assert!(value["models"]["gpt-5"].get("id").is_none());
    assert!(value["providers"]["openai-prod"]["base_url"] == "https://api.openai.com/v1");
    assert!(
        rendered.contains("#   models.gpt-5: ") && rendered.contains("conf.d/models/openai.yaml"),
        "{rendered}"
    );
}

#[test]
fn checked_in_config_dir_example_is_valid() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/examples/config-dir");
    let loaded = GatewayConfig::load(&ConfigSources {
        path: example.join("gateway.yaml"),
        path_is_explicit: true,
        dir: Some(example.join("conf.d")),
    })
    .expect("example config should load");
    let model_ids = loaded
        .config
        .models
        .iter()
        .map(|model| model.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        model_ids,
        [
            "claude-sonnet-4.6",
            "gpt-oss-120b",
            "gpt-6-astra",
            "gpt-fast",
            "gpt-5.6-luna"
        ]
    );
}

#[cfg(unix)]
#[test]
fn follows_symlinks_without_looping_or_reading_a_file_twice() {
    let tmp = tempdir().expect("tempdir");
    write(tmp.path(), "gateway.yaml", MAIN);
    write(
        tmp.path(),
        "conf.d/models/openai.yaml",
        "models:\n  gpt-5:\n    route: { provider: openai-prod, upstream_model: gpt-5 }\n",
    );
    let conf = tmp.path().join("conf.d");
    // A loop back to the root and a second alias for the same directory.
    std::os::unix::fs::symlink(&conf, conf.join("models/loop")).expect("loop symlink");
    std::os::unix::fs::symlink(conf.join("models"), conf.join("alias")).expect("alias symlink");
    write(tmp.path(), "conf.d/separators.yaml", "---\n---\n");

    let loaded = GatewayConfig::load(&sources(tmp.path())).expect("config should load");
    assert_eq!(loaded.config.models.len(), 1);
    render_config(&sources(tmp.path()), false).expect("render skips blank fragments");
}

#[test]
fn render_redacts_literal_secrets_headers_and_url_passwords() {
    let tmp = tempdir().expect("tempdir");
    write(
        tmp.path(),
        "gateway.yaml",
        r#"
providers:
  self-hosted:
    type: openai_compat
    base_url: https://oceans:hunter2@llm.internal/v1
    pricing_provider_id: openai
  openai-prod:
    type: openai_compat
    base_url: https://api.openai.com/v1
    default_headers:
      OpenAI-Organization: org-secret
    auth: { token: literal.sk-live-secret }
  openrouter:
    type: openai_compat
    base_url: https://openrouter.ai/api/v1
    auth: { token: env.OPENROUTER_API_KEY }
  query-key:
    type: openai_compat
    base_url: https://llm.example.com/v1?api_key=query-secret&region=eu
    pricing_provider_id: openai
  user-token:
    type: openai_compat
    base_url: https://token-secret@llm.example.net/v1
    pricing_provider_id: openai
"#,
    );
    write(tmp.path(), "conf.d/models.yaml", "models: {}\n");

    let rendered = render_config(&sources(tmp.path()), false).expect("render");
    for secret in [
        "sk-live-secret",
        "hunter2",
        "org-secret",
        "query-secret",
        "token-secret",
    ] {
        assert!(!rendered.contains(secret), "{secret} leaked:\n{rendered}");
    }
    assert!(rendered.contains("literal.<redacted>"), "{rendered}");
    assert!(rendered.contains("env.OPENROUTER_API_KEY"), "{rendered}");
    assert!(rendered.contains("llm.internal/v1"), "{rendered}");
    assert!(rendered.contains("api_key=%3Credacted%3E"), "{rendered}");
}
