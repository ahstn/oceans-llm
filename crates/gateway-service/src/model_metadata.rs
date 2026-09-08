//! API-key-scoped discovery. Reads stored catalogs only; no upstream requests.
use std::collections::{BTreeSet, HashMap};

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
    pub supplement: serde_json::Value,
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
    pub merge_report: crate::model_metadata_supplement::MergeReport,
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
    let all_models = repo.list_models().await?;
    let by_key = all_models
        .iter()
        .map(|model| (model.model_key.as_str(), model))
        .collect::<HashMap<_, _>>();
    let executions = models
        .iter()
        .map(|model| execution_model(&by_key, model))
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
        .filter(|route| route.enabled)
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
                .filter(|route| route.enabled)
                .map(|route| route_metadata(route, providers.get(&route.provider_key), snapshot))
                .collect::<Vec<_>>();
            summarize(model.model_key.clone(), details)
        })
        .collect();
    Ok(ModelMetadataResponse {
        schema_version: 1,
        catalog: snapshot.metadata.clone(),
        supplement: serde_json::to_value(crate::model_metadata_supplement::snapshot())
            .map_err(|error| GatewayError::Internal(error.to_string()))?,
        data,
    })
}

fn execution_model<'a>(
    models: &HashMap<&str, &'a GatewayModel>,
    model: &'a GatewayModel,
) -> Option<&'a GatewayModel> {
    let mut current = model;
    let mut seen = BTreeSet::new();
    for _ in 0..=8 {
        if !seen.insert(current.model_key.as_str()) {
            return None;
        }
        let Some(target) = current.alias_target_model_key.as_deref() else {
            return Some(current);
        };
        current = models.get(target)?;
    }
    None
}

fn route_metadata(
    route: &ModelRoute,
    provider: Option<&ProviderConnection>,
    snapshot: &PricingCatalogSnapshot,
) -> RouteMetadata {
    let target = provider.and_then(|provider| catalog_metadata_target_for_route(provider, route));
    let catalog = target
        .as_ref()
        .and_then(|(provider, model)| snapshot.document.providers.get(provider)?.models.get(model));
    let mut limits = catalog.map(|model| model.limit.clone()).unwrap_or_default();
    let mut metadata = catalog
        .map(|model| model.metadata.clone())
        .unwrap_or_default();
    let mut catalog_pricing = catalog.map(|model| model.cost.clone()).unwrap_or_default();
    let supplement = crate::model_metadata_supplement::snapshot();
    let secondary = target
        .as_ref()
        .filter(|(id, _)| id == &supplement.provider_id)
        .and_then(|(_, id)| supplement.models.get(id));
    let merge_report = secondary
        .map(|secondary| {
            crate::model_metadata_supplement::merge(
                &mut metadata,
                &mut limits,
                &mut catalog_pricing,
                secondary,
            )
        })
        .unwrap_or_default();
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
    let transport = crate::admin_models::effective_provider_route_capabilities(
        Some(route.capabilities),
        provider,
        Some(route),
    );
    let capabilities = ModelCapabilities {
        reasoning: metadata.reasoning,
        tools: gated(transport.tools && provider.is_some(), metadata.tool_call),
        structured_output: gated(
            transport.json_schema && provider.is_some(),
            metadata.structured_output,
        ),
        vision: gated(
            transport.vision && provider.is_some(),
            catalog.and_then(|model| {
                (!model.modalities.input.is_empty())
                    .then(|| model.modalities.input.iter().any(|value| value == "image"))
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
        (
            (catalog.is_some() || secondary.is_some()).then_some(catalog_pricing),
            (catalog.is_some() || secondary.is_some()).then_some("catalog"),
        )
    } else {
        (None, None)
    };
    RouteMetadata {
        limits,
        capabilities,
        catalog_metadata: (catalog.is_some() || secondary.is_some()).then_some(metadata),
        merge_report,
        deprecated_date: secondary.and_then(|model| model.deprecated_date.clone()),
        modalities: catalog.map(|model| model.modalities.clone()),
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
