//! Projection of LiteLLM's `model_prices_and_context_window.json` onto gateway catalog
//! identities, by exact key only.

mod cost;

use std::{borrow::Cow, collections::BTreeMap};

use gateway_core::GatewayError;
use serde::Deserialize;
use serde_json::{Map, Number, Value};
use tracing::debug;

use super::Providers;
use crate::pricing_catalog::{
    PricingCatalogLimitDocument, PricingCatalogModalitiesDocument, PricingCatalogModelDocument,
    PricingCatalogProviderDocument,
    metadata::{CatalogApiEndpoints, CatalogModelMetadata},
    target::{
        AMAZON_BEDROCK_PRICING_PROVIDER_ID, GITHUB_COPILOT_PRICING_PROVIDER_ID,
        GOOGLE_VERTEX_ANTHROPIC_PRICING_PROVIDER_ID, GOOGLE_VERTEX_PRICING_PROVIDER_ID,
        OPENAI_PRICING_PROVIDER_ID, catalog_model_id_fallback,
    },
};

/// The documentation entry at the top of the LiteLLM table; not a model.
const SAMPLE_SPEC_KEY: &str = "sample_spec";
const VERTEX_ANTHROPIC_LITELLM_PROVIDER: &str = "vertex_ai-anthropic_models";
const ANTHROPIC_LITELLM_PROVIDER: &str = "anthropic";

/// Whether a LiteLLM key names a gateway model directly or only when no direct key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentityRank {
    Primary,
    Fallback,
}

#[derive(Debug, Deserialize)]
struct LiteLlmEntry {
    #[serde(default)]
    litellm_provider: String,
    mode: Option<String>,
    supported_regions: Option<Value>,
    max_input_tokens: Option<Number>,
    max_output_tokens: Option<Number>,
    supports_reasoning: Option<bool>,
    supports_function_calling: Option<bool>,
    supports_response_schema: Option<bool>,
    supports_vision: Option<bool>,
    supports_pdf_input: Option<bool>,
    supports_audio_input: Option<bool>,
    supported_modalities: Option<Vec<String>>,
    supported_output_modalities: Option<Vec<String>>,
    supported_endpoints: Option<Vec<String>>,
}

/// Project the LiteLLM table into gateway providers documents. Entries that do not map
/// to a gateway identity, or whose fields have unexpected types, are skipped.
///
/// # Errors
///
/// Fails when the body is not a JSON object.
pub(crate) fn project(raw: &[u8]) -> Result<Providers, GatewayError> {
    let entries = serde_json::from_slice::<Map<String, Value>>(raw).map_err(|error| {
        GatewayError::Internal(format!("failed parsing LiteLLM catalog: {error}"))
    })?;
    let anthropic_fast_multipliers = anthropic_fast_multipliers(&entries);

    let mut providers = Providers::new();
    let mut fallbacks = Vec::new();
    for (key, value) in &entries {
        if key == SAMPLE_SPEC_KEY {
            continue;
        }
        let Some(fields) = value.as_object() else {
            continue;
        };
        let entry = match LiteLlmEntry::deserialize(value) {
            Ok(entry) => entry,
            Err(error) => {
                debug!(key = %key, %error, "skipping LiteLLM catalog entry with unexpected fields");
                continue;
            }
        };
        let Some((provider_id, model_id, rank)) = gateway_identity(key, &entry) else {
            continue;
        };
        let fast_multiplier = own_fast_multiplier(fields).or_else(|| {
            inherited_fast_multiplier(provider_id, &model_id, &anthropic_fast_multipliers)
        });
        let document = model_document(&model_id, entry, fields, fast_multiplier);
        match rank {
            IdentityRank::Primary => {
                provider_models(&mut providers, provider_id)
                    .insert(model_id.into_owned(), document);
            }
            IdentityRank::Fallback => fallbacks.push((provider_id, model_id, document)),
        }
    }
    for (provider_id, model_id, document) in fallbacks {
        provider_models(&mut providers, provider_id)
            .entry(model_id.into_owned())
            .or_insert(document);
    }
    Ok(providers)
}

/// Anthropic's fast mode multiplier per model family, from the bare Anthropic keys that
/// list one. Anthropic itself is not a gateway pricing provider.
fn anthropic_fast_multipliers(entries: &Map<String, Value>) -> BTreeMap<&str, f64> {
    entries
        .iter()
        .filter(|(key, _)| !key.contains('/'))
        .filter_map(|(key, value)| {
            let fields = value.as_object()?;
            let provider = fields.get("litellm_provider")?.as_str()?;
            (provider == ANTHROPIC_LITELLM_PROVIDER)
                .then(|| own_fast_multiplier(fields))
                .flatten()
                .map(|multiplier| (key.as_str(), multiplier))
        })
        .collect()
}

/// The fast mode multiplier an entry lists in `provider_specific_entry.fast`: the
/// priority tier costs that multiple of the base schedule.
fn own_fast_multiplier(fields: &Map<String, Value>) -> Option<f64> {
    fields
        .get("provider_specific_entry")?
        .get("fast")?
        .as_f64()
        .filter(|multiplier| multiplier.is_finite() && *multiplier > 0.0)
}

/// Anthropic's multiplier for a Vertex Anthropic model of the same family: models.dev
/// prices Vertex's Claude fast mode at the same multiple as Anthropic's. Bedrock's
/// priority tier is AWS's own and Copilot serves only the default tier, so neither
/// inherits it.
fn inherited_fast_multiplier(
    provider_id: &str,
    model_id: &str,
    anthropic_fast_multipliers: &BTreeMap<&str, f64>,
) -> Option<f64> {
    if provider_id != GOOGLE_VERTEX_ANTHROPIC_PRICING_PROVIDER_ID {
        return None;
    }
    let family = model_id
        .split_once('@')
        .map_or(model_id, |(family, _)| family);
    anthropic_fast_multipliers.get(family).copied()
}

fn provider_models<'doc>(
    providers: &'doc mut Providers,
    provider_id: &str,
) -> &'doc mut BTreeMap<String, PricingCatalogModelDocument> {
    &mut providers
        .entry(provider_id.to_string())
        .or_insert_with(|| PricingCatalogProviderDocument {
            display_name: provider_display_name(provider_id).to_string(),
            models: BTreeMap::new(),
        })
        .models
}

/// Provider display names, matching models.dev so merged documents read the same.
fn provider_display_name(provider_id: &str) -> &'static str {
    match provider_id {
        OPENAI_PRICING_PROVIDER_ID => "OpenAI",
        GITHUB_COPILOT_PRICING_PROVIDER_ID => "GitHub Copilot",
        AMAZON_BEDROCK_PRICING_PROVIDER_ID => "Amazon Bedrock",
        GOOGLE_VERTEX_PRICING_PROVIDER_ID => "Vertex",
        GOOGLE_VERTEX_ANTHROPIC_PRICING_PROVIDER_ID => "Vertex (Anthropic)",
        _ => "",
    }
}

/// The gateway provider id and model id a LiteLLM key names, by exact key shape only.
///
/// Vertex Anthropic ids take the versioned form models.dev and catalog lookups use: a
/// bare `vertex_ai/<model>` key becomes `<model>@default` and only fills that id when
/// no explicit `vertex_ai/<model>@default` key exists.
fn gateway_identity<'key>(
    key: &'key str,
    entry: &LiteLlmEntry,
) -> Option<(&'static str, Cow<'key, str>, IdentityRank)> {
    let (provider_id, model_id, rank) = if let Some(model) = key.strip_prefix("github_copilot/") {
        (
            GITHUB_COPILOT_PRICING_PROVIDER_ID,
            model,
            IdentityRank::Primary,
        )
    } else if let Some(model) = key.strip_prefix("bedrock_mantle/") {
        (
            AMAZON_BEDROCK_PRICING_PROVIDER_ID,
            model,
            IdentityRank::Fallback,
        )
    } else if let Some(model) = key.strip_prefix("vertex_ai/") {
        let provider = entry.litellm_provider.as_str();
        let provider_id = if provider == VERTEX_ANTHROPIC_LITELLM_PROVIDER {
            GOOGLE_VERTEX_ANTHROPIC_PRICING_PROVIDER_ID
        } else if provider.starts_with("vertex_ai") {
            GOOGLE_VERTEX_PRICING_PROVIDER_ID
        } else {
            return None;
        };
        (provider_id, model, IdentityRank::Primary)
    } else if key.contains('/') {
        return None;
    } else {
        let provider_id = match entry.litellm_provider.as_str() {
            "openai" if entry.supported_regions.is_none() => OPENAI_PRICING_PROVIDER_ID,
            "bedrock" | "bedrock_converse" => AMAZON_BEDROCK_PRICING_PROVIDER_ID,
            _ => return None,
        };
        (provider_id, key, IdentityRank::Primary)
    };
    if model_id.is_empty() {
        return None;
    }
    let identity = match catalog_model_id_fallback(provider_id, model_id) {
        Some(versioned) => (provider_id, Cow::Owned(versioned), IdentityRank::Fallback),
        None => (provider_id, Cow::Borrowed(model_id), rank),
    };
    Some(identity)
}

fn model_document(
    model_id: &str,
    entry: LiteLlmEntry,
    fields: &Map<String, Value>,
    fast_multiplier: Option<f64>,
) -> PricingCatalogModelDocument {
    let api_endpoints = api_endpoints(entry.supported_endpoints.as_deref(), entry.mode.as_deref());
    let other_input_support = [
        (entry.supports_pdf_input, "pdf"),
        (entry.supports_audio_input, "audio"),
    ];
    let modalities = PricingCatalogModalitiesDocument {
        input: entry.supported_modalities.unwrap_or_else(|| {
            derived_input_modalities(entry.supports_vision, other_input_support)
        }),
        output: entry
            .supported_output_modalities
            .unwrap_or_else(|| mode_output_modalities(entry.mode.as_deref())),
    };
    PricingCatalogModelDocument {
        id: model_id.to_string(),
        display_name: model_id.to_string(),
        release_date: String::new(),
        last_updated: String::new(),
        metadata: CatalogModelMetadata {
            reasoning: entry.supports_reasoning,
            tool_call: entry.supports_function_calling,
            structured_output: entry.supports_response_schema,
            api_endpoints,
            ..CatalogModelMetadata::default()
        },
        cost: cost::cost_document(fields, fast_multiplier),
        limit: PricingCatalogLimitDocument {
            context: None,
            input: entry.max_input_tokens.as_ref().and_then(token_limit),
            output: entry.max_output_tokens.as_ref().and_then(token_limit),
        },
        modalities,
    }
}

/// A token limit; LiteLLM writes some as floats (`128000.0`). Non-integral or
/// out-of-range values are not limits.
fn token_limit(number: &Number) -> Option<i64> {
    /// Integral floats below this convert to `i64` exactly.
    const MAX_EXACT_FLOAT_INTEGER: f64 = 9.0e15;
    number.as_i64().or_else(|| {
        let value = number.as_f64()?;
        (value.fract() == 0.0 && value.abs() < MAX_EXACT_FLOAT_INTEGER).then_some(value as i64)
    })
}

fn api_endpoints(supported: Option<&[String]>, mode: Option<&str>) -> Option<CatalogApiEndpoints> {
    if let Some(endpoints) = supported {
        let lists = |path: &str| endpoints.iter().any(|endpoint| endpoint == path);
        return Some(CatalogApiEndpoints {
            chat_completions: lists("/v1/chat/completions"),
            responses: lists("/v1/responses"),
            embeddings: lists("/v1/embeddings"),
            images: lists("/v1/images/generations"),
        });
    }
    let only = CatalogApiEndpoints::default();
    match mode? {
        "embedding" => Some(CatalogApiEndpoints {
            embeddings: true,
            ..only
        }),
        "image_generation" => Some(CatalogApiEndpoints {
            images: true,
            ..only
        }),
        "responses" => Some(CatalogApiEndpoints {
            responses: true,
            ..only
        }),
        _ => None,
    }
}

/// Text plus each modality whose `supports_*` flag is `true`. Empty (unknown) when the
/// vision flag is unset: a known list without `image` means "no vision", and the list
/// cannot say that image support is unknown.
fn derived_input_modalities(vision: Option<bool>, other: [(Option<bool>, &str); 2]) -> Vec<String> {
    let Some(vision) = vision else {
        return Vec::new();
    };
    let supported = std::iter::once((vision, "image"))
        .chain(other.map(|(flag, modality)| (flag == Some(true), modality)))
        .filter_map(|(supported, modality)| supported.then_some(modality));
    std::iter::once("text")
        .chain(supported)
        .map(str::to_string)
        .collect()
}

fn mode_output_modalities(mode: Option<&str>) -> Vec<String> {
    let modality = match mode {
        Some("chat" | "completion" | "responses" | "audio_transcription" | "ocr") => "text",
        Some("image_generation" | "image_edit") => "image",
        Some("audio_speech") => "audio",
        Some("video_generation") => "video",
        _ => return Vec::new(),
    };
    vec![modality.to_string()]
}

#[cfg(test)]
mod tests {
    use gateway_core::ServiceTier;
    use serde_json::json;

    use super::*;
    use crate::pricing_catalog::{PricingCatalogContextTier, PricingCatalogServiceTierCost};

    fn project_json(body: &Value) -> Providers {
        project(body.to_string().as_bytes()).expect("project LiteLLM fixture")
    }

    fn model_ids(providers: &Providers, provider_id: &str) -> Vec<String> {
        providers
            .get(provider_id)
            .map(|provider| provider.models.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[test]
    fn identity_mapping_uses_exact_keys_per_gateway_provider() {
        let providers = project_json(&json!({
            "sample_spec": {"litellm_provider": "openai", "max_tokens": "LEGACY"},
            "gpt-5": {"litellm_provider": "openai"},
            "gpt-5-eu": {"litellm_provider": "openai", "supported_regions": ["eu"]},
            "azure/gpt-5": {"litellm_provider": "azure"},
            "github_copilot/gpt-5-mini": {"litellm_provider": "github_copilot"},
            "anthropic.claude-sonnet-4-6": {"litellm_provider": "bedrock_converse"},
            "us.amazon.nova-pro-v1:0": {"litellm_provider": "bedrock"},
            "bedrock/us-east-1/anthropic.claude-sonnet-4-6": {"litellm_provider": "bedrock"},
            "bedrock_mantle/anthropic.claude-sonnet-4-6": {
                "litellm_provider": "bedrock_mantle", "input_cost_per_token": 9e-06
            },
            "bedrock_mantle/openai.gpt-oss-120b": {"litellm_provider": "bedrock_mantle"},
            "vertex_ai/gemini-2.5-pro": {"litellm_provider": "vertex_ai-language-models"},
            "vertex_ai/claude-sonnet-4-6@default": {"litellm_provider": "vertex_ai-anthropic_models"},
            "vertex_ai/mistral-large": {"litellm_provider": "mistral"},
            "gemini-2.5-pro": {"litellm_provider": "vertex_ai-language-models"},
        }));

        assert_eq!(model_ids(&providers, "openai"), ["gpt-5"]);
        assert_eq!(model_ids(&providers, "github-copilot"), ["gpt-5-mini"]);
        assert_eq!(
            model_ids(&providers, "amazon-bedrock"),
            [
                "anthropic.claude-sonnet-4-6",
                "openai.gpt-oss-120b",
                "us.amazon.nova-pro-v1:0"
            ]
        );
        assert_eq!(model_ids(&providers, "google-vertex"), ["gemini-2.5-pro"]);
        assert_eq!(
            model_ids(&providers, "google-vertex-anthropic"),
            ["claude-sonnet-4-6@default"]
        );
        assert_eq!(providers.len(), 5, "no other providers: {providers:?}");
        // The bare Bedrock key wins over its `bedrock_mantle/` fallback.
        assert_eq!(
            providers["amazon-bedrock"].models["anthropic.claude-sonnet-4-6"]
                .cost
                .input,
            None
        );
    }

    #[test]
    fn vertex_anthropic_default_key_wins_over_its_bare_twin() {
        let providers = project_json(&json!({
            "vertex_ai/claude-sonnet-4-6": {
                "litellm_provider": "vertex_ai-anthropic_models", "input_cost_per_token": 9e-06
            },
            "vertex_ai/claude-sonnet-4-6@default": {
                "litellm_provider": "vertex_ai-anthropic_models", "input_cost_per_token": 3e-06
            },
            "vertex_ai/claude-opus-4@20250514": {"litellm_provider": "vertex_ai-anthropic_models"},
        }));

        assert_eq!(
            model_ids(&providers, "google-vertex-anthropic"),
            ["claude-opus-4@20250514", "claude-sonnet-4-6@default"]
        );
        let model = &providers["google-vertex-anthropic"].models["claude-sonnet-4-6@default"];
        assert_eq!(model.cost.input.as_deref(), Some("3.0000"));
        assert_eq!(model.id, "claude-sonnet-4-6@default");
    }

    #[test]
    fn vertex_anthropic_bare_key_alone_takes_the_default_id() {
        let providers = project_json(&json!({
            "vertex_ai/claude-haiku-4-5": {
                "litellm_provider": "vertex_ai-anthropic_models", "input_cost_per_token": 1e-06
            },
        }));

        assert_eq!(
            model_ids(&providers, "google-vertex-anthropic"),
            ["claude-haiku-4-5@default"]
        );
        let model = &providers["google-vertex-anthropic"].models["claude-haiku-4-5@default"];
        assert_eq!(model.cost.input.as_deref(), Some("1.0000"));
        assert_eq!(model.id, "claude-haiku-4-5@default");
    }

    #[test]
    fn rates_convert_per_token_floats_to_per_million() {
        let providers = project_json(&json!({"gpt-5": {
            "litellm_provider": "openai",
            "input_cost_per_token": 3e-06,
            "output_cost_per_token": 1.5e-05,
            "cache_read_input_token_cost": 1.25e-07,
            "cache_creation_input_token_cost": 3.75e-06,
            "output_cost_per_image_token": 4e-05,
        }}));
        let cost = &providers["openai"].models["gpt-5"].cost;
        assert_eq!(cost.input.as_deref(), Some("3.0000"));
        assert_eq!(cost.output.as_deref(), Some("15.0000"));
        assert_eq!(cost.cache_read.as_deref(), Some("0.1250"));
        assert_eq!(cost.cache_write.as_deref(), Some("3.7500"));
        assert_eq!(cost.output_image.as_deref(), Some("40.0000"));
        assert!(cost.tiers.is_empty());
    }

    #[test]
    fn tiers_come_only_from_unsuffixed_above_k_tokens_keys() {
        let providers = project_json(&json!({"gpt-5": {
            "litellm_provider": "openai",
            "input_cost_per_token": 1e-06,
            "output_cost_per_token": 2e-06,
            "input_cost_per_token_above_272k_tokens": 2e-06,
            "output_cost_per_token_above_272k_tokens": 4e-06,
            "input_cost_per_token_above_128k_tokens": 1.5e-06,
            "cache_read_input_token_cost_above_128k_tokens": 3e-07,
            "input_cost_per_token_above_272k_tokens_priority": 9e-06,
            "output_cost_per_token_batches": 1e-06,
            "input_cost_per_token_above_1m_tokens": 9e-06,
            "cache_creation_input_token_cost_above_1hr": 9e-06,
            "cache_creation_input_token_cost_above_1hr_above_200k_tokens": 9e-06,
        }}));
        let tiers = &providers["openai"].models["gpt-5"].cost.tiers;
        assert_eq!(
            tiers,
            &[
                PricingCatalogContextTier {
                    above_input_tokens: 128_000,
                    input: Some("1.5000".to_string()),
                    output: None,
                    cache_read: Some("0.3000".to_string()),
                    cache_write: None,
                },
                PricingCatalogContextTier {
                    above_input_tokens: 272_000,
                    input: Some("2.0000".to_string()),
                    output: Some("4.0000".to_string()),
                    cache_read: None,
                    cache_write: None,
                },
            ]
        );
    }

    #[test]
    fn limits_metadata_and_modalities_follow_the_field_mapping() {
        let providers = project_json(&json!({
            "gpt-5": {
                "litellm_provider": "openai", "mode": "chat",
                "max_input_tokens": 272000, "max_output_tokens": 128000.0,
                "supports_reasoning": true, "supports_function_calling": false,
                "supports_response_schema": true, "supports_vision": true,
                "supports_pdf_input": true,
            },
            "gpt-image-2": {
                "litellm_provider": "openai", "mode": "image_generation",
                "supported_modalities": ["text", "image"],
                "supported_output_modalities": ["image"],
            },
            "text-embedding-3-small": {"litellm_provider": "openai", "mode": "embedding"},
            "gpt-5-pro": {
                "litellm_provider": "openai", "mode": "responses",
                "supported_endpoints": ["/v1/responses", "/v1/batch", "/v1/chat/completions"],
            },
        }));
        let models = &providers["openai"].models;

        let chat = &models["gpt-5"];
        assert_eq!(chat.limit.context, None);
        assert_eq!(chat.limit.input, Some(272_000));
        assert_eq!(chat.limit.output, Some(128_000));
        assert_eq!(chat.metadata.reasoning, Some(true));
        assert_eq!(chat.metadata.tool_call, Some(false));
        assert_eq!(chat.metadata.structured_output, Some(true));
        assert_eq!(chat.metadata.api_endpoints, None);
        assert_eq!(chat.modalities.input, ["text", "image", "pdf"]);
        assert_eq!(chat.modalities.output, ["text"]);

        let image = &models["gpt-image-2"];
        assert_eq!(image.modalities.input, ["text", "image"]);
        assert_eq!(image.modalities.output, ["image"]);
        assert_eq!(
            image.metadata.api_endpoints,
            Some(CatalogApiEndpoints {
                images: true,
                ..CatalogApiEndpoints::default()
            })
        );

        assert_eq!(
            models["text-embedding-3-small"].metadata.api_endpoints,
            Some(CatalogApiEndpoints {
                embeddings: true,
                ..CatalogApiEndpoints::default()
            })
        );
        assert_eq!(
            models["gpt-5-pro"].metadata.api_endpoints,
            Some(CatalogApiEndpoints {
                chat_completions: true,
                responses: true,
                ..CatalogApiEndpoints::default()
            })
        );
    }

    #[test]
    fn input_modalities_and_vision_hint_are_unknown_without_a_vision_flag() {
        let providers = project_json(&json!({
            "gpt-4o": {"litellm_provider": "openai", "mode": "chat"},
            "gpt-4o-mini": {
                "litellm_provider": "openai", "mode": "chat", "supports_pdf_input": true,
            },
            "gpt-4.1": {
                "litellm_provider": "openai", "mode": "chat", "supports_vision": false,
                "supports_audio_input": true,
            },
            "gpt-4.1-mini": {
                "litellm_provider": "openai", "mode": "chat", "supports_vision": true,
            },
            "gpt-4.1-nano": {
                "litellm_provider": "openai", "mode": "chat",
                "supported_modalities": ["text", "audio"], "supports_vision": true,
            },
        }));
        let models = &providers["openai"].models;
        let input_and_vision_hint = |model_id: &str| {
            let model = &models[model_id];
            let vision = crate::capability_hints::hints_for_model(Some(model)).vision;
            (model.modalities.input.clone(), vision)
        };

        assert_eq!(input_and_vision_hint("gpt-4o"), (vec![], None));
        assert_eq!(input_and_vision_hint("gpt-4o-mini"), (vec![], None));
        assert_eq!(
            input_and_vision_hint("gpt-4.1"),
            (vec!["text".to_string(), "audio".to_string()], Some(false))
        );
        assert_eq!(
            input_and_vision_hint("gpt-4.1-mini"),
            (vec!["text".to_string(), "image".to_string()], Some(true))
        );
        // Explicit modalities win over the flags.
        assert_eq!(
            input_and_vision_hint("gpt-4.1-nano"),
            (vec!["text".to_string(), "audio".to_string()], Some(false))
        );
        assert_eq!(models["gpt-4o"].modalities.output, ["text"]);
    }

    #[test]
    fn anthropic_fast_multiplier_prices_vertex_priority_but_not_bedrock() {
        let rates = json!({"input_cost_per_token": 4e-06, "output_cost_per_token": 2e-05});
        let entry = |provider: &str| {
            let mut entry = rates.clone();
            entry["litellm_provider"] = json!(provider);
            entry
        };
        let mut anthropic = entry("anthropic");
        anthropic["provider_specific_entry"] = json!({"us": 1.1, "fast": 2});
        let providers = project_json(&json!({
            "claude-opus-5-5": anthropic,
            "vertex_ai/claude-opus-5-5@default": entry("vertex_ai-anthropic_models"),
            "vertex_ai/claude-sonnet-5-5@default": entry("vertex_ai-anthropic_models"),
            "anthropic.claude-opus-5-5": entry("bedrock_converse"),
        }));

        let vertex = &providers["google-vertex-anthropic"].models;
        assert_eq!(
            vertex["claude-opus-5-5@default"].cost.service_tiers[&ServiceTier::Priority],
            PricingCatalogServiceTierCost {
                input: Some("8.0000".to_string()),
                output: Some("40.0000".to_string()),
                ..PricingCatalogServiceTierCost::default()
            }
        );
        assert!(
            vertex["claude-sonnet-5-5@default"]
                .cost
                .service_tiers
                .is_empty(),
            "a family without a multiplier gets no tier"
        );
        assert!(
            providers["amazon-bedrock"].models["anthropic.claude-opus-5-5"]
                .cost
                .service_tiers
                .is_empty()
        );
        assert!(
            !providers.contains_key("anthropic"),
            "Anthropic is not a pricing provider"
        );
    }
}
