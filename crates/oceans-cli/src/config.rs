//! Gateway URL and API key resolution. See the ADR section "CLI configuration and key
//! resolution" in `docs/adr/2026-11-01-git-ai-attribution-via-oceans-hooks.md`.
//!
//! Precedence: command-line flag, then `OCEANS_URL` / `OCEANS_API_KEY`, then the config file
//! (`$XDG_CONFIG_HOME/oceans/config.yaml`, else `~/.config/oceans/config.yaml`).

use std::{
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, anyhow, bail};
use serde_yaml::Value;
use url::Url;

use crate::process::{self, RunError};

pub const DEFAULT_URL: &str = "http://127.0.0.1:8080";
/// How long an `api_key: "!command"` may run before resolution fails.
pub const KEY_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_KEY_COMMAND_OUTPUT: usize = 64 * 1024;
const NO_KEY: &str = "no Oceans API key: run oceans login, set OCEANS_API_KEY, or pass --api-key";

/// Environment lookups. Tests use a fixed map instead of the process environment.
#[derive(Clone, Debug)]
pub enum Env {
    Process,
    #[cfg(test)]
    Fixed {
        vars: std::collections::BTreeMap<String, OsString>,
        home: Option<PathBuf>,
    },
}

impl Env {
    #[cfg(test)]
    pub fn fixed(vars: &[(&str, &str)], home: Option<&Path>) -> Self {
        Self::Fixed {
            vars: vars
                .iter()
                .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
                .collect(),
            home: home.map(Path::to_path_buf),
        }
    }

    /// A variable's value; unset and empty are the same.
    pub fn var_os(&self, name: &str) -> Option<OsString> {
        let value = match self {
            Self::Process => std::env::var_os(name),
            #[cfg(test)]
            Self::Fixed { vars, .. } => vars.get(name).cloned(),
        };
        value.filter(|value| !value.is_empty())
    }

    pub fn var(&self, name: &str) -> Option<String> {
        self.var_os(name).and_then(|value| value.into_string().ok())
    }

    /// A directory variable. Relative values are ignored, as the XDG base directory spec says.
    pub fn dir_var(&self, name: &str) -> Option<PathBuf> {
        self.var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    }

    pub fn home(&self) -> Option<PathBuf> {
        let home = match self {
            Self::Process => std::env::home_dir(),
            #[cfg(test)]
            Self::Fixed { home, .. } => home.clone(),
        };
        home.filter(|path| path.is_absolute())
    }
}

/// Values given on the command line, which win over everything else.
#[derive(Clone, Copy, Default)]
pub struct Overrides<'a> {
    pub url: Option<&'a Url>,
    pub api_key: Option<&'a str>,
}

#[derive(Debug, Default, PartialEq)]
pub struct ConfigFile {
    pub url: Option<String>,
    pub api_key: Option<String>,
}

pub struct Connection {
    pub url: Url,
    /// Secret: never log or display it.
    pub api_key: String,
}

pub fn config_path(env: &Env) -> Option<PathBuf> {
    env.dir_var("XDG_CONFIG_HOME")
        .or_else(|| env.home().map(|home| home.join(".config")))
        .map(|directory| directory.join("oceans").join("config.yaml"))
}

/// Resolves the gateway URL and API key, running an `api_key` command when the file has one.
/// The file is read only when a flag or variable doesn't already supply a value.
pub fn resolve_connection(
    overrides: &Overrides<'_>,
    env: &Env,
    key_timeout: Duration,
) -> anyhow::Result<Connection> {
    let env_url = env.var("OCEANS_URL");
    let env_key = env.var("OCEANS_API_KEY");
    let needs_file = (overrides.url.is_none() && env_url.is_none())
        || (overrides.api_key.is_none() && env_key.is_none());
    let path = config_path(env);
    let file = match (&path, needs_file) {
        (Some(path), true) => read_config(path)?,
        _ => None,
    };
    let url = resolve_url(
        overrides,
        env_url.as_deref(),
        file.as_ref(),
        path.as_deref(),
    )?;
    let api_key = match (overrides.api_key, env_key) {
        (Some(key), _) => literal_key(key)?,
        (None, Some(key)) => literal_key(&key)?,
        (None, None) => {
            let value = file.and_then(|file| file.api_key).context(NO_KEY)?;
            let path = path.expect("a config file was read");
            KeySource::parse(&value)
                .resolve(env, key_timeout)
                .with_context(|| format!("could not resolve api_key from {}", path.display()))?
        }
    };
    Ok(Connection { url, api_key })
}

/// The gateway URL alone: flag, then `OCEANS_URL`, then the config file, then the default.
pub fn resolve_url(
    overrides: &Overrides<'_>,
    env_url: Option<&str>,
    file: Option<&ConfigFile>,
    path: Option<&Path>,
) -> anyhow::Result<Url> {
    if let Some(url) = overrides.url {
        return Ok(url.clone());
    }
    if let Some(url) = env_url {
        return Url::parse(url).map_err(|_| anyhow!("OCEANS_URL is not a valid URL"));
    }
    if let Some(url) = file.and_then(|file| file.url.as_deref()) {
        let path = path.map_or_else(String::new, |path| path.display().to_string());
        return Url::parse(url).map_err(|_| anyhow!("url in {path} is not a valid URL"));
    }
    Ok(Url::parse(DEFAULT_URL).expect("default URL is valid"))
}

fn literal_key(key: &str) -> anyhow::Result<String> {
    let key = key.trim();
    if key.is_empty() {
        bail!("the Oceans API key is empty");
    }
    Ok(key.to_owned())
}

/// Where an `api_key` value comes from. Resolution order follows the ADR: `!command`, then
/// `env:NAME`, then a literal key.
#[derive(Debug, PartialEq)]
pub enum KeySource {
    Command(String),
    Env(String),
    Literal(String),
}

impl KeySource {
    pub fn parse(value: &str) -> Self {
        if let Some(command) = value.strip_prefix('!') {
            Self::Command(command.trim().to_owned())
        } else if let Some(name) = value.strip_prefix("env:") {
            Self::Env(name.trim().to_owned())
        } else {
            Self::Literal(value.to_owned())
        }
    }

    /// Resolves the key. Errors never include the key or a command's output.
    pub fn resolve(&self, env: &Env, timeout: Duration) -> anyhow::Result<String> {
        match self {
            Self::Literal(key) => literal_key(key),
            Self::Env(name) => {
                validate_env_name(name)?;
                let value = env.var_os(name).with_context(|| {
                    format!(
                        "api_key refers to environment variable {name}, which is unset or empty"
                    )
                })?;
                let value = value
                    .into_string()
                    .map_err(|_| anyhow!("environment variable {name} is not valid UTF-8"))?;
                literal_key(&value)
                    .with_context(|| format!("environment variable {name} holds no key"))
            }
            Self::Command(command) => {
                let output = run_key_command(command, timeout)?;
                literal_key(&output).context("the api_key command printed no key")
            }
        }
    }
}

pub fn validate_env_name(name: &str) -> anyhow::Result<()> {
    let mut bytes = name.bytes();
    let valid = bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if !valid {
        bail!("environment variable names use letters, digits and underscores");
    }
    Ok(())
}

fn run_key_command(command: &str, timeout: Duration) -> anyhow::Result<String> {
    if command.is_empty() {
        bail!("the api_key command is empty");
    }
    let output = process::run_captured(
        &mut process::shell_command(command),
        timeout,
        MAX_KEY_COMMAND_OUTPUT,
    )
    .map_err(|error| match error {
        RunError::TimedOut => anyhow!(
            "the api_key command didn't finish within {} seconds",
            timeout.as_secs()
        ),
        RunError::OutputTooLarge => anyhow!("the api_key command printed more than 64 KiB"),
        RunError::NotFound | RunError::Spawn(_) | RunError::Io(_) => {
            anyhow!("the api_key command could not run")
        }
    })?;
    if !output.status.success() {
        // Only the exit status: stdout could hold a partial secret.
        bail!("the api_key command failed ({})", output.status);
    }
    String::from_utf8(output.stdout)
        .map_err(|_| anyhow!("the api_key command printed invalid UTF-8"))
}

/// Reads the config file. A missing file is `None`.
pub fn read_config(path: &Path) -> anyhow::Result<Option<ConfigFile>> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("could not open {}", path.display()));
        }
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    ensure_not_shared(path, &metadata)?;
    let mut text = String::new();
    (&mut file)
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut text)
        .with_context(|| format!("could not read {} as UTF-8", path.display()))?;
    if text.len() as u64 > MAX_CONFIG_BYTES {
        bail!("{} is larger than 64 KiB", path.display());
    }
    parse_config(&text)
        .map(Some)
        .with_context(|| format!("invalid config file {}", path.display()))
}

/// A file others can write to, or one another user owns, could swap in a command that runs
/// on every commit, or point a key at another gateway, so none of its values are used.
#[cfg(unix)]
fn ensure_not_shared(path: &Path, metadata: &fs::Metadata) -> anyhow::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    // SAFETY: geteuid has no preconditions and cannot fail.
    ensure_owned(path, metadata.uid(), unsafe { libc::geteuid() })?;
    if metadata.permissions().mode() & 0o022 != 0 {
        bail!(
            "{} is writable by other users, so oceans won't use it; run chmod 600 {}",
            path.display(),
            path.display()
        );
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_owned(path: &Path, file_uid: u32, current_uid: u32) -> anyhow::Result<()> {
    if file_uid != current_uid {
        bail!(
            "{} belongs to another user, so oceans won't use it",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_not_shared(_path: &Path, _metadata: &fs::Metadata) -> anyhow::Result<()> {
    Ok(())
}

fn parse_config(text: &str) -> anyhow::Result<ConfigFile> {
    let document: Value = serde_yaml::from_str(text).map_err(|error| anyhow!("{error}"))?;
    let mapping = match document {
        Value::Null => return Ok(ConfigFile::default()),
        Value::Mapping(mapping) => mapping,
        _ => bail!("the config file must be a YAML mapping"),
    };
    let mut config = ConfigFile::default();
    for (key, value) in mapping {
        let Value::String(key) = key else {
            bail!("config keys must be strings");
        };
        let slot = match key.as_str() {
            "url" => &mut config.url,
            "api_key" => &mut config.api_key,
            other => bail!(
                "unknown setting `{}`; the file supports url and api_key",
                other.escape_default()
            ),
        };
        *slot = setting(&key, value)?;
    }
    Ok(config)
}

fn setting(key: &str, value: Value) -> anyhow::Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value)),
        // An unquoted `!fnox get KEY` is a YAML tag, not a string.
        Value::Tagged(tagged) => {
            let mut intended = tagged.tag.to_string();
            match tagged.value {
                Value::String(rest) if !rest.is_empty() => {
                    intended.push(' ');
                    intended.push_str(&rest);
                }
                Value::Null | Value::String(_) => {}
                other => {
                    if let Ok(rest) = serde_yaml::to_string(&other) {
                        intended.push(' ');
                        intended.push_str(rest.trim_end());
                    }
                }
            }
            bail!(
                "{key} starts with `!`, which YAML reads as a tag; quote the whole value: {key}: {}",
                yaml_string(&intended)
            )
        }
        _ => bail!("{key} must be a string"),
    }
}

/// A double-quoted scalar. JSON string escapes are valid YAML.
fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).expect("strings serialize")
}

/// Writes the config file with owner-only permissions, replacing any existing file.
pub fn write_config(path: &Path, url: &Url, api_key: &str) -> anyhow::Result<()> {
    let directory = path.parent().context("the config path has no directory")?;
    create_private_dir(directory)
        .with_context(|| format!("could not create {}", directory.display()))?;
    let contents = format!(
        "# Written by `oceans login`. See https://oceans-llm.com/harnesses/oceans-cli/\nurl: {}\napi_key: {}\n",
        yaml_string(url.as_str()),
        yaml_string(api_key)
    );
    // tempfile creates owner-only (0600) files on Unix.
    let mut pending = tempfile::NamedTempFile::new_in(directory)?;
    pending.write_all(contents.as_bytes())?;
    pending.as_file().sync_all()?;
    pending
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn create_private_dir(directory: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_dir(directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
