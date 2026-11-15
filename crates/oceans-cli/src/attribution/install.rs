//! `oceans hook install`: registers the hooks with git-ai, Claude Code and OMP. Each target
//! is idempotent: running it again changes nothing, and a moved `oceans` binary is updated in
//! place where the target's format allows it.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{Context, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::{
    config::Env,
    process::{find_on_path, run_captured},
};

const GIT_AI_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_GIT_AI_OUTPUT: usize = 1024 * 1024;
const GIT_AI_HOOK: &str = "git_ai_hooks.post_notes_updated";
const CLAUDE_EVENTS: [&str; 2] = ["SessionStart", "SessionEnd"];
const OMP_EXTENSION: &str = include_str!("../../assets/omp/oceans-git-ai.ts");
const OMP_EXTENSION_FILE: &str = "oceans-git-ai.ts";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    Unchanged,
    Updated,
    Added,
}

/// Registers `<oceans> hook notes` as git-ai's `post_notes_updated` hook and restarts the
/// git-ai background service so it takes effect. The restart also runs when the hook is
/// already registered, so a retry after a failed restart still loads it.
pub fn git_ai(oceans: &str) -> anyhow::Result<Change> {
    let git_ai = find_on_path("git-ai").context("git-ai is not on PATH; install Git AI first")?;
    let command = format!("{oceans} hook notes");
    let output = run_git_ai(&git_ai, &["config", "git_ai_hooks"])?;
    if registered_hooks(&output)?.contains(&command) {
        run_git_ai(&git_ai, &["bg", "restart"])
            .context("git-ai bg restart failed; restart it to load the hook")?;
        return Ok(Change::Unchanged);
    }
    run_git_ai(&git_ai, &["config", "--add", GIT_AI_HOOK, &command])?;
    run_git_ai(&git_ai, &["bg", "restart"]).context(
        "registered the hook, but git-ai bg restart failed; restart it to load the hook",
    )?;
    Ok(Change::Added)
}

fn run_git_ai(git_ai: &Path, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let output = run_captured(
        Command::new(git_ai).args(args),
        GIT_AI_TIMEOUT,
        MAX_GIT_AI_OUTPUT,
    )
    .with_context(|| format!("git-ai {} could not run", args[..2].join(" ")))?;
    if !output.status.success() {
        bail!("git-ai {} failed ({})", args[..2].join(" "), output.status);
    }
    Ok(output.stdout)
}

/// The `post_notes_updated` commands in `git-ai config git_ai_hooks` output, a JSON object.
fn registered_hooks(output: &[u8]) -> anyhow::Result<Vec<String>> {
    let hooks: serde_json::Value = serde_json::from_slice(output)
        .context("git-ai printed its hooks in an unexpected shape")?;
    Ok(match hooks.get("post_notes_updated") {
        Some(serde_json::Value::String(command)) => vec![command.clone()],
        Some(serde_json::Value::Array(commands)) => commands
            .iter()
            .filter_map(|command| command.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    })
}

pub fn claude_settings_path(env: &Env) -> Option<PathBuf> {
    env.var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| env.home().map(|home| home.join(".claude")))
        .map(|directory| directory.join("settings.json"))
}

/// Adds `command` as a `SessionStart` and `SessionEnd` hook in Claude Code's settings,
/// keeping every other setting and hook in place.
pub fn claude(settings: &Path, command: &str) -> anyhow::Result<Change> {
    // Writing through a symlink keeps settings that live in a dotfiles repository.
    let target = match fs::canonicalize(settings) {
        Ok(target) => target,
        // A symlink to a missing file: writing here would replace the link with a file.
        Err(_) if fs::symlink_metadata(settings).is_ok_and(|meta| meta.is_symlink()) => bail!(
            "{} is a symlink to a file that doesn't exist; create that file, then run this again",
            settings.display()
        ),
        Err(_) => settings.to_path_buf(),
    };
    let (text, permissions) = match fs::read_to_string(&target) {
        Ok(text) => (text, Some(fs::metadata(&target)?.permissions())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (String::new(), None),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", target.display()));
        }
    };
    let mut document = if text.trim().is_empty() {
        IndexMap::new()
    } else {
        match serde_json::from_str(&text) {
            Ok(Json::Object(document)) => document,
            _ => bail!(
                "{} is not a JSON object; fix it, then run this again",
                target.display()
            ),
        }
    };
    let change = merge_claude_hooks(&mut document, command)
        .with_context(|| format!("could not add hooks to {}", target.display()))?;
    if change == Change::Unchanged {
        return Ok(change);
    }
    let mut updated = serde_json::to_string_pretty(&document)?;
    updated.push('\n');
    write_replacing(&target, updated.as_bytes(), permissions)?;
    Ok(change)
}

/// A JSON value that keeps object keys in file order, so rewriting the user's settings
/// doesn't reorder them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Json {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Json>),
    Object(IndexMap<String, Json>),
}

fn merge_claude_hooks(
    document: &mut IndexMap<String, Json>,
    command: &str,
) -> anyhow::Result<Change> {
    let hooks = document
        .entry("hooks".to_owned())
        .or_insert_with(|| Json::Object(IndexMap::new()));
    let Json::Object(hooks) = hooks else {
        bail!("hooks is not an object");
    };
    let mut change = Change::Unchanged;
    for event in CLAUDE_EVENTS {
        let groups = hooks
            .entry(event.to_owned())
            .or_insert_with(|| Json::Array(Vec::new()));
        let Json::Array(groups) = groups else {
            bail!("hooks.{event} is not a list");
        };
        change = change.max(merge_event(groups, command));
    }
    Ok(change)
}

fn merge_event(groups: &mut Vec<Json>, command: &str) -> Change {
    let mut change = None;
    let handlers = groups
        .iter_mut()
        .filter_map(|group| match group {
            Json::Object(group) => match group.get_mut("hooks") {
                Some(Json::Array(handlers)) => Some(handlers),
                _ => None,
            },
            _ => None,
        })
        .flatten();
    for handler in handlers {
        let Json::Object(handler) = handler else {
            continue;
        };
        let Some(Json::String(existing)) = handler.get_mut("command") else {
            continue;
        };
        if existing == command {
            change = change.max(Some(Change::Unchanged));
        } else if is_oceans_command(existing, " hook claude") {
            command.clone_into(existing);
            change = Some(Change::Updated);
        }
    }
    change.unwrap_or_else(|| {
        let handler = IndexMap::from([
            ("type".to_owned(), Json::String("command".into())),
            ("command".to_owned(), Json::String(command.into())),
        ]);
        let group =
            IndexMap::from([("hooks".to_owned(), Json::Array(vec![Json::Object(handler)]))]);
        groups.push(Json::Object(group));
        Change::Added
    })
}

/// Whether `command` runs an `oceans` binary with `suffix`, so an install from another path
/// replaces it instead of adding a second hook. The program must be one shell word, as
/// `process::shell_word` writes it, so a compound command that ends in an oceans hook is kept.
fn is_oceans_command(command: &str, suffix: &str) -> bool {
    let Some(program) = command.strip_suffix(suffix) else {
        return false;
    };
    let program = program.trim();
    let quoted = ['\'', '"'].into_iter().find_map(|quote| {
        let inner = program.strip_prefix(quote)?.strip_suffix(quote)?;
        (!inner.contains(quote)).then_some(inner)
    });
    let program = match quoted {
        Some(inner) => inner,
        None if program.contains(|c: char| c.is_whitespace() || "'\";&|<>()$`".contains(c)) => {
            return false;
        }
        None => program,
    };
    Path::new(program)
        .file_name()
        .is_some_and(|name| name == "oceans" || name == "oceans.exe")
}

/// `$PI_CODING_AGENT_DIR/extensions/oceans-git-ai.ts`, else `~/.omp/agent/extensions/…`.
pub fn omp_extension_path(env: &Env) -> Option<PathBuf> {
    env.var_os("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .or_else(|| env.home().map(|home| home.join(".omp").join("agent")))
        .map(|directory| directory.join("extensions").join(OMP_EXTENSION_FILE))
}

/// The OMP extension with both binary paths filled in. The extension spawns them without a
/// shell, so they are plain paths in string literals.
pub fn render_omp_extension(oceans: &str, git_ai: &str) -> String {
    let literal = |value: &str| serde_json::to_string(value).expect("strings serialize");
    OMP_EXTENSION
        .replace("\"__OCEANS_BIN__\"", &literal(oceans))
        .replace("\"__GIT_AI_BIN__\"", &literal(git_ai))
}

pub fn omp(path: &Path, contents: &str) -> anyhow::Result<Change> {
    let change = match fs::read_to_string(path) {
        Ok(existing) if existing == contents => return Ok(Change::Unchanged),
        Ok(_) => Change::Updated,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Change::Added,
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()));
        }
    };
    let directory = path
        .parent()
        .context("the extension path has no directory")?;
    fs::create_dir_all(directory)
        .with_context(|| format!("could not create {}", directory.display()))?;
    write_replacing(path, contents.as_bytes(), None)?;
    Ok(change)
}

/// Replaces `target` through a temporary file and a rename, keeping its permissions.
fn write_replacing(
    target: &Path,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
) -> anyhow::Result<()> {
    use std::io::Write;
    let directory = target.parent().context("the path has no directory")?;
    fs::create_dir_all(directory)
        .with_context(|| format!("could not create {}", directory.display()))?;
    let mut pending = tempfile::Builder::new()
        .prefix(".oceans-")
        .tempfile_in(directory)?;
    pending.write_all(bytes)?;
    if let Some(permissions) = permissions {
        fs::set_permissions(pending.path(), permissions)?;
    }
    pending.as_file().sync_all()?;
    pending
        .persist(target)
        .map_err(|error| error.error)
        .with_context(|| format!("could not write {}", target.display()))?;
    Ok(())
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;
