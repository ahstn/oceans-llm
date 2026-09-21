use anyhow::bail;
use serde::Deserialize;

use super::references::{resolve_secret_reference, validate_env_reference_if_needed};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkCatalogConfig {
    #[serde(default)]
    pub artificial_analysis: Option<ArtificialAnalysisBenchmarkConfig>,
}

impl BenchmarkCatalogConfig {
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        let Some(config) = &self.artificial_analysis else {
            return Ok(());
        };
        if config.api_key.trim().is_empty() {
            bail!("benchmark_catalog.artificial_analysis.api_key cannot be empty");
        }
        validate_env_reference_if_needed(&config.api_key)
    }

    pub(super) fn artificial_analysis_api_key(&self) -> anyhow::Result<Option<String>> {
        self.artificial_analysis
            .as_ref()
            .map(|config| {
                let api_key = resolve_secret_reference(&config.api_key)?;
                if api_key.trim().is_empty() {
                    bail!(
                        "benchmark_catalog.artificial_analysis.api_key resolved to an empty value"
                    );
                }
                Ok(api_key)
            })
            .transpose()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtificialAnalysisBenchmarkConfig {
    pub api_key: String,
}
