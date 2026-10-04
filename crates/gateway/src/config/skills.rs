use std::time::Duration;

use anyhow::{Context, bail};
use gateway_skills::BundleLimits;
use gateway_store::S3SkillStorageConfig;
use serde::Deserialize;

use super::references::{resolve_path_reference, resolve_secret_reference};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SkillsConfig {
    pub enabled: bool,
    pub storage: SkillStorageConfig,
    pub limits: BundleLimits,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SkillStorageConfig {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub prefix: String,
    pub force_path_style: bool,
    pub allow_http: bool,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub session_token: Option<String>,
}

impl Default for SkillStorageConfig {
    fn default() -> Self {
        Self {
            bucket: String::new(),
            region: "us-east-1".to_string(),
            endpoint: None,
            prefix: "skills/".to_string(),
            force_path_style: false,
            allow_http: false,
            access_key_id: None,
            secret_access_key: None,
            session_token: None,
        }
    }
}

impl SkillsConfig {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        if !self.enabled {
            return Ok(());
        }
        self.storage_options()?;
        Ok(())
    }

    pub fn storage_options(&self) -> anyhow::Result<S3SkillStorageConfig> {
        if self.limits.max_archive_bytes == 0
            || self.limits.max_expanded_bytes == 0
            || self.limits.max_files == 0
        {
            bail!("skills.limits values must be greater than zero");
        }
        let max_upload_bytes = usize::try_from(self.limits.max_archive_bytes)
            .context("skills.limits.max_archive_bytes is too large for this platform")?;
        self.storage.validate()?;
        Ok(S3SkillStorageConfig {
            bucket: self.storage.bucket.clone(),
            region: self.storage.region.clone(),
            endpoint: self.storage.resolved_endpoint()?,
            prefix: self.storage.prefix.clone(),
            force_path_style: self.storage.force_path_style,
            allow_http: self.storage.allow_http,
            access_key_id: resolve_optional_secret(
                self.storage.access_key_id.as_deref(),
                "skills.storage.access_key_id",
            )?,
            secret_access_key: resolve_optional_secret(
                self.storage.secret_access_key.as_deref(),
                "skills.storage.secret_access_key",
            )?,
            session_token: resolve_optional_secret(
                self.storage.session_token.as_deref(),
                "skills.storage.session_token",
            )?,
            max_upload_bytes,
            request_timeout: Duration::from_secs(30),
        })
    }
}

impl SkillStorageConfig {
    fn validate(&self) -> anyhow::Result<()> {
        for (name, value) in [("bucket", &self.bucket), ("region", &self.region)] {
            if value.is_empty() || value.chars().any(char::is_whitespace) {
                bail!("skills.storage.{name} must be nonempty and contain no whitespace");
            }
        }
        let prefix = self.prefix.trim_end_matches('/');
        if self.prefix.len() > 1024
            || self.prefix.contains('\\')
            || self.prefix.chars().any(char::is_control)
            || (!self.prefix.is_empty()
                && prefix
                    .split('/')
                    .any(|part| matches!(part, "" | "." | "..")))
        {
            bail!("skills.storage.prefix must be a relative object prefix without dot segments");
        }
        if self.access_key_id.is_some() != self.secret_access_key.is_some() {
            bail!("skills.storage.access_key_id and secret_access_key must be set together");
        }
        if self.session_token.is_some() && self.access_key_id.is_none() {
            bail!("skills.storage.session_token requires explicit access and secret keys");
        }
        Ok(())
    }

    fn resolved_endpoint(&self) -> anyhow::Result<Option<String>> {
        let Some(endpoint) = self.endpoint.as_deref() else {
            return Ok(None);
        };
        let endpoint = resolve_path_reference(endpoint).context("skills.storage.endpoint")?;
        let parsed = url::Url::parse(&endpoint).context("skills.storage.endpoint is invalid")?;
        let scheme_allowed =
            parsed.scheme() == "https" || (parsed.scheme() == "http" && self.allow_http);
        if !scheme_allowed || parsed.host().is_none() {
            bail!("skills.storage.endpoint must use https; http requires allow_http: true");
        }
        if !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            bail!(
                "skills.storage.endpoint must be an origin without credentials, path, query, or fragment"
            );
        }
        Ok(Some(parsed.origin().ascii_serialization()))
    }
}

fn resolve_optional_secret(value: Option<&str>, field: &str) -> anyhow::Result<Option<String>> {
    value
        .map(|value| {
            let secret = resolve_secret_reference(value).with_context(|| field.to_string())?;
            if secret.trim().is_empty() {
                bail!("{field} cannot resolve to an empty value");
            }
            Ok(secret)
        })
        .transpose()
}
