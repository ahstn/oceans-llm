//! `/model_group/info` projection tests, sharing the `/v1/models` fixtures.
use super::*;
use crate::model_metadata::litellm::list_model_groups;

const COST_FIELDS: [&str; 4] = [
    "input_cost_per_token",
    "output_cost_per_token",
    "cache_read_input_token_cost",
    "cache_creation_input_token_cost",
];

fn model(key: &str) -> GatewayModel {
    GatewayModel {
        id: Uuid::new_v4(),
        model_key: key.into(),
        alias_target_model_key: None,
        max_reasoning_effort: None,
        routing: None,
        description: None,
        tags: vec![],
        rank: 0,
    }
}

fn vertex_claude_route() -> ModelRoute {
    let mut claude = route();
    claude.provider_key = "vertex".into();
    claude.upstream_model = "anthropic/claude-opus-4-7@default".into();
    claude.context_window_tokens = None;
    claude
}

async fn group_info_with(
    model: GatewayModel,
    routes: Vec<ModelRoute>,
    provider: ProviderConnection,
    snapshot: &PricingCatalogSnapshot,
) -> serde_json::Value {
    let routes = routes
        .into_iter()
        .map(|mut route| {
            route.model_id = model.id;
            route
        })
        .collect();
    let repo = ListingRepo {
        models: vec![model.clone()],
        routes,
        provider,
    };
    let listed = list_model_groups(&repo, vec![model], snapshot)
        .await
        .unwrap();
    serde_json::to_value(&listed.data[0]).unwrap()
}

async fn group_info(
    model: GatewayModel,
    routes: Vec<ModelRoute>,
    provider: ProviderConnection,
) -> serde_json::Value {
    let snapshot = crate::pricing_catalog::load_vendored_fallback_snapshot();
    group_info_with(model, routes, provider, &snapshot).await
}

/// Checks the leading cost fields, in `COST_FIELDS` order.
fn assert_costs(card: &serde_json::Value, expected: &[f64]) {
    for (field, expected) in COST_FIELDS.into_iter().zip(expected.iter().copied()) {
        let actual = card[field]
            .as_f64()
            .unwrap_or_else(|| panic!("{field} missing"));
        assert!(
            (actual - expected).abs() < 1e-15,
            "{field}: {actual} != {expected}"
        );
    }
}

fn assert_no_costs(card: &serde_json::Value) {
    for field in COST_FIELDS {
        assert!(card.get(field).is_none(), "{field} must be omitted");
    }
}

fn params(card: &serde_json::Value) -> Vec<&str> {
    card["supported_openai_params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn model_group_info_converts_per_million_prices_to_per_token_costs() {
    let mut priced = route();
    priced.pricing_override = Some(RoutePricingOverride {
        input_cost_per_million_tokens: Money4::from_scaled(40_000),
        output_cost_per_million_tokens: Money4::from_scaled(200_000),
        cache_read_cost_per_million_tokens: Some(Money4::from_scaled(2_000)),
        cache_write_cost_per_million_tokens: Some(Money4::from_scaled(50_000)),
    });
    let configured = group_info(model("gpt"), vec![priced], provider()).await;
    assert_costs(&configured, &[4e-6, 20e-6, 0.2e-6, 5e-6]);

    // Catalog prices take the same conversion: Opus 4.7 lists 5 / 25 / 0.5 / 6.25 per million.
    let catalog = group_info(
        model("house-large"),
        vec![vertex_claude_route()],
        vertex_provider(),
    )
    .await;
    assert_costs(&catalog, &[5e-6, 25e-6, 0.5e-6, 6.25e-6]);
}

#[tokio::test]
async fn model_group_info_keeps_known_prices_when_a_cache_rate_is_missing() {
    // The catalog prices gpt-5 input, output and cache reads, but not cache writes. Gateway
    // ids such as `gpt` have no bundled omp price, so dropping the set would make them free.
    let catalog = group_info(model("gpt"), vec![route()], provider()).await;
    assert_costs(&catalog, &[1.25e-6, 10e-6, 0.125e-6]);
    assert!(catalog.get("cache_creation_input_token_cost").is_none());

    let mut partial = route();
    partial.pricing_override = Some(RoutePricingOverride {
        input_cost_per_million_tokens: Money4::from_scaled(10_000),
        output_cost_per_million_tokens: Money4::from_scaled(20_000),
        cache_read_cost_per_million_tokens: None,
        cache_write_cost_per_million_tokens: None,
    });
    let configured = group_info(model("gpt"), vec![partial], provider()).await;
    assert_costs(&configured, &[1e-6, 2e-6]);
    for field in &COST_FIELDS[2..] {
        assert!(configured.get(*field).is_none(), "{field} must be omitted");
    }
}

#[tokio::test]
async fn model_group_info_claims_openai_only_when_every_chat_route_serves_responses() {
    let responses = group_info(model("gpt"), vec![route()], provider()).await;
    assert_eq!(responses["providers"], json!(["openai"]));

    // omp would send `["openai"]` models over `/v1/responses`, which this route cannot serve.
    let mut chat_only = route();
    chat_only.capabilities.responses = false;
    let behind_chat = group_info(model("gpt"), vec![chat_only.clone()], provider()).await;
    assert_eq!(behind_chat["providers"], json!(["openai_compat"]));

    // A fallback route can serve the request too, so it must also take Responses.
    let mixed = group_info(model("gpt"), vec![route(), chat_only], provider()).await;
    assert_eq!(mixed["providers"], json!(["openai_compat"]));

    // omp sends every other vendor over chat completions, which no route here serves.
    let mut responses_only = route();
    responses_only.upstream_model = "claude-sonnet-4-5".into();
    responses_only.capabilities.chat_completions = false;
    let claude_responses = group_info(model("claude"), vec![responses_only], provider()).await;
    assert_eq!(claude_responses["providers"], json!(["openai"]));

    let claude = group_info(
        model("house-large"),
        vec![vertex_claude_route()],
        vertex_provider(),
    )
    .await;
    assert_eq!(claude["providers"], json!(["anthropic"]));

    let mut namespaced = route();
    namespaced.upstream_model = "meta-llama/llama-4-maverick".into();
    let namespaced = group_info(model("llama"), vec![namespaced], provider()).await;
    assert_eq!(namespaced["providers"], json!(["meta-llama"]));

    let mut unknown = route();
    unknown.upstream_model = "house-tuned".into();
    let unknown = group_info(model("house-small"), vec![unknown], provider()).await;
    assert_eq!(unknown["providers"], json!(["openai_compat"]));
}

#[tokio::test]
async fn model_group_info_reports_unknown_capabilities_as_false() {
    let mut unknown = route();
    unknown.upstream_model = "house-tuned".into();
    let card = group_info(model("house-small"), vec![unknown], provider()).await;

    assert_eq!(card["model_group"], json!("house-small"));
    assert_eq!(card["mode"], json!("chat"));
    // omp probes more endpoints when vision is unknown, so it is always a boolean.
    assert_eq!(card["supports_vision"], json!(false));
    assert_eq!(card["supports_function_calling"], json!(false));
    assert_eq!(card["supports_reasoning"], json!(false));
    assert_eq!(card["supports_response_schema"], json!(false));
    // The configured context window is the only known limit.
    assert_eq!(card["max_input_tokens"], json!(1000));
    assert_eq!(card["max_output_tokens"], json!(null));
    assert_no_costs(&card);
    assert_eq!(
        params(&card),
        [
            "max_tokens",
            "max_completion_tokens",
            "stream",
            "stream_options",
            "temperature",
        ]
    );
}

#[tokio::test]
async fn model_group_info_counts_only_routes_that_routing_keeps_for_a_feature() {
    // Routing sends a request with tools, images, a JSON schema, or `stream` only to routes
    // whose transport serves it, so a route without that transport does not remove it.
    let mut plain = route();
    plain.capabilities.tools = false;
    plain.capabilities.vision = false;
    plain.capabilities.json_schema = false;
    plain.capabilities.stream = false;
    let subset = group_info(model("gpt"), vec![route(), plain.clone()], provider()).await;
    assert_eq!(subset["supports_function_calling"], json!(true));
    assert_eq!(subset["supports_vision"], json!(true));
    assert_eq!(subset["supports_response_schema"], json!(true));
    for param in [
        "stream",
        "stream_options",
        "tools",
        "tool_choice",
        "response_format",
    ] {
        assert!(params(&subset).contains(&param), "{param} missing");
    }

    let none = group_info(model("gpt"), vec![plain], provider()).await;
    assert_eq!(none["supports_function_calling"], json!(false));
    assert_eq!(none["supports_vision"], json!(false));
    assert_eq!(none["supports_response_schema"], json!(false));
    for param in [
        "stream",
        "stream_options",
        "tools",
        "tool_choice",
        "response_format",
    ] {
        assert!(!params(&none).contains(&param), "{param} must be omitted");
    }

    // Routing keeps this route for tools too, but the catalog cannot vouch for its model.
    let mut unknown = route();
    unknown.upstream_model = "house-tuned".into();
    let mixed = group_info(model("gpt"), vec![route(), unknown], provider()).await;
    assert_eq!(mixed["supports_function_calling"], json!(false));
    assert!(!params(&mixed).contains(&"tools"));
}

#[tokio::test]
async fn model_group_info_marks_embeddings_only_models() {
    let mut embeddings = route();
    embeddings.upstream_model = "text-embedding-3-large".into();
    embeddings.capabilities.chat_completions = false;
    embeddings.capabilities.responses = false;
    let card = group_info(model("embed"), vec![embeddings], provider()).await;

    assert_eq!(card["mode"], json!("embedding"));
    // No chat route serves Responses, so the OpenAI vendor is not claimed.
    assert_eq!(card["providers"], json!(["openai_compat"]));
}

#[tokio::test]
async fn model_group_info_offers_reasoning_effort_without_anthropic_routes() {
    // `/v1/models` hides thinking here because the route is not Anthropic Messages native;
    // `reasoning_effort` on chat completions and Responses needs no such gate.
    let card = group_info(model("gpt"), vec![route()], provider()).await;
    assert_eq!(card["supports_reasoning"], json!(true));
    assert_eq!(card["supports_function_calling"], json!(true));
    assert_eq!(card["supports_vision"], json!(true));
    assert_eq!(card["supports_response_schema"], json!(true));
    assert_eq!(card["max_input_tokens"], json!(1000));
    // The catalog marks gpt-5 `temperature: false`.
    assert_eq!(
        params(&card),
        [
            "max_tokens",
            "max_completion_tokens",
            "stream",
            "stream_options",
            "tools",
            "tool_choice",
            "reasoning_effort",
            "response_format",
        ]
    );

    // A thinking policy implies reasoning even when the catalog has no entry.
    let mut uncataloged = route();
    uncataloged.upstream_model = "house-tuned".into();
    let gemini = group_info(
        model("gemini-2.5-pro-house"),
        vec![uncataloged.clone()],
        provider(),
    )
    .await;
    assert_eq!(gemini["supports_reasoning"], json!(true));
    assert!(params(&gemini).contains(&"reasoning_effort"));

    let plain = group_info(model("house-small"), vec![uncataloged], provider()).await;
    assert!(!params(&plain).contains(&"reasoning_effort"));

    // A ceiling below the effort ladder leaves nothing to request.
    let capped = GatewayModel {
        max_reasoning_effort: Some(ReasoningEffort::Minimal),
        ..model("gpt")
    };
    let capped = group_info(capped, vec![route()], provider()).await;
    assert_eq!(capped["supports_reasoning"], json!(true));
    assert!(!params(&capped).contains(&"reasoning_effort"));
}

#[tokio::test]
async fn model_group_info_needs_reasoning_on_every_chat_route() {
    // The primary route's name implies Gemini thinking, but the fallback serves gpt-4.1,
    // which cannot reason. A request with effort can land on either route.
    let mut gemini = route();
    gemini.upstream_model = "gemini-2.5-pro-house".into();
    let mut fallback = route();
    fallback.upstream_model = "gpt-4.1".into();
    fallback.priority = 10;

    let primary_only = group_info(model("house"), vec![gemini.clone()], provider()).await;
    assert_eq!(primary_only["supports_reasoning"], json!(true));
    assert!(params(&primary_only).contains(&"reasoning_effort"));

    let mixed = group_info(model("house"), vec![gemini, fallback], provider()).await;
    assert_eq!(mixed["supports_reasoning"], json!(false));
    assert!(!params(&mixed).contains(&"reasoning_effort"));
}

#[tokio::test]
async fn model_group_info_drops_temperature_when_the_catalog_rejects_it() {
    let mut snapshot = crate::pricing_catalog::load_vendored_fallback_snapshot();
    let mut route = route();
    route.upstream_model = "gpt-4.1".into();
    let accepted = group_info_with(model("gpt"), vec![route.clone()], provider(), &snapshot).await;
    assert!(params(&accepted).contains(&"temperature"));

    snapshot
        .document
        .providers
        .get_mut("openai")
        .unwrap()
        .models
        .get_mut("gpt-4.1")
        .unwrap()
        .metadata
        .temperature = Some(false);
    let rejected = group_info_with(model("gpt"), vec![route], provider(), &snapshot).await;
    assert!(!params(&rejected).contains(&"temperature"));
}

#[tokio::test]
async fn model_group_info_lists_the_same_models_as_v1_models() {
    let target = model("house-large");
    let alias = GatewayModel {
        alias_target_model_key: Some(target.model_key.clone()),
        ..model("house-default")
    };
    let mut claude = vertex_claude_route();
    claude.model_id = target.id;
    let repo = ListingRepo {
        models: vec![target.clone(), alias.clone()],
        routes: vec![claude],
        provider: vertex_provider(),
    };
    let snapshot = crate::pricing_catalog::load_vendored_fallback_snapshot();
    let visible = vec![target, alias];

    let cards = super::super::listing::list_models(&repo, visible.clone(), &snapshot)
        .await
        .unwrap();
    let groups = list_model_groups(&repo, visible, &snapshot).await.unwrap();

    let card_ids = cards
        .data
        .iter()
        .map(|card| card.id.as_str())
        .collect::<Vec<_>>();
    let group_ids = groups
        .data
        .iter()
        .map(|group| group.model_group.as_str())
        .collect::<Vec<_>>();
    assert_eq!(card_ids, ["house-large", "house-default"]);
    assert_eq!(group_ids, card_ids);
    // The alias resolves to its target's routes and limits.
    assert_eq!(groups.data[1].providers, ["anthropic"]);
    assert_eq!(groups.data[1].max_input_tokens, Some(1_000_000));
}
