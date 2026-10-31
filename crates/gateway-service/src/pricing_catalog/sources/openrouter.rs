//! Projection of OpenRouter's `/api/v1/models?output_modalities=all` list onto the
//! gateway's `openrouter` catalog provider.

use std::collections::BTreeMap;

use gateway_core::GatewayError;
use serde::Deserialize;
use serde_json::Value;
use tracing::debug;

use super::{Providers, per_token_to_per_million};
use crate::pricing_catalog::{
    PricingCatalogCostDocument, PricingCatalogLimitDocument, PricingCatalogModalitiesDocument,
    PricingCatalogModelDocument, PricingCatalogProviderDocument,
    metadata::{CatalogApiEndpoints, CatalogModelMetadata},
    target::OPENROUTER_PRICING_PROVIDER_ID,
};

#[derive(Debug, Deserialize)]
struct OpenRouterModels {
    data: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModel {
    id: String,
    #[serde(default)]
    name: String,
    context_length: Option<i64>,
    architecture: Option<OpenRouterArchitecture>,
    pricing: Option<OpenRouterPricing>,
    top_provider: Option<OpenRouterTopProvider>,
    supported_parameters: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenRouterArchitecture {
    #[serde(default)]
    input_modalities: Vec<String>,
    #[serde(default)]
    output_modalities: Vec<String>,
}

/// Per-token USD prices as decimal strings; `"-1"` marks a variable price.
#[derive(Debug, Default, Deserialize)]
struct OpenRouterPricing {
    prompt: Option<String>,
    completion: Option<String>,
    input_cache_read: Option<String>,
    input_cache_write: Option<String>,
    image_output: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenRouterTopProvider {
    max_completion_tokens: Option<i64>,
}

/// Project the OpenRouter model list into a providers document holding only the
/// `openrouter` provider. Models with unexpected field types are skipped.
///
/// # Errors
///
/// Fails when the body is not an object with a `data` array.
pub(crate) fn project(raw: &[u8]) -> Result<Providers, GatewayError> {
    let list = serde_json::from_slice::<OpenRouterModels>(raw).map_err(|error| {
        GatewayError::Internal(format!("failed parsing OpenRouter model list: {error}"))
    })?;

    let mut models = BTreeMap::new();
    for value in &list.data {
        let model = match OpenRouterModel::deserialize(value) {
            Ok(model) => model,
            Err(error) => {
                debug!(%error, "skipping OpenRouter model with unexpected fields");
                continue;
            }
        };
        if model.id.trim().is_empty() {
            continue;
        }
        models.insert(model.id.clone(), model_document(model));
    }

    let mut providers = Providers::new();
    if !models.is_empty() {
        providers.insert(
            OPENROUTER_PRICING_PROVIDER_ID.to_string(),
            PricingCatalogProviderDocument {
                display_name: "OpenRouter".to_string(),
                models,
            },
        );
    }
    Ok(providers)
}

fn model_document(model: OpenRouterModel) -> PricingCatalogModelDocument {
    let architecture = model.architecture.unwrap_or_default();
    let api_endpoints =
        (architecture.output_modalities == ["image"]).then(|| CatalogApiEndpoints {
            images: true,
            ..CatalogApiEndpoints::default()
        });
    let supports = |parameter: &str| {
        model
            .supported_parameters
            .as_ref()
            .map(|parameters| parameters.iter().any(|supported| supported == parameter))
    };
    let metadata = CatalogModelMetadata {
        reasoning: supports("reasoning"),
        tool_call: supports("tools"),
        structured_output: supports("structured_outputs"),
        api_endpoints,
        ..CatalogModelMetadata::default()
    };
    let display_name = if model.name.trim().is_empty() {
        model.id.clone()
    } else {
        model.name
    };
    PricingCatalogModelDocument {
        id: model.id,
        display_name,
        release_date: String::new(),
        last_updated: String::new(),
        metadata,
        cost: cost_document(&model.pricing.unwrap_or_default()),
        limit: PricingCatalogLimitDocument {
            context: model.context_length,
            input: None,
            output: model
                .top_provider
                .and_then(|provider| provider.max_completion_tokens),
        },
        modalities: PricingCatalogModalitiesDocument {
            input: catalog_modalities(architecture.input_modalities),
            output: catalog_modalities(architecture.output_modalities),
        },
    }
}

/// The price schedule, or none at all when any price is variable (negative), so a
/// partial schedule is never billed.
fn cost_document(pricing: &OpenRouterPricing) -> PricingCatalogCostDocument {
    let prices = [
        &pricing.prompt,
        &pricing.completion,
        &pricing.input_cache_read,
        &pricing.input_cache_write,
        &pricing.image_output,
    ];
    let variable = prices
        .into_iter()
        .flatten()
        .any(|price| price.trim_start().starts_with('-'));
    if variable {
        return PricingCatalogCostDocument::default();
    }
    let rate = |price: &Option<String>| price.as_deref().and_then(per_token_to_per_million);
    PricingCatalogCostDocument {
        input: rate(&pricing.prompt),
        output: rate(&pricing.completion),
        cache_read: rate(&pricing.input_cache_read),
        cache_write: rate(&pricing.input_cache_write),
        output_image: rate(&pricing.image_output),
        ..PricingCatalogCostDocument::default()
    }
}

/// OpenRouter modality names in the catalog vocabulary: `file` is `pdf`.
fn catalog_modalities(modalities: Vec<String>) -> Vec<String> {
    modalities
        .into_iter()
        .map(|modality| {
            if modality == "file" {
                "pdf".to_string()
            } else {
                modality
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn project_models(data: Value) -> Providers {
        project(json!({ "data": data }).to_string().as_bytes()).expect("project OpenRouter fixture")
    }

    #[test]
    fn projects_prices_limits_modalities_and_parameters() {
        let providers = project_models(json!([{
            "id": "anthropic/claude-sonnet-4.6",
            "name": "Anthropic: Claude Sonnet 4.6",
            "context_length": 1_000_000,
            "architecture": {"input_modalities": ["text", "image", "file"], "output_modalities": ["text"]},
            "pricing": {
                "prompt": "0.000003", "completion": "0.000015",
                "input_cache_read": "0.0000003", "input_cache_write": "0.00000375",
                "request": "0"
            },
            "top_provider": {"max_completion_tokens": 64000},
            "supported_parameters": ["tools", "reasoning", "max_tokens"]
        }]));
        let model = &providers["openrouter"].models["anthropic/claude-sonnet-4.6"];
        assert_eq!(model.display_name, "Anthropic: Claude Sonnet 4.6");
        assert_eq!(model.cost.input.as_deref(), Some("3.0000"));
        assert_eq!(model.cost.output.as_deref(), Some("15.0000"));
        assert_eq!(model.cost.cache_read.as_deref(), Some("0.3000"));
        assert_eq!(model.cost.cache_write.as_deref(), Some("3.7500"));
        assert_eq!(model.cost.output_image, None);
        assert_eq!(model.limit.context, Some(1_000_000));
        assert_eq!(model.limit.output, Some(64_000));
        assert_eq!(model.modalities.input, ["text", "image", "pdf"]);
        assert_eq!(model.metadata.tool_call, Some(true));
        assert_eq!(model.metadata.reasoning, Some(true));
        assert_eq!(model.metadata.structured_output, Some(false));
        assert_eq!(model.metadata.api_endpoints, None);
    }

    #[test]
    fn variable_price_skips_the_whole_schedule() {
        let providers = project_models(json!([{
            "id": "openrouter/auto",
            "pricing": {"prompt": "-1", "completion": "-1"}
        }, {
            "id": "vendor/mixed",
            "pricing": {"prompt": "0.000001", "completion": "-1"}
        }]));
        for model_id in ["openrouter/auto", "vendor/mixed"] {
            let cost = &providers["openrouter"].models[model_id].cost;
            assert_eq!(cost.input, None, "{model_id}");
            assert_eq!(cost.output, None, "{model_id}");
        }
        let auto = &providers["openrouter"].models["openrouter/auto"];
        assert_eq!(auto.display_name, "openrouter/auto");
        assert_eq!(auto.metadata.tool_call, None);
    }

    #[test]
    fn image_only_output_lists_only_the_images_endpoint() {
        let providers = project_models(json!([{
            "id": "openai/gpt-image-2.5-flare",
            "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["image"]},
            "pricing": {"prompt": "0.000005", "completion": "0.00001", "image_output": "0.00004"}
        }, {
            "id": "google/gemini-3-pro-image",
            "architecture": {"input_modalities": ["text"], "output_modalities": ["image", "text"]}
        }]));
        let models = &providers["openrouter"].models;
        let image_only = &models["openai/gpt-image-2.5-flare"];
        assert_eq!(
            image_only.metadata.api_endpoints,
            Some(CatalogApiEndpoints {
                images: true,
                ..CatalogApiEndpoints::default()
            })
        );
        assert_eq!(image_only.cost.output_image.as_deref(), Some("40.0000"));
        assert_eq!(
            models["google/gemini-3-pro-image"].metadata.api_endpoints,
            None
        );
    }
}
