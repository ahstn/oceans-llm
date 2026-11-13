use std::{ffi::OsString, path::PathBuf, time::Duration};

use clap::{Args, Parser, Subcommand};
use url::Url;

/// Hooks that git-ai, Claude Code and OMP run. They never fail their caller.
const HOOK_ENTRYPOINTS: [&str; 5] = ["notes", "claude", "omp", "prs", "send"];

#[derive(Parser)]
#[command(
    name = "oceans",
    version,
    about = "Use Oceans LLM skills, providers and Git AI attribution"
)]
pub struct Cli {
    /// Gateway URL. Overrides OCEANS_URL and the config file; the default is
    /// http://127.0.0.1:8080. HTTPS is required except for loopback IP addresses.
    #[arg(long, global = true, value_name = "URL")]
    pub url: Option<Url>,

    /// Oceans API key. Overrides OCEANS_API_KEY and the config file. Other local users can
    /// see command-line arguments, so prefer those.
    #[arg(long, global = true, value_name = "KEY")]
    pub api_key: Option<String>,

    /// Print structured results as JSON.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Browse, upload, and install authenticated skills.
    #[command(subcommand)]
    Skills(SkillsCommand),
    /// Manage your provider credentials on the gateway.
    #[command(subcommand)]
    Providers(ProvidersCommand),
    /// Check an API key and save the gateway URL and key source to the config file.
    Login(LoginArgs),
    /// Git AI attribution hooks, their queue and their installation.
    #[command(subcommand)]
    Hook(HookCommand),
}

#[derive(Args)]
pub struct LoginArgs {
    /// Save a command that prints the key, such as "fnox get OCEANS_API_KEY". It runs
    /// whenever a key is needed, so the key itself is never saved.
    #[arg(long, value_name = "COMMAND", conflicts_with = "api_key_env")]
    pub api_key_command: Option<String>,
    /// Save the name of an environment variable that holds the key.
    #[arg(long, value_name = "NAME")]
    pub api_key_env: Option<String>,
}

#[derive(Subcommand)]
pub enum HookCommand {
    /// git-ai post_notes_updated hook: queue the notes on standard input.
    Notes,
    /// Claude Code SessionStart and SessionEnd hook: queue the event on standard input.
    Claude,
    /// OMP extension hook: queue the session event on standard input.
    Omp,
    /// Queue recently merged pull requests and their commits, read with gh.
    Prs(PrsArgs),
    /// Deliver queued entries to the gateway.
    Send(SendArgs),
    /// List queued and quarantined entries, or resend or delete quarantined ones.
    Queue(QueueArgs),
    /// Register the hooks with git-ai, Claude Code or OMP.
    Install(InstallArgs),
}

#[derive(Args)]
pub struct PrsArgs {
    /// Repository directory. Defaults to the current directory.
    #[arg(long, value_name = "DIR")]
    pub repo_dir: Option<PathBuf>,
    /// How many merged pull requests to read, newest first.
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u32).range(1..=1000))]
    pub limit: u32,
    /// Skip the sync if this repository synced more recently, such as 30s, 15m, 1h or 2d.
    #[arg(long, value_name = "AGE", value_parser = parse_age)]
    pub if_stale: Option<Duration>,
}

#[derive(Args)]
pub struct SendArgs {
    /// Wait for delivery and report the result. Without it, problems are only logged.
    #[arg(long)]
    pub foreground: bool,
}

#[derive(Args)]
pub struct QueueArgs {
    /// Move quarantined entries back to the queue and send them.
    #[arg(long, conflicts_with = "discard")]
    pub resend: bool,
    /// Delete quarantined entries.
    #[arg(long)]
    pub discard: bool,
}

#[derive(Args)]
#[group(required = true, multiple = true)]
pub struct InstallArgs {
    /// Register `oceans hook notes` as git-ai's post_notes_updated hook.
    #[arg(long)]
    pub git_ai: bool,
    /// Add `oceans hook claude` as a SessionStart and SessionEnd hook in Claude Code.
    #[arg(long)]
    pub claude: bool,
    /// Install the OMP extension that reports sessions and marks edits for git-ai.
    #[arg(long)]
    pub omp: bool,
}

/// `30s`, `15m`, `1h` or `2d`.
fn parse_age(value: &str) -> Result<Duration, String> {
    const USAGE: &str = "use a whole number with s, m, h or d, such as 1h";
    let split = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: u64 = number.parse().map_err(|_| USAGE.to_string())?;
    let seconds = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return Err(USAGE.into()),
    };
    number
        .checked_mul(seconds)
        .map(Duration::from_secs)
        .ok_or_else(|| USAGE.into())
}

#[derive(Subcommand)]
pub enum ProvidersCommand {
    /// Store your GitHub token for a Copilot provider's GitHub user authentication.
    SetCopilotToken(CopilotTokenArgs),
}

#[derive(Args)]
pub struct CopilotTokenArgs {
    /// GitHub token. Prefer the hidden prompt or --token-stdin to avoid shell history.
    #[arg(value_name = "TOKEN", conflicts_with = "token_stdin")]
    pub token: Option<String>,
    /// Read the token from standard input instead of the hidden prompt.
    #[arg(long)]
    pub token_stdin: bool,
    /// Configured provider key. Required when more than one Copilot provider exists.
    #[arg(long, value_name = "KEY")]
    pub provider: Option<String>,
}

impl Cli {
    pub fn parse_safe() -> Self {
        let args: Vec<OsString> = std::env::args_os().collect();
        let hook = is_hook_entrypoint(&args);
        Self::try_parse_safe_from(args).unwrap_or_else(|error| {
            let informational = matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            );
            if hook && !informational {
                // A hook's caller must never see it fail, even when an upgrade changed its flags.
                let _ = error.print();
                std::process::exit(0);
            }
            error.exit()
        })
    }

    fn try_parse_safe_from(
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Self, clap::Error> {
        let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        let provider_credentials = args
            .iter()
            .any(|value| value == "providers" || value == "set-copilot-token");
        let api_key = args.iter().any(|value| {
            value == "login"
                || value == "--api-key"
                || value.to_string_lossy().starts_with("--api-key=")
        });
        Self::try_parse_from(args).map_err(|error| {
            if matches!(error.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion) {
                error
            } else if provider_credentials {
                // Clap can include a rejected positional token in its diagnostics.
                clap::Error::raw(error.kind(), "invalid provider command arguments; run oceans providers set-copilot-token --help for usage")
            } else if api_key {
                // Clap can include a rejected key or key command in its diagnostics.
                clap::Error::raw(error.kind(), "invalid arguments; run oceans --help for usage")
            } else {
                error
            }
        })
    }
}

/// Whether `args` run a hook entrypoint: the first `hook` followed, after any global options,
/// by one of `HOOK_ENTRYPOINTS`.
fn is_hook_entrypoint(args: &[OsString]) -> bool {
    let mut rest = args.iter().skip(1).skip_while(|arg| *arg != "hook").skip(1);
    while let Some(arg) = rest.next() {
        if arg == "--url" || arg == "--api-key" {
            rest.next();
            continue;
        }
        let text = arg.to_string_lossy();
        if arg == "--json" || text.starts_with("--url=") || text.starts_with("--api-key=") {
            continue;
        }
        return HOOK_ENTRYPOINTS.iter().any(|name| arg == *name);
    }
    false
}

#[derive(Subcommand)]
pub enum SkillsCommand {
    /// Show your namespace, or register an immutable handle once.
    Namespace { handle: Option<String> },
    /// List skills that authenticated callers can read.
    List {
        #[arg(long)]
        namespace: Option<String>,
    },
    /// Show a skill and its selected version.
    Show(VersionArgs),
    /// Upload a skill directory or ZIP to your namespace; append if it exists.
    Upload {
        path: PathBuf,
        /// Skip the upload when its content is identical to the latest version.
        #[arg(long)]
        skip_unchanged: bool,
    },
    /// List the immutable versions of a skill.
    Versions { skill: String },
    /// Change the default version of a skill you own.
    SetDefault {
        skill: String,
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        version: u32,
    },
    /// Download and verify a ZIP without installing it.
    Download {
        #[command(flatten)]
        selected: VersionArgs,
        #[arg(long)]
        output: PathBuf,
    },
    /// Install a verified skill in an agent's skill directory.
    Install {
        #[command(flatten)]
        selected: VersionArgs,
        #[arg(long, default_value = ".agents/skills")]
        directory: PathBuf,
        /// Replace only this skill directory, including local edits.
        #[arg(long)]
        replace: bool,
    },
}

#[derive(Args)]
pub struct VersionArgs {
    /// Skill address: namespace/skill-name.
    pub skill: String,
    /// Immutable version number. Defaults to the registry's default version.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub version: Option<u32>,
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, time::Duration};

    use clap::{CommandFactory, Parser};

    use super::{
        Cli, Command, HookCommand, ProvidersCommand, SkillsCommand, is_hook_entrypoint, parse_age,
    };

    #[test]
    fn command_contract_is_valid() {
        Cli::command().debug_assert();
        assert!(
            Cli::try_parse_from(["oceans", "skills", "show", "alice/review", "--version", "0"])
                .is_err()
        );
        let cli = Cli::try_parse_from([
            "oceans",
            "skills",
            "upload",
            "bundled-skills/review",
            "--skip-unchanged",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Skills(SkillsCommand::Upload {
                skip_unchanged: true,
                ..
            })
        ));
    }

    #[test]
    fn copilot_token_input_contract_is_valid() {
        for source in [vec![], vec!["dummy_github_token"], vec!["--token-stdin"]] {
            let mut args = vec![
                "oceans",
                "providers",
                "set-copilot-token",
                "--provider",
                "copilot-main",
            ];
            args.extend(source);
            let cli = Cli::try_parse_safe_from(args).unwrap();
            assert!(
                matches!(cli.command, Command::Providers(ProvidersCommand::SetCopilotToken(args)) if args.provider.as_deref() == Some("copilot-main"))
            );
        }
    }

    #[test]
    fn credential_argument_errors_do_not_echo_secrets() {
        for extra in [
            vec!["--token-stdin"],
            vec!["unexpected_dummy_secret"],
            vec!["--unknown=another_dummy_secret"],
        ] {
            let mut args = vec![
                "oceans",
                "providers",
                "set-copilot-token",
                "dummy_github_token",
            ];
            args.extend(extra);
            let error = Cli::try_parse_safe_from(args).err().unwrap().to_string();
            assert!(!error.contains("dummy"), "{error}");
            assert!(error.contains("--help"));
        }
        let help = Cli::try_parse_safe_from(["oceans", "providers", "set-copilot-token", "--help"])
            .err()
            .unwrap();
        assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
        assert!(help.to_string().contains("--token-stdin"));
    }

    #[test]
    fn api_key_argument_errors_do_not_echo_keys() {
        for args in [
            vec![
                "oceans",
                "--api-key",
                "dummy_api_key",
                "skills",
                "list",
                "--bogus",
            ],
            vec!["oceans", "--api-key=dummy_api_key", "unknown"],
            vec![
                "oceans",
                "login",
                "--api-key-command",
                "dummy_key_command",
                "extra",
            ],
        ] {
            let error = Cli::try_parse_safe_from(args).err().unwrap().to_string();
            assert!(!error.contains("dummy"), "{error}");
            assert!(error.contains("--help"), "{error}");
        }
        let cli = Cli::try_parse_safe_from([
            "oceans",
            "hook",
            "send",
            "--api-key",
            "k",
            "--url",
            "https://o.example.com",
        ])
        .unwrap();
        assert_eq!(cli.api_key.as_deref(), Some("k"));
        assert_eq!(cli.url.unwrap().as_str(), "https://o.example.com/");
    }

    #[test]
    fn hook_commands_parse() {
        let cli = Cli::try_parse_from([
            "oceans",
            "hook",
            "prs",
            "--repo-dir",
            "/r",
            "--if-stale",
            "1h",
        ])
        .unwrap();
        let Command::Hook(HookCommand::Prs(args)) = cli.command else {
            panic!("expected hook prs");
        };
        assert_eq!(
            (args.limit, args.if_stale),
            (30, Some(Duration::from_secs(3600)))
        );
        assert!(Cli::try_parse_from(["oceans", "hook", "prs", "--limit", "0"]).is_err());
        assert!(Cli::try_parse_from(["oceans", "hook", "install"]).is_err());
        assert!(Cli::try_parse_from(["oceans", "hook", "install", "--claude", "--omp"]).is_ok());
        assert!(Cli::try_parse_from(["oceans", "hook", "queue", "--resend", "--discard"]).is_err());
        assert!(
            Cli::try_parse_from([
                "oceans",
                "login",
                "--api-key-command",
                "x",
                "--api-key-env",
                "Y"
            ])
            .is_err()
        );
        for (value, seconds) in [("30s", 30), ("15m", 900), ("2d", 172_800)] {
            assert_eq!(parse_age(value), Ok(Duration::from_secs(seconds)));
        }
        for value in ["", "1", "h", "1w", "-1h", "99999999999999999999d"] {
            assert!(parse_age(value).is_err(), "{value}");
        }
    }

    #[test]
    fn hook_entrypoints_are_recognised() {
        let args = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(is_hook_entrypoint(&args(&["oceans", "hook", "notes"])));
        assert!(is_hook_entrypoint(&args(&[
            "/bin/oceans",
            "--url",
            "https://x",
            "hook",
            "send",
            "--bogus"
        ])));
        assert!(is_hook_entrypoint(&args(&[
            "oceans",
            "hook",
            "--url",
            "https://x",
            "notes",
            "--unknown"
        ])));
        assert!(is_hook_entrypoint(&args(&[
            "oceans",
            "hook",
            "--api-key",
            "k",
            "--json",
            "--url=https://x",
            "--api-key=k",
            "claude"
        ])));
        assert!(!is_hook_entrypoint(&args(&[
            "oceans", "hook", "install", "--claude"
        ])));
        assert!(!is_hook_entrypoint(&args(&[
            "oceans", "hook", "--json", "install", "--claude"
        ])));
        assert!(!is_hook_entrypoint(&args(&["oceans", "hook", "--url"])));
        assert!(!is_hook_entrypoint(&args(&["oceans", "hook", "queue"])));
        assert!(!is_hook_entrypoint(&args(&["oceans", "skills", "list"])));
    }
}
