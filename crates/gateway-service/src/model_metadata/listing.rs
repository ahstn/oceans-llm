//! `/v1/models`: one list that satisfies both the OpenAI and Anthropic list-models contracts,
//! plus additive discovery fields in the style of OpenRouter. Field names are chosen so they
//! never collide with either vendor's schema.
use std::collections::BTreeSet;

use gateway_client_config::{
    ApiFormat, ClientConfigInput, ClientHints, ClientModelCapabilities, ThinkingPolicy,
    client_hints,
};
use gateway_core::{
    GatewayError, GatewayModel, ModelRepository, ProviderCapabilities, ProviderRepository,
    ReasoningEffort,
};
use serde::Serialize;
use time::{Date, Month, OffsetDateTime, format_description::well_known::Rfc3339};

use super::{RouteMetadata, load_routes, route_metadata, summarize};
use crate::{
    admin_models::{
        reasoning_level, route_is_eligible, route_thinking_policy, select_display_route,
    },
    effective_route_metadata::{effective_provider_route_capabilities, native_api_format},
    pricing_catalog::PricingCatalogSnapshot,
    resolve_provider_display,
};

const MODEL_OWNER: &str = "oceans-llm";

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
    /// OpenRouter and new-api spelling of `display_name`, read by omp proxy discovery.
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
    /// new-api endpoint types for the inference APIs, preferred first. omp `discovery.type: proxy` picks the wire
    /// from this list and lets `anthropic` win, so `anthropic` appears only for models that
    /// prefer Anthropic Messages. `client_hints.api_formats` lists every accepted format.
    pub supported_endpoint_types: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_hints: Option<ClientHints>,
}

#[derive(Debug, Serialize)]
pub struct ModelArchitecture {
    /// `None` when no enabled route has catalog modality data.
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
        .map(|(index, model)| {
            let details = loaded
                .enabled_routes(index)
                .map(|route| {
                    route_metadata(route, loaded.providers.get(&route.provider_key), snapshot)
                })
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
            // priority tier and falls back to later tiers. Served APIs are therefore the union,
            // while Anthropic request features need every chat-capable route to honour them.
            let eligible = routes
                .iter()
                .filter(|route| route_is_eligible(&loaded.providers, route))
                .filter_map(|route| {
                    let provider = loaded.providers.get(&route.provider_key)?;
                    let capabilities = effective_provider_route_capabilities(
                        Some(route.capabilities),
                        Some(provider),
                        Some(route),
                    );
                    Some((route, provider, capabilities))
                })
                .collect::<Vec<_>>();
            let transport =
                eligible
                    .iter()
                    .fold(ProviderCapabilities::none(), |acc, (_, _, caps)| {
                        ProviderCapabilities {
                            chat_completions: acc.chat_completions || caps.chat_completions,
                            responses: acc.responses || caps.responses,
                            decisions: acc.decisions || caps.decisions,
                            ..acc
                        }
                    });
            let mut chat_routes = eligible
                .iter()
                .filter(|(_, _, caps)| caps.chat_completions)
                .peekable();
            let messages_native = chat_routes.peek().is_some()
                && chat_routes.all(|(route, provider, _)| {
                    native_api_format(provider, route) == Some(ApiFormat::AnthropicMessages)
                });
            model_card(ModelCardContext {
                model,
                reasoning_ceiling: loaded.reasoning_ceilings[index],
                messages_native,
                details,
                thinking_policy,
                transport,
                upstream_model: primary.map(|(route, _)| route.upstream_model.clone()),
                native_api_format: primary
                    .and_then(|(route, provider)| native_api_format(provider, route)),
            })
        })
        .collect::<Vec<_>>();
    Ok(ModelsListResponse {
        object: "list",
        has_more: false,
        first_id: data.first().map(|card| card.id.clone()),
        last_id: data.last().map(|card| card.id.clone()),
        data,
    })
}

struct ModelCardContext<'a> {
    model: &'a GatewayModel,
    /// The strictest ceiling along the alias chain, as enforced at request time.
    reasoning_ceiling: Option<ReasoningEffort>,
    /// Every chat-capable route speaks Anthropic Messages upstream.
    messages_native: bool,
    details: Vec<RouteMetadata>,
    thinking_policy: Option<ThinkingPolicy>,
    transport: ProviderCapabilities,
    upstream_model: Option<String>,
    native_api_format: Option<gateway_client_config::ApiFormat>,
}

fn model_card(context: ModelCardContext<'_>) -> ModelCard {
    let ModelCardContext {
        model,
        reasoning_ceiling,
        messages_native,
        details,
        thinking_policy,
        transport,
        upstream_model,
        native_api_format,
    } = context;
    let display_name = details
        .iter()
        .find_map(|route| route.display_name.clone())
        .unwrap_or_else(|| model.model_key.clone());
    let released = details
        .iter()
        .find_map(|route| route.release_date.as_deref().and_then(parse_release_date))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let input_modalities = common_modalities(&details, |route| {
        route
            .modalities
            .as_ref()
            .map(|modalities| &modalities.input)
    });
    let output_modalities = with_decisions_output(
        common_modalities(&details, |route| {
            route
                .modalities
                .as_ref()
                .map(|modalities| &modalities.output)
        }),
        transport.decisions,
    );
    let summary = summarize(model.model_key.clone(), details);

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
    let reasoning = summary.capabilities.reasoning == Some(true)
        || matches!(
            thinking_policy,
            Some(
                ThinkingPolicy::AnthropicSafeEffort
                    | ThinkingPolicy::GeminiLevel { .. }
                    | ThinkingPolicy::GeminiBudget
            )
        );
    // `capabilities` describes Anthropic Messages request features. `/v1/messages` hands
    // content blocks, `thinking`, and `output_config` to the route unchanged, so they are only
    // advertised when every route a request could land on speaks Anthropic Messages upstream.
    let reasoning = messages_native && reasoning;
    let efforts = effort_levels(reasoning, thinking_policy, reasoning_ceiling);
    let effort = |level| CapabilitySupport::from(efforts.contains(&level));
    let adaptive = reasoning && thinking_policy == Some(ThinkingPolicy::AnthropicSafeEffort);

    let hints = client_hints(&ClientConfigInput {
        model_id: model.model_key.clone(),
        display_name: display_name.clone(),
        upstream_model,
        capabilities: ClientModelCapabilities {
            chat_completions: transport.chat_completions,
            responses: transport.responses,
            tool_calling: summary.capabilities.tools == Some(true),
            attachments: pdf_input,
            vision: image_input,
        },
        thinking_policy,
        native_api_format,
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

/// OpenRouter marks System One decision models with a `decisions` output modality. The
/// catalog has no decision models, so the gateway adds it from route capabilities.
fn with_decisions_output(modalities: Option<Vec<String>>, decisions: bool) -> Option<Vec<String>> {
    if !decisions {
        return modalities;
    }
    let mut modalities = modalities.unwrap_or_default();
    if !modalities.iter().any(|value| value == "decisions") {
        modalities.push("decisions".to_string());
        modalities.sort();
    }
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
