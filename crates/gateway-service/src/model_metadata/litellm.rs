//! LiteLLM `GET /model_group/info`, the metadata omp `discovery.type: litellm` reads. It
//! projects the same per-model view as `/v1/models`, so both list the same models with the
//! same limits and capabilities.
use gateway_core::{
    GatewayError, GatewayModel, ModelRepository, ProviderCapabilities, ProviderRepository,
};
use serde::Serialize;

use super::{
    listing::{MODEL_OWNER, ModelView, effort_levels},
    load_routes,
};
use crate::{
    ModelIconKey,
    pricing_catalog::{PricingCatalogCostDocument, PricingCatalogSnapshot},
    resolve_model_icon_key,
};

const OPENAI: &str = "openai";
const TOKENS_PER_MILLION: f64 = 1_000_000.0;
const BASE_OPENAI_PARAMS: [&str; 4] = [
    "max_tokens",
    "max_completion_tokens",
    "stream",
    "stream_options",
];

#[derive(Debug, Serialize)]
pub struct ModelGroupInfoResponse {
    pub data: Vec<ModelGroupInfo>,
}

#[derive(Debug, Serialize)]
pub struct ModelGroupInfo {
    pub model_group: String,
    /// One lowercase vendor family, never empty. omp sends `["openai"]` models over
    /// `/v1/responses` and everything else over the provider's configured API.
    pub providers: Vec<String>,
    pub mode: ModelGroupMode,
    /// omp reads this as the context window.
    pub max_input_tokens: Option<i64>,
    pub max_output_tokens: Option<i64>,
    /// USD per token. The four costs are all present or all absent, because a partial set
    /// makes omp probe further LiteLLM endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_cost_per_token: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_cost_per_token: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_token_cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_token_cost: Option<f64>,
    pub supports_function_calling: bool,
    /// Always a boolean: an unknown value makes omp probe further LiteLLM endpoints.
    pub supports_vision: bool,
    pub supports_reasoning: bool,
    pub supports_response_schema: bool,
    pub supported_openai_params: Vec<&'static str>,
}

/// LiteLLM has no mode that excludes System One decision models, so they stay `chat`;
/// clients list them as chat models either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelGroupMode {
    Chat,
    Embedding,
}

#[derive(Debug, Clone, Copy)]
struct TokenCosts {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

pub(crate) async fn list_model_groups<R>(
    repo: &R,
    models: Vec<GatewayModel>,
    snapshot: &PricingCatalogSnapshot,
) -> Result<ModelGroupInfoResponse, GatewayError>
where
    R: ModelRepository + ProviderRepository,
{
    let loaded = load_routes(repo, &models).await?;
    let data = models
        .iter()
        .enumerate()
        .map(|(index, model)| model_group_info(&ModelView::build(&loaded, index, model, snapshot)))
        .collect();
    Ok(ModelGroupInfoResponse { data })
}

fn model_group_info(view: &ModelView<'_>) -> ModelGroupInfo {
    let capabilities = &view.summary.capabilities;
    let limits = &view.summary.limits;
    // Unlike `/v1/models` thinking, this is not gated on Anthropic Messages routes: omp asks
    // for effort over the OpenAI wires (`reasoning_effort`, `reasoning.effort`), not `thinking`.
    let reasoning = view.can_reason();
    let costs = view
        .primary_detail
        .and_then(|index| view.summary.routes[index].pricing.as_ref())
        .and_then(per_token_costs);
    ModelGroupInfo {
        model_group: view.model.model_key.clone(),
        providers: vec![provider_family(view)],
        mode: mode(view.transport),
        max_input_tokens: limits.input.or(limits.context),
        max_output_tokens: limits.output,
        input_cost_per_token: costs.map(|costs| costs.input),
        output_cost_per_token: costs.map(|costs| costs.output),
        cache_read_input_token_cost: costs.map(|costs| costs.cache_read),
        cache_creation_input_token_cost: costs.map(|costs| costs.cache_write),
        supports_function_calling: capabilities.tools == Some(true),
        supports_vision: capabilities.vision == Some(true),
        supports_reasoning: reasoning,
        supports_response_schema: capabilities.structured_output == Some(true),
        supported_openai_params: supported_openai_params(view, reasoning),
    }
}

/// Catalog prices are USD per million tokens; LiteLLM wants USD per token. `None` unless
/// all four rates are known.
fn per_token_costs(pricing: &PricingCatalogCostDocument) -> Option<TokenCosts> {
    let rate = |value: Option<&str>| {
        value?
            .trim()
            .parse::<f64>()
            .ok()
            .map(|usd_per_million| usd_per_million / TOKENS_PER_MILLION)
    };
    Some(TokenCosts {
        input: rate(pricing.input.as_deref())?,
        output: rate(pricing.output.as_deref())?,
        cache_read: rate(pricing.cache_read.as_deref())?,
        cache_write: rate(pricing.cache_write.as_deref())?,
    })
}

fn mode(transport: ProviderCapabilities) -> ModelGroupMode {
    if transport.embeddings && !transport.chat_completions && !transport.responses {
        ModelGroupMode::Embedding
    } else {
        ModelGroupMode::Chat
    }
}

/// The vendor family of the primary route's upstream model. `openai` is claimed only when
/// every chat route serves Responses, because omp then sends the model over `/v1/responses`;
/// otherwise the route's provider type stands in.
fn provider_family(view: &ModelView<'_>) -> String {
    let host = view
        .primary
        .map(|(_, provider)| provider.provider_type.trim().to_ascii_lowercase())
        .filter(|provider_type| !provider_type.is_empty());
    let vendor = view
        .primary
        .and_then(|(route, _)| upstream_vendor(&route.upstream_model))
        .or_else(|| host.clone());
    match vendor {
        Some(vendor) if vendor != OPENAI || view.responses_everywhere => vendor,
        _ => host
            .filter(|host| host != OPENAI)
            .unwrap_or_else(|| MODEL_OWNER.to_string()),
    }
}

/// The model-name identity the request log uses, then the `vendor/` namespace prefix.
fn upstream_vendor(upstream_model: &str) -> Option<String> {
    resolve_model_icon_key([upstream_model])
        .and_then(icon_vendor)
        .map(str::to_string)
        .or_else(|| {
            upstream_model
                .split_once('/')
                .map(|(namespace, _)| namespace.trim().to_ascii_lowercase())
                .filter(|namespace| !namespace.is_empty())
        })
}

const fn icon_vendor(key: ModelIconKey) -> Option<&'static str> {
    match key {
        ModelIconKey::Anthropic | ModelIconKey::Claude => Some("anthropic"),
        ModelIconKey::OpenAI => Some(OPENAI),
        ModelIconKey::Gemini => Some("google"),
        ModelIconKey::DeepSeek => Some("deepseek"),
        ModelIconKey::Qwen => Some("qwen"),
        ModelIconKey::TypeSafe => Some("typesafe"),
        // Hosts, not model vendors.
        ModelIconKey::OpenRouter | ModelIconKey::VertexAI => None,
    }
}

fn supported_openai_params(view: &ModelView<'_>, reasoning: bool) -> Vec<&'static str> {
    let capabilities = &view.summary.capabilities;
    let mut params = BASE_OPENAI_PARAMS.to_vec();
    if accepts_temperature(view) {
        params.push("temperature");
    }
    if capabilities.tools == Some(true) {
        params.extend(["tools", "tool_choice"]);
    }
    // Manual-budget Claude models and ceilings below the ladder leave no effort to request.
    if !effort_levels(reasoning, view.thinking_policy, view.reasoning_ceiling).is_empty() {
        params.push("reasoning_effort");
    }
    if capabilities.structured_output == Some(true) {
        params.push("response_format");
    }
    params
}

/// A request can land on any enabled route, so one route rejecting `temperature` drops it.
fn accepts_temperature(view: &ModelView<'_>) -> bool {
    !view.summary.routes.iter().any(|route| {
        route
            .catalog_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.temperature == Some(false))
    })
}
