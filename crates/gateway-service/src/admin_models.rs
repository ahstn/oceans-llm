use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gateway_client_config::{
    ClientConfig, ClientConfigInput, ClientConfigInputSet, ClientModelCapabilities,
    DEFAULT_API_KEY_ENV_VAR, DEFAULT_GATEWAY_BASE_URL, DEFAULT_PROVIDER_ID,
    infer_anthropic_thinking_policy, render_default_configs, render_default_configs_for_models,
};
use gateway_core::{
    GatewayError, GatewayModel, ModelAllowlistPolicy, ModelRepository, ModelRoute,
    PricingCatalogRepository, PricingLimits, PricingModalities, ProviderCapabilities,
    ProviderConnection, ProviderRepository, vertex_route_capabilities_for_upstream_model,
};
use time::OffsetDateTime;

use crate::{
    EffectiveMetadataSource, EffectiveRouteMetadata, ModelIconKey, ProviderIconKey,
    resolve_effective_route_metadata, resolve_model_icon_key, resolve_provider_display,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminModelStatus {
    Healthy,
    Degraded,
}

impl AdminModelStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdminModelSummary {
    pub id: String,
    pub model_id: String,
    pub resolved_model_key: String,
    pub alias_of: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub allowlist: Option<ModelAllowlistPolicy>,
    pub status: AdminModelStatus,
    pub provider_key: Option<String>,
    pub provider_label: Option<String>,
    pub provider_icon_key: Option<ProviderIconKey>,
    pub upstream_model: Option<String>,
    pub model_icon_key: Option<ModelIconKey>,
    pub input_cost_per_million_tokens_usd_10000: Option<i64>,
    pub output_cost_per_million_tokens_usd_10000: Option<i64>,
    pub cache_read_cost_per_million_tokens_usd_10000: Option<i64>,
    pub cache_write_cost_per_million_tokens_usd_10000: Option<i64>,
    pub pricing_source: Option<EffectiveMetadataSource>,
    pub pricing_varies_by_route: bool,
    pub context_window_tokens: Option<i64>,
    pub context_window_source: Option<EffectiveMetadataSource>,
    pub input_window_tokens: Option<i64>,
    pub output_window_tokens: Option<i64>,
    pub supports_streaming: Option<bool>,
    pub supports_vision: Option<bool>,
    pub supports_tool_calling: Option<bool>,
    pub supports_structured_output: Option<bool>,
    pub supports_attachments: Option<bool>,
    pub client_configurations: Vec<ClientConfig>,
}

#[derive(Debug, Clone)]
pub struct AdminModelsService<R> {
    repo: Arc<R>,
    client_config_gateway_base_url: String,
}

impl<R> AdminModelsService<R>
where
    R: ModelRepository + ProviderRepository + PricingCatalogRepository + Send + Sync + 'static,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self {
            repo,
            client_config_gateway_base_url: DEFAULT_GATEWAY_BASE_URL.to_string(),
        }
    }

    #[must_use]
    pub fn with_client_config_gateway_base_url(
        mut self,
        gateway_base_url: impl Into<String>,
    ) -> Self {
        self.client_config_gateway_base_url = gateway_base_url.into();
        self
    }

    pub async fn list_models(&self) -> Result<Vec<AdminModelSummary>, GatewayError> {
        Ok(self
            .list_model_items()
            .await?
            .into_iter()
            .map(|item| item.summary)
            .collect())
    }

    pub async fn render_client_configurations(
        &self,
        model_keys: &[String],
    ) -> Result<Vec<ClientConfig>, GatewayError> {
        if model_keys.is_empty() {
            return Err(GatewayError::InvalidRequest(
                "model_keys must include at least one model".to_string(),
            ));
        }

        let inputs_by_key = self
            .list_model_items()
            .await?
            .into_iter()
            .filter_map(|item| {
                item.client_config_input
                    .map(|input| (item.summary.id, input))
            })
            .collect::<HashMap<_, _>>();

        let mut inputs = Vec::with_capacity(model_keys.len());
        let mut seen_model_keys = HashSet::with_capacity(model_keys.len());
        for model_key in model_keys {
            if !seen_model_keys.insert(model_key) {
                return Err(GatewayError::InvalidRequest(format!(
                    "model_key `{model_key}` cannot be repeated"
                )));
            }

            let input = inputs_by_key.get(model_key).ok_or_else(|| {
                GatewayError::InvalidRequest(format!(
                    "model_key `{model_key}` is not available for client config generation"
                ))
            })?;
            inputs.push(input.clone());
        }

        Ok(render_default_configs_for_models(
            ClientConfigInputSet::new(inputs),
        ))
    }

    async fn list_model_items(&self) -> Result<Vec<AdminModelItem>, GatewayError> {
        let pricing_time = OffsetDateTime::now_utc();
        let models = self.repo.list_models().await?;
        let model_ids = models.iter().map(|model| model.id).collect::<Vec<_>>();
        let allowlists_by_model = self
            .repo
            .list_model_allowlists_for_models(&model_ids)
            .await?;
        let by_key = models
            .iter()
            .cloned()
            .map(|model| (model.model_key.clone(), model))
            .collect::<HashMap<_, _>>();
        let execution_models = models
            .iter()
            .map(|model| {
                (
                    model.model_key.clone(),
                    resolve_execution_model(&by_key, model).unwrap_or_else(|| model.clone()),
                )
            })
            .collect::<HashMap<_, _>>();
        let execution_model_ids = execution_models
            .values()
            .map(|model| model.id)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let routes_by_model = self
            .repo
            .list_routes_for_models(&execution_model_ids)
            .await?;
        let provider_keys = routes_by_model
            .values()
            .flat_map(|routes| routes.iter().map(|route| route.provider_key.clone()))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let providers_by_key = self.repo.list_providers_by_keys(&provider_keys).await?;
        let mut items = Vec::with_capacity(models.len());

        for model in models {
            let execution_model = execution_models
                .get(&model.model_key)
                .cloned()
                .unwrap_or_else(|| model.clone());
            let routes = routes_by_model
                .get(&execution_model.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let primary_route = select_display_route(&providers_by_key, routes);
            let primary_provider =
                primary_route.and_then(|route| providers_by_key.get(&route.provider_key));
            let status = route_health(&providers_by_key, routes);
            let provider_display = primary_route.map(|route| {
                resolve_provider_display(route.provider_key.as_str(), primary_provider)
            });
            let route_capabilities = primary_route.map(|route| route.capabilities);
            let primary_metadata = match primary_route {
                Some(route) => Some(
                    resolve_effective_route_metadata(
                        self.repo.as_ref(),
                        primary_provider,
                        route,
                        pricing_time,
                    )
                    .await?,
                ),
                None => None,
            };
            let mut route_metadata = Vec::new();
            for route in routes
                .iter()
                .filter(|route| route_is_eligible(&providers_by_key, route))
            {
                let metadata = if primary_route.is_some_and(|primary| primary.id == route.id) {
                    primary_metadata
                        .as_ref()
                        .expect("primary route metadata was resolved")
                        .clone()
                } else {
                    let provider = providers_by_key
                        .get(&route.provider_key)
                        .expect("eligible route provider exists");
                    resolve_effective_route_metadata(
                        self.repo.as_ref(),
                        Some(provider),
                        route,
                        pricing_time,
                    )
                    .await?
                };
                route_metadata.push((route.id, metadata));
            }
            let aggregate = aggregate_model_metadata(&route_metadata);
            let primary_metadata = primary_metadata.as_ref();
            let primary_pricing = primary_metadata.and_then(|metadata| metadata.pricing.as_ref());
            let model_icon_key = resolve_model_icon_key(
                primary_route
                    .map(|route| route.upstream_model.as_str())
                    .into_iter()
                    .chain([execution_model.model_key.as_str(), model.model_key.as_str()]),
            );
            let client_config_input = build_client_config_input(ClientConfigContext {
                model: &model,
                execution_model: &execution_model,
                primary_route,
                primary_provider,
                provider_display: provider_display.as_ref(),
                metadata: primary_metadata,
                limits: &aggregate.limits,
                route_capabilities,
                gateway_base_url: &self.client_config_gateway_base_url,
            });
            let client_configurations = client_config_input
                .as_ref()
                .map(render_default_configs)
                .unwrap_or_default();

            items.push(AdminModelItem {
                summary: AdminModelSummary {
                    id: model.model_key.clone(),
                    model_id: model.id.to_string(),
                    resolved_model_key: execution_model.model_key.clone(),
                    alias_of: model.alias_target_model_key.clone(),
                    description: model.description.clone(),
                    tags: model.tags.clone(),
                    allowlist: allowlists_by_model.get(&model.id).cloned(),
                    status,
                    provider_key: primary_route.map(|route| route.provider_key.clone()),
                    provider_label: provider_display
                        .as_ref()
                        .map(|display| display.label.clone()),
                    provider_icon_key: provider_display.map(|display| display.icon_key),
                    upstream_model: primary_route.map(|route| route.upstream_model.clone()),
                    model_icon_key,
                    input_cost_per_million_tokens_usd_10000: primary_pricing
                        .and_then(|pricing| pricing.input_cost_per_million_tokens)
                        .map(|value| value.as_scaled_i64()),
                    output_cost_per_million_tokens_usd_10000: primary_pricing
                        .and_then(|pricing| pricing.output_cost_per_million_tokens)
                        .map(|value| value.as_scaled_i64()),
                    cache_read_cost_per_million_tokens_usd_10000: primary_pricing
                        .and_then(|pricing| pricing.cache_read_cost_per_million_tokens)
                        .map(|value| value.as_scaled_i64()),
                    cache_write_cost_per_million_tokens_usd_10000: primary_pricing
                        .and_then(|pricing| pricing.cache_write_cost_per_million_tokens)
                        .map(|value| value.as_scaled_i64()),
                    pricing_source: primary_metadata
                        .and_then(|metadata| metadata.pricing_source.clone()),
                    pricing_varies_by_route: aggregate.pricing_varies_by_route,
                    context_window_tokens: aggregate.limits.context,
                    context_window_source: aggregate.context_source.clone(),
                    input_window_tokens: aggregate.limits.input,
                    output_window_tokens: aggregate.limits.output,
                    supports_streaming: route_capabilities.map(|caps| caps.stream),
                    supports_vision: route_capabilities.map(|caps| caps.vision),
                    supports_tool_calling: route_capabilities.map(|caps| caps.tools),
                    supports_structured_output: route_capabilities.map(|caps| caps.json_schema),
                    supports_attachments: primary_metadata
                        .and_then(|metadata| metadata.modalities.as_ref())
                        .map(supports_attachments),
                    client_configurations,
                },
                client_config_input,
            });
        }

        Ok(items)
    }
}

#[derive(Debug, Clone)]
struct AdminModelItem {
    summary: AdminModelSummary,
    client_config_input: Option<ClientConfigInput>,
}

struct ClientConfigContext<'a> {
    model: &'a GatewayModel,
    execution_model: &'a GatewayModel,
    primary_route: Option<&'a ModelRoute>,
    primary_provider: Option<&'a ProviderConnection>,
    provider_display: Option<&'a crate::ProviderDisplayIdentity>,
    metadata: Option<&'a EffectiveRouteMetadata>,
    limits: &'a PricingLimits,
    route_capabilities: Option<ProviderCapabilities>,
    gateway_base_url: &'a str,
}

fn build_client_config_input(context: ClientConfigContext<'_>) -> Option<ClientConfigInput> {
    let primary_route = context.primary_route?;
    context.primary_provider?;
    let thinking_policy = infer_anthropic_thinking_policy(
        Some(primary_route.upstream_model.as_str())
            .into_iter()
            .chain(
                context
                    .primary_provider
                    .map(|provider| provider.provider_key.as_str()),
            )
            .chain(
                context
                    .primary_provider
                    .map(|provider| provider.provider_type.as_str()),
            )
            .chain(
                context
                    .provider_display
                    .map(|display| display.label.as_str()),
            )
            .chain([
                context.execution_model.model_key.as_str(),
                context.model.model_key.as_str(),
            ]),
    );
    let capabilities = effective_provider_route_capabilities(
        context.route_capabilities,
        context.primary_provider,
        Some(primary_route),
    );
    let pricing = context
        .metadata
        .and_then(|metadata| metadata.pricing.as_ref());

    Some(ClientConfigInput {
        model_id: context.model.model_key.clone(),
        display_name: context
            .model
            .description
            .clone()
            .or_else(|| {
                context
                    .metadata
                    .and_then(|metadata| metadata.display_name.clone())
            })
            .unwrap_or_else(|| context.model.model_key.clone()),
        upstream_model: Some(primary_route.upstream_model.clone()),
        provider_id: DEFAULT_PROVIDER_ID.to_string(),
        provider_name: DEFAULT_PROVIDER_ID.to_string(),
        gateway_base_url: context.gateway_base_url.to_string(),
        api_key_env_var: DEFAULT_API_KEY_ENV_VAR.to_string(),
        input_cost_per_million_tokens_usd_10000: pricing
            .and_then(|pricing| pricing.input_cost_per_million_tokens)
            .map(|value| value.as_scaled_i64()),
        output_cost_per_million_tokens_usd_10000: pricing
            .and_then(|pricing| pricing.output_cost_per_million_tokens)
            .map(|value| value.as_scaled_i64()),
        cache_read_cost_per_million_tokens_usd_10000: pricing
            .and_then(|pricing| pricing.cache_read_cost_per_million_tokens)
            .map(|value| value.as_scaled_i64()),
        cache_write_cost_per_million_tokens_usd_10000: pricing
            .and_then(|pricing| pricing.cache_write_cost_per_million_tokens)
            .map(|value| value.as_scaled_i64()),
        context_window_tokens: context.limits.context,
        input_window_tokens: context.limits.input,
        output_window_tokens: context.limits.output,
        capabilities: ClientModelCapabilities {
            responses: capabilities.responses,
            tool_calling: capabilities.tools,
            attachments: context
                .metadata
                .and_then(|metadata| metadata.modalities.as_ref())
                .is_some_and(supports_attachments),
            vision: capabilities.vision,
        },
        thinking_policy,
    })
}

fn effective_provider_route_capabilities(
    route_capabilities: Option<ProviderCapabilities>,
    provider: Option<&ProviderConnection>,
    route: Option<&ModelRoute>,
) -> ProviderCapabilities {
    provider
        .map(|provider| provider_capabilities(provider, route))
        .unwrap_or_default()
        .intersect(route_capabilities.unwrap_or_default())
}

fn provider_capabilities(
    provider: &ProviderConnection,
    route: Option<&ModelRoute>,
) -> ProviderCapabilities {
    match provider.provider_type.as_str() {
        "openai_compat" | "gcp_cloud_run_openai_compat" => {
            ProviderCapabilities::openai_compat_baseline()
        }
        "gcp_vertex" => vertex_route_capabilities(route),
        "aws_bedrock" => ProviderCapabilities {
            chat_completions: true,
            responses: true,
            stream: true,
            embeddings: false,
            tools: true,
            vision: true,
            json_schema: true,
            developer_role: true,
        },
        _ => ProviderCapabilities::all_enabled(),
    }
}

fn vertex_route_capabilities(route: Option<&ModelRoute>) -> ProviderCapabilities {
    vertex_route_capabilities_for_upstream_model(route.map(|route| route.upstream_model.as_str()))
}

#[derive(Debug)]
struct AggregatedModelMetadata {
    limits: PricingLimits,
    context_source: Option<EffectiveMetadataSource>,
    pricing_varies_by_route: bool,
}

fn aggregate_model_metadata(
    route_metadata: &[(uuid::Uuid, EffectiveRouteMetadata)],
) -> AggregatedModelMetadata {
    let context = aggregate_limit(route_metadata, |metadata| metadata.limits.context);
    let input = aggregate_limit(route_metadata, |metadata| metadata.limits.input);
    let output = aggregate_limit(route_metadata, |metadata| metadata.limits.output);
    let context_source = context.and_then(|minimum| {
        merge_sources(
            route_metadata
                .iter()
                .filter(|(_, metadata)| metadata.limits.context == Some(minimum))
                .filter_map(|(_, metadata)| metadata.context_source.clone()),
        )
    });
    let pricing_varies_by_route = route_metadata.first().is_some_and(|(_, first)| {
        route_metadata
            .iter()
            .skip(1)
            .any(|(_, metadata)| metadata.pricing != first.pricing)
    });

    AggregatedModelMetadata {
        limits: PricingLimits {
            context,
            input,
            output,
        },
        context_source,
        pricing_varies_by_route,
    }
}

fn aggregate_limit(
    route_metadata: &[(uuid::Uuid, EffectiveRouteMetadata)],
    select: impl Fn(&EffectiveRouteMetadata) -> Option<i64>,
) -> Option<i64> {
    let mut values = route_metadata.iter().map(|(_, metadata)| select(metadata));
    let first = values.next()??;
    values.try_fold(first, |minimum, value| {
        value.map(|value| minimum.min(value))
    })
}

fn merge_sources(
    mut sources: impl Iterator<Item = EffectiveMetadataSource>,
) -> Option<EffectiveMetadataSource> {
    let first = sources.next()?;
    if sources.all(|source| source == first) {
        Some(first)
    } else {
        Some(EffectiveMetadataSource::mixed())
    }
}

fn supports_attachments(modalities: &PricingModalities) -> bool {
    modalities
        .input
        .iter()
        .any(|value| matches!(value.as_str(), "audio" | "file" | "image" | "pdf" | "video"))
}

fn route_is_eligible(
    providers_by_key: &HashMap<String, ProviderConnection>,
    route: &ModelRoute,
) -> bool {
    route.enabled && route.weight > 0.0 && providers_by_key.contains_key(&route.provider_key)
}

fn route_health(
    providers_by_key: &HashMap<String, ProviderConnection>,
    routes: &[gateway_core::ModelRoute],
) -> AdminModelStatus {
    if routes
        .iter()
        .any(|route| route_is_eligible(providers_by_key, route))
    {
        AdminModelStatus::Healthy
    } else {
        AdminModelStatus::Degraded
    }
}

fn select_display_route<'a>(
    providers_by_key: &HashMap<String, ProviderConnection>,
    routes: &'a [gateway_core::ModelRoute],
) -> Option<&'a gateway_core::ModelRoute> {
    routes
        .iter()
        .find(|route| route_is_eligible(providers_by_key, route))
        .or_else(|| {
            routes
                .iter()
                .find(|route| providers_by_key.contains_key(&route.provider_key))
        })
        .or_else(|| routes.iter().find(|route| route.enabled))
        .or_else(|| routes.first())
}

fn resolve_execution_model(
    by_key: &HashMap<String, GatewayModel>,
    model: &GatewayModel,
) -> Option<GatewayModel> {
    let mut current = model.clone();
    let mut seen = std::collections::BTreeSet::from([model.model_key.clone()]);

    loop {
        let Some(alias_of) = current.alias_target_model_key.clone() else {
            return Some(current);
        };

        let next = by_key.get(&alias_of)?.clone();
        if !seen.insert(next.model_key.clone()) {
            return None;
        }
        current = next;
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use gateway_core::{
        GatewayError, GatewayModel, ModelAllowlistPolicy, ModelPricingRecord,
        ModelPricingSyncChanges, ModelRepository, ModelRoute, Money4, PricingCatalogCacheRecord,
        PricingCatalogRepository, PricingLimits, PricingModalities, PricingProvenance,
        ProviderCapabilities, ProviderConnection, ProviderRepository, StoreError,
    };
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::{
        AdminModelStatus, AdminModelsService, EffectiveMetadataSource, EffectiveRouteMetadata,
        aggregate_model_metadata, effective_provider_route_capabilities, provider_capabilities,
    };
    use crate::EffectiveRoutePricing;

    fn effective_metadata(
        limits: PricingLimits,
        pricing: Option<EffectiveRoutePricing>,
    ) -> EffectiveRouteMetadata {
        EffectiveRouteMetadata {
            display_name: None,
            pricing,
            pricing_source: Some(EffectiveMetadataSource::configured_override()),
            catalog_limits: PricingLimits {
                context: None,
                input: None,
                output: None,
            },
            limits,
            context_source: Some(EffectiveMetadataSource::configured_override()),
            modalities: Some(PricingModalities {
                input: Vec::new(),
                output: Vec::new(),
            }),
        }
    }

    #[test]
    fn empty_route_metadata_has_unknown_aggregate_limits() {
        let aggregate = aggregate_model_metadata(&[]);

        assert_eq!(aggregate.limits.context, None);
        assert_eq!(aggregate.limits.input, None);
        assert_eq!(aggregate.limits.output, None);
        assert_eq!(aggregate.context_source, None);
        assert!(!aggregate.pricing_varies_by_route);
    }

    #[test]
    fn aggregate_limits_are_unknown_when_any_selectable_route_is_unknown() {
        let priced = EffectiveRoutePricing {
            input_cost_per_million_tokens: Some(Money4::from_scaled(10_000)),
            output_cost_per_million_tokens: Some(Money4::from_scaled(20_000)),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
        };
        let route_metadata = vec![
            (
                Uuid::new_v4(),
                effective_metadata(
                    PricingLimits {
                        context: Some(128_000),
                        input: Some(100_000),
                        output: Some(32_000),
                    },
                    Some(priced),
                ),
            ),
            (
                Uuid::new_v4(),
                effective_metadata(
                    PricingLimits {
                        context: None,
                        input: Some(80_000),
                        output: Some(16_000),
                    },
                    None,
                ),
            ),
        ];

        let aggregate = aggregate_model_metadata(&route_metadata);

        assert_eq!(aggregate.limits.context, None);
        assert_eq!(aggregate.limits.input, Some(80_000));
        assert_eq!(aggregate.limits.output, Some(16_000));
        assert!(aggregate.pricing_varies_by_route);
    }

    #[derive(Default)]
    struct CountingRepo {
        models: Vec<GatewayModel>,
        routes_by_model: HashMap<Uuid, Vec<ModelRoute>>,
        providers_by_key: HashMap<String, ProviderConnection>,
        pricing_by_key: HashMap<(String, String), ModelPricingRecord>,
        allowlists_by_model: HashMap<Uuid, ModelAllowlistPolicy>,
        list_routes_for_model_calls: AtomicUsize,
        list_routes_for_models_calls: AtomicUsize,
        list_model_allowlists_for_models_calls: AtomicUsize,
        get_provider_by_key_calls: AtomicUsize,
        list_providers_by_keys_calls: AtomicUsize,
        fail_pricing_sync: AtomicBool,
    }

    #[async_trait]
    impl ModelRepository for CountingRepo {
        async fn list_models(&self) -> Result<Vec<GatewayModel>, StoreError> {
            Ok(self.models.clone())
        }

        async fn get_model_by_key(
            &self,
            model_key: &str,
        ) -> Result<Option<GatewayModel>, StoreError> {
            Ok(self
                .models
                .iter()
                .find(|model| model.model_key == model_key)
                .cloned())
        }

        async fn list_models_for_api_key(
            &self,
            _api_key_id: Uuid,
        ) -> Result<Vec<GatewayModel>, StoreError> {
            Ok(Vec::new())
        }

        async fn list_model_allowlists_for_models(
            &self,
            model_ids: &[Uuid],
        ) -> Result<HashMap<Uuid, ModelAllowlistPolicy>, StoreError> {
            self.list_model_allowlists_for_models_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(model_ids
                .iter()
                .filter_map(|model_id| {
                    self.allowlists_by_model
                        .get(model_id)
                        .cloned()
                        .map(|policy| (*model_id, policy))
                })
                .collect())
        }

        async fn list_routes_for_model(
            &self,
            model_id: Uuid,
        ) -> Result<Vec<ModelRoute>, StoreError> {
            self.list_routes_for_model_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(self
                .routes_by_model
                .get(&model_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn list_routes_for_models(
            &self,
            model_ids: &[Uuid],
        ) -> Result<HashMap<Uuid, Vec<ModelRoute>>, StoreError> {
            self.list_routes_for_models_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(model_ids
                .iter()
                .filter_map(|model_id| {
                    self.routes_by_model
                        .get(model_id)
                        .cloned()
                        .map(|routes| (*model_id, routes))
                })
                .collect())
        }
    }

    #[async_trait]
    impl ProviderRepository for CountingRepo {
        async fn get_provider_by_key(
            &self,
            provider_key: &str,
        ) -> Result<Option<ProviderConnection>, StoreError> {
            self.get_provider_by_key_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(self.providers_by_key.get(provider_key).cloned())
        }

        async fn list_providers_by_keys(
            &self,
            provider_keys: &[String],
        ) -> Result<HashMap<String, ProviderConnection>, StoreError> {
            self.list_providers_by_keys_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(provider_keys
                .iter()
                .filter_map(|provider_key| {
                    self.providers_by_key
                        .get(provider_key)
                        .cloned()
                        .map(|provider| (provider_key.clone(), provider))
                })
                .collect())
        }
    }

    #[async_trait]
    impl PricingCatalogRepository for CountingRepo {
        async fn get_pricing_catalog_cache(
            &self,
            _catalog_key: &str,
        ) -> Result<Option<PricingCatalogCacheRecord>, StoreError> {
            Ok(None)
        }

        async fn compare_and_swap_pricing_catalog_cache(
            &self,
            _cache: &PricingCatalogCacheRecord,
            _expected_fetched_at: Option<OffsetDateTime>,
        ) -> Result<bool, StoreError> {
            Ok(true)
        }

        async fn list_active_model_pricing(&self) -> Result<Vec<ModelPricingRecord>, StoreError> {
            if self.fail_pricing_sync.load(Ordering::SeqCst) {
                return Err(StoreError::Unavailable("pricing sync failed".to_string()));
            }
            Ok(self.pricing_by_key.values().cloned().collect())
        }

        async fn insert_model_pricing(
            &self,
            _record: &ModelPricingRecord,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn close_model_pricing(
            &self,
            _model_pricing_id: Uuid,
            _effective_end_at: OffsetDateTime,
            _updated_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn apply_model_pricing_sync(
            &self,
            _changes: &ModelPricingSyncChanges,
            _effective_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn resolve_model_pricing_at(
            &self,
            pricing_provider_id: &str,
            pricing_model_id: &str,
            _occurred_at: OffsetDateTime,
        ) -> Result<Option<ModelPricingRecord>, StoreError> {
            Ok(self
                .pricing_by_key
                .get(&(
                    pricing_provider_id.to_string(),
                    pricing_model_id.to_string(),
                ))
                .cloned())
        }
    }

    fn pricing_record(
        pricing_provider_id: &str,
        pricing_model_id: &str,
        input_cost: &str,
        output_cost: &str,
        limits: (Option<i64>, Option<i64>, Option<i64>),
        input_modalities: &[&str],
    ) -> ModelPricingRecord {
        let now = OffsetDateTime::now_utc();

        ModelPricingRecord {
            model_pricing_id: Uuid::new_v4(),
            pricing_provider_id: pricing_provider_id.to_string(),
            pricing_model_id: pricing_model_id.to_string(),
            display_name: pricing_model_id.to_string(),
            input_cost_per_million_tokens: Some(
                Money4::from_decimal_str(input_cost).expect("input cost"),
            ),
            output_cost_per_million_tokens: Some(
                Money4::from_decimal_str(output_cost).expect("output cost"),
            ),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            input_audio_cost_per_million_tokens: None,
            output_audio_cost_per_million_tokens: None,
            release_date: "2025-01-01".to_string(),
            last_updated: "2025-01-01".to_string(),
            effective_start_at: now,
            effective_end_at: None,
            limits: PricingLimits {
                context: limits.0,
                input: limits.1,
                output: limits.2,
            },
            modalities: PricingModalities {
                input: input_modalities
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect(),
                output: vec!["text".to_string()],
            },
            provenance: PricingProvenance {
                source: "test".to_string(),
                etag: Some("etag-1".to_string()),
                fetched_at: now,
            },
            created_at: now,
            updated_at: now,
        }
    }

    fn provider_connection(provider_type: &str) -> ProviderConnection {
        ProviderConnection {
            provider_key: "vertex".to_string(),
            provider_type: provider_type.to_string(),
            config: json!({}),
            secrets: None,
        }
    }

    fn model_route(upstream_model: &str, capabilities: ProviderCapabilities) -> ModelRoute {
        ModelRoute {
            id: Uuid::new_v4(),
            model_id: Uuid::new_v4(),
            provider_key: "vertex".to_string(),
            upstream_model: upstream_model.to_string(),
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
    fn vertex_provider_capabilities_are_route_aware_for_embeddings() {
        let provider = provider_connection("gcp_vertex");

        let embedding_route = model_route(
            "google/gemini-embedding-001",
            ProviderCapabilities::all_enabled(),
        );
        let embedding_capabilities = provider_capabilities(&provider, Some(&embedding_route));
        assert!(embedding_capabilities.embeddings);
        assert!(!embedding_capabilities.chat_completions);
        assert!(!embedding_capabilities.responses);
        assert!(!embedding_capabilities.stream);
        assert!(!embedding_capabilities.tools);

        let chat_route = model_route(
            "google/gemini-2.0-flash",
            ProviderCapabilities::all_enabled(),
        );
        let chat_capabilities = provider_capabilities(&provider, Some(&chat_route));
        assert!(chat_capabilities.chat_completions);
        assert!(chat_capabilities.stream);
        assert!(!chat_capabilities.embeddings);
        assert!(!chat_capabilities.tools);

        let anthropic_route = model_route(
            "anthropic/claude-sonnet-4-6",
            ProviderCapabilities::all_enabled(),
        );
        let anthropic_capabilities = provider_capabilities(&provider, Some(&anthropic_route));
        assert!(anthropic_capabilities.chat_completions);
        assert!(!anthropic_capabilities.embeddings);
        assert!(anthropic_capabilities.tools);
    }

    #[test]
    fn vertex_embedding_effective_capabilities_require_route_embedding_capability() {
        let provider = provider_connection("gcp_vertex");
        let route = model_route(
            "google/text-multilingual-embedding-002",
            ProviderCapabilities {
                embeddings: false,
                ..ProviderCapabilities::all_enabled()
            },
        );

        let capabilities = effective_provider_route_capabilities(
            Some(route.capabilities),
            Some(&provider),
            Some(&route),
        );

        assert!(!capabilities.embeddings);
        assert!(!capabilities.chat_completions);
        assert!(!capabilities.tools);
    }

    #[tokio::test]
    async fn list_models_batches_route_and_provider_loading() {
        let execution_model_id = Uuid::new_v4();
        let alias_model_id = Uuid::new_v4();
        let route_id = Uuid::new_v4();

        let repo = Arc::new(CountingRepo {
            models: vec![
                GatewayModel {
                    id: alias_model_id,
                    model_key: "friendly-alias".to_string(),
                    alias_target_model_key: Some("gpt-4.1".to_string()),
                    description: Some("alias".to_string()),
                    tags: vec!["alias".to_string()],
                    rank: 1,
                },
                GatewayModel {
                    id: execution_model_id,
                    model_key: "gpt-4.1".to_string(),
                    alias_target_model_key: None,
                    description: Some("base".to_string()),
                    tags: vec!["base".to_string()],
                    rank: 2,
                },
            ],
            routes_by_model: HashMap::from([(
                execution_model_id,
                vec![ModelRoute {
                    id: route_id,
                    model_id: execution_model_id,
                    provider_key: "openai".to_string(),
                    upstream_model: "gpt-4.1".to_string(),
                    priority: 0,
                    weight: 1.0,
                    enabled: true,
                    context_window_tokens: None,
                    pricing_override: None,
                    extra_headers: Default::default(),
                    extra_body: Default::default(),
                    capabilities: Default::default(),
                    compatibility: Default::default(),
                }],
            )]),
            providers_by_key: HashMap::from([(
                "openai".to_string(),
                ProviderConnection {
                    provider_key: "openai".to_string(),
                    provider_type: "openai_compat".to_string(),
                    config: json!({
                        "display": {"label": "OpenAI"},
                        "pricing_provider_id": "openai"
                    }),
                    secrets: None,
                },
            )]),
            pricing_by_key: HashMap::from([(
                ("openai".to_string(), "gpt-4.1".to_string()),
                pricing_record(
                    "openai",
                    "gpt-4.1",
                    "1.2500",
                    "10.0000",
                    (Some(400_000), Some(272_000), Some(128_000)),
                    &["text", "image"],
                ),
            )]),
            allowlists_by_model: HashMap::from([
                (
                    alias_model_id,
                    ModelAllowlistPolicy {
                        users: vec!["alias-user".to_string()],
                        teams: vec!["alias-team".to_string()],
                    },
                ),
                (
                    execution_model_id,
                    ModelAllowlistPolicy {
                        users: vec!["base-user".to_string()],
                        teams: vec!["base-team".to_string()],
                    },
                ),
            ]),
            ..Default::default()
        });

        let service = AdminModelsService::new(repo.clone());
        let items = service.list_models().await.expect("admin models");

        assert_eq!(items.len(), 2);
        assert_eq!(repo.list_routes_for_models_calls.load(Ordering::SeqCst), 1);
        assert_eq!(repo.list_routes_for_model_calls.load(Ordering::SeqCst), 0);
        assert_eq!(repo.list_providers_by_keys_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            repo.list_model_allowlists_for_models_calls
                .load(Ordering::SeqCst),
            1
        );
        assert_eq!(repo.get_provider_by_key_calls.load(Ordering::SeqCst), 0);

        let alias = items
            .iter()
            .find(|item| item.id == "friendly-alias")
            .expect("alias item");
        assert_eq!(alias.resolved_model_key, "gpt-4.1");
        assert_eq!(alias.status, AdminModelStatus::Healthy);
        assert_eq!(alias.provider_key.as_deref(), Some("openai"));
        assert_eq!(alias.upstream_model.as_deref(), Some("gpt-4.1"));
        assert_eq!(alias.input_cost_per_million_tokens_usd_10000, Some(12_500));
        assert_eq!(
            alias.output_cost_per_million_tokens_usd_10000,
            Some(100_000)
        );
        assert_eq!(alias.cache_read_cost_per_million_tokens_usd_10000, None);
        assert_eq!(alias.cache_write_cost_per_million_tokens_usd_10000, None);
        assert_eq!(
            alias.pricing_source.as_ref().map(|source| source.kind),
            Some(crate::EffectiveMetadataSourceKind::Catalog)
        );
        assert!(!alias.pricing_varies_by_route);
        assert_eq!(alias.context_window_tokens, Some(400_000));
        assert_eq!(
            alias
                .context_window_source
                .as_ref()
                .map(|source| source.kind),
            Some(crate::EffectiveMetadataSourceKind::Catalog)
        );
        assert_eq!(alias.input_window_tokens, Some(272_000));
        assert_eq!(alias.output_window_tokens, Some(128_000));
        assert_eq!(alias.supports_streaming, Some(true));
        assert_eq!(alias.supports_vision, Some(true));
        assert_eq!(alias.supports_tool_calling, Some(true));
        assert_eq!(alias.supports_structured_output, Some(true));
        assert_eq!(alias.supports_attachments, Some(true));
        assert_eq!(
            alias
                .allowlist
                .as_ref()
                .map(|policy| policy.users.as_slice()),
            Some(&["alias-user".to_string()][..])
        );
        assert_eq!(
            alias
                .allowlist
                .as_ref()
                .map(|policy| policy.teams.as_slice()),
            Some(&["alias-team".to_string()][..])
        );
        assert_eq!(
            alias
                .client_configurations
                .iter()
                .map(|config| config.key.as_str())
                .collect::<Vec<_>>(),
            vec!["opencode", "pi", "codex"]
        );

        let duplicate_error = service
            .render_client_configurations(&[
                "friendly-alias".to_string(),
                "friendly-alias".to_string(),
            ])
            .await
            .expect_err("duplicate model keys should be rejected");
        assert!(matches!(duplicate_error, GatewayError::InvalidRequest(_)));
    }

    #[tokio::test]
    async fn list_models_keeps_degraded_status_when_provider_is_missing() {
        let model_id = Uuid::new_v4();
        let route_id = Uuid::new_v4();
        let repo = Arc::new(CountingRepo {
            models: vec![GatewayModel {
                id: model_id,
                model_key: "missing-provider-model".to_string(),
                alias_target_model_key: None,
                description: None,
                tags: Vec::new(),
                rank: 1,
            }],
            routes_by_model: HashMap::from([(
                model_id,
                vec![ModelRoute {
                    id: route_id,
                    model_id,
                    provider_key: "missing".to_string(),
                    upstream_model: "upstream".to_string(),
                    priority: 0,
                    weight: 1.0,
                    enabled: true,
                    context_window_tokens: None,
                    pricing_override: None,
                    extra_headers: Default::default(),
                    extra_body: Default::default(),
                    capabilities: Default::default(),
                    compatibility: Default::default(),
                }],
            )]),
            ..Default::default()
        });

        let service = AdminModelsService::new(repo);
        let items = service.list_models().await.expect("admin models");

        assert_eq!(items[0].status, AdminModelStatus::Degraded);
        assert_eq!(items[0].provider_key.as_deref(), Some("missing"));
        assert_eq!(items[0].input_cost_per_million_tokens_usd_10000, None);
        assert_eq!(items[0].cache_read_cost_per_million_tokens_usd_10000, None);
        assert_eq!(items[0].supports_streaming, Some(true));
        assert_eq!(items[0].supports_attachments, None);
        assert!(items[0].client_configurations.is_empty());
    }

    #[tokio::test]
    async fn degraded_model_keeps_primary_pricing_but_has_unknown_aggregate_limits() {
        let model_id = Uuid::new_v4();
        let route = ModelRoute {
            id: Uuid::new_v4(),
            model_id,
            provider_key: "openai".to_string(),
            upstream_model: "upstream".to_string(),
            priority: 0,
            weight: 1.0,
            enabled: false,
            context_window_tokens: None,
            pricing_override: None,
            extra_headers: Default::default(),
            extra_body: Default::default(),
            capabilities: ProviderCapabilities::all_enabled(),
            compatibility: Default::default(),
        };
        let provider = ProviderConnection {
            provider_key: route.provider_key.clone(),
            provider_type: "openai_compat".to_string(),
            config: json!({"pricing_provider_id": "openai"}),
            secrets: None,
        };
        let pricing = pricing_record(
            "openai",
            &route.upstream_model,
            "1.2500",
            "5.0000",
            (Some(128_000), Some(96_000), Some(32_000)),
            &["text"],
        );
        let repo = Arc::new(CountingRepo {
            models: vec![GatewayModel {
                id: model_id,
                model_key: "disabled-model".to_string(),
                alias_target_model_key: None,
                description: None,
                tags: Vec::new(),
                rank: 1,
            }],
            routes_by_model: HashMap::from([(model_id, vec![route])]),
            providers_by_key: HashMap::from([("openai".to_string(), provider)]),
            pricing_by_key: HashMap::from([(
                ("openai".to_string(), "upstream".to_string()),
                pricing,
            )]),
            ..Default::default()
        });

        let items = AdminModelsService::new(repo)
            .list_models()
            .await
            .expect("admin models");
        let model = &items[0];

        assert_eq!(model.status, AdminModelStatus::Degraded);
        assert_eq!(model.input_cost_per_million_tokens_usd_10000, Some(12_500));
        assert_eq!(model.context_window_tokens, None);
        assert_eq!(
            model.pricing_source.as_ref().map(|source| source.kind),
            Some(crate::EffectiveMetadataSourceKind::Catalog)
        );
        assert!(!model.client_configurations.is_empty());
    }

    #[tokio::test]
    async fn list_models_prefers_viable_enabled_route_for_display_when_healthy() {
        let model_id = Uuid::new_v4();
        let missing_route_id = Uuid::new_v4();
        let healthy_route_id = Uuid::new_v4();
        let repo = Arc::new(CountingRepo {
            models: vec![GatewayModel {
                id: model_id,
                model_key: "fallback-model".to_string(),
                alias_target_model_key: None,
                description: None,
                tags: Vec::new(),
                rank: 1,
            }],
            routes_by_model: HashMap::from([(
                model_id,
                vec![
                    ModelRoute {
                        id: missing_route_id,
                        model_id,
                        provider_key: "missing".to_string(),
                        upstream_model: "broken-upstream".to_string(),
                        priority: 0,
                        weight: 1.0,
                        enabled: true,
                        context_window_tokens: None,
                        pricing_override: None,
                        extra_headers: Default::default(),
                        extra_body: Default::default(),
                        capabilities: Default::default(),
                        compatibility: Default::default(),
                    },
                    ModelRoute {
                        id: healthy_route_id,
                        model_id,
                        provider_key: "openai".to_string(),
                        upstream_model: "healthy-upstream".to_string(),
                        priority: 1,
                        weight: 1.0,
                        enabled: true,
                        context_window_tokens: None,
                        pricing_override: None,
                        extra_headers: Default::default(),
                        extra_body: Default::default(),
                        capabilities: ProviderCapabilities::with_dimensions(
                            true, true, false, false, false, true, true,
                        ),
                        compatibility: Default::default(),
                    },
                ],
            )]),
            providers_by_key: HashMap::from([(
                "openai".to_string(),
                ProviderConnection {
                    provider_key: "openai".to_string(),
                    provider_type: "openai_compat".to_string(),
                    config: json!({
                        "display": {"label": "OpenAI", "icon_key": "openai"},
                        "pricing_provider_id": "openai"
                    }),
                    secrets: None,
                },
            )]),
            pricing_by_key: HashMap::from([(
                ("openai".to_string(), "healthy-upstream".to_string()),
                pricing_record(
                    "openai",
                    "healthy-upstream",
                    "2.0000",
                    "12.0000",
                    (Some(200_000), None, Some(64_000)),
                    &["text"],
                ),
            )]),
            ..Default::default()
        });

        let service = AdminModelsService::new(repo);
        let items = service.list_models().await.expect("admin models");

        assert_eq!(items[0].status, AdminModelStatus::Healthy);
        assert_eq!(items[0].provider_key.as_deref(), Some("openai"));
        assert_eq!(items[0].provider_label.as_deref(), Some("OpenAI"));
        assert_eq!(items[0].upstream_model.as_deref(), Some("healthy-upstream"));
        assert_eq!(
            items[0].input_cost_per_million_tokens_usd_10000,
            Some(20_000)
        );
        assert_eq!(items[0].context_window_tokens, Some(200_000));
        assert_eq!(items[0].input_window_tokens, None);
        assert_eq!(items[0].output_window_tokens, Some(64_000));
        assert_eq!(items[0].supports_tool_calling, Some(false));
        assert_eq!(items[0].supports_vision, Some(false));
        assert_eq!(items[0].supports_structured_output, Some(true));
        assert_eq!(items[0].supports_attachments, Some(false));
        assert_eq!(
            items[0]
                .client_configurations
                .iter()
                .map(|config| config.key.as_str())
                .collect::<Vec<_>>(),
            vec!["opencode", "pi"]
        );
    }

    #[tokio::test]
    async fn list_models_does_not_reconcile_pricing() {
        let model_id = Uuid::new_v4();
        let repo = Arc::new(CountingRepo {
            models: vec![GatewayModel {
                id: model_id,
                model_key: "gpt-4.1".to_string(),
                alias_target_model_key: None,
                description: None,
                tags: Vec::new(),
                rank: 1,
            }],
            fail_pricing_sync: AtomicBool::new(true),
            ..Default::default()
        });

        let service = AdminModelsService::new(repo);
        let items = service.list_models().await.expect("admin models");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "gpt-4.1");
        assert_eq!(items[0].input_cost_per_million_tokens_usd_10000, None);
    }

    #[tokio::test]
    async fn list_models_leaves_pricing_empty_for_unsupported_pricing_paths() {
        let model_id = Uuid::new_v4();
        let route_id = Uuid::new_v4();
        let repo = Arc::new(CountingRepo {
            models: vec![GatewayModel {
                id: model_id,
                model_key: "unpriced-model".to_string(),
                alias_target_model_key: None,
                description: None,
                tags: Vec::new(),
                rank: 1,
            }],
            routes_by_model: HashMap::from([(
                model_id,
                vec![ModelRoute {
                    id: route_id,
                    model_id,
                    provider_key: "openai".to_string(),
                    upstream_model: "gpt-5".to_string(),
                    priority: 0,
                    weight: 1.0,
                    enabled: true,
                    context_window_tokens: None,
                    pricing_override: None,
                    extra_headers: Default::default(),
                    extra_body: json!({"service_tier": "priority"})
                        .as_object()
                        .cloned()
                        .expect("object"),
                    capabilities: ProviderCapabilities::with_dimensions(
                        true, false, false, true, false, true, true,
                    ),
                    compatibility: Default::default(),
                }],
            )]),
            providers_by_key: HashMap::from([(
                "openai".to_string(),
                ProviderConnection {
                    provider_key: "openai".to_string(),
                    provider_type: "openai_compat".to_string(),
                    config: json!({
                        "display": {"label": "OpenAI"},
                        "pricing_provider_id": "openai"
                    }),
                    secrets: None,
                },
            )]),
            ..Default::default()
        });

        let service = AdminModelsService::new(repo);
        let items = service.list_models().await.expect("admin models");

        assert_eq!(items[0].status, AdminModelStatus::Healthy);
        assert_eq!(items[0].input_cost_per_million_tokens_usd_10000, None);
        assert_eq!(items[0].context_window_tokens, None);
        assert_eq!(items[0].supports_streaming, Some(false));
        assert_eq!(items[0].supports_tool_calling, Some(true));
        assert_eq!(items[0].supports_structured_output, Some(true));
        assert_eq!(items[0].supports_attachments, None);
        assert_eq!(
            items[0]
                .client_configurations
                .iter()
                .map(|config| config.key.as_str())
                .collect::<Vec<_>>(),
            vec!["opencode", "pi"]
        );
    }

    #[tokio::test]
    async fn list_models_includes_client_configs_for_anthropic_labeled_models() {
        let model_id = Uuid::new_v4();
        let route_id = Uuid::new_v4();
        let mut pricing = pricing_record(
            "google-vertex-anthropic",
            "claude-sonnet-4-6@default",
            "3.0000",
            "15.0000",
            (Some(200_000), None, Some(64_000)),
            &["text", "image"],
        );
        pricing.cache_read_cost_per_million_tokens =
            Some(Money4::from_decimal_str("0.3000").expect("cache read cost"));
        pricing.cache_write_cost_per_million_tokens =
            Some(Money4::from_decimal_str("0.7500").expect("cache write cost"));

        let build_repo = |provider_type: &str, capabilities: ProviderCapabilities| {
            Arc::new(CountingRepo {
                models: vec![GatewayModel {
                    id: model_id,
                    model_key: "claude-sonnet".to_string(),
                    alias_target_model_key: None,
                    description: Some("Claude Sonnet".to_string()),
                    tags: vec!["anthropic".to_string()],
                    rank: 1,
                }],
                routes_by_model: HashMap::from([(
                    model_id,
                    vec![ModelRoute {
                        id: route_id,
                        model_id,
                        provider_key: "anthropic-prod".to_string(),
                        upstream_model: "anthropic/claude-sonnet-4-6".to_string(),
                        priority: 0,
                        weight: 1.0,
                        enabled: true,
                        context_window_tokens: None,
                        pricing_override: None,
                        extra_headers: Default::default(),
                        extra_body: Default::default(),
                        capabilities,
                        compatibility: Default::default(),
                    }],
                )]),
                providers_by_key: HashMap::from([(
                    "anthropic-prod".to_string(),
                    ProviderConnection {
                        provider_key: "anthropic-prod".to_string(),
                        provider_type: provider_type.to_string(),
                        config: json!({
                            "display": {"label": "Anthropic", "icon_key": "anthropic"},
                            "location": "global"
                        }),
                        secrets: None,
                    },
                )]),
                pricing_by_key: HashMap::from([(
                    (
                        "google-vertex-anthropic".to_string(),
                        "claude-sonnet-4-6@default".to_string(),
                    ),
                    pricing.clone(),
                )]),
                ..Default::default()
            })
        };

        let service = AdminModelsService::new(build_repo(
            "gcp_vertex",
            ProviderCapabilities::with_dimensions(true, false, true, true, true, true, true),
        ));
        let items = service.list_models().await.expect("admin models");

        assert_eq!(
            items[0].cache_read_cost_per_million_tokens_usd_10000,
            Some(3_000)
        );
        assert_eq!(
            items[0].cache_write_cost_per_million_tokens_usd_10000,
            Some(7_500)
        );
        assert_eq!(
            items[0].pricing_source.as_ref().map(|source| source.kind),
            Some(crate::EffectiveMetadataSourceKind::Catalog)
        );
        assert!(!items[0].pricing_varies_by_route);
        assert_eq!(items[0].supports_tool_calling, Some(true));
        assert_eq!(items[0].client_configurations.len(), 3);
        assert_eq!(items[0].client_configurations[0].key, "opencode");
        assert!(
            items[0].client_configurations[0]
                .setup
                .iter()
                .any(|item| item.value == "~/.config/opencode/opencode.json")
        );
        assert!(
            items[0].client_configurations[0].blocks[0]
                .content
                .contains("\"cache_read\": 0.3")
        );
        assert!(
            items[0].client_configurations[0].blocks[0]
                .content
                .contains("\"variants\"")
        );
        assert!(
            items[0].client_configurations[0].blocks[0]
                .content
                .contains("\"tool_call\": true")
        );
        assert_eq!(items[0].client_configurations[1].key, "pi");
        assert!(items[0].client_configurations[1].setup.iter().any(|item| {
            item.value.contains("~/.pi/agent/models.json")
                && item.value.contains("~/.pi/agent/settings.json")
                && item.value.contains(".pi/settings.json")
        }));
        assert!(
            items[0].client_configurations[1].blocks[0]
                .content
                .contains("\"thinkingLevelMap\"")
        );
        assert!(
            items[0].client_configurations[1].blocks[0]
                .content
                .contains("\"cacheWrite\": 0.75")
        );
        assert_eq!(items[0].client_configurations[2].key, "claude-code");
        assert_eq!(items[0].client_configurations[2].blocks.len(), 2);
        assert!(
            items[0].client_configurations[2]
                .setup
                .iter()
                .any(|item| item.value.contains("<gateway api token>"))
        );
        assert!(
            items[0].client_configurations[2].blocks[0]
                .content
                .contains("\"modelOverrides\"")
        );
        assert!(
            items[0].client_configurations[2].blocks[1]
                .content
                .contains("\"CLAUDE_CODE_AUTO_COMPACT_WINDOW\": \"200000\"")
        );

        let service = AdminModelsService::new(build_repo(
            "gcp_vertex",
            ProviderCapabilities::with_dimensions(true, false, true, true, true, true, true),
        ))
        .with_client_config_gateway_base_url("https://gateway.example.com/v1");
        let items = service.list_models().await.expect("admin models");

        assert!(
            items[0].client_configurations[0].blocks[0]
                .content
                .contains("\"baseURL\": \"https://gateway.example.com\"")
        );
        assert!(
            items[0].client_configurations[2].blocks[0]
                .content
                .contains("\"ANTHROPIC_BASE_URL\": \"https://gateway.example.com\"")
        );

        let service = AdminModelsService::new(build_repo(
            "gcp_vertex",
            ProviderCapabilities {
                chat_completions: true,
                responses: true,
                stream: false,
                embeddings: true,
                tools: true,
                vision: true,
                json_schema: true,
                developer_role: true,
            },
        ));
        let items = service.list_models().await.expect("admin models");

        assert_eq!(items[0].client_configurations.len(), 3);
        assert!(
            !items[0]
                .client_configurations
                .iter()
                .any(|config| config.key == "codex")
        );

        let service = AdminModelsService::new(build_repo(
            "openai_compat",
            ProviderCapabilities {
                chat_completions: true,
                responses: true,
                stream: false,
                embeddings: true,
                tools: true,
                vision: true,
                json_schema: true,
                developer_role: true,
            },
        ));
        let items = service.list_models().await.expect("admin models");

        assert_eq!(items[0].client_configurations.len(), 4);
        assert_eq!(items[0].client_configurations[3].key, "codex");
        assert!(
            items[0].client_configurations[3].blocks[0]
                .content
                .contains("[model_providers.oceans-llm]")
        );
        assert!(
            items[0].client_configurations[3].blocks[0]
                .content
                .contains("wire_api = \"responses\"")
        );
    }

    #[test]
    fn vertex_provider_capabilities_are_tool_capable_only_for_anthropic_routes() {
        let provider = ProviderConnection {
            provider_key: "vertex-prod".to_string(),
            provider_type: "gcp_vertex".to_string(),
            config: json!({}),
            secrets: None,
        };
        let anthropic_route = ModelRoute {
            id: Uuid::new_v4(),
            model_id: Uuid::new_v4(),
            provider_key: "vertex-prod".to_string(),
            upstream_model: "anthropic/claude-sonnet-4-6".to_string(),
            priority: 0,
            weight: 1.0,
            enabled: true,
            context_window_tokens: None,
            pricing_override: None,
            extra_headers: Default::default(),
            extra_body: Default::default(),
            capabilities: ProviderCapabilities::all_enabled(),
            compatibility: Default::default(),
        };
        let mut google_route = anthropic_route.clone();
        google_route.upstream_model = "google/gemini-2.0-flash".to_string();

        assert!(provider_capabilities(&provider, Some(&anthropic_route)).tools);
        assert!(!provider_capabilities(&provider, Some(&google_route)).tools);
    }
}
