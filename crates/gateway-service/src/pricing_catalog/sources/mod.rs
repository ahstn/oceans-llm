//! Catalog sources beside models.dev, each projected into the gateway's providers
//! document shape keyed by gateway catalog provider ids.

use std::{collections::BTreeMap, fmt};

use anyhow::Context;
use gateway_core::{GatewayError, Money4};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::{
    DEFAULT_PRICING_CATALOG_SOURCE_URL, LITELLM_CATALOG_URL, MAX_LITELLM_RESPONSE_BYTES,
    MAX_OPENROUTER_RESPONSE_BYTES, OPENROUTER_CATALOG_URL, PricingCatalogProviderDocument,
    PricingCatalogSnapshotFile, ProjectedSnapshot, VENDORED_LITELLM_SOURCE,
    VENDORED_OPENROUTER_SOURCE, fetch_vendored_snapshot, projected_snapshot,
};
use crate::catalog_fetch::{CatalogFetch, catalog_http_client, fetch_bounded};

pub(crate) mod litellm;
pub(crate) mod openrouter;

/// A projected providers document: gateway provider id to provider document.
pub(crate) type Providers = BTreeMap<String, PricingCatalogProviderDocument>;

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
    /// The serde name of the source.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelsDev => "models_dev",
            Self::OpenRouter => "openrouter",
            Self::LiteLlm => "litellm",
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
    type Project = fn(&[u8]) -> Result<Providers, GatewayError>;
    let (url, max_bytes, vendored_source, project): (&str, usize, &str, Project) = match source {
        CatalogSourceId::ModelsDev => {
            return fetch_vendored_snapshot(DEFAULT_PRICING_CATALOG_SOURCE_URL).await;
        }
        CatalogSourceId::OpenRouter => (
            OPENROUTER_CATALOG_URL,
            MAX_OPENROUTER_RESPONSE_BYTES,
            VENDORED_OPENROUTER_SOURCE,
            openrouter::project,
        ),
        CatalogSourceId::LiteLlm => (
            LITELLM_CATALOG_URL,
            MAX_LITELLM_RESPONSE_BYTES,
            VENDORED_LITELLM_SOURCE,
            litellm::project,
        ),
    };
    let fetched = fetch_bounded(catalog_http_client().get(url), max_bytes)
        .await
        .with_context(|| format!("failed fetching {source} catalog from `{url}`"))?;
    // No `If-None-Match` is sent, so a 304 is a misbehaving server.
    let CatalogFetch::Fetched(body) = fetched else {
        anyhow::bail!("{source} catalog fetch returned HTTP 304 to an unconditional request");
    };
    let providers = project(&body.bytes).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let ProjectedSnapshot { snapshot, .. } = projected_snapshot(
        providers,
        vendored_source,
        body.etag,
        OffsetDateTime::now_utc(),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(PricingCatalogSnapshotFile {
        metadata: snapshot.metadata,
        providers: snapshot.document.providers,
    })
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
