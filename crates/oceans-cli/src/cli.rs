use std::{ffi::OsString, path::PathBuf};

use clap::{Args, Parser, Subcommand};
use url::Url;

#[derive(Parser)]
#[command(
    name = "oceans",
    version,
    about = "Use Oceans LLM skills and providers"
)]
pub struct Cli {
    /// Gateway URL. HTTPS is required except for loopback IP addresses.
    /// Authentication uses the OCEANS_API_KEY environment variable.
    #[arg(
        long,
        global = true,
        env = "OCEANS_URL",
        hide_env_values = true,
        default_value = "http://127.0.0.1:8080"
    )]
    pub url: Url,

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
        Self::try_parse_safe_from(std::env::args_os()).unwrap_or_else(|error| error.exit())
    }

    fn try_parse_safe_from(
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Self, clap::Error> {
        let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        let credentials = args
            .iter()
            .any(|value| value == "providers" || value == "set-copilot-token");
        Self::try_parse_from(args).map_err(|error| {
            if credentials && !matches!(error.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion) {
                // Clap can include a rejected positional token in its diagnostics.
                clap::Error::raw(error.kind(), "invalid provider command arguments; run oceans providers set-copilot-token --help for usage")
            } else {
                error
            }
        })
    }
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
    use clap::{CommandFactory, Parser};

    use super::{Cli, Command, ProvidersCommand, SkillsCommand};

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
}
