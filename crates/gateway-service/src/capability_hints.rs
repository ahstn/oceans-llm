//! Route capability hints read from the merged model catalog.
//!
//! Hints only fill route capability fields that explicit config and compatibility
//! inference leave unset; the precedence lives in
//! [`gateway_core::effective_route_capabilities`].

use std::sync::Arc;

use gateway_core::{ModelRoute, PartialCapabilities, ProviderConnection};

use crate::pricing_catalog::{
    PricingCatalogModelDocument, PricingCatalogSnapshot, catalog_metadata_target_for_route,
    catalog_model_id_fallback,
};

/// The capability fields a catalog entry states. Streaming, developer-role, and
/// decisions support are adapter facts no catalog source describes, so they stay unset.
pub(crate) fn hints_for_model(model: Option<&PricingCatalogModelDocument>) -> PartialCapabilities {
    let Some(model) = model else {
        return PartialCapabilities::default();
    };
    let metadata = &model.metadata;
    let endpoints = metadata.api_endpoints;
    let input = &model.modalities.input;
    PartialCapabilities {
        chat_completions: endpoints.map(|endpoints| endpoints.chat_completions),
        responses: endpoints.map(|endpoints| endpoints.responses),
        embeddings: endpoints.map(|endpoints| endpoints.embeddings),
        images: endpoints.map(|endpoints| endpoints.images),
        tools: metadata.tool_call,
        json_schema: metadata.structured_output,
        // An empty list means the source did not say, not "text only".
        vision: (!input.is_empty()).then(|| input.iter().any(|modality| modality == "image")),
        stream: None,
        decisions: None,
        developer_role: None,
    }
}

/// Hints for the catalog entry `route` resolves to through `provider`; empty when the
/// route has no catalog identity or the catalog lacks the model.
pub(crate) fn hints_for_route(
    snapshot: &PricingCatalogSnapshot,
    provider: &ProviderConnection,
    route: &ModelRoute,
) -> PartialCapabilities {
    let Some((provider_id, model_id)) = catalog_metadata_target_for_route(provider, route) else {
        return PartialCapabilities::default();
    };
    let model = snapshot.model(&provider_id, &model_id).or_else(|| {
        catalog_model_id_fallback(&provider_id, &model_id)
            .and_then(|fallback| snapshot.model(&provider_id, &fallback))
    });
    hints_for_model(model)
}

/// Capability hints from one catalog snapshot; cheap to clone.
#[derive(Debug, Clone)]
pub struct CatalogCapabilityHints {
    snapshot: Arc<PricingCatalogSnapshot>,
}

impl CatalogCapabilityHints {
    pub(crate) const fn new(snapshot: Arc<PricingCatalogSnapshot>) -> Self {
        Self { snapshot }
    }

    /// Hints for `route`; a route whose provider is not stored has no catalog identity.
    pub(crate) fn for_route(
        &self,
        provider: Option<&ProviderConnection>,
        route: &ModelRoute,
    ) -> PartialCapabilities {
        provider.map_or_else(PartialCapabilities::default, |provider| {
            hints_for_route(&self.snapshot, provider, route)
        })
    }
}

#[cfg(test)]
mod tests {
    use gateway_core::{ProviderCapabilities, effective_route_capabilities};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::pricing_catalog::{
        PricingCatalogModalitiesDocument, PricingCatalogProviderDocument,
        load_vendored_fallback_snapshot,
        metadata::{CatalogApiEndpoints, CatalogModelMetadata},
    };

    fn model(metadata: CatalogModelMetadata, input: &[&str]) -> PricingCatalogModelDocument {
        PricingCatalogModelDocument {
            id: "hinted".into(),
            display_name: "Hinted".into(),
            release_date: String::new(),
            last_updated: String::new(),
            metadata,
            cost: Default::default(),
            limit: Default::default(),
            modalities: PricingCatalogModalitiesDocument {
                input: input.iter().map(ToString::to_string).collect(),
                output: Vec::new(),
            },
        }
    }

    const IMAGES_ONLY: CatalogApiEndpoints = CatalogApiEndpoints {
        chat_completions: false,
        responses: false,
        embeddings: false,
        images: true,
    };

    fn provider(provider_type: &str, config: serde_json::Value) -> ProviderConnection {
        ProviderConnection {
            provider_key: "upstream".into(),
            provider_type: provider_type.into(),
            config,
            secrets: None,
        }
    }

    fn route(upstream_model: &str, capabilities: PartialCapabilities) -> ModelRoute {
        ModelRoute {
            id: Uuid::from_u128(1),
            model_id: Uuid::from_u128(2),
            provider_key: "upstream".into(),
            upstream_model: upstream_model.into(),
            priority: 0,
            weight: 1.0,
            enabled: true,
            context_window_tokens: None,
            pricing_override: None,
            extra_headers: Default::default(),
            extra_body: Default::default(),
            capabilities,
            compatibility: Default::default(),
        }
    }

    #[test]
    fn missing_model_gives_no_hints() {
        assert_eq!(hints_for_model(None), PartialCapabilities::default());
    }

    #[test]
    fn catalog_fields_map_to_their_capability() {
        let metadata = CatalogModelMetadata {
            tool_call: Some(false),
            structured_output: Some(true),
            api_endpoints: Some(CatalogApiEndpoints {
                chat_completions: true,
                responses: false,
                embeddings: false,
                images: true,
            }),
            ..Default::default()
        };
        let hints = hints_for_model(Some(&model(metadata, &["text"])));
        assert_eq!(
            hints,
            PartialCapabilities {
                chat_completions: Some(true),
                responses: Some(false),
                embeddings: Some(false),
                images: Some(true),
                tools: Some(false),
                json_schema: Some(true),
                vision: Some(false),
                ..Default::default()
            }
        );
        let vision = hints_for_model(Some(&model(Default::default(), &["text", "image"])));
        assert_eq!(vision.vision, Some(true));
    }

    #[test]
    fn unknown_endpoints_and_modalities_stay_unset() {
        let metadata = CatalogModelMetadata {
            reasoning: Some(true),
            tool_call: Some(true),
            structured_output: Some(true),
            attachment: Some(true),
            api_endpoints: None,
            ..Default::default()
        };
        let hints = hints_for_model(Some(&model(metadata, &[])));
        assert_eq!(
            (
                hints.chat_completions,
                hints.responses,
                hints.embeddings,
                hints.images
            ),
            (None, None, None, None)
        );
        assert_eq!(hints.vision, None, "empty input modalities are unknown");
        assert_eq!(
            (hints.stream, hints.developer_role, hints.decisions),
            (None, None, None),
            "no catalog field decides stream, developer_role, or decisions"
        );
    }

    #[test]
    fn image_only_catalog_entry_disables_chat_unless_the_route_says_otherwise() {
        let mut snapshot = load_vendored_fallback_snapshot();
        let metadata = CatalogModelMetadata {
            api_endpoints: Some(IMAGES_ONLY),
            ..Default::default()
        };
        snapshot
            .document
            .providers
            .get_mut("openai")
            .expect("vendored catalog has openai")
            .models
            .insert("image-only-test".into(), model(metadata, &["text"]));
        let provider = provider("openai_compat", json!({"pricing_provider_id": "openai"}));
        let effective = |route: &ModelRoute| {
            effective_route_capabilities(
                route,
                &provider.provider_type,
                hints_for_route(&snapshot, &provider, route),
                ProviderCapabilities::all_enabled(),
            )
        };

        let unset = effective(&route("image-only-test", PartialCapabilities::default()));
        assert!(!unset.chat_completions);
        assert!(unset.images);

        let explicit = effective(&route(
            "image-only-test",
            PartialCapabilities {
                chat_completions: Some(true),
                ..Default::default()
            },
        ));
        assert!(explicit.chat_completions, "explicit route config wins");
    }

    #[test]
    fn route_lookup_uses_the_catalog_id_fallback() {
        let mut snapshot = load_vendored_fallback_snapshot();
        let metadata = CatalogModelMetadata {
            tool_call: Some(false),
            ..Default::default()
        };
        snapshot
            .document
            .providers
            .entry("google-vertex-anthropic".into())
            .or_insert_with(|| PricingCatalogProviderDocument {
                display_name: "Vertex Anthropic".into(),
                models: Default::default(),
            })
            .models
            .insert(
                "claude-hint-test@default".into(),
                model(metadata, &["text"]),
            );
        let provider = provider("gcp_vertex", json!({}));

        let hints = hints_for_route(
            &snapshot,
            &provider,
            &route("anthropic/claude-hint-test", PartialCapabilities::default()),
        );
        assert_eq!(hints.tools, Some(false));
    }
}
