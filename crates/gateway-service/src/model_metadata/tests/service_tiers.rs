//! Service tiers in discovery: adapter support, forced route tiers, and the model allow-list.
use super::*;
use crate::model_metadata::{
    listing::list_models, litellm::list_model_groups, service_tiers::servable_service_tiers,
};
use ServiceTier::{Default as Standard, Flex, Priority, Ultrafast};

/// The tiers the real adapters send: OpenAI-compatible hosts take every tier, Anthropic
/// takes Fast, and Copilot takes none.
struct ByProvider;

impl AdapterServiceTiers for ByProvider {
    fn adapter_service_tiers(&self, route: &ModelRoute) -> &'static [ServiceTier] {
        match route.provider_key.as_str() {
            "openrouter" => &ServiceTier::ALL,
            "anthropic" => &[Standard, Priority],
            _ => &[Standard],
        }
    }
}

/// OpenRouter reports each request's cost, so every tier it serves can be billed.
fn openrouter() -> ProviderConnection {
    ProviderConnection {
        provider_key: "openrouter".into(),
        provider_type: "openai_compat".into(),
        config: json!({"pricing_provider_id": "openrouter"}),
        secrets: None,
    }
}

fn copilot() -> ProviderConnection {
    ProviderConnection {
        provider_key: "copilot".into(),
        provider_type: "github_copilot".into(),
        config: json!({}),
        secrets: None,
    }
}

fn route_on(provider: &ProviderConnection, forced: Option<ServiceTier>) -> ModelRoute {
    let mut route = ModelRoute {
        provider_key: provider.provider_key.clone(),
        upstream_model: "openai/gpt-5".into(),
        service_tier: forced,
        ..route()
    };
    if provider.provider_type == "github_copilot" {
        route.compatibility.github_copilot = Some(gateway_core::GitHubCopilotRouteCompatibility {
            chat_api: Some(gateway_core::GitHubCopilotChatApi::ChatCompletions),
            supports_responses: false,
            supports_embeddings: false,
            upstream_supports: Default::default(),
        });
    }
    route
}

fn servable(
    routes: &[(ModelRoute, ProviderConnection)],
    allowed: Option<&[ServiceTier]>,
) -> Vec<ServiceTier> {
    let snapshot = crate::pricing_catalog::load_vendored_fallback_snapshot();
    servable_service_tiers(
        routes.iter().map(|(route, provider)| (route, provider)),
        allowed,
        &snapshot,
        &ByProvider,
    )
}

#[test]
fn servable_tiers_follow_adapters_forced_routes_and_the_allow_list() {
    let openrouter = openrouter();
    let copilot = copilot();
    let open = (route_on(&openrouter, None), openrouter.clone());
    let fast = (route_on(&openrouter, Some(Priority)), openrouter.clone());
    let copilot_only = (route_on(&copilot, None), copilot.clone());

    assert_eq!(
        servable(std::slice::from_ref(&open), None),
        [Standard, Flex, Priority, Ultrafast]
    );
    // A `-fast` route serves Priority whatever the caller asks for.
    assert_eq!(servable(std::slice::from_ref(&fast), None), [Priority]);
    assert_eq!(
        servable(std::slice::from_ref(&copilot_only), None),
        [Standard]
    );
    assert_eq!(
        servable(std::slice::from_ref(&open), Some(&[Priority])),
        [Standard, Priority]
    );
    // The allow-list limits what callers ask for, not what an admin forced on a route.
    assert_eq!(
        servable(std::slice::from_ref(&fast), Some(&[Flex])),
        [Priority]
    );
    // A request can land on any route, so the model serves the union.
    assert_eq!(servable(&[copilot_only, fast], None), [Standard, Priority]);
    assert_eq!(servable(&[], None), []);
}

#[test]
fn forced_tiers_the_adapter_cannot_send_are_not_servable() {
    let copilot = copilot();
    assert_eq!(
        servable(
            &[(route_on(&copilot, Some(Priority)), copilot.clone())],
            None
        ),
        []
    );
}

/// The `/v1/models` card, `/model_group/info` row, and `/v1/model-metadata` entry for a
/// model served by `routes`.
async fn listings(
    model: GatewayModel,
    routes: Vec<ModelRoute>,
    provider: ProviderConnection,
) -> (serde_json::Value, serde_json::Value, serde_json::Value) {
    let routes = routes
        .into_iter()
        .map(|route| ModelRoute {
            model_id: model.id,
            ..route
        })
        .collect();
    let repo = ListingRepo {
        models: vec![model.clone()],
        routes,
        provider,
    };
    let snapshot = crate::pricing_catalog::load_vendored_fallback_snapshot();
    let visible = vec![model];
    let cards = list_models(&repo, visible.clone(), &snapshot, &ByProvider)
        .await
        .unwrap();
    let groups = list_model_groups(&repo, visible.clone(), &snapshot, &ByProvider)
        .await
        .unwrap();
    let metadata = list_metadata(&repo, visible, &snapshot, &ByProvider)
        .await
        .unwrap();
    (
        serde_json::to_value(&cards.data[0]).unwrap(),
        serde_json::to_value(&groups.data[0]).unwrap(),
        serde_json::to_value(&metadata.data[0]).unwrap(),
    )
}

fn tiered_model(allowed: Option<Vec<ServiceTier>>) -> GatewayModel {
    GatewayModel {
        id: Uuid::new_v4(),
        model_key: "gpt-house".into(),
        alias_target_model_key: None,
        max_reasoning_effort: None,
        routing: None,
        description: None,
        tags: vec![],
        rank: 0,
        service_tiers: allowed,
    }
}

fn offers_service_tier(group: &serde_json::Value) -> bool {
    group["supported_openai_params"]
        .as_array()
        .unwrap()
        .contains(&json!("service_tier"))
}

#[tokio::test]
async fn discovery_lists_the_tiers_a_model_serves() {
    let openrouter = openrouter();
    let (card, group, metadata) = listings(
        tiered_model(None),
        vec![route_on(&openrouter, None)],
        openrouter.clone(),
    )
    .await;
    let every_tier = json!(["default", "flex", "priority", "ultrafast"]);
    assert_eq!(card["service_tiers"], every_tier);
    assert!(offers_service_tier(&group));
    assert_eq!(metadata["service_tiers"], every_tier);
    assert_eq!(metadata["routes"][0]["service_tier"], json!(null));
    assert_eq!(
        card["client_hints"]["harnesses"]["opencode"]["variants"],
        json!([
            {"id": "flex", "settings": {"serviceTier": "flex"}},
            {"id": "fast", "settings": {"serviceTier": "priority"}},
            {"id": "ultrafast", "settings": {"serviceTier": "ultrafast"}}
        ])
    );

    let (card, group, metadata) = listings(
        tiered_model(Some(vec![Priority])),
        vec![route_on(&openrouter, None)],
        openrouter.clone(),
    )
    .await;
    assert_eq!(card["service_tiers"], json!(["default", "priority"]));
    assert!(offers_service_tier(&group));
    assert_eq!(metadata["service_tiers"], json!(["default", "priority"]));

    // A forced tier leaves nothing to choose, so the OpenCode hints add no variant.
    let (card, group, metadata) = listings(
        tiered_model(None),
        vec![route_on(&openrouter, Some(Priority))],
        openrouter,
    )
    .await;
    assert_eq!(card["service_tiers"], json!(["priority"]));
    assert!(offers_service_tier(&group));
    assert_eq!(metadata["routes"][0]["service_tier"], json!("priority"));
    assert!(
        card["client_hints"]["harnesses"]["opencode"]
            .get("variants")
            .is_none()
    );
}

#[tokio::test]
async fn variants_offer_only_tiers_the_allow_list_lets_a_caller_request() {
    let openrouter = openrouter();
    // Requests can land on the Priority route, so discovery lists Priority. An explicit
    // Priority request still fails the `[flex]` allow-list with 403, so no variant asks for it.
    let (card, group, metadata) = listings(
        tiered_model(Some(vec![Flex])),
        vec![
            route_on(&openrouter, None),
            route_on(&openrouter, Some(Priority)),
        ],
        openrouter,
    )
    .await;
    let served = json!(["default", "flex", "priority"]);
    assert_eq!(card["service_tiers"], served);
    assert_eq!(metadata["service_tiers"], served);
    assert!(offers_service_tier(&group));
    assert_eq!(
        card["client_hints"]["harnesses"]["opencode"]["variants"],
        json!([{"id": "flex", "settings": {"serviceTier": "flex"}}])
    );
}

#[tokio::test]
async fn default_only_models_advertise_no_tiers() {
    let copilot = copilot();
    let (card, group, metadata) =
        listings(tiered_model(None), vec![route_on(&copilot, None)], copilot).await;
    assert!(card.get("service_tiers").is_none());
    assert!(!offers_service_tier(&group));
    assert_eq!(metadata["service_tiers"], json!(["default"]));
}

#[test]
fn forced_tier_routes_report_that_tiers_catalog_schedule() {
    let mut model = catalog_model("gpt-5");
    model.cost.input = Some("1.2500".into());
    model.cost.output = Some("10.0000".into());
    model.cost.service_tiers.insert(
        Priority,
        crate::pricing_catalog::PricingCatalogServiceTierCost {
            input: Some("2.5000".into()),
            output: Some("20.0000".into()),
            cache_read: Some("0.2500".into()),
            cache_write: None,
            cache_write_1h: None,
            context_tiers: Vec::new(),
        },
    );
    let snapshot = merged_snapshot(openai_providers(vec![model]), Providers::new());
    let mut fast = route();
    fast.service_tier = Some(Priority);

    let priced = route_metadata(&fast, Some(&provider()), &snapshot);
    assert_eq!(priced.pricing_source, Some("catalog"));
    let pricing = serde_json::to_value(priced.pricing.unwrap()).unwrap();
    assert_eq!(pricing["input"], json!("2.5000"));
    assert_eq!(pricing["output"], json!("20.0000"));
    assert_eq!(pricing["cache_read"], json!("0.2500"));
    assert_eq!(pricing["cache_write"], json!(null));
    assert!(pricing.get("service_tiers").is_none());

    // No Flex schedule: the route has no price rather than the default rates.
    fast.service_tier = Some(Flex);
    let unpriced = route_metadata(&fast, Some(&provider()), &snapshot);
    assert!(unpriced.pricing.is_none());
    assert_eq!(unpriced.pricing_source, None);
}
