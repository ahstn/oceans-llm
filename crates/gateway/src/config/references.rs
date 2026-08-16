use super::*;

fn resolve_env_reference(value: &str) -> anyhow::Result<String> {
    let env_var_name = value
        .strip_prefix("env.")
        .ok_or_else(|| anyhow::anyhow!("expected env.* secret reference, got `{value}`"))?;

    let resolved = env::var(env_var_name)
        .with_context(|| format!("required environment variable `{env_var_name}` is not set"))?;

    Ok(resolved)
}

pub(crate) fn resolve_secret_reference(value: &str) -> anyhow::Result<String> {
    if value.starts_with("env.") {
        resolve_env_reference(value)
    } else if let Some(literal) = value.strip_prefix("literal.") {
        Ok(literal.to_string())
    } else {
        bail!("unsupported secret reference `{value}`; use env.* or literal.* for this phase")
    }
}

pub(super) fn resolve_path_reference(value: &str) -> anyhow::Result<String> {
    if value.starts_with("env.") {
        resolve_env_reference(value)
    } else if let Some(literal) = value.strip_prefix("literal.") {
        Ok(literal.to_string())
    } else {
        Ok(value.to_string())
    }
}

pub(super) enum ResolvedCopilotPrivateKey {
    Pem(String),
    Path(String),
}

pub(super) fn resolve_copilot_private_key(
    value: &str,
) -> anyhow::Result<ResolvedCopilotPrivateKey> {
    let is_secret_reference = value.starts_with("env.") || value.starts_with("literal.");
    let resolved = resolve_path_reference(value)?;

    if is_secret_reference && resolved.contains("BEGIN ") {
        Ok(ResolvedCopilotPrivateKey::Pem(resolved))
    } else {
        Ok(ResolvedCopilotPrivateKey::Path(resolved))
    }
}

pub(super) fn validate_env_reference_if_needed(value: &str) -> anyhow::Result<()> {
    if value.starts_with("env.") {
        resolve_env_reference(value)?;
    }
    Ok(())
}
