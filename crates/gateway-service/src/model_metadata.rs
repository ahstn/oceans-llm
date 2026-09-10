//! API-key-scoped discovery. Reads stored catalogs only; no upstream requests.
mod catalog;
pub use catalog::{FieldConflict, MergeReport, SupplementProvenance};
use std::collections::{BTreeSet, HashMap};

use crate::model_resolution::{execution_model_from_snapshot, load_missing_alias_targets};
use gateway_core::{
    GatewayError, GatewayModel, ModelRepository, ModelRoute, ProviderConnection, ProviderRepository,
};
use serde::Serialize;

use crate::pricing_catalog::{
    PricingCatalogCostDocument, PricingCatalogLimitDocument, PricingCatalogModalitiesDocument,
    PricingCatalogSnapshot, PricingCatalogSnapshotMetadata, catalog_metadata_target_for_route,
    catalog_pricing_supported_for_route, metadata::CatalogModelMetadata,
};

#[derive(Debug, Serialize)]
pub struct ModelMetadataResponse {
    pub schema_version: u32,
    pub catalog: PricingCatalogSnapshotMetadata,
    pub supplement: SupplementProvenance,
    pub data: Vec<ModelMetadata>,
}

#[derive(Debug, Serialize)]
pub struct ModelMetadata {
    pub id: String,
    pub enabled_route_count: usize,
    pub limits: PricingCatalogLimitDocument,
    pub capabilities: ModelCapabilities,
    /// Each entry describes one enabled route. This is not a health probe.
    pub routes: Vec<RouteMetadata>,
}

#[derive(Debug, Default, Serialize)]
pub struct ModelCapabilities {
    pub reasoning: Option<bool>,
    pub tools: Option<bool>,
    pub structured_output: Option<bool>,
    pub vision: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct RouteMetadata {
    pub merge_report: MergeReport,
    pub deprecated_date: Option<String>,
    pub limits: PricingCatalogLimitDocument,
    pub capabilities: ModelCapabilities,
    pub catalog_metadata: Option<CatalogModelMetadata>,
    pub modalities: Option<PricingCatalogModalitiesDocument>,
    /// USD per million tokens. Null means unknown or unsupported billing conditions.
    pub pricing: Option<PricingCatalogCostDocument>,
    pub pricing_source: Option<&'static str>,
}

pub(crate) async fn list_metadata<R>(
    repo: &R,
    models: Vec<GatewayModel>,
    snapshot: &PricingCatalogSnapshot,
) -> Result<ModelMetadataResponse, GatewayError>
where
    R: ModelRepository + ProviderRepository,
{
    let alias_targets = load_missing_alias_targets(repo, &models).await?;
    let by_key = models
        .iter()
        .chain(&alias_targets)
        .map(|model| (model.model_key.as_str(), model))
        .collect::<HashMap<_, _>>();
    let executions = models
        .iter()
        .map(|model| execution_model_from_snapshot(&by_key, model))
        .collect::<Vec<_>>();
    let ids = executions
        .iter()
        .flatten()
        .map(|model| model.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let routes = repo.list_routes_for_models(&ids).await?;
    let keys = routes
        .values()
        .flatten()
        .filter(|route| route.enabled && route.weight > 0.0)
        .map(|route| route.provider_key.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let providers = repo.list_providers_by_keys(&keys).await?;
    let data = models
        .iter()
        .zip(executions)
        .map(|(model, execution)| {
            let details = execution
                .and_then(|execution| routes.get(&execution.id))
                .into_iter()
                .flatten()
                .filter(|route| route.enabled && route.weight > 0.0)
                .map(|route| route_metadata(route, providers.get(&route.provider_key), snapshot))
                .collect::<Vec<_>>();
            summarize(model.model_key.clone(), details)
        })
        .collect();
    Ok(ModelMetadataResponse {
        schema_version: 1,
        catalog: snapshot.metadata.clone(),
        supplement: catalog::snapshot().provenance.clone(),
        data,
    })
}

fn route_metadata(
    route: &ModelRoute,
    provider: Option<&ProviderConnection>,
    snapshot: &PricingCatalogSnapshot,
) -> RouteMetadata {
    let target = provider.and_then(|provider| catalog_metadata_target_for_route(provider, route));
    let catalog = catalog::resolve(target.as_ref(), snapshot);
    let mut limits = catalog.limits;
    if let Some(configured) = route.context_window_tokens {
        limits.context = Some(
            limits
                .context
                .map_or(configured, |value| value.min(configured)),
        );
    }
    if let Some(context) = limits.context {
        limits.input = limits.input.map(|value| value.min(context));
        limits.output = limits.output.map(|value| value.min(context));
    }
    let transport = crate::effective_route_metadata::effective_provider_route_capabilities(
        Some(route.capabilities),
        provider,
        Some(route),
    );
    let metadata = catalog.metadata.as_ref();
    let capabilities = ModelCapabilities {
        reasoning: metadata.and_then(|metadata| metadata.reasoning),
        tools: gated(
            transport.tools && provider.is_some(),
            metadata.and_then(|metadata| metadata.tool_call),
        ),
        structured_output: gated(
            transport.json_schema && provider.is_some(),
            metadata.and_then(|metadata| metadata.structured_output),
        ),
        vision: gated(
            transport.vision && provider.is_some(),
            catalog.modalities.as_ref().and_then(|modalities| {
                (!modalities.input.is_empty())
                    .then(|| modalities.input.iter().any(|value| value == "image"))
            }),
        ),
    };
    let (pricing, pricing_source) = if let Some(pricing) = &route.pricing_override {
        (
            Some(PricingCatalogCostDocument {
                input: Some(pricing.input_cost_per_million_tokens.format_4dp()),
                output: Some(pricing.output_cost_per_million_tokens.format_4dp()),
                cache_read: pricing
                    .cache_read_cost_per_million_tokens
                    .map(|value| value.format_4dp()),
                cache_write: pricing
                    .cache_write_cost_per_million_tokens
                    .map(|value| value.format_4dp()),
                ..Default::default()
            }),
            Some("configured_override"),
        )
    } else if provider
        .zip(target.as_ref())
        .is_some_and(|(provider, (id, _))| catalog_pricing_supported_for_route(provider, route, id))
    {
        let source = catalog.pricing.as_ref().map(|_| "catalog");
        (catalog.pricing, source)
    } else {
        (None, None)
    };
    RouteMetadata {
        limits,
        capabilities,
        catalog_metadata: catalog.metadata,
        merge_report: catalog.merge_report,
        deprecated_date: catalog.deprecated_date,
        modalities: catalog.modalities,
        pricing,
        pricing_source,
    }
}

fn gated(transport: bool, catalog: Option<bool>) -> Option<bool> {
    if transport { catalog } else { Some(false) }
}

fn summarize(id: String, routes: Vec<RouteMetadata>) -> ModelMetadata {
    let limits = PricingCatalogLimitDocument {
        context: minimum(routes.iter().map(|route| route.limits.context)),
        input: minimum(routes.iter().map(|route| route.limits.input)),
        output: minimum(routes.iter().map(|route| route.limits.output)),
    };
    let capabilities = ModelCapabilities {
        reasoning: guaranteed(routes.iter().map(|route| route.capabilities.reasoning)),
        tools: guaranteed(routes.iter().map(|route| route.capabilities.tools)),
        structured_output: guaranteed(
            routes
                .iter()
                .map(|route| route.capabilities.structured_output),
        ),
        vision: guaranteed(routes.iter().map(|route| route.capabilities.vision)),
    };
    ModelMetadata {
        id,
        enabled_route_count: routes.len(),
        limits,
        capabilities,
        routes,
    }
}

fn minimum(values: impl Iterator<Item = Option<i64>>) -> Option<i64> {
    values.collect::<Option<Vec<_>>>()?.into_iter().min()
}

fn guaranteed(values: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let mut result = Some(true);
    let mut count = 0;
    for value in values {
        count += 1;
        if value == Some(false) {
            return Some(false);
        }
        if value.is_none() {
            result = None;
        }
    }
    if count == 0 { None } else { result }
}

#[cfg(test)]
mod tests;
