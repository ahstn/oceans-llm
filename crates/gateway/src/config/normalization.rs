use anyhow::bail;
use gateway_core::{RequestTag, validate_entity_tags};

pub(super) fn normalize_config_email(email: &str) -> anyhow::Result<String> {
    let normalized = email.trim().to_ascii_lowercase();
    if normalized.is_empty() || !normalized.contains('@') {
        bail!("email must be a valid email address");
    }
    Ok(normalized)
}

pub(super) fn normalize_config_entity_tags(
    tags: &[RequestTag],
    context: &str,
) -> anyhow::Result<Vec<RequestTag>> {
    validate_entity_tags(tags, context).map_err(|error| anyhow::anyhow!("{error}"))
}

pub(super) fn normalize_optional_config_entity_tags(
    tags: Option<&[RequestTag]>,
    context: &str,
) -> anyhow::Result<Option<Vec<RequestTag>>> {
    tags.map(|tags| normalize_config_entity_tags(tags, context))
        .transpose()
}

pub(super) fn normalize_config_team_key(team_key: &str) -> anyhow::Result<String> {
    let normalized = team_key.trim().to_string();
    if normalized.is_empty() {
        bail!("team key cannot be empty");
    }
    Ok(normalized)
}
