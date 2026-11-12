//! `oceans login`: checks an API key against the gateway and saves how to find it in the
//! config file. A key command or variable name is saved as given, never the key it yields.

use std::io::{self, IsTerminal};

use anyhow::{Context, anyhow, bail};
use serde::Serialize;

use crate::{
    attribution::{ownership, state::State},
    cli::LoginArgs,
    client::Client,
    config::{self, Env, KeySource, Overrides},
};

#[derive(Serialize)]
struct LoginReport<'a> {
    url: &'a str,
    config_path: String,
    ownership_scope: &'a str,
    owner_kind: &'a str,
    owner_id: &'a str,
}

pub async fn run(
    args: LoginArgs,
    overrides: Overrides<'_>,
    env: &Env,
    json: bool,
) -> anyhow::Result<()> {
    let path = config::config_path(env)
        .context("could not find a config directory; set HOME or XDG_CONFIG_HOME")?;
    let api_key_setting = api_key_setting(&args, overrides.api_key)?;
    // An unreadable existing file still lets login replace it.
    let existing = config::read_config(&path).ok().flatten();
    let url = config::resolve_url(
        &overrides,
        env.var("OCEANS_URL").as_deref(),
        existing.as_ref(),
        Some(&path),
    )?;
    let api_key = KeySource::parse(&api_key_setting).resolve(env, config::KEY_COMMAND_TIMEOUT)?;
    let client = Client::new(url.clone(), &api_key)?;
    let owner = ownership::lookup(&client)
        .await
        .map_err(|error| anyhow!("could not check the API key: {error}"))?;
    config::write_config(&path, &url, &api_key_setting)?;
    State::from_env(env)?.record_ownership(
        url.as_str(),
        &ownership::key_fingerprint(&api_key),
        &owner,
    )?;
    let report = LoginReport {
        url: url.as_str(),
        config_path: path.display().to_string(),
        ownership_scope: &owner.ownership_scope,
        owner_kind: &owner.owner_kind,
        owner_id: &owner.owner_id,
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "Logged in to {} as {} {}. Saved {}.",
            report.url,
            owner.owner_kind.replace('_', " "),
            owner.owner_id,
            report.config_path
        );
    }
    Ok(())
}

/// The `api_key` value to save: `!command`, `env:NAME` or the key itself.
fn api_key_setting(args: &LoginArgs, literal: Option<&str>) -> anyhow::Result<String> {
    match (&args.api_key_command, &args.api_key_env, literal) {
        (Some(command), None, None) => {
            let command = command.trim();
            let command = command.strip_prefix('!').unwrap_or(command).trim();
            if command.is_empty() {
                bail!("--api-key-command is empty");
            }
            Ok(format!("!{command}"))
        }
        (None, Some(name), None) => {
            config::validate_env_name(name)?;
            Ok(format!("env:{name}"))
        }
        (None, None, Some(key)) => literal_setting(key),
        (None, None, None) if io::stdin().is_terminal() => {
            let key = rpassword::prompt_password("Oceans API key (hidden): ")
                .context("could not read the API key from the terminal")?;
            literal_setting(&key)
        }
        (None, None, None) => bail!(
            "no API key supplied; pass --api-key-command, --api-key-env or --api-key, or run oceans login in a terminal"
        ),
        _ => bail!("use only one of --api-key-command, --api-key-env and --api-key"),
    }
}

/// A key saved as itself must not read back as a command or variable name.
fn literal_setting(key: &str) -> anyhow::Result<String> {
    let key = key.trim();
    if key.is_empty() {
        bail!("the Oceans API key is empty");
    }
    if key.starts_with('!') || key.starts_with("env:") {
        bail!(
            "a key starting with ! or env: would be read back as a command or variable; use --api-key-command or --api-key-env"
        );
    }
    Ok(key.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(command: Option<&str>, env: Option<&str>) -> LoginArgs {
        LoginArgs {
            api_key_command: command.map(str::to_owned),
            api_key_env: env.map(str::to_owned),
        }
    }

    #[test]
    fn key_settings_keep_the_source_not_the_key() {
        assert_eq!(
            api_key_setting(&args(Some("fnox get OCEANS_INGEST_KEY"), None), None).unwrap(),
            "!fnox get OCEANS_INGEST_KEY"
        );
        assert_eq!(
            api_key_setting(&args(Some("!op read op://x/y"), None), None).unwrap(),
            "!op read op://x/y"
        );
        assert_eq!(
            api_key_setting(&args(None, Some("OCEANS_INGEST_KEY")), None).unwrap(),
            "env:OCEANS_INGEST_KEY"
        );
        assert_eq!(
            api_key_setting(&args(None, None), Some(" gwk_abc ")).unwrap(),
            "gwk_abc"
        );
        assert!(api_key_setting(&args(None, Some("NOT-A-NAME")), None).is_err());
        assert!(api_key_setting(&args(None, None), Some("!rm -rf")).is_err());
        assert!(api_key_setting(&args(None, None), Some("env:KEY")).is_err());
        assert!(api_key_setting(&args(Some("x"), Some("Y")), None).is_err());
        assert!(api_key_setting(&args(Some("  ! "), None), None).is_err());
    }
}
