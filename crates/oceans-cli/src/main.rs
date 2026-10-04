mod cli;
mod client;
mod install;
mod providers;
mod skills;

use anyhow::Context;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse_safe();
    let api_key = std::env::var("OCEANS_API_KEY")
        .context("set OCEANS_API_KEY to an Oceans gateway API key")?;
    let gateway = cli.url.to_string();
    let client = client::Client::new(cli.url, &api_key)?;
    match cli.command {
        cli::Command::Skills(command) => skills::run(&client, command, cli.json).await,
        cli::Command::Providers(command) => {
            providers::run(&client, command, &gateway, cli.json).await
        }
    }
}
