//! `/v1/models`: one list that satisfies both the OpenAI and Anthropic list-models contracts,
//! plus additive discovery fields in the style of OpenRouter. Field names are chosen so they
//! never collide with either vendor's schema.
use std::collections::BTreeSet;

use gateway_client_config::{
    ApiFormat, ClientConfigInput, ClientHints, ClientModelCapabilities, ThinkingPolicy,
    client_hints,
};
use gateway_core::{
    GatewayError, GatewayModel, ModelRepository, ModelRoute, PartialCapabilities,
    ProviderCapabilities, ProviderConnection, ProviderRepository, ReasoningEffort,
};
use serde::Serialize;
use time::{Date, Month, OffsetDateTime, format_description::well_known::Rfc3339};

use super::{LoadedRoutes, ModelMetadata, RouteMetadata, load_routes, route_metadata, summarize};
use crate::{
    admin_models::{
        reasoning_level, route_is_eligible, route_thinking_policy, select_display_route,
    },
    effective_route_metadata::{effective_provider_route_capabilities, native_api_format},
    pricing_catalog::PricingCatalogSnapshot,
    resolve_provider_display,
};

pub(super) const MODEL_OWNER: &str = "oceans-llm";

#[derive(Debug, Serialize)]
pub struct ModelsListResponse {
    pub object: &'static str,
    pub data: Vec<ModelCard>,
    /// Anthropic pagination. The gateway always returns the full visible list.
    pub has_more: bool,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ModelCard {
    pub id: String,
    /// OpenAI object discriminator.
    pub object: &'static str,
    /// Anthropic object discriminator.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// Unix seconds of the catalog release date; `0` when unknown.
    pub created: i64,
    /// RFC 3339 form of `created`, as Anthropic clients expect.
    pub created_at: String,
    pub owned_by: &'static str,
    pub display_name: String,
    /// OpenRouter and new-api spelling of `display_name`, read by proxy-style discovery clients.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The model key this alias resolves to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias_of: Option<String>,
    /// Limits are the minimum across enabled routes, so any route can honour them.
    pub context_length: Option<i64>,
    pub max_input_tokens: Option<i64>,
    pub max_tokens: Option<i64>,
    pub architecture: ModelArchitecture,
    /// Anthropic `ModelCapabilities` shape; features the gateway cannot serve report `false`.
    pub capabilities: ModelCapabilities,
    /// new-api endpoint types for the inference APIs, preferred first. Proxy-style discovery
    /// clients pick the wire from this list and let `anthropic` win, so `anthropic` appears
    /// only for models that prefer Anthropic Messages. `client_hints.api_formats` lists every
    /// accepted format. omp should use `discovery.type: litellm`, which reads
    /// `/model_group/info` instead of this list.
    pub supported_endpoint_types: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_hints: Option<ClientHints>,
}

#[derive(Debug, Serialize)]
pub struct ModelArchitecture {
    /// `None` when no enabled route has catalog modality data or serves image generation.
    pub input_modalities: Option<Vec<String>>,
    pub output_modalities: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CapabilitySupport {
    pub supported: bool,
}

impl CapabilitySupport {
    const NO: Self = Self { supported: false };

    const fn from(supported: bool) -> Self {
        Self { supported }
    }
}

#[derive(Debug, Serialize)]
pub struct ModelCapabilities {
    pub batch: CapabilitySupport,
    pub citations: CapabilitySupport,
    pub code_execution: CapabilitySupport,
    pub context_management: ContextManagementCapability,
    pub effort: EffortCapability,
    pub image_input: CapabilitySupport,
    pub pdf_input: CapabilitySupport,
    pub structured_outputs: CapabilitySupport,
    pub thinking: ThinkingCapability,
}

#[derive(Debug, Serialize)]
pub struct ContextManagementCapability {
    pub supported: bool,
    pub clear_thinking_20251015: Option<CapabilitySupport>,
    pub clear_tool_uses_20250919: Option<CapabilitySupport>,
    pub compact_20260112: Option<CapabilitySupport>,
}

#[derive(Debug, Serialize)]
pub struct EffortCapability {
    pub supported: bool,
    pub low: CapabilitySupport,
    pub medium: CapabilitySupport,
    pub high: CapabilitySupport,
    pub xhigh: Option<CapabilitySupport>,
    pub max: CapabilitySupport,
}

#[derive(Debug, Serialize)]
pub struct ThinkingCapability {
    pub supported: bool,
    pub types: ThinkingTypes,
}

#[derive(Debug, Serialize)]
pub struct ThinkingTypes {
    pub adaptive: CapabilitySupport,
    pub enabled: CapabilitySupport,
}

pub(crate) async fn list_models<R>(
    repo: &R,
    models: Vec<GatewayModel>,
    snapshot: &PricingCatalogSnapshot,
) -> Result<ModelsListResponse, GatewayError>
where
    R: ModelRepository + ProviderRepository,
{
    let loaded = load_routes(repo, &models).await?;
    let data = models
        .iter()
        .enumerate()
        .map(|(index, model)| model_card(&ModelView::build(&loaded, index, model, snapshot)))
        .collect::<Vec<_>>();
    Ok(ModelsListResponse {
        object: "list",
        has_more: false,
        first_id: data.first().map(|card| card.id.clone()),
        last_id: data.last().map(|card| card.id.clone()),
        data,
    })
}

/// Per-model facts shared by `/v1/models` and `/model_group/info`, so the two listings
/// cannot drift apart.
pub(super) struct ModelView<'a> {
    pub(super) model: &'a GatewayModel,
    /// The strictest ceiling along the alias chain, as enforced at request time.
    pub(super) reasoning_ceiling: Option<ReasoningEffort>,
    /// Union of the APIs the eligible routes serve.
    pub(super) transport: ProviderCapabilities,
    /// Every chat-capable route speaks Anthropic Messages upstream.
    pub(super) messages_native: bool,
    /// Every chat-capable route (chat completions or Responses) serves the Responses API.
    pub(super) responses_everywhere: bool,
    /// The display route: the first eligible route, as the admin listing picks it.
    pub(super) primary: Option<(&'a ModelRoute, &'a ProviderConnection)>,
    /// Index of the primary route in `summary.routes`, when that route is enabled.
    pub(super) primary_detail: Option<usize>,
    pub(super) thinking_policy: Option<ThinkingPolicy>,
    /// Reasoning that every chat-capable route honours.
    pub(super) chat_reasoning: ChatReasoning,
    /// Request features a chat request can rely on, after routing filters on them.
    pub(super) chat_features: ChatFeatures,
    /// Limits and capabilities summarised across the enabled routes.
    pub(super) summary: ModelMetadata,
}

/// Reasoning a client can rely on whichever chat-capable route serves the request.
/// Providers reject `reasoning_effort` for a model that cannot think, and route selection
/// does not check for reasoning, so one route without it removes it for the model.
#[derive(Debug, Default)]
pub(super) struct ChatReasoning {
    pub(super) reasons: bool,
    /// Effort levels every chat-capable route accepts, within the model's ceiling.
    pub(super) effort_levels: Vec<ReasoningEffort>,
}

/// Request features routing filters on. A request that uses one goes only to the eligible
/// routes whose transport serves it, so those routes alone decide the answer: at least one
/// must exist, and each must serve a model the catalog says has the feature. Routes without
/// the transport do not count against it.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct ChatFeatures {
    pub(super) stream: bool,
    pub(super) tools: bool,
    pub(super) vision: bool,
    pub(super) structured_output: bool,
}

type EligibleRoute<'a> = (&'a ModelRoute, &'a ProviderConnection, ProviderCapabilities);
/// An eligible chat-capable route and its metadata.
type ChatRoute<'a, 'b> = (&'b EligibleRoute<'a>, Option<&'b RouteMetadata>);

impl<'a> ModelView<'a> {
    pub(super) fn build(
        loaded: &'a LoadedRoutes,
        index: usize,
        model: &'a GatewayModel,
        snapshot: &PricingCatalogSnapshot,
    ) -> Self {
        let enabled = loaded.enabled_routes(index).collect::<Vec<_>>();
        let details = enabled
            .iter()
            .map(|route| route_metadata(route, loaded.providers.get(&route.provider_key), snapshot))
            .collect::<Vec<_>>();
        let execution = loaded.execution(index).unwrap_or(model);
        let routes = loaded
            .routes
            .get(&execution.id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let primary = select_display_route(&loaded.providers, routes).and_then(|route| {
            loaded
                .providers
                .get(&route.provider_key)
                .map(|provider| (route, provider))
        });
        let thinking_policy = primary.and_then(|(route, provider)| {
            let display = resolve_provider_display(&route.provider_key, Some(provider));
            route_thinking_policy(model, execution, route, provider, Some(&display))
        });
        // A request can land on any eligible route: the planner picks by weight within a
        // priority tier and falls back to later tiers. Served APIs are therefore the union.
        // Features routing filters on need only the routes that pass the filter; others,
        // such as reasoning, need every chat-capable route to honour them.
        let eligible = eligible_routes(loaded, routes);
        let chat_routes = eligible
            .iter()
            .filter(|(_, _, caps)| caps.chat_completions || caps.responses)
            .map(|eligible| {
                let detail = enabled
                    .iter()
                    .position(|enabled| enabled.id == eligible.0.id)
                    .map(|position| &details[position]);
                (eligible, detail)
            })
            .collect::<Vec<ChatRoute<'_, '_>>>();
        let reasoning_ceiling = loaded.reasoning_ceilings[index];
        let chat_reasoning = chat_reasoning(&chat_routes, reasoning_ceiling, |route, provider| {
            let display = resolve_provider_display(&route.provider_key, Some(provider));
            route_thinking_policy(model, execution, route, provider, Some(&display))
        });
        let chat_features = chat_features(&chat_routes);
        Self {
            model,
            reasoning_ceiling,
            transport: served_apis(&eligible),
            messages_native: every_chat_route(
                &eligible,
                |caps| caps.chat_completions,
                |(route, provider, _)| {
                    native_api_format(provider, route) == Some(ApiFormat::AnthropicMessages)
                },
            ),
            responses_everywhere: every_chat_route(
                &eligible,
                |caps| caps.chat_completions || caps.responses,
                |(_, _, caps)| caps.responses,
            ),
            primary_detail: primary
                .and_then(|(primary, _)| enabled.iter().position(|route| route.id == primary.id)),
            primary,
            thinking_policy,
            chat_reasoning,
            chat_features,
            summary: summarize(model.model_key.clone(), details),
        }
    }

    /// Catalog reasoning, or a thinking policy that implies it, whatever wire the client uses.
    pub(super) fn can_reason(&self) -> bool {
        self.summary.capabilities.reasoning == Some(true) || implies_reasoning(self.thinking_policy)
    }
}

fn implies_reasoning(policy: Option<ThinkingPolicy>) -> bool {
    matches!(
        policy,
        Some(
            ThinkingPolicy::AnthropicSafeEffort
                | ThinkingPolicy::GeminiLevel { .. }
                | ThinkingPolicy::GeminiBudget
        )
    )
}

/// Intersects the reasoning of every chat-capable route: its catalog reasoning flag, or a
/// thinking policy from `policy` that implies reasoning.
fn chat_reasoning(
    chat_routes: &[ChatRoute<'_, '_>],
    ceiling: Option<ReasoningEffort>,
    policy: impl Fn(&ModelRoute, &ProviderConnection) -> Option<ThinkingPolicy>,
) -> ChatReasoning {
    chat_routes
        .iter()
        .map(|((route, provider, _), detail)| {
            let catalog = detail.and_then(|detail| detail.capabilities.reasoning);
            let policy = policy(route, provider);
            let reasons = catalog == Some(true) || implies_reasoning(policy);
            ChatReasoning {
                reasons,
                effort_levels: effort_levels(reasons, policy, ceiling),
            }
        })
        .reduce(|acc, next| ChatReasoning {
            reasons: acc.reasons && next.reasons,
            effort_levels: acc
                .effort_levels
                .into_iter()
                .filter(|level| next.effort_levels.contains(level))
                .collect(),
        })
        .unwrap_or_default()
}

fn chat_features(chat_routes: &[ChatRoute<'_, '_>]) -> ChatFeatures {
    let routed = |transport: fn(&ProviderCapabilities) -> bool,
                  catalog: fn(&super::ModelCapabilities) -> Option<bool>| {
        let mut serving = chat_routes
            .iter()
            .filter(|((_, _, caps), _)| transport(caps))
            .peekable();
        serving.peek().is_some()
            && serving.all(|(_, detail)| {
                detail.is_some_and(|detail| catalog(&detail.capabilities) == Some(true))
            })
    };
    ChatFeatures {
        stream: chat_routes.iter().any(|((_, _, caps), _)| caps.stream),
        tools: routed(|caps| caps.tools, |catalog| catalog.tools),
        vision: routed(|caps| caps.vision, |catalog| catalog.vision),
        structured_output: routed(|caps| caps.json_schema, |catalog| catalog.structured_output),
    }
}

fn eligible_routes<'a>(
    loaded: &'a LoadedRoutes,
    routes: &'a [ModelRoute],
) -> Vec<EligibleRoute<'a>> {
    routes
        .iter()
        .filter(|route| route_is_eligible(&loaded.providers, route))
        .filter_map(|route| {
            let provider = loaded.providers.get(&route.provider_key)?;
            let capabilities = effective_provider_route_capabilities(
                route,
                Some(provider),
                PartialCapabilities::default(),
            );
            Some((route, provider, capabilities))
        })
        .collect()
}

fn served_apis(eligible: &[EligibleRoute<'_>]) -> ProviderCapabilities {
    eligible
        .iter()
        .fold(ProviderCapabilities::none(), |acc, (_, _, caps)| {
            ProviderCapabilities {
                chat_completions: acc.chat_completions || caps.chat_completions,
                responses: acc.responses || caps.responses,
                embeddings: acc.embeddings || caps.embeddings,
                images: acc.images || caps.images,
                decisions: acc.decisions || caps.decisions,
                ..acc
            }
        })
}

/// At least one route is chat-capable by `chat`, and every such route satisfies `holds`.
fn every_chat_route(
    eligible: &[EligibleRoute<'_>],
    chat: impl Fn(&ProviderCapabilities) -> bool,
    holds: impl Fn(&EligibleRoute<'_>) -> bool,
) -> bool {
    let mut chat_routes = eligible.iter().filter(|(_, _, caps)| chat(caps)).peekable();
    chat_routes.peek().is_some() && chat_routes.all(holds)
}

fn model_card(view: &ModelView<'_>) -> ModelCard {
    let ModelView {
        model,
        reasoning_ceiling,
        messages_native,
        thinking_policy,
        transport,
        primary,
        ref summary,
        ..
    } = *view;
    let details = &summary.routes;
    let display_name = details
        .iter()
        .find_map(|route| route.display_name.clone())
        .unwrap_or_else(|| model.model_key.clone());
    let released = details
        .iter()
        .find_map(|route| route.release_date.as_deref().and_then(parse_release_date))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let input_modalities = common_modalities(details, |route| {
        route
            .modalities
            .as_ref()
            .map(|modalities| &modalities.input)
    });
    let output_modalities = with_served_outputs(
        common_modalities(details, |route| {
            route
                .modalities
                .as_ref()
                .map(|modalities| &modalities.output)
        }),
        transport,
    );

    let image_input = summary.capabilities.vision == Some(true);
    let pdf_input = image_input
        && input_modalities
            .as_ref()
            .is_some_and(|modalities| modalities.iter().any(|value| value == "pdf"));
    let input_modalities = input_modalities.map(|modalities| {
        modalities
            .into_iter()
            .filter(|value| image_input || !matches!(value.as_str(), "image" | "pdf"))
            .collect()
    });
    // `capabilities` describes Anthropic Messages request features. `/v1/messages` hands
    // content blocks, `thinking`, and `output_config` to the route unchanged, so they are only
    // advertised when every route a request could land on speaks Anthropic Messages upstream.
    let reasoning = messages_native && view.can_reason();
    let efforts = effort_levels(reasoning, thinking_policy, reasoning_ceiling);
    let effort = |level| CapabilitySupport::from(efforts.contains(&level));
    let adaptive = reasoning && thinking_policy == Some(ThinkingPolicy::AnthropicSafeEffort);

    let hints = client_hints(&ClientConfigInput {
        model_id: model.model_key.clone(),
        display_name: display_name.clone(),
        upstream_model: primary.map(|(route, _)| route.upstream_model.clone()),
        capabilities: ClientModelCapabilities {
            chat_completions: transport.chat_completions,
            responses: transport.responses,
            tool_calling: summary.capabilities.tools == Some(true),
            attachments: pdf_input,
            vision: image_input,
        },
        thinking_policy,
        native_api_format: primary.and_then(|(route, provider)| native_api_format(provider, route)),
        max_reasoning_effort: reasoning_ceiling.map(reasoning_level),
        ..ClientConfigInput::default()
    });

    ModelCard {
        id: model.model_key.clone(),
        object: "model",
        kind: "model",
        created: released.unix_timestamp(),
        created_at: released
            .format(&Rfc3339)
            .expect("UTC midnight formats as RFC 3339"),
        owned_by: MODEL_OWNER,
        name: display_name.clone(),
        display_name,
        description: model.description.clone(),
        alias_of: model.alias_target_model_key.clone(),
        context_length: summary.limits.context,
        max_input_tokens: summary.limits.input.or(summary.limits.context),
        max_tokens: summary.limits.output,
        architecture: ModelArchitecture {
            input_modalities,
            output_modalities,
        },
        capabilities: ModelCapabilities {
            // The gateway serves neither Anthropic Message Batches nor Anthropic-only
            // server tools, so these stay off regardless of the upstream model.
            batch: CapabilitySupport::NO,
            citations: CapabilitySupport::NO,
            code_execution: CapabilitySupport::NO,
            context_management: ContextManagementCapability {
                supported: false,
                clear_thinking_20251015: None,
                clear_tool_uses_20250919: None,
                compact_20260112: None,
            },
            effort: EffortCapability {
                supported: !efforts.is_empty(),
                low: effort(ReasoningEffort::Low),
                medium: effort(ReasoningEffort::Medium),
                high: effort(ReasoningEffort::High),
                xhigh: Some(effort(ReasoningEffort::XHigh)),
                max: effort(ReasoningEffort::Max),
            },
            image_input: CapabilitySupport::from(messages_native && image_input),
            pdf_input: CapabilitySupport::from(messages_native && pdf_input),
            structured_outputs: CapabilitySupport::from(
                messages_native && summary.capabilities.structured_output == Some(true),
            ),
            thinking: ThinkingCapability {
                supported: reasoning,
                types: ThinkingTypes {
                    adaptive: CapabilitySupport::from(adaptive),
                    // Adaptive-only Claude families reject manual budgets.
                    enabled: CapabilitySupport::from(reasoning && !adaptive),
                },
            },
        },
        supported_endpoint_types: supported_endpoint_types(
            transport,
            hints.as_ref().map(|hints| hints.preferred_api_format),
        ),
        client_hints: hints,
    }
}

fn supported_endpoint_types(
    transport: ProviderCapabilities,
    preferred: Option<ApiFormat>,
) -> Vec<&'static str> {
    let responses_first = preferred == Some(ApiFormat::OpenAiResponses);
    [
        (preferred == Some(ApiFormat::AnthropicMessages), "anthropic"),
        (responses_first && transport.responses, "openai-response"),
        (transport.chat_completions, "openai"),
        (!responses_first && transport.responses, "openai-response"),
    ]
    .into_iter()
    .filter_map(|(supported, endpoint)| supported.then_some(endpoint))
    .collect()
}

/// Effort levels a client may request, mirroring the Pi `thinkingLevelMap` translation and
/// clamped to the model's configured ceiling.
fn effort_levels(
    reasoning: bool,
    policy: Option<ThinkingPolicy>,
    ceiling: Option<ReasoningEffort>,
) -> Vec<ReasoningEffort> {
    use ReasoningEffort::{High, Low, Max, Medium, XHigh};
    if !reasoning {
        return Vec::new();
    }
    let levels: &[ReasoningEffort] = match policy {
        Some(ThinkingPolicy::AnthropicSafeEffort) => &[Low, Medium, High, XHigh, Max],
        Some(ThinkingPolicy::AnthropicManualBudget) => &[],
        Some(ThinkingPolicy::GeminiLevel {
            supports_medium: false,
            ..
        }) => &[Low, High],
        Some(ThinkingPolicy::GeminiLevel { .. } | ThinkingPolicy::GeminiBudget) | None => {
            &[Low, Medium, High]
        }
    };
    levels
        .iter()
        .copied()
        .filter(|level| ceiling.is_none_or(|ceiling| *level <= ceiling))
        .collect()
}

/// Modalities every route with catalog data agrees on.
fn common_modalities<'a>(
    routes: &'a [RouteMetadata],
    select: impl Fn(&'a RouteMetadata) -> Option<&'a Vec<String>>,
) -> Option<Vec<String>> {
    routes
        .iter()
        .filter_map(select)
        .filter(|values| !values.is_empty())
        .map(|values| values.iter().cloned().collect::<BTreeSet<_>>())
        .reduce(|acc, values| acc.intersection(&values).cloned().collect())
        .map(|values| values.into_iter().collect())
}

/// Outputs a request can reach on some eligible route, which the intersection of catalog
/// modalities would drop. OpenRouter marks System One decision models with a `decisions`
/// output modality and image generators with `image`; the catalog has no decision models
/// and no image-only models, so the gateway adds both from route capabilities. A separate
/// image-only or decisions-only route also empties that intersection, so text comes back
/// whenever some route serves chat.
fn with_served_outputs(
    modalities: Option<Vec<String>>,
    transport: ProviderCapabilities,
) -> Option<Vec<String>> {
    if !transport.decisions && !transport.images {
        return modalities;
    }
    let mut modalities = modalities.unwrap_or_default();
    for (served, modality) in [
        (transport.decisions, "decisions"),
        (transport.images, "image"),
        (transport.chat_completions || transport.responses, "text"),
    ] {
        if served && !modalities.iter().any(|value| value == modality) {
            modalities.push(modality.to_string());
        }
    }
    modalities.sort();
    Some(modalities)
}

/// Accepts the catalog's `YYYY-MM-DD` and `YYYY-MM` release dates.
fn parse_release_date(value: &str) -> Option<OffsetDateTime> {
    let mut parts = value.trim().splitn(3, '-');
    let year = parts.next()?.parse().ok()?;
    let month = Month::try_from(parts.next()?.parse::<u8>().ok()?).ok()?;
    let day = parts.next().map_or(Some(1), |day| day.parse().ok())?;
    Some(
        Date::from_calendar_date(year, month, day)
            .ok()?
            .midnight()
            .assume_utc(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ApiFormat, ProviderCapabilities, ThinkingPolicy, effort_levels, parse_release_date,
        supported_endpoint_types,
    };
    use gateway_core::ReasoningEffort::{High, Low, Max, Medium, XHigh};

    #[test]
    fn release_dates_parse_day_and_month_precision() {
        assert_eq!(
            parse_release_date("2025-05-22").map(|value| value.unix_timestamp()),
            Some(1_747_872_000)
        );
        assert_eq!(
            parse_release_date("2025-05").map(|value| value.unix_timestamp()),
            Some(1_746_057_600)
        );
        assert!(parse_release_date("unknown").is_none());
        assert!(parse_release_date("2025-13-01").is_none());
    }

    #[test]
    fn effort_levels_follow_policy_and_ceiling() {
        let safe = Some(ThinkingPolicy::AnthropicSafeEffort);
        assert_eq!(
            effort_levels(true, safe, None),
            [Low, Medium, High, XHigh, Max]
        );
        assert_eq!(effort_levels(true, safe, Some(High)), [Low, Medium, High]);
        assert!(effort_levels(false, safe, None).is_empty());
        assert!(effort_levels(true, Some(ThinkingPolicy::AnthropicManualBudget), None).is_empty());
        assert_eq!(
            effort_levels(
                true,
                Some(ThinkingPolicy::GeminiLevel {
                    supports_minimal: false,
                    supports_medium: false,
                }),
                None,
            ),
            [Low, High]
        );
        assert_eq!(effort_levels(true, None, None), [Low, Medium, High]);
    }

    #[test]
    fn endpoint_types_put_anthropic_first_only_for_anthropic_preferred_models() {
        let chat = ProviderCapabilities {
            chat_completions: true,
            responses: true,
            ..ProviderCapabilities::none()
        };
        assert_eq!(
            supported_endpoint_types(chat, Some(ApiFormat::AnthropicMessages)),
            ["anthropic", "openai", "openai-response"]
        );
        // `/v1/messages` still serves this model, but omp must keep it on chat completions.
        assert_eq!(
            supported_endpoint_types(chat, Some(ApiFormat::OpenAiChatCompletions)),
            ["openai", "openai-response"]
        );
        // Responses-native routes that also serve chat lead with the preferred format.
        assert_eq!(
            supported_endpoint_types(chat, Some(ApiFormat::OpenAiResponses)),
            ["openai-response", "openai"]
        );
        let responses_only = ProviderCapabilities {
            responses: true,
            ..ProviderCapabilities::none()
        };
        assert_eq!(
            supported_endpoint_types(responses_only, Some(ApiFormat::OpenAiResponses)),
            ["openai-response"]
        );
    }
}
