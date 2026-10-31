//! One route target's entry in the merged catalog snapshot, with its merge report.
use crate::pricing_catalog::{
    PricingCatalogCostDocument, PricingCatalogLimitDocument, PricingCatalogModalitiesDocument,
    PricingCatalogModelDocument, PricingCatalogSnapshot, catalog_model_id_fallback,
    merge::MergedModelReport, metadata::CatalogModelMetadata,
};

#[derive(Default)]
pub(super) struct CatalogModel {
    pub metadata: Option<CatalogModelMetadata>,
    pub limits: PricingCatalogLimitDocument,
    pub pricing: Option<PricingCatalogCostDocument>,
    pub modalities: Option<PricingCatalogModalitiesDocument>,
    pub display_name: Option<String>,
    pub release_date: Option<String>,
    pub merge_report: Option<MergedModelReport>,
}

/// The merged catalog entry for `target`, trying the exact model id before its
/// [`catalog_model_id_fallback`], the same order billing resolves prices in.
pub(super) fn resolve(
    target: Option<&(String, String)>,
    snapshot: &PricingCatalogSnapshot,
) -> CatalogModel {
    let Some((model, report)) =
        target.and_then(|(provider_id, model_id)| find(snapshot, provider_id, model_id))
    else {
        return CatalogModel::default();
    };
    CatalogModel {
        metadata: Some(model.metadata.clone()),
        limits: model.limit.clone(),
        pricing: has_rates(&model.cost).then(|| model.cost.clone()),
        modalities: Some(model.modalities.clone()),
        display_name: Some(model.display_name.clone()),
        release_date: Some(model.release_date.clone()),
        merge_report: report.cloned(),
    }
}

fn find<'snap>(
    snapshot: &'snap PricingCatalogSnapshot,
    provider_id: &str,
    model_id: &str,
) -> Option<(
    &'snap PricingCatalogModelDocument,
    Option<&'snap MergedModelReport>,
)> {
    if let Some(model) = snapshot.model(provider_id, model_id) {
        return Some((model, snapshot.report(provider_id, model_id)));
    }
    let fallback = catalog_model_id_fallback(provider_id, model_id)?;
    let model = snapshot.model(provider_id, &fallback)?;
    Some((model, snapshot.report(provider_id, &fallback)))
}

fn has_rates(pricing: &PricingCatalogCostDocument) -> bool {
    pricing.input.is_some()
        || pricing.output.is_some()
        || pricing.cache_read.is_some()
        || pricing.cache_write.is_some()
        || pricing.input_audio.is_some()
        || pricing.output_audio.is_some()
        || pricing.output_image.is_some()
        || !pricing.tiers.is_empty()
        || !pricing.conditions.is_empty()
}
