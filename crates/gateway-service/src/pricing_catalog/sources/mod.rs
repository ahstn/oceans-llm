//! The catalog sources (models.dev, OpenRouter, LiteLLM), each projected into the
//! gateway's providers document shape keyed by gateway catalog provider ids.

use std::{collections::BTreeMap, fmt};

use anyhow::Context;
use gateway_core::{GatewayError, Money4};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::{
    PricingCatalogProviderDocument, PricingCatalogSnapshotFile, ProjectedSnapshot,
    projected_snapshot,
};
use crate::catalog_fetch::{CatalogFetch, catalog_http_client, fetch_bounded};

pub(crate) mod litellm;
pub(crate) mod models_dev;
pub(crate) mod openrouter;

/// A projected providers document: gateway provider id to provider document.
pub(crate) type Providers = BTreeMap<String, PricingCatalogProviderDocument>;

/// How one catalog source is fetched, cached and projected.
pub(crate) struct SourceDescriptor {
    pub(crate) id: CatalogSourceId,
    pub(crate) url: &'static str,
    /// The source's `pricing_catalog_cache` row.
    pub(crate) cache_key: &'static str,
    /// Largest accepted response body, far above the expected payload.
    pub(crate) max_bytes: usize,
    /// Provenance `source` of a snapshot fetched at runtime.
    pub(crate) remote_source: &'static str,
    /// Provenance `source` of the checked-in snapshot.
    pub(crate) vendored_source: &'static str,
    /// The checked-in snapshot file, refreshed by the `sync_pricing_catalog` bin.
    vendored_json: &'static str,
    pub(crate) project: fn(&[u8]) -> Result<Providers, GatewayError>,
}

impl SourceDescriptor {
    /// The checked-in snapshot. `fetched_at` is truncated to the whole second the stores
    /// keep, so rows stamped from it compare equal once persisted.
    ///
    /// # Panics
    ///
    /// Panics when the checked-in file does not deserialize, which the crate's tests
    /// rule out.
    pub(crate) fn vendored_snapshot(&self) -> PricingCatalogSnapshotFile {
        let mut snapshot = serde_json::from_str::<PricingCatalogSnapshotFile>(self.vendored_json)
            .unwrap_or_else(|error| {
                panic!("vendored {} catalog should deserialize: {error}", self.id)
            });
        snapshot.metadata.fetched_at = snapshot
            .metadata
            .fetched_at
            .replace_nanosecond(0)
            .expect("zero nanoseconds is in range");
        snapshot
    }
}

static MODELS_DEV: SourceDescriptor = SourceDescriptor {
    id: CatalogSourceId::ModelsDev,
    url: "https://models.dev/api.json",
    cache_key: "models_dev_supported_v4",
    // The raw `api.json` is around 6 MB.
    max_bytes: 32 * 1024 * 1024,
    remote_source: "models_dev_api",
    vendored_source: "vendored_models_dev",
    vendored_json: include_str!("../../../data/pricing_catalog_fallback.json"),
    project: models_dev::project,
};

static OPENROUTER: SourceDescriptor = SourceDescriptor {
    id: CatalogSourceId::OpenRouter,
    // `output_modalities=all` includes image-only models.
    url: "https://openrouter.ai/api/v1/models?output_modalities=all",
    cache_key: "openrouter_v1",
    // The model list is around 1 MB.
    max_bytes: 16 * 1024 * 1024,
    remote_source: "openrouter_api",
    vendored_source: "vendored_openrouter",
    vendored_json: include_str!("../../../data/openrouter_catalog_fallback.json"),
    project: openrouter::project,
};

static LITELLM: SourceDescriptor = SourceDescriptor {
    id: CatalogSourceId::LiteLlm,
    url: "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json",
    cache_key: "litellm_v1",
    // The model price table is around 3 MB.
    max_bytes: 16 * 1024 * 1024,
    remote_source: "litellm_github",
    vendored_source: "vendored_litellm",
    vendored_json: include_str!("../../../data/litellm_catalog_fallback.json"),
    project: litellm::project,
};

/// A catalog source, in the stable spelling used by cache keys and provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CatalogSourceId {
    #[serde(rename = "models_dev")]
    ModelsDev,
    #[serde(rename = "openrouter")]
    OpenRouter,
    #[serde(rename = "litellm")]
    LiteLlm,
}

impl CatalogSourceId {
    /// Every source, in the order refresh reports list them.
    pub const ALL: [Self; 3] = [Self::ModelsDev, Self::OpenRouter, Self::LiteLlm];

    /// The serde name of the source.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelsDev => "models_dev",
            Self::OpenRouter => "openrouter",
            Self::LiteLlm => "litellm",
        }
    }

    pub(crate) fn descriptor(self) -> &'static SourceDescriptor {
        match self {
            Self::ModelsDev => &MODELS_DEV,
            Self::OpenRouter => &OPENROUTER,
            Self::LiteLlm => &LITELLM,
        }
    }
}

impl fmt::Display for CatalogSourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Fetch `source` without a conditional header and project it into a vendored snapshot
/// file.
///
/// # Errors
///
/// Fails when the fetch fails, exceeds the source's size cap, answers 304, or the body
/// does not project.
pub async fn fetch_vendored_source_snapshot(
    source: CatalogSourceId,
) -> anyhow::Result<PricingCatalogSnapshotFile> {
    let descriptor = source.descriptor();
    let url = descriptor.url;
    let fetched = fetch_bounded(catalog_http_client().get(url), descriptor.max_bytes)
        .await
        .with_context(|| format!("failed fetching {source} catalog from `{url}`"))?;
    // No `If-None-Match` is sent, so a 304 is a misbehaving server.
    let CatalogFetch::Fetched(body) = fetched else {
        anyhow::bail!("{source} catalog fetch returned HTTP 304 to an unconditional request");
    };
    let providers =
        (descriptor.project)(&body.bytes).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let ProjectedSnapshot { snapshot, .. } = projected_snapshot(
        providers,
        descriptor.vendored_source,
        body.etag,
        OffsetDateTime::now_utc(),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(snapshot)
}

/// Converts a per-token USD price float to USD per million tokens at 4 dp. The float is
/// read through its shortest round-trip decimal, so `3e-6` becomes exactly `3.0000`.
fn f64_per_token_to_per_million(per_token: f64) -> Option<String> {
    if !per_token.is_finite() {
        return None;
    }
    per_token_to_per_million(&per_token.to_string())
}

/// Converts a non-negative per-token USD decimal (`"0.000003"`, `"3e-6"`) to USD per
/// million tokens at 4 dp, rounding half away from zero. Shifting the decimal digits
/// keeps the conversion exact; `None` for anything that is not such a decimal or whose
/// result overflows.
fn per_token_to_per_million(per_token: &str) -> Option<String> {
    /// Digits to move the decimal point right: 6 for per million, 4 for the 4 dp scale.
    const PER_MILLION_SCALED_SHIFT: i64 = 10;

    let trimmed = per_token.trim();
    let (mantissa, exponent) = match trimmed.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, i64::from(exponent.parse::<i32>().ok()?)),
        None => (trimmed, 0),
    };
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{integer}{fraction}");
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let fraction_len = i64::try_from(fraction.len()).ok()?;
    let shift = exponent
        .checked_sub(fraction_len)?
        .checked_add(PER_MILLION_SCALED_SHIFT)?;

    let dropped = usize::try_from(shift.saturating_neg()).unwrap_or(0);
    let kept = digits.len().saturating_sub(dropped);
    let mut scaled = 0_i64;
    for byte in digits[..kept].bytes() {
        scaled = scaled
            .checked_mul(10)?
            .checked_add(i64::from(byte - b'0'))?;
    }
    if shift > 0 {
        scaled = scaled.checked_mul(10_i64.checked_pow(u32::try_from(shift).ok()?)?)?;
    } else if dropped > 0 && dropped <= digits.len() && digits.as_bytes()[kept] >= b'5' {
        scaled = scaled.checked_add(1)?;
    }
    Some(Money4::from_scaled(scaled).format_4dp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_token_floats_convert_exactly_to_per_million() {
        let cases = [
            (3e-06, "3.0000"),
            (1.25e-07, "0.1250"),
            (2.5e-06, "2.5000"),
            (1e-05, "10.0000"),
            (0.0, "0.0000"),
            (1.23456e-07, "0.1235"),
            (1.23444e-07, "0.1234"),
            (4e-11, "0.0000"),
        ];
        for (per_token, want) in cases {
            assert_eq!(
                f64_per_token_to_per_million(per_token).as_deref(),
                Some(want),
                "per token {per_token}"
            );
        }
    }

    #[test]
    fn per_token_strings_convert_and_reject_non_decimals() {
        assert_eq!(
            per_token_to_per_million("0.000003").as_deref(),
            Some("3.0000")
        );
        assert_eq!(per_token_to_per_million("3e-6").as_deref(), Some("3.0000"));
        assert_eq!(per_token_to_per_million("0").as_deref(), Some("0.0000"));
        assert_eq!(per_token_to_per_million("-1"), None);
        assert_eq!(per_token_to_per_million(""), None);
        assert_eq!(per_token_to_per_million("free"), None);
        assert_eq!(f64_per_token_to_per_million(f64::NAN), None);
    }

    #[test]
    fn source_ids_use_stable_snake_case_names() {
        for source in [
            CatalogSourceId::ModelsDev,
            CatalogSourceId::OpenRouter,
            CatalogSourceId::LiteLlm,
        ] {
            assert_eq!(
                serde_json::to_value(source).expect("serialize source id"),
                serde_json::Value::from(source.as_str())
            );
        }
        assert_eq!(CatalogSourceId::OpenRouter.as_str(), "openrouter");
        assert_eq!(CatalogSourceId::LiteLlm.as_str(), "litellm");
    }
}
