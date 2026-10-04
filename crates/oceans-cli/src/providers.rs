use std::io::{self, IsTerminal, Read};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::{
    cli::{CopilotTokenArgs, ProvidersCommand},
    client::Client,
};

const MAX_TOKEN_BYTES: usize = 4096;
const CREDENTIAL_PATH: &[&str] = &["me", "provider-credentials"];

#[derive(Deserialize)]
struct ProviderCredentialStatus {
    provider_key: String,
    configured: bool,
}

#[derive(Serialize)]
struct SetTokenRequest<'a> {
    token: &'a str,
}

#[derive(Serialize)]
struct StoredCredential<'a> {
    gateway: &'a str,
    provider_key: &'a str,
    configured: bool,
    status: &'static str,
}

pub async fn run(
    client: &Client,
    command: ProvidersCommand,
    gateway: &str,
    as_json: bool,
) -> anyhow::Result<()> {
    let ProvidersCommand::SetCopilotToken(args) = command;
    let stdin = io::stdin();
    let token = read_token(&args, stdin.lock(), stdin.is_terminal())?;
    let provider_key = store_token(client, args.provider.as_deref(), &token).await?;
    let result = StoredCredential {
        gateway,
        provider_key: &provider_key,
        configured: true,
        status: "stored",
    };
    println!("{}", render_result(&result, as_json)?);
    Ok(())
}

fn read_token(
    args: &CopilotTokenArgs,
    reader: impl Read,
    terminal: bool,
) -> anyhow::Result<String> {
    let token = match (&args.token, args.token_stdin) {
        (Some(_), true) => bail!("use either TOKEN or --token-stdin"),
        (Some(token), false) => token.clone(),
        (None, true) => {
            // Allow a maximum-size token plus CRLF, then read one byte to detect overflow.
            let mut bytes = Vec::new();
            reader
                .take((MAX_TOKEN_BYTES + 3) as u64)
                .read_to_end(&mut bytes)
                .context("could not read the GitHub token from standard input")?;
            if bytes.len() > MAX_TOKEN_BYTES + 2 {
                bail!("GitHub token input exceeds the size limit");
            }
            String::from_utf8(bytes).map_err(|_| {
                anyhow::anyhow!("GitHub token must contain printable ASCII without whitespace")
            })?
        }
        (None, false) if terminal => rpassword::prompt_password("GitHub token (hidden): ")
            .context("could not read the GitHub token from the terminal")?,
        (None, false) => {
            bail!("no token supplied; use --token-stdin when standard input is not a terminal")
        }
    };
    normalize_token(&token)
}

fn normalize_token(token: &str) -> anyhow::Result<String> {
    let token = token.trim();
    if token.is_empty()
        || token.len() > MAX_TOKEN_BYTES
        || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        bail!("GitHub token must contain 1 to 4096 printable ASCII bytes without whitespace");
    }
    Ok(token.to_owned())
}

async fn store_token(
    client: &Client,
    requested_provider: Option<&str>,
    token: &str,
) -> anyhow::Result<String> {
    let providers: Vec<ProviderCredentialStatus> = client.get_sensitive(CREDENTIAL_PATH).await?;
    let provider_key = select_provider(&providers, requested_provider)?;
    let stored: ProviderCredentialStatus = client
        .put_sensitive(
            &["me", "provider-credentials", provider_key],
            &SetTokenRequest { token },
        )
        .await?;
    if stored.provider_key != provider_key || !stored.configured {
        bail!("gateway did not confirm that the token was stored for the selected provider");
    }
    Ok(provider_key.to_owned())
}

fn select_provider<'a>(
    providers: &'a [ProviderCredentialStatus],
    requested: Option<&str>,
) -> anyhow::Result<&'a str> {
    if providers.is_empty() {
        bail!(
            "this gateway has no Copilot provider configured for GitHub user authentication; ask a platform admin to configure one"
        );
    }
    if let Some(requested) = requested {
        return providers
            .iter()
            .find(|provider| provider.provider_key == requested)
            .map(|provider| provider.provider_key.as_str())
            .context("the selected provider is not configured for GitHub user authentication");
    }
    if providers.len() == 1 {
        return Ok(&providers[0].provider_key);
    }
    let keys: Vec<&str> = providers
        .iter()
        .map(|provider| provider.provider_key.as_str())
        .collect();
    bail!(
        "multiple Copilot providers are available; select one with --provider: {}",
        serde_json::to_string(&keys)?
    );
}

fn render_result(result: &StoredCredential<'_>, as_json: bool) -> anyhow::Result<String> {
    if as_json {
        return serde_json::to_string_pretty(result)
            .context("could not format the stored credential status");
    }
    Ok(format!(
        "Token stored for provider {} on {}.",
        result.provider_key.escape_default(),
        result.gateway.escape_default()
    ))
}

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;
