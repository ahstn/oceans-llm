use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use url::Url;

#[derive(Parser)]
#[command(name = "oceans", version, about = "Use the Oceans LLM skill registry")]
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

    use super::{Cli, Command, SkillsCommand};

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
}
