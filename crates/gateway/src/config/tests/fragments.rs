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
  fragment-token:
    type: openai_compat
    base_url: https://llm.example.org/v1#fragment-secret
    pricing_provider_id: openai
  raw-token:
    type: openai_compat
    base_url: https://llm.example.com/v2
    pricing_provider_id: openai
    auth: { token: raw-secret }
"#,
    );
    let main = std::fs::read_to_string(tmp.path().join("gateway.yaml")).expect("read main");
    write(
        tmp.path(),
        "gateway.yaml",
        &format!(
            "{main}teams:\n  - id: platform\n    name: Platform\n    tags: [{{ key: owner, value: platform-team }}]\nservice_accounts:\n  - id: ci\n    team: platform\n    budget: {{ cadence: monthly, amount_usd: \"10\" }}\n    keys:\n      - id: primary\n        value: gwk_ci.service-account-secret\n"
        ),
    );
    write(tmp.path(), "conf.d/models.yaml", "models: {}\n");

    // Failure messages name the case, never the secret or the rendered text.
    let rendered = render_config(&sources(tmp.path()), false).expect("render");
    for (case, secret) in [
        ("literal token", "sk-live-secret"),
        ("URL password", "hunter2"),
        ("header value", "org-secret"),
        ("URL query value", "query-secret"),
        ("URL username", "token-secret"),
        ("URL fragment", "fragment-secret"),
        ("raw credential field", "raw-secret"),
        ("service-account key value", "service-account-secret"),
    ] {
        assert!(!rendered.contains(secret), "{case} was not redacted");
    }
    assert!(
        rendered.contains("literal.<redacted>"),
        "literal marker missing"
    );
    assert!(
        rendered.contains("platform-team"),
        "tag values outside service-account keys should stay"
    );
    assert!(
        rendered.contains("env.OPENROUTER_API_KEY"),
        "env reference should stay visible"
    );
    assert!(
        rendered.contains("llm.internal/v1"),
        "URL host and path should stay"
    );
    assert!(
        rendered.contains("api_key=%3Credacted%3E"),
        "query key should stay with a redacted value"
    );
}

#[cfg(unix)]
#[test]
fn rejects_symlinks_that_resolve_outside_the_config_directory() {
    let tmp = tempdir().expect("tempdir");
    write(tmp.path(), "gateway.yaml", MAIN);
    write(
        tmp.path(),
        "elsewhere/models.yaml",
        "models:\n  gpt-5:\n    route: { provider: openai-prod, upstream_model: gpt-5 }\n",
    );
    std::fs::create_dir_all(tmp.path().join("conf.d")).expect("conf.d");
    std::os::unix::fs::symlink(
        tmp.path().join("elsewhere/models.yaml"),
        tmp.path().join("conf.d/models.yaml"),
    )
    .expect("outside symlink");

    let error = GatewayConfig::load(&sources(tmp.path())).expect_err("outside symlink");
    assert!(
        format!("{error:#}").contains("outside the config directory"),
        "unexpected error: {error:#}"
    );
}

#[cfg(unix)]
#[test]
fn records_the_listed_path_while_reading_the_validated_target() {
    let tmp = tempdir().expect("tempdir");
    write(tmp.path(), "gateway.yaml", MAIN);
    write(
        tmp.path(),
        "conf.d/..data/models.yaml",
        "models:\n  gpt-5:\n    route: { provider: openai-prod, upstream_model: gpt-5 }\n",
    );
    // The Kubernetes ConfigMap layout: a visible name linking into the hidden data directory.
    std::os::unix::fs::symlink(
        tmp.path().join("conf.d/..data/models.yaml"),
        tmp.path().join("conf.d/models.yaml"),
    )
    .expect("data symlink");

    let loaded = GatewayConfig::load(&sources(tmp.path())).expect("config should load");
    assert_eq!(loaded.config.models.len(), 1);
    let source = loaded
        .sources
        .iter()
        .find(|source| source.id == "gpt-5")
        .expect("model source");
    assert!(
        source.path.ends_with("conf.d/models.yaml"),
        "sources name the listed path, not the symlink target: {}",
        source.path.display()
    );
}

#[test]
fn rendered_config_loads_back_to_the_same_models_and_providers() {
    // `render_config` merges raw YAML while `load` merges typed values. Loading the rendered
    // output must give the same result, so the two merges cannot drift apart.
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/examples/config-dir");
    let sources = ConfigSources {
        path: example.join("gateway.yaml"),
        path_is_explicit: true,
        dir: Some(example.join("conf.d")),
    };
    let loaded = GatewayConfig::load(&sources).expect("example config should load");
    let rendered = render_config(&sources, false).expect("render");

    let tmp = tempdir().expect("tempdir");
    let rendered_path = tmp.path().join("rendered.yaml");
    write_config(&rendered_path, &rendered);
    let reloaded =
        GatewayConfig::load(&ConfigSources::file(&rendered_path)).expect("rendered config");

    let model_seeds = |config: &GatewayConfig| {
        let mut seeds = config
            .seed_models()
            .expect("seed models")
            .into_iter()
            .map(|seed| format!("{seed:?}"))
            .collect::<Vec<_>>();
        seeds.sort();
        seeds
    };
    let providers = |config: &GatewayConfig| {
        let mut providers = config
            .providers
            .iter()
            .map(|provider| (provider.id().to_string(), provider.provider_type()))
            .collect::<Vec<_>>();
        providers.sort();
        providers
    };
    assert_eq!(model_seeds(&reloaded.config), model_seeds(&loaded.config));
    assert_eq!(providers(&reloaded.config), providers(&loaded.config));
}

#[test]
fn a_blank_main_file_is_the_default_config() {
    let tmp = tempdir().expect("tempdir");
    write(
        tmp.path(),
        "conf.d/models/openai.yaml",
        "providers:\n  openai-prod:\n    type: openai_compat\n    base_url: https://api.openai.com/v1\nmodels:\n  gpt-5:\n    route: { provider: openai-prod, upstream_model: gpt-5 }\n",
    );
    for main in ["", "# fragments hold everything\n", "---\n"] {
        write(tmp.path(), "gateway.yaml", main);
        let loaded = GatewayConfig::load(&sources(tmp.path()))
            .unwrap_or_else(|error| panic!("blank main {main:?}: {error:#}"));
        assert_eq!(loaded.config.models.len(), 1, "blank main {main:?}");
        render_config(&sources(tmp.path()), false).expect("render with a blank main file");
    }
}

#[test]
fn a_main_file_inside_the_config_directory_is_read_once() {
    let tmp = tempdir().expect("tempdir");
    write(
        tmp.path(),
        "gateway.yaml",
        &format!("server:\n  bind: 127.0.0.1:8080\n{MAIN}"),
    );
    write(
        tmp.path(),
        "models/openai.yaml",
        "models:\n  gpt-5:\n    route: { provider: openai-prod, upstream_model: gpt-5 }\n",
    );

    let loaded = GatewayConfig::load(&ConfigSources {
        path: tmp.path().join("gateway.yaml"),
        path_is_explicit: true,
        dir: Some(tmp.path().to_path_buf()),
    })
    .expect("main file is not also a fragment");
    assert_eq!(loaded.config.providers.len(), 1);
    assert_eq!(loaded.config.models.len(), 1);
}
