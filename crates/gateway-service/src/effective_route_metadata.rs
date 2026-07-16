use gateway_core::{
    GatewayError, ModelRoute, Money4, PricingCatalogRepository, PricingLimits, PricingModalities,
    PricingProvenance, ProviderConnection,
};
use time::OffsetDateTime;

use crate::pricing_catalog::{
    catalog_metadata_target_for_route, catalog_pricing_supported_for_route,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveMetadataSourceKind {
    ConfiguredOverride,
    Catalog,
    Mixed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveMetadataSource {
    pub kind: EffectiveMetadataSourceKind,
    pub catalog_source: Option<String>,
    pub catalog_etag: Option<String>,
    pub catalog_fetched_at: Option<OffsetDateTime>,
}

impl EffectiveMetadataSource {
    #[must_use]
    pub const fn configured_override() -> Self {
        Self {
            kind: EffectiveMetadataSourceKind::ConfiguredOverride,
            catalog_source: None,
            catalog_etag: None,
            catalog_fetched_at: None,
        }
    }

    #[must_use]
    pub fn catalog(provenance: &PricingProvenance) -> Self {
        Self {
            kind: EffectiveMetadataSourceKind::Catalog,
            catalog_source: Some(provenance.source.clone()),
            catalog_etag: provenance.etag.clone(),
            catalog_fetched_at: Some(provenance.fetched_at),
        }
    }

    #[must_use]
    pub const fn mixed() -> Self {
        Self {
            kind: EffectiveMetadataSourceKind::Mixed,
            catalog_source: None,
            catalog_etag: None,
            catalog_fetched_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRoutePricing {
    pub input_cost_per_million_tokens: Option<Money4>,
    pub output_cost_per_million_tokens: Option<Money4>,
    pub cache_read_cost_per_million_tokens: Option<Money4>,
    pub cache_write_cost_per_million_tokens: Option<Money4>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRouteMetadata {
    pub display_name: Option<String>,
    pub pricing: Option<EffectiveRoutePricing>,
    pub pricing_source: Option<EffectiveMetadataSource>,
    pub catalog_limits: PricingLimits,
    pub limits: PricingLimits,
    pub context_source: Option<EffectiveMetadataSource>,
    pub modalities: Option<PricingModalities>,
}

pub async fn resolve_effective_route_metadata<R>(
    repo: &R,
    provider: Option<&ProviderConnection>,
    route: &ModelRoute,
    occurred_at: OffsetDateTime,
) -> Result<EffectiveRouteMetadata, GatewayError>
where
    R: PricingCatalogRepository + Send + Sync + 'static,
{
    let (catalog_record, catalog_pricing_supported) = match provider
        .and_then(|provider| catalog_metadata_target_for_route(provider, route))
    {
        Some((pricing_provider_id, pricing_model_id)) => {
            let pricing_supported = provider.is_some_and(|provider| {
                catalog_pricing_supported_for_route(provider, route, &pricing_provider_id)
            });
            (
                repo.resolve_model_pricing_at(&pricing_provider_id, &pricing_model_id, occurred_at)
                    .await?,
                pricing_supported,
            )
        }
        None => (None, false),
    };

    let catalog_source = catalog_record
        .as_ref()
        .map(|record| EffectiveMetadataSource::catalog(&record.provenance));
    let pricing = if let Some(pricing) = &route.pricing_override {
        Some(EffectiveRoutePricing {
            input_cost_per_million_tokens: Some(pricing.input_cost_per_million_tokens),
            output_cost_per_million_tokens: Some(pricing.output_cost_per_million_tokens),
            cache_read_cost_per_million_tokens: pricing.cache_read_cost_per_million_tokens,
            cache_write_cost_per_million_tokens: pricing.cache_write_cost_per_million_tokens,
        })
    } else if catalog_pricing_supported {
        catalog_record.as_ref().map(|record| EffectiveRoutePricing {
            input_cost_per_million_tokens: record.input_cost_per_million_tokens,
            output_cost_per_million_tokens: record.output_cost_per_million_tokens,
            cache_read_cost_per_million_tokens: record.cache_read_cost_per_million_tokens,
            cache_write_cost_per_million_tokens: record.cache_write_cost_per_million_tokens,
        })
    } else {
        None
    };
    let pricing_source = if route.pricing_override.is_some() {
        Some(EffectiveMetadataSource::configured_override())
    } else if catalog_pricing_supported {
        catalog_source.clone()
    } else {
        None
    };

    let catalog_limits = catalog_record
        .as_ref()
        .map(|record| record.limits.clone())
        .unwrap_or(PricingLimits {
            context: None,
            input: None,
            output: None,
        });
    let (limits, context_source) =
        effective_limits(&catalog_limits, catalog_source, route.context_window_tokens);

    Ok(EffectiveRouteMetadata {
        display_name: catalog_record
            .as_ref()
            .map(|record| record.display_name.clone()),
        pricing,
        pricing_source,
        catalog_limits,
        limits,
        context_source,
        modalities: catalog_record
            .as_ref()
            .map(|record| record.modalities.clone()),
    })
}

fn effective_limits(
    catalog: &PricingLimits,
    catalog_source: Option<EffectiveMetadataSource>,
    configured_context: Option<i64>,
) -> (PricingLimits, Option<EffectiveMetadataSource>) {
    let Some(configured_context) = configured_context else {
        return (catalog.clone(), catalog.context.and(catalog_source));
    };

    let (context, context_source) = match catalog.context {
        Some(catalog_context) if catalog_context < configured_context => {
            (catalog_context, catalog_source)
        }
        _ => (
            configured_context,
            Some(EffectiveMetadataSource::configured_override()),
        ),
    };

    (
        PricingLimits {
            context: Some(context),
            input: catalog.input.map(|value| value.min(context)),
            output: catalog.output.map(|value| value.min(context)),
        },
        context_source,
    )
}

#[cfg(test)]
mod tests {
    use super::{EffectiveMetadataSource, EffectiveMetadataSourceKind, effective_limits};
    use gateway_core::{PricingLimits, PricingProvenance};
    use time::OffsetDateTime;

    fn catalog_source() -> EffectiveMetadataSource {
        EffectiveMetadataSource::catalog(&PricingProvenance {
            source: "models_dev_api".to_string(),
            etag: Some("etag-1".to_string()),
            fetched_at: OffsetDateTime::UNIX_EPOCH,
        })
    }

    #[test]
    fn configured_context_caps_catalog_dimensions() {
        let catalog = PricingLimits {
            context: Some(256_000),
            input: Some(200_000),
            output: Some(160_000),
        };

        let (effective, source) = effective_limits(&catalog, Some(catalog_source()), Some(128_000));

        assert_eq!(
            effective,
            PricingLimits {
                context: Some(128_000),
                input: Some(128_000),
                output: Some(128_000),
            }
        );
        assert_eq!(
            source.map(|source| source.kind),
            Some(EffectiveMetadataSourceKind::ConfiguredOverride)
        );
    }

    #[test]
    fn smaller_catalog_context_remains_authoritative() {
        let catalog = PricingLimits {
            context: Some(64_000),
            input: Some(48_000),
            output: Some(32_000),
        };

        let (effective, source) = effective_limits(&catalog, Some(catalog_source()), Some(128_000));

        assert_eq!(effective, catalog);
        assert_eq!(
            source.map(|source| source.kind),
            Some(EffectiveMetadataSourceKind::Catalog)
        );
    }

    #[test]
    fn configured_context_is_used_when_catalog_context_is_unknown() {
        let catalog = PricingLimits {
            context: None,
            input: None,
            output: None,
        };

        let (effective, source) = effective_limits(&catalog, None, Some(128_000));

        assert_eq!(effective.context, Some(128_000));
        assert_eq!(effective.input, None);
        assert_eq!(effective.output, None);
        assert_eq!(
            source.map(|source| source.kind),
            Some(EffectiveMetadataSourceKind::ConfiguredOverride)
        );
    }

    #[test]
    fn absent_configured_context_preserves_catalog_limits_and_provenance() {
        let catalog = PricingLimits {
            context: Some(256_000),
            input: Some(200_000),
            output: Some(64_000),
        };

        let (effective, source) = effective_limits(&catalog, Some(catalog_source()), None);

        assert_eq!(effective, catalog);
        assert_eq!(
            source.map(|source| source.kind),
            Some(EffectiveMetadataSourceKind::Catalog)
        );
    }
}
