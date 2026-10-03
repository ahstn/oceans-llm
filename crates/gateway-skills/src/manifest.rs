use crate::{BundleError, SkillManifest};

/// Validate the portable ASCII name used by Agent Skills and installation paths.
pub fn validate_name(name: &str) -> Result<(), BundleError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
    {
        return Err(BundleError::Manifest("name must be 1–64 lowercase ASCII letters, numbers, or single hyphens, without a leading or trailing hyphen".into()));
    }
    Ok(())
}

/// Namespace handles use the same portable slug rules as skill names.
pub fn validate_namespace(handle: &str) -> Result<(), BundleError> {
    validate_name(handle)
}

/// Validate a file path without rewriting ambiguous or platform-specific names.
pub fn validate_file_path(path: &str) -> Result<(), BundleError> {
    if path.is_empty()
        || path.len() > 1024
        || path.contains(['\\', ':'])
        || path.chars().any(char::is_control)
        || path.split('/').count() > 64
    {
        return Err(BundleError::UnsafePath(path.into()));
    }
    for part in path.split('/') {
        let device = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let is_device = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || device.strip_prefix("COM").is_some_and(is_device_number)
            || device.strip_prefix("LPT").is_some_and(is_device_number);
        if part.is_empty()
            || part.len() > 255
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part.contains(['<', '>', '"', '|', '?', '*'])
            || is_device
        {
            return Err(BundleError::UnsafePath(path.into()));
        }
    }
    Ok(())
}

fn is_device_number(value: &str) -> bool {
    (value.len() == 1 && matches!(value.as_bytes()[0], b'1'..=b'9'))
        || matches!(value, "¹" | "²" | "³")
}

pub(crate) fn parse_manifest(instructions: &str) -> Result<SkillManifest, BundleError> {
    let mut lines = instructions.lines();
    if lines.next() != Some("---") {
        return Err(BundleError::Manifest(
            "SKILL.md must start with YAML frontmatter".into(),
        ));
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line == "---" {
            closed = true;
            break;
        }
        if yaml.len().saturating_add(line.len()).saturating_add(1) > 64 * 1024 {
            return Err(BundleError::Manifest("frontmatter exceeds 64 KiB".into()));
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err(BundleError::Manifest(
            "frontmatter has no closing delimiter".into(),
        ));
    }
    // Bound decoded YAML, including aliases, before constructing extension values.
    let value = crate::frontmatter::parse_frontmatter(&yaml).map_err(BundleError::Manifest)?;
    let mapping = value
        .as_object()
        .ok_or_else(|| BundleError::Manifest("frontmatter must be a mapping".into()))?;
    for field in [
        "name",
        "description",
        "license",
        "compatibility",
        "allowed-tools",
    ] {
        if mapping.get(field).is_some_and(|value| !value.is_string()) {
            return Err(BundleError::Manifest(format!("{field} must be a string")));
        }
    }
    if let Some(metadata) = mapping.get("metadata") {
        let metadata = metadata
            .as_object()
            .ok_or_else(|| BundleError::Manifest("metadata must map strings to strings".into()))?;
        if metadata.values().any(|value| !value.is_string()) {
            return Err(BundleError::Manifest(
                "metadata must map strings to strings".into(),
            ));
        }
    }
    let manifest: SkillManifest =
        serde_json::from_value(value).map_err(|error| BundleError::Manifest(error.to_string()))?;
    validate_name(&manifest.name)?;
    if manifest.description.trim().is_empty() || manifest.description.chars().count() > 1024 {
        return Err(BundleError::Manifest(
            "description must contain 1–1024 characters".into(),
        ));
    }
    if manifest
        .compatibility
        .as_ref()
        .is_some_and(|value| value.trim().is_empty() || value.chars().count() > 500)
    {
        return Err(BundleError::Manifest(
            "compatibility must contain 1–500 characters".into(),
        ));
    }
    Ok(manifest)
}
