mod attribution;
mod cli;
mod client;
mod config;
mod install;
mod login;
mod process;
mod providers;
mod skills;

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse_safe();
    let overrides = config::Overrides {
        url: cli.url.as_ref(),
        api_key: cli.api_key.as_deref(),
    };
    let env = config::Env::Process;
    match cli.command {
        cli::Command::Skills(command) => block_on(async {
            let client = connect(&overrides, &env)?;
            skills::run(&client, command, cli.json).await
        }),
        cli::Command::Providers(command) => block_on(async {
            let connection =
                config::resolve_connection(&overrides, &env, config::KEY_COMMAND_TIMEOUT)?;
            let gateway = connection.url.to_string();
            let client = client::Client::new(connection.url, &connection.api_key)?;
            providers::run(&client, command, &gateway, cli.json).await
        }),
        cli::Command::Login(args) => block_on(login::run(args, overrides, &env, cli.json)),
        // Hooks don't resolve a key unless they have something to send.
        cli::Command::Hook(command) => attribution::run(command, overrides, cli.json),
    }
}

fn connect(overrides: &config::Overrides<'_>, env: &config::Env) -> anyhow::Result<client::Client> {
    let connection = config::resolve_connection(overrides, env, config::KEY_COMMAND_TIMEOUT)?;
    client::Client::new(connection.url, &connection.api_key)
}

/// Runs `future` on a single-threaded runtime. Only commands that reach the network build one.
pub(crate) fn block_on<T>(future: impl Future<Output = anyhow::Result<T>>) -> anyhow::Result<T> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("could not start the async runtime")?
        .block_on(future)
}
