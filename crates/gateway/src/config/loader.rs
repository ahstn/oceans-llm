//! Loads the main config file plus optional `--config-dir` fragments.
//!
//! Fragments may only contribute `providers` and `models`. They merge with the main file as a
//! union keyed by id: an id defined in two places is an error that names both files, and no
//! file overrides another. Validation runs once on the merged config.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_yaml::{Mapping, Value};

use super::{
    GatewayConfig,
    collections::{deserialize_models, deserialize_providers, keyed_entries},
    models::ModelConfig,
    providers::ProviderConfig,
};

/// Where the gateway reads its configuration from.
#[derive(Debug, Clone)]
pub struct ConfigSources {
    pub path: PathBuf,
    /// Set when the path came from `--config` or `GATEWAY_CONFIG`. An explicit path must exist;
    /// only the implicit `./gateway.yaml` default may be absent.
    pub path_is_explicit: bool,
    pub dir: Option<PathBuf>,
}

impl ConfigSources {
    #[must_use]
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            path_is_explicit: true,
            dir: None,
        }
    }
}

/// The file that defined one provider or model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntitySource {
    pub collection: &'static str,
    pub id: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct LoadedConfig {
    pub config: GatewayConfig,
    pub sources: Vec<ConfigEntitySource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFragment {
    #[serde(default, deserialize_with = "deserialize_providers")]
    providers: Vec<ProviderConfig>,
    #[serde(default, deserialize_with = "deserialize_models")]
    models: Vec<ModelConfig>,
}

const FRAGMENT_COLLECTIONS: [&str; 2] = ["providers", "models"];

impl GatewayConfig {
    pub fn load(sources: &ConfigSources) -> anyhow::Result<LoadedConfig> {
        let mut config = read_main_file(sources)?;
        let mut tracker = SourceTracker::default();
        tracker.record_main(&config, &sources.path)?;

        if let Some(dir) = &sources.dir {
            for path in fragment_files(dir)? {
                let raw = fs::read_to_string(&path).with_context(|| {
                    format!("failed reading config fragment `{}`", path.display())
                })?;
                if is_blank_yaml(&raw) {
                    continue;
                }
                let fragment: ConfigFragment = serde_yaml::from_str(&raw).with_context(|| {
                    format!(
                        "failed parsing config fragment `{}`; fragments may only contain {}",
                        path.display(),
                        FRAGMENT_COLLECTIONS.join(" and ")
                    )
                })?;
                for provider in &fragment.providers {
                    tracker.record("providers", provider.id(), &path)?;
                }
                for model in &fragment.models {
                    tracker.record("models", &model.id, &path)?;
                }
                config.providers.extend(fragment.providers);
                config.models.extend(fragment.models);
            }
        }

        config.apply_inferred_defaults();
        config.validate().with_context(|| match &sources.dir {
            Some(dir) => format!(
                "invalid gateway configuration `{}` with fragments from `{}`",
                sources.path.display(),
                dir.display()
            ),
            None => format!("invalid gateway configuration `{}`", sources.path.display()),
        })?;

        Ok(LoadedConfig {
            config,
            sources: tracker.sources,
        })
    }

    /// Loads a single config file. A missing file yields the default config.
    pub fn from_path(path: &Path) -> anyhow::Result<Self> {
        Self::load(&ConfigSources {
            path: path.to_path_buf(),
            path_is_explicit: false,
            dir: None,
        })
        .map(|loaded| loaded.config)
    }
}

fn read_main_file(sources: &ConfigSources) -> anyhow::Result<GatewayConfig> {
    let path = &sources.path;
    if !path.exists() {
        if sources.path_is_explicit {
            bail!("gateway configuration `{}` does not exist", path.display());
        }
        return Ok(GatewayConfig::default());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed reading config file `{}`", path.display()))?;
    serde_yaml::from_str(&raw)
        .with_context(|| format!("failed parsing yaml config `{}`", path.display()))
}

/// YAML files under `dir`, recursively, in lexical order of their relative path.
///
/// Entries whose name starts with `.` are skipped. Kubernetes ConfigMap mounts place the real
/// files under `..data` and timestamped `..2026_...` directories and link to them from the
/// visible names, so skipping dot entries and following symlinks reads each file once.
fn fragment_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let metadata = fs::metadata(dir)
        .with_context(|| format!("config directory `{}` does not exist", dir.display()))?;
    if !metadata.is_dir() {
        bail!("config directory `{}` is not a directory", dir.display());
    }
    let root =
        fs::canonicalize(dir).with_context(|| format!("failed resolving `{}`", dir.display()))?;
    let mut files = Vec::new();
    let mut visited = BTreeSet::new();
    collect_fragment_files(&root, dir, &mut files, &mut visited)?;
    files.sort_by(|left, right| {
        left.strip_prefix(dir)
            .ok()
            .cmp(&right.strip_prefix(dir).ok())
    });
    Ok(files)
}

/// Canonicalizes `path` and rejects it when a symlink resolves outside the config directory.
/// Kubernetes ConfigMap mounts link only within the mount, so they stay inside `root`.
fn resolve_inside(root: &Path, path: &Path) -> anyhow::Result<PathBuf> {
    let canonical =
        fs::canonicalize(path).with_context(|| format!("failed resolving `{}`", path.display()))?;
    if !canonical.starts_with(root) {
        bail!(
            "config fragment `{}` resolves to `{}`, outside the config directory `{}`",
            path.display(),
            canonical.display(),
            root.display()
        );
    }
    Ok(canonical)
}

/// `visited` holds canonical paths, so a symlinked directory that points back at an ancestor is
/// not walked again, and a directory or file reachable through two aliases is read once.
fn collect_fragment_files(
    root: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
    visited: &mut BTreeSet<PathBuf>,
) -> anyhow::Result<()> {
    if !visited.insert(resolve_inside(root, dir)?) {
        return Ok(());
    }
    let entries =
        fs::read_dir(dir).with_context(|| format!("failed reading `{}`", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("failed reading `{}`", dir.display()))?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let metadata =
            fs::metadata(&path).with_context(|| format!("failed reading `{}`", path.display()))?;
        if metadata.is_dir() {
            collect_fragment_files(root, &path, files, visited)?;
        } else if metadata.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| matches!(extension, "yaml" | "yml"))
            && visited.insert(resolve_inside(root, &path)?)
        {
            files.push(path);
        }
    }
    Ok(())
}

fn is_blank_yaml(raw: &str) -> bool {
    raw.lines()
        .map(str::trim)
        .all(|line| line.is_empty() || line.starts_with('#') || line == "---")
}

#[derive(Default)]
struct SourceTracker {
    sources: Vec<ConfigEntitySource>,
    seen: BTreeMap<(&'static str, String), PathBuf>,
}

impl SourceTracker {
    fn record_main(&mut self, config: &GatewayConfig, path: &Path) -> anyhow::Result<()> {
        for provider in &config.providers {
            self.record("providers", provider.id(), path)?;
        }
        for model in &config.models {
            self.record("models", &model.id, path)?;
        }
        Ok(())
    }

    fn record(&mut self, collection: &'static str, id: &str, path: &Path) -> anyhow::Result<()> {
        let id = id.trim().to_string();
        let singular = collection.trim_end_matches('s');
        if let Some(existing) = self.seen.get(&(collection, id.clone())) {
            if existing == path {
                bail!("{singular} `{id}` is defined twice in `{}`", path.display());
            }
            bail!(
                "{singular} `{id}` is defined in both `{}` and `{}`",
                existing.display(),
                path.display()
            );
        }
        self.seen
            .insert((collection, id.clone()), path.to_path_buf());
        self.sources.push(ConfigEntitySource {
            collection,
            id,
            path: path.to_path_buf(),
        });
        Ok(())
    }
}

/// Prints the merged config as loaded, before inferred defaults are applied, with secrets
/// redacted (see [`redact_secrets`]). `providers` and
/// `models` are rendered in map form. With `show_sources`, a trailing comment block names the
/// file that defined each provider and model.
pub fn render_config(sources: &ConfigSources, show_sources: bool) -> anyhow::Result<String> {
    let loaded = GatewayConfig::load(sources)?;

    let mut merged = read_yaml_mapping(&sources.path)?;
    let mut collections: BTreeMap<&str, Mapping> = BTreeMap::new();
    take_collections(&mut merged, &mut collections)?;
    if let Some(dir) = &sources.dir {
        for path in fragment_files(dir)? {
            take_collections(&mut read_yaml_mapping(&path)?, &mut collections)?;
        }
    }
    for collection in FRAGMENT_COLLECTIONS {
        if let Some(entries) = collections.remove(collection) {
            merged.insert(
                Value::String(collection.to_string()),
                Value::Mapping(entries),
            );
        }
    }

    let mut merged = Value::Mapping(merged);
    redact_secrets(&mut merged, false);
    let mut rendered = serde_yaml::to_string(&merged)?;
    if show_sources {
        rendered.push_str("# sources:\n");
        for source in &loaded.sources {
            rendered.push_str(&format!(
                "#   {}.{}: {}\n",
                source.collection,
                source.id,
                source.path.display()
            ));
        }
    }
    Ok(rendered)
}

const REDACTED: &str = "<redacted>";

/// Fields that hold credentials. Their raw values are redacted even without a `literal.` prefix;
/// such values fail at runtime anyway, but `config render` must not print them.
const CREDENTIAL_FIELDS: &[&str] = &[
    "access_key_id",
    "api_key",
    "client_secret",
    "password",
    "private_key",
    "secret",
    "secret_access_key",
    "session_token",
    "token",
];

/// Replaces values that can carry credentials. Rendered output still contains identity data,
/// such as user emails, so it is not anonymized:
/// `literal.` secret references, raw values in credential fields, every header value, and URL
/// credentials (userinfo, query values, and fragments). `env.` and
/// `file.` references name where a secret lives without containing it, so they stay visible.
fn redact_secrets(value: &mut Value, in_headers: bool) {
    match value {
        Value::Mapping(mapping) => {
            for (key, entry) in mapping.iter_mut() {
                if let (Some(name), Value::String(text)) = (key.as_str(), &mut *entry)
                    && CREDENTIAL_FIELDS.contains(&name)
                    && !["env.", "file.", "literal."]
                        .iter()
                        .any(|prefix| text.starts_with(prefix))
                {
                    *text = REDACTED.to_string();
                    continue;
                }
                let headers = in_headers
                    || matches!(
                        key.as_str(),
                        Some("default_headers" | "extra_headers" | "headers")
                    );
                redact_secrets(entry, headers);
            }
        }
        Value::Sequence(items) => {
            for item in items {
                redact_secrets(item, false);
            }
        }
        Value::String(text) => {
            if in_headers {
                *text = REDACTED.to_string();
            } else if text.starts_with("literal.") {
                *text = format!("literal.{REDACTED}");
            } else if let Some(redacted) = redact_url_credentials(text) {
                *text = redacted;
            }
        }
        _ => {}
    }
}

/// Returns the URL with its username, password, every query value, and its fragment redacted,
/// or `None` when the text is not a URL or carries none of them. Query strings on endpoint URLs
/// are rare and often hold API keys, so all values are redacted rather than guessing names.
fn redact_url_credentials(text: &str) -> Option<String> {
    let mut url = url::Url::parse(text).ok()?;
    if url.cannot_be_a_base() {
        return None;
    }
    let mut changed = false;
    if !url.username().is_empty() {
        changed |= url.set_username(REDACTED).is_ok();
    }
    if url.password().is_some() {
        changed |= url.set_password(Some(REDACTED)).is_ok();
    }
    if url.query().is_some() {
        let keys = url
            .query_pairs()
            .map(|(key, _)| key.into_owned())
            .collect::<Vec<_>>();
        url.query_pairs_mut()
            .clear()
            .extend_pairs(keys.iter().map(|key| (key.as_str(), REDACTED)));
        changed = true;
    }
    if url.fragment().is_some() {
        url.set_fragment(Some(REDACTED));
        changed = true;
    }
    changed.then(|| url.to_string())
}

fn take_collections(
    document: &mut Mapping,
    collections: &mut BTreeMap<&'static str, Mapping>,
) -> anyhow::Result<()> {
    for collection in FRAGMENT_COLLECTIONS {
        let Some(value) = document.remove(collection) else {
            continue;
        };
        let entries = collections.entry(collection).or_default();
        for (id, entry) in keyed_entries(value, collection)? {
            if let Value::Mapping(mut fields) = entry {
                fields.remove("id");
                entries.insert(Value::String(id), Value::Mapping(fields));
            }
        }
    }
    Ok(())
}

fn read_yaml_mapping(path: &Path) -> anyhow::Result<Mapping> {
    if !path.exists() {
        return Ok(Mapping::new());
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed reading `{}`", path.display()))?;
    // Match `GatewayConfig::load`, which skips comment-only and `---`-only fragments.
    if is_blank_yaml(&raw) {
        return Ok(Mapping::new());
    }
    match serde_yaml::from_str(&raw)
        .with_context(|| format!("failed parsing yaml `{}`", path.display()))?
    {
        Value::Mapping(mapping) => Ok(mapping),
        Value::Null => Ok(Mapping::new()),
        _ => bail!("`{}` must contain a YAML mapping", path.display()),
    }
}
