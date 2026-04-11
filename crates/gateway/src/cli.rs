use clap::{ArgAction, Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "gateway",
    about = "Oceans LLM gateway runtime and maintenance CLI"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        env = "GATEWAY_CONFIG",
        default_value = "./gateway.yaml"
    )]
    pub config: String,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Serve(ServeArgs),
    Migrate(MigrateArgs),
    BootstrapAdmin,
    SeedConfig,
    SeedLocalDemo,
}

#[derive(Debug, Clone, Args)]
pub struct ServeArgs {
    #[arg(
        long,
        env = "GATEWAY_RUN_MIGRATIONS",
        default_value_t = true,
        action = ArgAction::Set
    )]
    pub run_migrations: bool,

    #[arg(
        long = "bootstrap-admin",
        env = "GATEWAY_BOOTSTRAP_ADMIN",
        default_value_t = true,
        action = ArgAction::Set
    )]
    pub bootstrap_admin: bool,

    #[arg(
        long = "seed-config",
        env = "GATEWAY_SEED_CONFIG",
        default_value_t = true,
        action = ArgAction::Set
    )]
    pub seed_config: bool,
}

impl Default for ServeArgs {
    fn default() -> Self {
        Self {
            run_migrations: true,
            bootstrap_admin: true,
            seed_config: true,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct MigrateArgs {
    #[arg(long, action = ArgAction::SetTrue)]
    pub check: bool,

    #[arg(long, action = ArgAction::SetTrue)]
    pub apply: bool,

    #[arg(long, action = ArgAction::SetTrue)]
    pub status: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrateAction {
    Check,
    Apply,
    Status,
}

impl MigrateArgs {
    pub fn action(&self) -> anyhow::Result<MigrateAction> {
        let selected = [self.check, self.apply, self.status]
            .into_iter()
            .filter(|value| *value)
            .count();

        if selected != 1 {
            anyhow::bail!("choose exactly one of --check, --apply, or --status");
        }

        if self.check {
            Ok(MigrateAction::Check)
        } else if self.apply {
            Ok(MigrateAction::Apply)
        } else {
            Ok(MigrateAction::Status)
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, MigrateAction};

    #[test]
    fn serve_flags_accept_explicit_false_values() {
        let cli = Cli::parse_from([
            "gateway",
            "serve",
            "--run-migrations=false",
            "--bootstrap-admin=false",
            "--seed-config=false",
        ]);

        let Command::Serve(args) = cli.command.expect("command") else {
            panic!("expected serve command");
        };
        assert!(!args.run_migrations);
        assert!(!args.bootstrap_admin);
        assert!(!args.seed_config);
    }

    #[test]
    fn migrate_flags_map_to_actions() {
        let cli = Cli::parse_from(["gateway", "migrate", "--status"]);
        let Command::Migrate(args) = cli.command.expect("command") else {
            panic!("expected migrate command");
        };
        assert_eq!(args.action().expect("action"), MigrateAction::Status);
    }

    #[test]
    fn parses_seed_local_demo_command() {
        let cli = Cli::parse_from(["gateway", "seed-local-demo"]);
        assert!(matches!(cli.command, Some(Command::SeedLocalDemo)));
    }
}
