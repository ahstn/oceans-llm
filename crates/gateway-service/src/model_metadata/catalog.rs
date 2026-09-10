//! Reviewed, exact OpenAI matches. The supplement is isolated from billing records.
use std::{collections::BTreeMap, sync::OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pricing_catalog::{
    PricingCatalogCostDocument, PricingCatalogLimitDocument, PricingCatalogModalitiesDocument,
    PricingCatalogSnapshot, metadata::CatalogModelMetadata,
};

#[derive(Debug, Deserialize)]
pub(super) struct Supplement {
    #[serde(flatten)]
    pub provenance: SupplementProvenance,
    models: BTreeMap<String, SupplementModel>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SupplementProvenance {
    pub source: String,
    pub provider_id: String,
    pub generated_at: String,
    pub models_dev_sha256: String,
    pub litellm_sha256: String,
}

#[derive(Debug, Deserialize)]
struct SupplementModel {
    pub metadata: CatalogModelMetadata,
    pub pricing: PricingCatalogCostDocument,
    pub limits: PricingCatalogLimitDocument,
    pub deprecated_date: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct MergeReport {
    pub supplemented_fields: Vec<&'static str>,
    pub conflicts: Vec<FieldConflict>,
}

#[derive(Debug, Serialize)]
pub struct FieldConflict {
    pub field: &'static str,
    pub models_dev: Value,
    pub litellm: Value,
}

pub(super) fn snapshot() -> &'static Supplement {
    static SNAPSHOT: OnceLock<Supplement> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        serde_json::from_str(include_str!("../../data/model_metadata_supplement.json"))
            .expect("reviewed model metadata supplement must deserialize")
    })
}

#[derive(Default)]
pub(super) struct CatalogModel {
    pub metadata: Option<CatalogModelMetadata>,
    pub limits: PricingCatalogLimitDocument,
    pub pricing: Option<PricingCatalogCostDocument>,
    pub modalities: Option<PricingCatalogModalitiesDocument>,
    pub deprecated_date: Option<String>,
    pub merge_report: MergeReport,
}

pub(super) fn resolve(
    target: Option<&(String, String)>,
    catalog: &PricingCatalogSnapshot,
) -> CatalogModel {
    let Some((provider_id, model_id)) = target else {
        return CatalogModel::default();
    };
    let primary = catalog
        .document
        .providers
        .get(provider_id)
        .and_then(|provider| provider.models.get(model_id));
    let supplement = snapshot();
    let secondary = (provider_id == &supplement.provenance.provider_id)
        .then(|| supplement.models.get(model_id))
        .flatten();
    if primary.is_none() && secondary.is_none() {
        return CatalogModel::default();
    }
    let mut metadata = primary
        .map(|model| model.metadata.clone())
        .unwrap_or_default();
    let mut limits = primary.map(|model| model.limit.clone()).unwrap_or_default();
    let mut pricing = primary.map(|model| model.cost.clone()).unwrap_or_default();
    let merge_report = secondary
        .map(|secondary| merge(&mut metadata, &mut limits, &mut pricing, secondary))
        .unwrap_or_default();
    CatalogModel {
        metadata: Some(metadata),
        limits,
        pricing: has_rates(&pricing).then_some(pricing),
        modalities: primary.map(|model| model.modalities.clone()),
        deprecated_date: secondary.and_then(|model| model.deprecated_date.clone()),
        merge_report,
    }
}

fn has_rates(pricing: &PricingCatalogCostDocument) -> bool {
    pricing.input.is_some()
        || pricing.output.is_some()
        || pricing.cache_read.is_some()
        || pricing.cache_write.is_some()
        || pricing.input_audio.is_some()
        || pricing.output_audio.is_some()
        || !pricing.conditions.is_empty()
}

fn merge(
    metadata: &mut CatalogModelMetadata,
    limits: &mut PricingCatalogLimitDocument,
    pricing: &mut PricingCatalogCostDocument,
    supplement: &SupplementModel,
) -> MergeReport {
    let mut report = MergeReport::default();
    report.field(
        "reasoning",
        &mut metadata.reasoning,
        supplement.metadata.reasoning,
    );
    report.field(
        "tool_call",
        &mut metadata.tool_call,
        supplement.metadata.tool_call,
    );
    report.field(
        "structured_output",
        &mut metadata.structured_output,
        supplement.metadata.structured_output,
    );
    report.field("limit.input", &mut limits.input, supplement.limits.input);
    report.field("limit.output", &mut limits.output, supplement.limits.output);
    for (name, primary, secondary) in [
        ("cost.input", &mut pricing.input, &supplement.pricing.input),
        (
            "cost.output",
            &mut pricing.output,
            &supplement.pricing.output,
        ),
        (
            "cost.cache_read",
            &mut pricing.cache_read,
            &supplement.pricing.cache_read,
        ),
        (
            "cost.cache_write",
            &mut pricing.cache_write,
            &supplement.pricing.cache_write,
        ),
    ] {
        // Catalog rates are decimal strings. Ignore formatting-only differences.
        if primary.as_deref().map(canonical_decimal) != secondary.as_deref().map(canonical_decimal)
        {
            report.field(name, primary, secondary.clone());
        }
    }
    report
}

fn canonical_decimal(value: &str) -> &str {
    if value.contains('.') {
        value.trim_end_matches('0').trim_end_matches('.')
    } else {
        value
    }
}

impl MergeReport {
    fn field<T: PartialEq + Clone + Into<Value>>(
        &mut self,
        field: &'static str,
        primary: &mut Option<T>,
        secondary: Option<T>,
    ) {
        let Some(secondary) = secondary else {
            return;
        };
        match primary {
            None => {
                *primary = Some(secondary);
                self.supplemented_fields.push(field);
            }
            Some(value) if value != &secondary => self.conflicts.push(FieldConflict {
                field,
                models_dev: value.clone().into(),
                litellm: secondary.into(),
            }),
            Some(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_only_merge_retains_false_zero_and_reports_conflicts() {
        let secondary: SupplementModel = serde_json::from_value(json!({
            "metadata":{"reasoning":true,"tool_call":false},
            "limits":{"input":200,"output":100},
            "pricing":{"input":"1","output":"0.00"}, "deprecated_date":null
        }))
        .unwrap();
        let mut primary = CatalogModelMetadata {
            reasoning: Some(false),
            ..Default::default()
        };
        let mut limits = PricingCatalogLimitDocument {
            context: Some(300),
            ..Default::default()
        };
        let mut cost = PricingCatalogCostDocument {
            input: Some("0.0000".into()),
            output: Some("0.0000".into()),
            ..Default::default()
        };
        let report = merge(&mut primary, &mut limits, &mut cost, &secondary);
        assert_eq!(primary.reasoning, Some(false));
        assert_eq!(primary.tool_call, Some(false));
        assert_eq!(cost.input.as_deref(), Some("0.0000"));
        assert_eq!(limits.context, Some(300));
        assert_eq!(limits.input, Some(200));
        assert_eq!(
            report
                .conflicts
                .iter()
                .map(|conflict| conflict.field)
                .collect::<Vec<_>>(),
            vec!["reasoning", "cost.input"]
        );
        assert!(report.supplemented_fields.contains(&"tool_call"));
    }
}
