use std::{collections::HashMap, sync::Arc};

use gateway_client_config::{
    ClientConfig, ClientConfigInput, ClientModelCapabilities, DEFAULT_API_KEY_ENV_VAR,
    DEFAULT_GATEWAY_BASE_URL, DEFAULT_PROVIDER_ID, infer_anthropic_thinking_policy,
    render_default_configs,
};
use gateway_core::{
    GatewayError, GatewayModel, ModelPricingRecord, ModelRepository, ModelRoute,
    PricingCatalogRepository, PricingModalities, ProviderCapabilities, ProviderConnection,
    ProviderRepository,
};
use time::OffsetDateTime;

use crate::pricing_catalog::exact_pricing_target_for_route;
use crate::{ModelIconKey, ProviderIconKey, resolve_model_icon_key, resolve_provider_display};

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
    pub resolved_model_key: String,
    pub alias_of: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub status: AdminModelStatus,
    pub provider_key: Option<String>,
    pub provider_label: Option<String>,
    pub provider_icon_key: Option<ProviderIconKey>,
    pub upstream_model: Option<String>,
    pub model_icon_key: Option<ModelIconKey>,
    pub input_cost_per_million_tokens_usd_10000: Option<i64>,
    pub output_cost_per_million_tokens_usd_10000: Option<i64>,
    pub cache_read_cost_per_million_tokens_usd_10000: Option<i64>,
    pub context_window_tokens: Option<i64>,
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
}

impl<R> AdminModelsService<R>
where
    R: ModelRepository + ProviderRepository + PricingCatalogRepository + Send + Sync + 'static,
{
    #[must_use]
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn list_models(&self) -> Result<Vec<AdminModelSummary>, GatewayError> {
        let pricing_time = OffsetDateTime::now_utc();
        let models = self.repo.list_models().await?;
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
            let pricing_record = match (primary_route, primary_provider) {
                (Some(route), Some(provider)) => {
                    resolve_display_pricing(self.repo.as_ref(), provider, route, pricing_time)
                        .await?
                }
                _ => None,
            };
            let model_icon_key = resolve_model_icon_key(
                primary_route
                    .map(|route| route.upstream_model.as_str())
                    .into_iter()
                    .chain([execution_model.model_key.as_str(), model.model_key.as_str()]),
            );
            let client_configurations = build_client_configurations(ClientConfigContext {
                model: &model,
                execution_model: &execution_model,
                primary_route,
                primary_provider,
                provider_display: provider_display.as_ref(),
                model_icon_key,
                pricing_record: pricing_record.as_ref(),
                route_capabilities,
            });

            items.push(AdminModelSummary {
                id: model.model_key.clone(),
                resolved_model_key: execution_model.model_key.clone(),
                alias_of: model.alias_target_model_key.clone(),
                description: model.description.clone(),
                tags: model.tags.clone(),
                status,
                provider_key: primary_route.map(|route| route.provider_key.clone()),
                provider_label: provider_display
                    .as_ref()
                    .map(|display| display.label.clone()),
                provider_icon_key: provider_display.map(|display| display.icon_key),
                upstream_model: primary_route.map(|route| route.upstream_model.clone()),
                model_icon_key,
                input_cost_per_million_tokens_usd_10000: pricing_record.as_ref().and_then(
                    |record| {
                        record
                            .input_cost_per_million_tokens
                            .map(|value| value.as_scaled_i64())
                    },
                ),
                output_cost_per_million_tokens_usd_10000: pricing_record.as_ref().and_then(
                    |record| {
                        record
                            .output_cost_per_million_tokens
                            .map(|value| value.as_scaled_i64())
                    },
                ),
                cache_read_cost_per_million_tokens_usd_10000: pricing_record.as_ref().and_then(
                    |record| {
                        record
                            .cache_read_cost_per_million_tokens
                            .map(|value| value.as_scaled_i64())
                    },
                ),
                context_window_tokens: pricing_record
                    .as_ref()
                    .and_then(|record| record.limits.context),
                input_window_tokens: pricing_record
                    .as_ref()
                    .and_then(|record| record.limits.input),
                output_window_tokens: pricing_record
                    .as_ref()
                    .and_then(|record| record.limits.output),
                supports_streaming: route_capabilities.map(|caps| caps.stream),
                supports_vision: route_capabilities.map(|caps| caps.vision),
                supports_tool_calling: route_capabilities.map(|caps| caps.tools),
                supports_structured_output: route_capabilities.map(|caps| caps.json_schema),
                supports_attachments: pricing_record
                    .as_ref()
                    .map(|record| supports_attachments(&record.modalities)),
                client_configurations,
            });
        }

        Ok(items)
    }
}

struct ClientConfigContext<'a> {
    model: &'a GatewayModel,
    execution_model: &'a GatewayModel,
    primary_route: Option<&'a ModelRoute>,
    primary_provider: Option<&'a ProviderConnection>,
    provider_display: Option<&'a crate::ProviderDisplayIdentity>,
    model_icon_key: Option<ModelIconKey>,
    pricing_record: Option<&'a ModelPricingRecord>,
    route_capabilities: Option<ProviderCapabilities>,
}

fn build_client_configurations(context: ClientConfigContext<'_>) -> Vec<ClientConfig> {
    if !is_anthropic_labeled(
        context.model,
        context.execution_model,
        context.primary_route,
        context.primary_provider,
        context.provider_display,
        context.model_icon_key,
    ) {
        return Vec::new();
    }

    let thinking_policy = infer_anthropic_thinking_policy(
        context
            .primary_route
            .map(|route| route.upstream_model.as_str())
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

    let capabilities = context.route_capabilities.unwrap_or_default();
    let input = ClientConfigInput {
        model_id: context.model.model_key.clone(),
        display_name: context
            .model
            .description
            .clone()
            .or_else(|| {
                context
                    .pricing_record
                    .map(|record| record.display_name.clone())
            })
            .unwrap_or_else(|| context.model.model_key.clone()),
        upstream_model: context
            .primary_route
            .map(|route| route.upstream_model.clone()),
        provider_id: DEFAULT_PROVIDER_ID.to_string(),
        provider_name: DEFAULT_PROVIDER_ID.to_string(),
        gateway_base_url: DEFAULT_GATEWAY_BASE_URL.to_string(),
        api_key_env_var: DEFAULT_API_KEY_ENV_VAR.to_string(),
        input_cost_per_million_tokens_usd_10000: context.pricing_record.and_then(|record| {
            record
                .input_cost_per_million_tokens
                .map(|value| value.as_scaled_i64())
        }),
        output_cost_per_million_tokens_usd_10000: context.pricing_record.and_then(|record| {
            record
                .output_cost_per_million_tokens
                .map(|value| value.as_scaled_i64())
        }),
        cache_read_cost_per_million_tokens_usd_10000: context.pricing_record.and_then(|record| {
            record
                .cache_read_cost_per_million_tokens
                .map(|value| value.as_scaled_i64())
        }),
        context_window_tokens: context
            .pricing_record
            .and_then(|record| record.limits.context),
        input_window_tokens: context
            .pricing_record
            .and_then(|record| record.limits.input),
        output_window_tokens: context
            .pricing_record
            .and_then(|record| record.limits.output),
        capabilities: ClientModelCapabilities {
            tool_calling: capabilities.tools,
            attachments: context
                .pricing_record
                .is_some_and(|record| supports_attachments(&record.modalities)),
            vision: capabilities.vision,
        },
        thinking_policy,
    };

    render_default_configs(&input)
}

fn is_anthropic_labeled(
    model: &GatewayModel,
    execution_model: &GatewayModel,
    primary_route: Option<&ModelRoute>,
    primary_provider: Option<&ProviderConnection>,
    provider_display: Option<&crate::ProviderDisplayIdentity>,
    model_icon_key: Option<ModelIconKey>,
) -> bool {
    matches!(
        model_icon_key,
        Some(ModelIconKey::Anthropic | ModelIconKey::Claude)
    ) || provider_display.is_some_and(|display| display.icon_key == ProviderIconKey::Anthropic)
        || [
            Some(model.model_key.as_str()),
            Some(execution_model.model_key.as_str()),
            primary_route.map(|route| route.upstream_model.as_str()),
            primary_provider.map(|provider| provider.provider_key.as_str()),
            primary_provider.map(|provider| provider.provider_type.as_str()),
            provider_display.map(|display| display.label.as_str()),
        ]
        .into_iter()
        .flatten()
        .any(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("anthropic") || value.contains("claude")
        })
}

async fn resolve_display_pricing<R>(
    repo: &R,
    provider: &ProviderConnection,
    route: &ModelRoute,
    pricing_time: OffsetDateTime,
) -> Result<Option<ModelPricingRecord>, GatewayError>
where
    R: PricingCatalogRepository + Send + Sync + 'static,
{
    let Some((pricing_provider_id, pricing_model_id)) =
        exact_pricing_target_for_route(provider, route)
    else {
        return Ok(None);
    };

    Ok(repo
        .resolve_model_pricing_at(&pricing_provider_id, &pricing_model_id, pricing_time)
        .await?)
}

fn supports_attachments(modalities: &PricingModalities) -> bool {
    modalities
        .input
        .iter()
        .any(|value| matches!(value.as_str(), "audio" | "file" | "image" | "pdf" | "video"))
}

fn route_health(
    providers_by_key: &HashMap<String, ProviderConnection>,
    routes: &[gateway_core::ModelRoute],
) -> AdminModelStatus {
    for route in routes {
        if route.enabled && providers_by_key.contains_key(&route.provider_key) {
            return AdminModelStatus::Healthy;
        }
    }

    AdminModelStatus::Degraded
}

fn select_display_route<'a>(
    providers_by_key: &HashMap<String, ProviderConnection>,
    routes: &'a [gateway_core::ModelRoute],
) -> Option<&'a gateway_core::ModelRoute> {
    routes
        .iter()
        .find(|route| route.enabled && providers_by_key.contains_key(&route.provider_key))
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
            atomic::{AtomicUsize, Ordering},
        },
    };

    use async_trait::async_trait;
    use gateway_core::{
        GatewayModel, ModelPricingRecord, ModelRepository, ModelRoute, Money4,
        PricingCatalogCacheRecord, PricingCatalogRepository, PricingLimits, PricingModalities,
        PricingProvenance, ProviderCapabilities, ProviderConnection, ProviderRepository,
        StoreError,
    };
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::{AdminModelStatus, AdminModelsService};

    #[derive(Default)]
    struct CountingRepo {
        models: Vec<GatewayModel>,
        routes_by_model: HashMap<Uuid, Vec<ModelRoute>>,
        providers_by_key: HashMap<String, ProviderConnection>,
        pricing_by_key: HashMap<(String, String), ModelPricingRecord>,
        list_routes_for_model_calls: AtomicUsize,
        list_routes_for_models_calls: AtomicUsize,
        get_provider_by_key_calls: AtomicUsize,
        list_providers_by_keys_calls: AtomicUsize,
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

        async fn upsert_pricing_catalog_cache(
            &self,
            _cache: &PricingCatalogCacheRecord,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn touch_pricing_catalog_cache_fetched_at(
            &self,
            _catalog_key: &str,
            _fetched_at: OffsetDateTime,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn list_active_model_pricing(&self) -> Result<Vec<ModelPricingRecord>, StoreError> {
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
            ..Default::default()
        });

        let service = AdminModelsService::new(repo.clone());
        let items = service.list_models().await.expect("admin models");

        assert_eq!(items.len(), 2);
        assert_eq!(repo.list_routes_for_models_calls.load(Ordering::SeqCst), 1);
        assert_eq!(repo.list_routes_for_model_calls.load(Ordering::SeqCst), 0);
        assert_eq!(repo.list_providers_by_keys_calls.load(Ordering::SeqCst), 1);
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
        assert_eq!(alias.context_window_tokens, Some(400_000));
        assert_eq!(alias.input_window_tokens, Some(272_000));
        assert_eq!(alias.output_window_tokens, Some(128_000));
        assert_eq!(alias.supports_streaming, Some(true));
        assert_eq!(alias.supports_vision, Some(true));
        assert_eq!(alias.supports_tool_calling, Some(true));
        assert_eq!(alias.supports_structured_output, Some(true));
        assert_eq!(alias.supports_attachments, Some(true));
        assert!(alias.client_configurations.is_empty());
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
        assert!(items[0].client_configurations.is_empty());
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
        assert!(items[0].client_configurations.is_empty());
    }

    #[tokio::test]
    async fn list_models_includes_client_configs_for_anthropic_labeled_models() {
        let model_id = Uuid::new_v4();
        let route_id = Uuid::new_v4();
        let mut pricing = pricing_record(
            "google-vertex-anthropic",
            "claude-sonnet-4-6",
            "3.0000",
            "15.0000",
            (Some(200_000), None, Some(64_000)),
            &["text", "image"],
        );
        pricing.cache_read_cost_per_million_tokens =
            Some(Money4::from_decimal_str("0.3000").expect("cache read cost"));

        let repo = Arc::new(CountingRepo {
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
                    extra_headers: Default::default(),
                    extra_body: Default::default(),
                    capabilities: ProviderCapabilities::with_dimensions(
                        true, false, true, true, true, true, true,
                    ),
                    compatibility: Default::default(),
                }],
            )]),
            providers_by_key: HashMap::from([(
                "anthropic-prod".to_string(),
                ProviderConnection {
                    provider_key: "anthropic-prod".to_string(),
                    provider_type: "gcp_vertex".to_string(),
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
                    "claude-sonnet-4-6".to_string(),
                ),
                pricing,
            )]),
            ..Default::default()
        });

        let service = AdminModelsService::new(repo);
        let items = service.list_models().await.expect("admin models");

        assert_eq!(
            items[0].cache_read_cost_per_million_tokens_usd_10000,
            Some(3_000)
        );
        assert_eq!(items[0].client_configurations.len(), 2);
        assert_eq!(items[0].client_configurations[0].key, "opencode");
        assert!(
            items[0].client_configurations[0]
                .content
                .contains("\"cache_read\": 0.3")
        );
        assert!(
            items[0].client_configurations[0]
                .content
                .contains("\"variants\"")
        );
        assert_eq!(items[0].client_configurations[1].key, "pi");
        assert!(
            items[0].client_configurations[1]
                .content
                .contains("\"thinkingLevelMap\"")
        );
    }
}
