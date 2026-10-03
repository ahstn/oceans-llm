mod cli;
mod client;
mod install;
mod skills;

use anyhow::Context;
use clap::Parser;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let api_key = std::env::var("OCEANS_API_KEY")
        .context("set OCEANS_API_KEY to an Oceans gateway API key")?;
    let client = client::Client::new(cli.url, &api_key)?;
    let cli::Command::Skills(command) = cli.command;
    skills::run(&client, command, cli.json).await
}
