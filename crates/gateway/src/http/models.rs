use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use gateway_core::GlobalRole;
use gateway_service::{
    AdminModelRouteSummary, AdminModelSummary, AdminModelsService, EffectiveMetadataSource,
    EffectiveMetadataSourceKind,
};

use crate::http::{
    admin_auth::{require_active_session, require_platform_admin},
    admin_contract::{
        AdminModelAllowlistView, AdminModelBenchmarkScoreView, AdminModelClientConfigView,
        AdminModelListQuery, AdminModelPageView, AdminModelRouteView, AdminModelView,
        EffectiveMetadataSourceKindView, EffectiveMetadataSourceView, Envelope,
        GenerateModelClientConfigsRequest, GenerateModelClientConfigsResponse,
        RefreshModelPricingCatalogResponse, envelope, format_timestamp,
    },
    error::AppError,
    state::AppState,
};

const DEFAULT_PAGE: u32 = 1;
const DEFAULT_PAGE_SIZE: u32 = 30;
const MAX_PAGE_SIZE: u32 = 100;

#[utoipa::path(
    get,
    path = "/api/v1/admin/models",
    params(AdminModelListQuery),
    responses((status = 200, body = Envelope<AdminModelPageView>)),
    security(("session_cookie" = []))
)]
pub async fn list_models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminModelListQuery>,
) -> Result<Json<Envelope<AdminModelPageView>>, AppError> {
    let actor = require_active_session(&state, &headers).await?;
    let include_admin_details = actor.global_role == GlobalRole::PlatformAdmin;

    let service = admin_models_service(&state);
    let models = service.list_models().await?;

    Ok(Json(envelope(model_page(
        models,
        query,
        include_admin_details,
    ))))
}

fn model_page(
    mut models: Vec<AdminModelSummary>,
    query: AdminModelListQuery,
    include_admin_details: bool,
) -> AdminModelPageView {
    let page = query.page.unwrap_or(DEFAULT_PAGE).max(1);
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    if !query.include_aliases.unwrap_or(true) {
        models.retain(|model| model.alias_of.is_none());
    }
    if let Some(search) = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        let search = search.to_lowercase();
        models.retain(|model| model_matches_search(model, &search, include_admin_details));
    }
    let total = models.len() as u64;
    let start = page.saturating_sub(1).saturating_mul(page_size) as usize;
    let items = models
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .map(|model| map_model_summary(model, include_admin_details))
        .collect();

    AdminModelPageView {
        items,
        page,
        page_size,
        total,
    }
}

fn model_matches_search(
    model: &AdminModelSummary,
    query: &str,
    include_admin_details: bool,
) -> bool {
    let matches = |value: &str| value.to_lowercase().contains(query);
    let visible_fields = [
        Some(model.id.as_str()),
        model.provider_key.as_deref(),
        model.provider_label.as_deref(),
        model.upstream_model.as_deref(),
    ];

    visible_fields
        .into_iter()
        .flatten()
        .chain(model.aliases.iter().map(String::as_str))
        .chain(model.tags.iter().map(String::as_str))
        .any(matches)
        || (include_admin_details
            && model.routes.iter().any(|route| {
                [
                    route.provider_key.as_str(),
                    route.provider_label.as_str(),
                    route.upstream_model.as_str(),
                ]
                .into_iter()
                .any(matches)
            }))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/models/client-configs",
    request_body = GenerateModelClientConfigsRequest,
    responses((status = 200, body = Envelope<GenerateModelClientConfigsResponse>)),
    security(("session_cookie" = []))
)]
pub async fn generate_model_client_configs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GenerateModelClientConfigsRequest>,
) -> Result<Json<Envelope<GenerateModelClientConfigsResponse>>, AppError> {
    require_active_session(&state, &headers).await?;

    let service = admin_models_service(&state);
    let client_configurations = service
        .render_client_configurations(&request.model_keys)
        .await?
        .into_iter()
        .map(AdminModelClientConfigView::from)
        .collect();

    Ok(Json(envelope(GenerateModelClientConfigsResponse {
        client_configurations,
    })))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/models/pricing-catalog/refresh",
    responses((status = 200, body = Envelope<RefreshModelPricingCatalogResponse>)),
    security(("session_cookie" = []))
)]
pub async fn refresh_model_pricing_catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Envelope<RefreshModelPricingCatalogResponse>>, AppError> {
    require_platform_admin(&state, &headers).await?;

    state.service.refresh_pricing_catalog_now().await?;

    Ok(Json(envelope(RefreshModelPricingCatalogResponse {
        refreshed: true,
    })))
}

fn admin_models_service(state: &AppState) -> AdminModelsService<gateway_store::AnyStore> {
    let service = AdminModelsService::new(state.store.clone())
        .with_benchmark_model_ids(state.benchmark_model_ids.clone());
    match state.client_config_gateway_base_url.as_ref().as_deref() {
        Some(gateway_base_url) => {
            service.with_client_config_gateway_base_url(gateway_base_url.to_string())
        }
        None => service,
    }
}

fn map_model_summary(model: AdminModelSummary, include_admin_details: bool) -> AdminModelView {
    let allowlist = if include_admin_details {
        model.allowlist.map(|policy| AdminModelAllowlistView {
            users: policy.users,
            teams: policy.teams,
        })
    } else {
        None
    };

    AdminModelView {
        id: model.id,
        model_id: model.model_id,
        resolved_model_key: model.resolved_model_key,
        alias_of: model.alias_of,
        aliases: model.aliases,
        description: model.description,
        tags: model.tags,
        allowlist,
        routing: if include_admin_details {
            model.routing.map(Into::into)
        } else {
            None
        },
        routes: include_admin_details
            .then(|| model.routes.into_iter().map(map_model_route).collect()),
        status: model.status.into(),
        provider_key: model.provider_key,
        provider_label: model.provider_label,
        provider_icon_key: model.provider_icon_key.map(Into::into),
        upstream_model: model.upstream_model,
        model_icon_key: model.model_icon_key.map(Into::into),
        input_cost_per_million_tokens_usd_10000: model.input_cost_per_million_tokens_usd_10000,
        output_cost_per_million_tokens_usd_10000: model.output_cost_per_million_tokens_usd_10000,
        cache_read_cost_per_million_tokens_usd_10000: model
            .cache_read_cost_per_million_tokens_usd_10000,
        cache_write_cost_per_million_tokens_usd_10000: model
            .cache_write_cost_per_million_tokens_usd_10000,
        pricing_source: model.pricing_source.map(map_metadata_source),
        pricing_varies_by_route: model.pricing_varies_by_route,
        context_window_tokens: model.context_window_tokens,
        context_window_source: model.context_window_source.map(map_metadata_source),
        input_window_tokens: model.input_window_tokens,
        output_window_tokens: model.output_window_tokens,
        supports_streaming: model.supports_streaming,
        supports_vision: model.supports_vision,
        supports_tool_calling: model.supports_tool_calling,
        supports_structured_output: model.supports_structured_output,
        supports_attachments: model.supports_attachments,
        benchmark_scores: model
            .benchmark_scores
            .into_iter()
            .map(|score| AdminModelBenchmarkScoreView {
                metric_key: score.metric.into(),
                label: score.label.to_string(),
                value: score.value,
                source: score.source.to_string(),
                source_model_id: score.source_model_id,
                source_url: score.source_url,
                match_kind: score.match_kind.into(),
                updated_at: format_timestamp(score.updated_at),
            })
            .collect(),
        supports_decisions: model.supports_decisions,
        client_configurations: model
            .client_configurations
            .into_iter()
            .map(AdminModelClientConfigView::from)
            .collect(),
    }
}

fn map_model_route(route: AdminModelRouteSummary) -> AdminModelRouteView {
    AdminModelRouteView {
        id: route.id,
        provider_key: route.provider_key,
        provider_label: route.provider_label,
        provider_icon_key: route.provider_icon_key.into(),
        upstream_model: route.upstream_model,
        priority: route.priority,
        weight: route.weight,
        enabled: route.enabled,
        provider_configured: route.provider_configured,
    }
}

fn map_metadata_source(source: EffectiveMetadataSource) -> EffectiveMetadataSourceView {
    EffectiveMetadataSourceView {
        kind: match source.kind {
            EffectiveMetadataSourceKind::ConfiguredOverride => {
                EffectiveMetadataSourceKindView::ConfiguredOverride
            }
            EffectiveMetadataSourceKind::Catalog => EffectiveMetadataSourceKindView::Catalog,
            EffectiveMetadataSourceKind::Mixed => EffectiveMetadataSourceKindView::Mixed,
        },
        catalog_source: source.catalog_source,
        catalog_etag: source.catalog_etag,
        catalog_fetched_at: source.catalog_fetched_at.map(format_timestamp),
    }
}

#[cfg(test)]
mod tests {
    use gateway_core::{
        ModelAllowlistPolicy, ModelRoutingPolicy, RoutingStrategy, SessionAffinityPolicy,
    };
    use gateway_service::{AdminModelStatus, ProviderIconKey};
    use serde_json::json;

    use super::*;

    fn model_summary() -> AdminModelSummary {
        AdminModelSummary {
            id: "gpt-6-sol".to_string(),
            model_id: "00000000-0000-4000-8000-000000000001".to_string(),
            resolved_model_key: "gpt-6-sol".to_string(),
            alias_of: None,
            aliases: vec!["coding".to_string(), "fast".to_string()],
            description: Some("Shared model".to_string()),
            tags: Vec::new(),
            allowlist: Some(ModelAllowlistPolicy {
                users: vec!["allowed-user".to_string()],
                teams: Vec::new(),
            }),
            routing: Some(ModelRoutingPolicy {
                strategy: RoutingStrategy::RoundRobin,
                affinity: Some(SessionAffinityPolicy::default()),
            }),
            routes: vec![
                AdminModelRouteSummary {
                    id: "route-copilot".to_string(),
                    provider_key: "copilot".to_string(),
                    provider_label: "GitHub Copilot".to_string(),
                    provider_icon_key: ProviderIconKey::OpenAI,
                    upstream_model: "gpt-6-sol".to_string(),
                    priority: 0,
                    weight: 2.0,
                    enabled: true,
                    provider_configured: true,
                },
                AdminModelRouteSummary {
                    id: "route-openrouter".to_string(),
                    provider_key: "openrouter-secondary".to_string(),
                    provider_label: "OpenRouter".to_string(),
                    provider_icon_key: ProviderIconKey::OpenRouter,
                    upstream_model: "openai/gpt-6-sol".to_string(),
                    priority: 10,
                    weight: 1.0,
                    enabled: false,
                    provider_configured: false,
                },
            ],
            status: AdminModelStatus::Healthy,
            provider_key: Some("copilot".to_string()),
            provider_label: Some("GitHub Copilot".to_string()),
            provider_icon_key: Some(ProviderIconKey::OpenAI),
            upstream_model: Some("gpt-6-sol".to_string()),
            model_icon_key: None,
            input_cost_per_million_tokens_usd_10000: None,
            output_cost_per_million_tokens_usd_10000: None,
            cache_read_cost_per_million_tokens_usd_10000: None,
            cache_write_cost_per_million_tokens_usd_10000: None,
            pricing_source: None,
            pricing_varies_by_route: true,
            context_window_tokens: None,
            context_window_source: None,
            input_window_tokens: None,
            output_window_tokens: None,
            supports_streaming: Some(true),
            supports_vision: None,
            supports_tool_calling: None,
            supports_structured_output: None,
            supports_attachments: None,
            benchmark_scores: Vec::new(),
            supports_decisions: None,
            client_configurations: Vec::new(),
        }
    }

    fn catalog_with_aliases() -> Vec<AdminModelSummary> {
        [
            ("coding", Some("gpt-6-sol")),
            ("gpt-6-sol", None),
            ("fast", Some("coding")),
            ("other-model", None),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (model_key, alias_of))| {
            let mut model = model_summary();
            model.id = model_key.to_string();
            model.model_id = format!("00000000-0000-4000-8000-{index:012}");
            model.alias_of = alias_of.map(str::to_string);
            model.aliases = ["coding", "fast", "gpt-6-sol"]
                .into_iter()
                .filter(|key| *key != model_key)
                .map(str::to_string)
                .collect();
            model.tags = vec!["Reasoning".to_string()];
            if model_key == "other-model" {
                model.resolved_model_key = model_key.to_string();
                model.aliases.clear();
                model.tags.clear();
                model.routes.clear();
                model.provider_key = None;
                model.provider_label = None;
                model.upstream_model = None;
            }
            model
        })
        .collect()
    }

    #[test]
    fn excluding_aliases_filters_before_count_and_pagination() {
        for (page, expected_ids) in [
            (1, vec!["gpt-6-sol"]),
            (2, vec!["other-model"]),
            (3, vec![]),
        ] {
            let uri = format!("/api/v1/admin/models?page={page}&page_size=1&include_aliases=false")
                .parse()
                .unwrap();
            let query = Query::<AdminModelListQuery>::try_from_uri(&uri).unwrap().0;
            let result = model_page(catalog_with_aliases(), query, true);

            assert_eq!(result.total, 2);
            assert_eq!(result.page, page);
            assert_eq!(result.page_size, 1);
            assert_eq!(
                result
                    .items
                    .iter()
                    .map(|model| model.id.as_str())
                    .collect::<Vec<_>>(),
                expected_ids
            );
        }
    }

    #[test]
    fn excluding_alias_rows_retains_complete_alias_metadata() {
        let result = model_page(
            catalog_with_aliases(),
            AdminModelListQuery {
                page_size: Some(1),
                include_aliases: Some(false),
                ..Default::default()
            },
            false,
        );

        assert_eq!(result.items[0].id, "gpt-6-sol");
        assert_eq!(result.items[0].aliases, ["coding", "fast"]);
        assert!(result.items[0].routes.is_none());
    }

    #[test]
    fn alias_rows_are_included_by_default_and_when_requested() {
        for query_string in ["", "?include_aliases=true"] {
            let uri = format!("/api/v1/admin/models{query_string}")
                .parse()
                .unwrap();
            let query = Query::<AdminModelListQuery>::try_from_uri(&uri).unwrap().0;
            let result = model_page(catalog_with_aliases(), query, false);

            assert_eq!(result.total, 4);
            assert_eq!(result.items.len(), 4);
            assert_eq!(result.items[0].alias_of.as_deref(), Some("gpt-6-sol"));
            assert_eq!(result.items[2].alias_of.as_deref(), Some("coding"));
        }
    }

    #[test]
    fn alias_search_finds_the_canonical_model_beyond_the_first_page() {
        let mut models = catalog_with_aliases();
        models.rotate_right(1);
        let uri = "/api/v1/admin/models?page_size=1&include_aliases=false&q=%20fAsT%20"
            .parse()
            .unwrap();
        let query = Query::<AdminModelListQuery>::try_from_uri(&uri).unwrap().0;
        let result = model_page(models, query, false);

        assert_eq!(result.total, 1);
        assert_eq!(result.items[0].id, "gpt-6-sol");
        assert_eq!(result.items[0].aliases, ["coding", "fast"]);
    }

    #[test]
    fn search_and_alias_filter_both_apply_before_count_and_pagination() {
        let mut models = catalog_with_aliases();
        let mut extra = model_summary();
        extra.id = "third-model".to_string();
        extra.aliases.clear();
        extra.tags = vec!["Reasoning".to_string()];
        models.push(extra);
        let result = model_page(
            models,
            AdminModelListQuery {
                page: Some(2),
                page_size: Some(1),
                include_aliases: Some(false),
                q: Some("reason".to_string()),
            },
            true,
        );

        assert_eq!(result.total, 2);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, "third-model");
    }

    #[test]
    fn omitted_empty_and_whitespace_searches_return_the_full_filtered_list() {
        for q in [None, Some(""), Some(" \t\n ")] {
            let result = model_page(
                catalog_with_aliases(),
                AdminModelListQuery {
                    include_aliases: Some(false),
                    q: q.map(str::to_string),
                    ..Default::default()
                },
                false,
            );

            assert_eq!(result.total, 2);
            assert_eq!(result.items.len(), 2);
        }
    }

    #[test]
    fn public_search_matches_model_metadata_but_not_database_ids() {
        for q in [
            "DISPLAY", "BETA", "GAMMA", "DELTA", "EPSILON", "ZETA", "00000000",
        ] {
            let mut model = model_summary();
            model.id = "model-display-alpha".to_string();
            model.aliases = vec!["alternate-beta".to_string()];
            model.provider_key = Some("vendor-key-gamma".to_string());
            model.provider_label = Some("Provider Delta".to_string());
            model.upstream_model = Some("upstream-epsilon".to_string());
            model.tags = vec!["tag-zeta".to_string()];
            let result = model_page(
                vec![model],
                AdminModelListQuery {
                    q: Some(q.to_string()),
                    ..Default::default()
                },
                false,
            );

            assert_eq!(result.total, u64::from(q != "00000000"), "query: {q}");
        }
    }

    #[test]
    fn search_cannot_reveal_route_metadata_hidden_from_non_admins() {
        for q in ["openrouter-secondary", "OpenRouter", "openai/gpt-6-sol"] {
            for include_admin_details in [false, true] {
                let result = model_page(
                    vec![model_summary()],
                    AdminModelListQuery {
                        q: Some(q.to_string()),
                        ..Default::default()
                    },
                    include_admin_details,
                );

                assert_eq!(result.total, u64::from(include_admin_details), "query: {q}");
                assert_eq!(result.items.len(), usize::from(include_admin_details));
            }
        }
    }

    #[test]
    fn aliases_remain_public_model_metadata_for_all_authenticated_roles() {
        for include_admin_details in [false, true] {
            let mut model = model_summary();
            model.alias_of = Some("canonical".to_string());
            model.aliases = vec!["canonical".to_string(), "other-alias".to_string()];
            let value =
                serde_json::to_value(map_model_summary(model, include_admin_details)).unwrap();
            assert_eq!(value["aliases"], json!(["canonical", "other-alias"]));
            assert_eq!(value["alias_of"], "canonical");
        }
    }

    #[test]
    fn platform_admin_receives_configured_routing_and_all_routes() {
        let value = serde_json::to_value(map_model_summary(model_summary(), true)).unwrap();

        assert_eq!(
            value["routing"],
            json!({
                "strategy": "round_robin",
                "affinity": { "idle_timeout_seconds": 3_600 }
            })
        );
        assert_eq!(
            value["routes"],
            json!([
                {
                    "id": "route-copilot",
                    "provider_key": "copilot",
                    "provider_label": "GitHub Copilot",
                    "provider_icon_key": "openai",
                    "upstream_model": "gpt-6-sol",
                    "priority": 0,
                    "weight": 2.0,
                    "enabled": true,
                    "provider_configured": true
                },
                {
                    "id": "route-openrouter",
                    "provider_key": "openrouter-secondary",
                    "provider_label": "OpenRouter",
                    "provider_icon_key": "openrouter",
                    "upstream_model": "openai/gpt-6-sol",
                    "priority": 10,
                    "weight": 1.0,
                    "enabled": false,
                    "provider_configured": false
                }
            ])
        );
        assert_eq!(value["allowlist"]["users"], json!(["allowed-user"]));
        assert_eq!(value["provider_key"], "copilot");
        assert_eq!(value["upstream_model"], "gpt-6-sol");
    }

    #[test]
    fn non_admin_receives_no_routing_or_route_details() {
        let value = serde_json::to_value(map_model_summary(model_summary(), false)).unwrap();

        assert_eq!(value.get("routing"), Some(&serde_json::Value::Null));
        assert_eq!(value.get("routes"), Some(&serde_json::Value::Null));
        assert_eq!(value.get("allowlist"), Some(&serde_json::Value::Null));
        assert_eq!(value["provider_key"], "copilot");
        assert_eq!(value["upstream_model"], "gpt-6-sol");
        let serialized = value.to_string();
        for private_value in [
            "route-copilot",
            "route-openrouter",
            "openrouter-secondary",
            "openai/gpt-6-sol",
            "idle_timeout_seconds",
            "allowed-user",
        ] {
            assert!(
                !serialized.contains(private_value),
                "leaked {private_value}"
            );
        }
    }

    #[test]
    fn platform_admin_can_distinguish_an_empty_pool_from_hidden_routes() {
        let mut model = model_summary();
        model.routing = None;
        model.routes.clear();

        let value = serde_json::to_value(map_model_summary(model, true)).unwrap();

        assert_eq!(value.get("routing"), Some(&serde_json::Value::Null));
        assert_eq!(value["routes"], json!([]));
    }

    #[test]
    fn routing_strategies_serialize_without_enabling_affinity() {
        for (strategy, expected) in [
            (RoutingStrategy::Preferred, "preferred"),
            (RoutingStrategy::WeightedRandom, "weighted_random"),
            (RoutingStrategy::RoundRobin, "round_robin"),
        ] {
            let mut model = model_summary();
            model.routing = Some(ModelRoutingPolicy {
                strategy,
                affinity: None,
            });

            let value = serde_json::to_value(map_model_summary(model, true)).unwrap();

            assert_eq!(
                value["routing"],
                json!({
                    "strategy": expected,
                    "affinity": null
                })
            );
        }
    }
}
