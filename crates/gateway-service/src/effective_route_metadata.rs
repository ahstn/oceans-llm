use gateway_client_config::ApiFormat;
use gateway_core::{
    AwsBedrockApiStyle, ContextPricingTier, GatewayError, GitHubCopilotChatApi, ModelPricingRecord,
    ModelRoute, Money4, PartialCapabilities, PricingCatalogRepository, PricingLimits,
    PricingModalities, PricingProvenance, ProviderCapabilities, ProviderConnection,
    RoutePricingOverride, ServiceTier, ServiceTierPricing, effective_route_capabilities,
    github_copilot_route_capabilities, vertex_route_capabilities_for_upstream_model,
};
use time::OffsetDateTime;

use crate::pricing_catalog::{
    catalog_metadata_target_for_route, catalog_pricing_location_supported,
    resolve_catalog_pricing_record,
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
    /// Catalog context tiers, ascending by threshold. Empty for a route
    /// `pricing_override`, which replaces the whole catalog schedule.
    pub context_tiers: Vec<ContextPricingTier>,
    /// Rates for non-default service tiers, sorted by tier, tiers unique. A catalog price
    /// lists the catalog's tier schedules. An override lists its base rates under the
    /// route's own non-default tier, then its flat `service_tiers` rates.
    pub service_tiers: Vec<ServiceTierPricing>,
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
    let (catalog_record, catalog_location_supported) =
        match provider.and_then(|provider| catalog_metadata_target_for_route(provider, route)) {
            Some((pricing_provider_id, pricing_model_id)) => {
                let location_supported = provider.is_some_and(|provider| {
                    catalog_pricing_location_supported(provider, &pricing_provider_id)
                });
                (
                    resolve_catalog_pricing_record(
                        repo,
                        &pricing_provider_id,
                        &pricing_model_id,
                        occurred_at,
                    )
                    .await?,
                    location_supported,
                )
            }
            None => (None, false),
        };

    let catalog_source = catalog_record
        .as_ref()
        .map(|record| EffectiveMetadataSource::catalog(&record.provenance));
    let pricing = if let Some(pricing) = &route.pricing_override {
        Some(override_pricing(pricing, route.service_tier))
    } else if catalog_location_supported {
        catalog_record
            .as_ref()
            .and_then(|record| catalog_pricing(record, route.service_tier))
    } else {
        None
    };
    let pricing_source = if route.pricing_override.is_some() {
        Some(EffectiveMetadataSource::configured_override())
    } else if pricing.is_some() {
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

/// The catalog schedule the route bills: the base schedule, or the schedule of the
/// non-default tier the route forces. A forced tier the catalog has no rates for is
/// unpriced, never billed at the base rates.
fn catalog_pricing(
    record: &ModelPricingRecord,
    route_tier: Option<ServiceTier>,
) -> Option<EffectiveRoutePricing> {
    let mut pricing = match route_tier.filter(|tier| *tier != ServiceTier::Default) {
        None => EffectiveRoutePricing {
            input_cost_per_million_tokens: record.input_cost_per_million_tokens,
            output_cost_per_million_tokens: record.output_cost_per_million_tokens,
            cache_read_cost_per_million_tokens: record.cache_read_cost_per_million_tokens,
            cache_write_cost_per_million_tokens: record.cache_write_cost_per_million_tokens,
            context_tiers: record.context_tiers.clone(),
            service_tiers: record.service_tiers.clone(),
        },
        Some(forced) => {
            let schedule = record
                .service_tiers
                .iter()
                .find(|schedule| schedule.service_tier == forced)?;
            EffectiveRoutePricing {
                input_cost_per_million_tokens: schedule.input,
                output_cost_per_million_tokens: schedule.output,
                cache_read_cost_per_million_tokens: schedule.cache_read,
                cache_write_cost_per_million_tokens: schedule.cache_write,
                context_tiers: schedule.context_tiers.clone(),
                service_tiers: record.service_tiers.clone(),
            }
        }
    };
    pricing
        .context_tiers
        .sort_by_key(|tier| tier.above_input_tokens);
    Some(pricing)
}

/// An override's base rates price the tier the route serves by default, so a route that
/// forces a non-default tier lists them under that tier as well.
fn override_pricing(
    pricing: &RoutePricingOverride,
    route_tier: Option<ServiceTier>,
) -> EffectiveRoutePricing {
    let own_tier = route_tier
        .filter(|tier| *tier != ServiceTier::Default)
        .map(|service_tier| ServiceTierPricing {
            service_tier,
            input: Some(pricing.input_cost_per_million_tokens),
            output: Some(pricing.output_cost_per_million_tokens),
            cache_read: pricing.cache_read_cost_per_million_tokens,
            cache_write: pricing.cache_write_cost_per_million_tokens,
            cache_write_1h: pricing.cache_write_1h_cost_per_million_tokens,
            context_tiers: Vec::new(),
        });
    let mut service_tiers = own_tier
        .into_iter()
        .chain(pricing.service_tiers.iter().map(|tier| ServiceTierPricing {
            service_tier: tier.service_tier,
            input: Some(tier.input_cost_per_million_tokens),
            output: Some(tier.output_cost_per_million_tokens),
            cache_read: tier.cache_read_cost_per_million_tokens,
            cache_write: tier.cache_write_cost_per_million_tokens,
            cache_write_1h: tier.cache_write_1h_cost_per_million_tokens,
            context_tiers: Vec::new(),
        }))
        .collect::<Vec<_>>();
    // Stable sort, so the route's own tier wins a duplicate.
    service_tiers.sort_by_key(|tier| tier.service_tier);
    service_tiers.dedup_by_key(|tier| tier.service_tier);
    EffectiveRoutePricing {
        input_cost_per_million_tokens: Some(pricing.input_cost_per_million_tokens),
        output_cost_per_million_tokens: Some(pricing.output_cost_per_million_tokens),
        cache_read_cost_per_million_tokens: pricing.cache_read_cost_per_million_tokens,
        cache_write_cost_per_million_tokens: pricing.cache_write_cost_per_million_tokens,
        context_tiers: Vec::new(),
        service_tiers,
    }
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

/// What `route` serves through its stored `provider`: the shared effective chain (explicit,
/// inferred, catalog `hints`, defaults) capped by [`provider_capabilities`]. A route whose
/// provider is not stored has no adapter ceiling.
pub(crate) fn effective_provider_route_capabilities(
    route: &ModelRoute,
    provider: Option<&ProviderConnection>,
    hints: PartialCapabilities,
) -> ProviderCapabilities {
    let (provider_type, adapter) =
        provider.map_or(("", ProviderCapabilities::all_enabled()), |provider| {
            (
                provider.provider_type.as_str(),
                provider_capabilities(provider, Some(route)),
            )
        });
    effective_route_capabilities(route, provider_type, hints, adapter)
}

pub(crate) fn provider_capabilities(
    provider: &ProviderConnection,
    route: Option<&ModelRoute>,
) -> ProviderCapabilities {
    match provider.provider_type.as_str() {
        "openai_compat" | "gcp_cloud_run_openai_compat" => {
            let mut capabilities = ProviderCapabilities::openai_compat_baseline();
            if route.is_some_and(|route| route.compatibility.selects_decisions_api()) {
                capabilities.decisions = true;
            }
            capabilities
        }
        "anthropic_compat" => ProviderCapabilities {
            chat_completions: true,
            responses: false,
            stream: true,
            embeddings: false,
            images: false,
            decisions: false,
            tools: true,
            vision: true,
            json_schema: false,
            developer_role: false,
        },
        "gcp_vertex" => vertex_route_capabilities_for_upstream_model(
            route.map(|route| route.upstream_model.as_str()),
        ),
        "github_copilot" => github_copilot_route_capabilities(
            route.and_then(|route| route.compatibility.github_copilot.as_ref()),
        ),
        "aws_bedrock" => ProviderCapabilities {
            chat_completions: true,
            responses: true,
            stream: true,
            embeddings: false,
            images: false,
            decisions: false,
            tools: true,
            vision: true,
            json_schema: true,
            developer_role: true,
        },
        "typesafe" => ProviderCapabilities {
            chat_completions: false,
            responses: false,
            stream: false,
            embeddings: false,
            images: false,
            decisions: true,
            tools: false,
            vision: false,
            json_schema: false,
            developer_role: false,
        },
        _ => ProviderCapabilities::all_enabled(),
    }
}

/// The client-facing format a route's upstream speaks natively, when the provider type
/// makes it knowable. OpenRouter Claude is known to use Messages; other generic OpenAI
/// providers and Bedrock Converse return `None` and fall back to model-name matching.
pub(crate) fn native_api_format(
    provider: &ProviderConnection,
    route: &ModelRoute,
) -> Option<ApiFormat> {
    match provider.provider_type.as_str() {
        "openai_compat" | "gcp_cloud_run_openai_compat"
            if gateway_core::route_capabilities::openrouter_uses_native_messages(
                &route.upstream_model,
                route.compatibility.openrouter.is_some(),
                provider
                    .config
                    .get("base_url")
                    .and_then(serde_json::Value::as_str),
            ) =>
        {
            Some(ApiFormat::AnthropicMessages)
        }
        "anthropic_compat" => Some(ApiFormat::AnthropicMessages),
        "gcp_vertex" if route.upstream_model.starts_with("anthropic/") => {
            Some(ApiFormat::AnthropicMessages)
        }
        "gcp_vertex" => Some(ApiFormat::OpenAiChatCompletions),
        "github_copilot" => {
            let compatibility = route.compatibility.github_copilot.as_ref()?;
            match compatibility.chat_api {
                Some(GitHubCopilotChatApi::AnthropicMessages) => Some(ApiFormat::AnthropicMessages),
                Some(GitHubCopilotChatApi::ChatCompletions) => {
                    Some(ApiFormat::OpenAiChatCompletions)
                }
                None if compatibility.supports_responses => Some(ApiFormat::OpenAiResponses),
                None => None,
            }
        }
        "aws_bedrock" => match route.compatibility.aws_bedrock.as_ref()?.api_style {
            AwsBedrockApiStyle::RuntimeAnthropicInvoke
            | AwsBedrockApiStyle::MantleAnthropicMessages => Some(ApiFormat::AnthropicMessages),
            AwsBedrockApiStyle::RuntimeOpenaiChat | AwsBedrockApiStyle::MantleOpenaiChat => {
                Some(ApiFormat::OpenAiChatCompletions)
            }
            AwsBedrockApiStyle::MantleOpenaiResponses => Some(ApiFormat::OpenAiResponses),
            AwsBedrockApiStyle::RuntimeConverse => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{EffectiveMetadataSource, EffectiveMetadataSourceKind, effective_limits};
    use gateway_core::{PricingLimits, PricingProvenance};
    use time::OffsetDateTime;

    #[test]
    fn openrouter_claude_routes_advertise_native_messages() {
        let mut provider = gateway_core::ProviderConnection {
            provider_key: "openrouter".into(),
            provider_type: "openai_compat".into(),
            config: serde_json::json!({"base_url":"https://openrouter.ai/api/v1"}),
            secrets: None,
        };
        let mut route = gateway_core::ModelRoute {
            id: uuid::Uuid::new_v4(),
            model_id: uuid::Uuid::new_v4(),
            provider_key: "openrouter".into(),
            upstream_model: "~anthropic/claude-opus-latest".into(),
            priority: 0,
            weight: 1.0,
            enabled: true,
            context_window_tokens: None,
            pricing_override: None,
            extra_headers: Default::default(),
            extra_body: Default::default(),
            capabilities: Default::default(),
            compatibility: Default::default(),
            service_tier: None,
        };
        for model in ["~anthropic/claude-opus-latest", "anthropic/claude-opus-5.5"] {
            route.upstream_model = model.into();
            for provider_type in ["openai_compat", "gcp_cloud_run_openai_compat"] {
                provider.provider_type = provider_type.into();
                for base_url in [
                    "https://openrouter.ai/api/v1",
                    "https://openrouter.ai/api/v1/",
                    "https://openrouter.ai/api/v1///",
                ] {
                    provider.config = serde_json::json!({"base_url":base_url});
                    assert_eq!(
                        super::native_api_format(&provider, &route),
                        Some(gateway_client_config::ApiFormat::AnthropicMessages)
                    );
                }
            }
            assert_eq!(
                gateway_client_config::infer_thinking_policy([model]),
                Some(gateway_client_config::ThinkingPolicy::AnthropicSafeEffort)
            );
        }
        for config in [
            serde_json::json!({"base_url":"https://example.test/v1"}),
            serde_json::json!({"base_url":"https://openrouter.ai/api"}),
            serde_json::json!({"base_url":"not a URL"}),
            serde_json::json!({}),
        ] {
            provider.config = config;
            assert_eq!(super::native_api_format(&provider, &route), None);
        }
        route.compatibility.openrouter =
            Some(serde_json::from_value(serde_json::json!({})).unwrap());
        assert_eq!(
            super::native_api_format(&provider, &route),
            Some(gateway_client_config::ApiFormat::AnthropicMessages)
        );
        route.upstream_model = "openai/gpt-4o-mini".into();
        assert_eq!(super::native_api_format(&provider, &route), None);
    }

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

    #[test]
    fn bedrock_native_format_follows_the_route_api_style() {
        use gateway_client_config::ApiFormat;
        use gateway_core::{
            AwsBedrockApiStyle, AwsBedrockRouteCompatibility, ModelRoute, PartialCapabilities,
            ProviderConnection,
        };

        let provider = ProviderConnection {
            provider_key: "bedrock".into(),
            provider_type: "aws_bedrock".into(),
            config: serde_json::json!({}),
            secrets: None,
        };
        let mut route = ModelRoute {
            id: uuid::Uuid::new_v4(),
            model_id: uuid::Uuid::new_v4(),
            provider_key: "bedrock".into(),
            upstream_model: "us.anthropic.claude-opus-4-7".into(),
            priority: 0,
            weight: 1.0,
            enabled: true,
            context_window_tokens: None,
            pricing_override: None,
            extra_headers: Default::default(),
            extra_body: Default::default(),
            capabilities: PartialCapabilities::default(),
            compatibility: Default::default(),
            service_tier: None,
        };
        assert_eq!(super::native_api_format(&provider, &route), None);

        for (api_style, expected) in [
            (
                AwsBedrockApiStyle::RuntimeAnthropicInvoke,
                Some(ApiFormat::AnthropicMessages),
            ),
            (
                AwsBedrockApiStyle::MantleAnthropicMessages,
                Some(ApiFormat::AnthropicMessages),
            ),
            (
                AwsBedrockApiStyle::MantleOpenaiChat,
                Some(ApiFormat::OpenAiChatCompletions),
            ),
            (
                AwsBedrockApiStyle::MantleOpenaiResponses,
                Some(ApiFormat::OpenAiResponses),
            ),
            (AwsBedrockApiStyle::RuntimeConverse, None),
        ] {
            route.compatibility.aws_bedrock = Some(AwsBedrockRouteCompatibility {
                api_style,
                openai_base_path: None,
                supports_strict_tools: None,
            });
            assert_eq!(
                super::native_api_format(&provider, &route),
                expected,
                "{api_style:?}"
            );
        }
    }
}
