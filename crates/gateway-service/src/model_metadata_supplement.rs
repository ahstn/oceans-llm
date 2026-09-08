//! Reviewed, exact OpenAI matches. The supplement is isolated from billing records.
use std::{collections::BTreeMap, sync::OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pricing_catalog::{
    PricingCatalogCostDocument, PricingCatalogLimitDocument, metadata::CatalogModelMetadata,
};

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Supplement {
    pub source: String,
    pub provider_id: String,
    pub generated_at: String,
    pub models_dev_sha256: String,
    pub litellm_sha256: String,
    #[serde(skip_serializing)]
    pub models: BTreeMap<String, SupplementModel>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SupplementModel {
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

pub(crate) fn snapshot() -> &'static Supplement {
    static SNAPSHOT: OnceLock<Supplement> = OnceLock::new();
    SNAPSHOT.get_or_init(|| {
        serde_json::from_str(include_str!("../data/model_metadata_supplement.json"))
            .expect("reviewed model metadata supplement must deserialize")
    })
}

pub(crate) fn merge(
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
    fn field<T: PartialEq + Serialize>(
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
                models_dev: serde_json::to_value(value).expect("scalar metadata serializes"),
                litellm: serde_json::to_value(secondary).expect("scalar metadata serializes"),
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
