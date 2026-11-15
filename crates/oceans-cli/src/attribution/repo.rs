//! Repository identity: normalised remote URLs and the git context of a working directory.

use std::{path::Path, process::Command, time::Duration};

use super::payload::is_commit_sha;
use crate::process::run_captured;

pub const MAX_REPOSITORY_LEN: usize = 256;
const GIT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_GIT_OUTPUT: usize = 64 * 1024;
/// Variables a git hook sets that would point `git -C <directory>` at the hook's repository.
const GIT_REPOSITORY_VARS: [&str; 6] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

/// Normalises a git remote to `host[:port]/org/repo`: no scheme, userinfo, default port or
/// `.git` suffix, and a lowercase host. Path case is kept. Local paths, unknown schemes and
/// anything the gateway's repository check would refuse return `None`, so credentials never
/// leave the machine.
pub fn normalize_repository(remote: &str) -> Option<String> {
    let remote = remote.trim();
    if remote.is_empty() || remote.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (authority, path, default_port) = match remote.split_once("://") {
        Some((scheme, rest)) => {
            let default_port = match scheme.to_ascii_lowercase().as_str() {
                "https" => 443,
                "http" => 80,
                "ssh" | "git+ssh" | "ssh+git" => 22,
                "git" => 9418,
                _ => return None,
            };
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
            (authority, path, Some(default_port))
        }
        None => {
            // scp-like `[user@]host:path`; git recognises it only without a slash before the colon.
            let (authority, path) = remote.split_once(':')?;
            if authority.contains('/') {
                return None;
            }
            (authority, path, None)
        }
    };
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let (host, port) = match default_port {
        Some(default_port) => {
            let (host, port) = split_host_port(host_port)?;
            let port = match port {
                Some(port) => Some(parse_port(port)?),
                None => None,
            };
            (host, port.filter(|port| *port != default_port))
        }
        None => (host_port, None),
    };
    let path = path.split(['?', '#']).next().unwrap_or_default();
    build(host, port, path)
}

/// Whether `value` is already a normalised repository.
pub fn is_repository(value: &str) -> bool {
    let Some((authority, path)) = value.split_once('/') else {
        return false;
    };
    let Some((host, port)) = split_host_port(authority) else {
        return false;
    };
    let port = match port.map(parse_port) {
        Some(None) => return false,
        port => port.flatten(),
    };
    build(host, port, path).is_some_and(|built| built == value)
}

fn build(host: &str, port: Option<u16>, path: &str) -> Option<String> {
    let mut repository = normalize_host(host)?;
    if let Some(port) = port {
        repository.push(':');
        repository.push_str(&port.to_string());
    }
    let mut segments: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    if let Some(last) = segments.last_mut() {
        *last = strip_git_suffix(last);
    }
    if segments.len() < 2 || !segments.iter().all(|segment| valid_segment(segment)) {
        return None;
    }
    for segment in segments {
        repository.push('/');
        repository.push_str(segment);
    }
    (repository.len() <= MAX_REPOSITORY_LEN).then_some(repository)
}

fn split_host_port(authority: &str) -> Option<(&str, Option<&str>)> {
    Some(match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    })
}

fn strip_git_suffix(segment: &str) -> &str {
    match segment.len().checked_sub(4) {
        Some(cut)
            if cut > 0
                && segment
                    .get(cut..)
                    .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".git")) =>
        {
            &segment[..cut]
        }
        _ => segment,
    }
}

fn parse_port(port: &str) -> Option<u16> {
    (!port.is_empty() && port.len() <= 5 && port.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| port.parse().ok())
        .flatten()
        .filter(|port| *port != 0)
}

/// DNS labels of `[a-z0-9-]`, as the gateway accepts. One letter is a Windows drive
/// (`C:\repo`), not a host.
fn normalize_host(host: &str) -> Option<String> {
    let host = host.to_ascii_lowercase();
    let valid = (2..=253).contains(&host.len())
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
    valid.then_some(host)
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'~' | b'+' | b'%')
        })
}

/// What a session reports about its working directory. Every field is optional: hooks run
/// outside repositories too.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct GitContext {
    pub repository: Option<String>,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
}

pub fn git_context(directory: &Path) -> GitContext {
    let remote = git(directory, &["remote", "get-url", "origin"]).or_else(|| {
        let remotes = git(directory, &["remote"])?;
        let first = remotes
            .lines()
            .map(str::trim)
            .find(|name| !name.starts_with('-'))?;
        git(directory, &["remote", "get-url", first])
    });
    let (head_sha, branch) = match git(directory, &["rev-parse", "HEAD", "--abbrev-ref", "HEAD"]) {
        Some(output) => {
            let mut lines = output.lines().map(str::to_owned);
            (lines.next(), lines.next())
        }
        // A repository without commits still has a branch.
        None => (
            None,
            git(directory, &["symbolic-ref", "--quiet", "--short", "HEAD"]),
        ),
    };
    GitContext {
        repository: remote.as_deref().and_then(normalize_repository),
        branch: branch.filter(|branch| !branch.is_empty() && branch != "HEAD"),
        head_sha: head_sha.filter(|sha| is_commit_sha(sha)),
    }
}

fn git(directory: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(args);
    for name in GIT_REPOSITORY_VARS {
        command.env_remove(name);
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
    let output = run_captured(&mut command, GIT_TIMEOUT, MAX_GIT_OUTPUT).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_normalise_to_host_and_path_without_credentials() {
        for (remote, expected) in [
            (
                "https://user:tok@GitHub.com/Acme/W.git",
                Some("github.com/Acme/W"),
            ),
            (
                "https://token@github.com/acme/widgets",
                Some("github.com/acme/widgets"),
            ),
            (
                "https://github.com/acme/widgets/",
                Some("github.com/acme/widgets"),
            ),
            (
                "https://github.com:443/acme/widgets.git",
                Some("github.com/acme/widgets"),
            ),
            (
                "http://127.0.0.1:3000/acme/widgets",
                Some("127.0.0.1:3000/acme/widgets"),
            ),
            ("git@github.com:acme/w.git", Some("github.com/acme/w")),
            ("GitHub.com:Acme/W", Some("github.com/Acme/W")),
            ("git@github.com:acme/W.GIT", Some("github.com/acme/W")),
            (
                "git@gitlab.example.com:/group/sub/repo.git",
                Some("gitlab.example.com/group/sub/repo"),
            ),
            ("ssh://git@host.example:22/a/b", Some("host.example/a/b")),
            (
                "ssh://git@host.example:2222/a/b.git",
                Some("host.example:2222/a/b"),
            ),
            (
                "git+ssh://git@github.com/acme/w.git",
                Some("github.com/acme/w"),
            ),
            ("git://host.xz/path/repo.git", Some("host.xz/path/repo")),
            (
                "https://dev.azure.com/org/My%20Project/_git/repo",
                Some("dev.azure.com/org/My%20Project/_git/repo"),
            ),
            (
                "https://gitlab.example.com/group/sub/repo.git?x=1",
                Some("gitlab.example.com/group/sub/repo"),
            ),
            ("", None),
            ("/srv/git/repo.git", None),
            ("../relative/repo", None),
            ("file:///srv/git/repo.git", None),
            ("C:\\repos\\widgets", None),
            ("C:/repos/widgets", None),
            ("https://github.com", None),
            ("https://github.com/acme", None),
            ("https://github.com/.git", None),
            ("https://github.com/@scope/widgets", None),
            ("https://github.com/acme/../widgets", None),
            ("https://github.com:99999/acme/widgets", None),
            ("https://git hub.com/acme/widgets", None),
            ("https://[2001:db8::1]:8443/acme/w", None),
        ] {
            assert_eq!(
                normalize_repository(remote).as_deref(),
                expected,
                "{remote}"
            );
            if let Some(expected) = expected {
                assert!(is_repository(expected), "{expected}");
            }
        }
        let long = format!("https://github.com/acme/{}", "a".repeat(300));
        assert_eq!(normalize_repository(&long), None);
    }

    #[test]
    fn only_normalised_repositories_validate() {
        for value in [
            "https://github.com/acme/w",
            "GitHub.com/acme/w",
            "github.com/acme/w.git",
            "user@github.com/acme/w",
            "github.com",
            "github.com/",
            "github.com:0/acme/w",
        ] {
            assert!(!is_repository(value), "{value}");
        }
    }

    #[test]
    fn git_context_reads_remote_branch_and_head() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path();
        let run = |args: &[&str]| {
            let mut command = Command::new("git");
            command
                .arg("-C")
                .arg(path)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1");
            for name in GIT_REPOSITORY_VARS {
                command.env_remove(name);
            }
            let status = command.output().unwrap();
            assert!(status.status.success(), "git {args:?}");
        };
        assert_eq!(git_context(path), GitContext::default());
        run(&["init", "--quiet", "--initial-branch=feature/x"]);
        run(&[
            "remote",
            "add",
            "origin",
            "https://user:tok@GitHub.com/Acme/Widgets.git",
        ]);
        let empty = git_context(path);
        assert_eq!(empty.repository.as_deref(), Some("github.com/Acme/Widgets"));
        assert_eq!(empty.branch.as_deref(), Some("feature/x"));
        assert_eq!(empty.head_sha, None);
        run(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "init",
        ]);
        let context = git_context(path);
        assert_eq!(context.branch.as_deref(), Some("feature/x"));
        assert!(context.head_sha.as_deref().is_some_and(is_commit_sha));
    }
}
