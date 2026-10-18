use std::collections::BTreeMap;

use anyhow::{Context, bail};
use gateway_core::{
    AwsBedrockApiStyle, AwsBedrockRouteCompatibility, GitHubCopilotRouteCompatibility, Money4,
    OpenAiCompatDecisionsApi, OpenAiCompatDeveloperRole, OpenAiCompatEmptyTools,
    OpenAiCompatMaxTokensField, OpenAiCompatReasoningEffort, OpenAiCompatRouteCompatibility,
    OpenRouterMaxPrice, OpenRouterPercentileCutoffs, OpenRouterPercentilePreference,
    OpenRouterProviderRouting, OpenRouterRouteCompatibility, ProviderCapabilities, ReasoningEffort,
    RouteCompatibility, RoutePricingOverride, enforce_reasoning_effort_map,
    route_capability_defaults,
};
use gateway_providers::BedrockEndpointKind;
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Value};

use super::models::is_valid_route_id;
use super::providers::{AwsBedrockProviderConfig, ProviderConfig};

const fn default_route_priority() -> i32 {
    100
}

const fn default_route_weight() -> f64 {
    1.0
}

const fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRouteConfig {
    #[serde(default)]
    pub id: Option<String>,
    pub provider: String,
    pub upstream_model: String,
    #[serde(default = "default_route_priority")]
    pub priority: i32,
    #[serde(default = "default_route_weight")]
    pub weight: f64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub context_window_tokens: Option<i64>,
    #[serde(default)]
    pub pricing_override: Option<RoutePricingOverrideConfig>,
    #[serde(default)]
    pub extra_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub extra_body: Map<String, Value>,
    #[serde(default)]
    pub capabilities: RouteCapabilitiesConfig,
    #[serde(default)]
    pub compatibility: RouteCompatibilityConfig,
}

impl ModelRouteConfig {
    /// Capabilities that the route's provider and compatibility profile already determine.
    /// Explicit `capabilities` flags override these, and validation still rejects flags a
    /// profile cannot serve. The policy lives in [`route_capability_defaults`] so seeded route
    /// flags and request routing cannot drift apart.
    pub(super) fn capability_defaults(
        &self,
        provider: Option<&ProviderConfig>,
    ) -> ProviderCapabilities {
        // An unknown provider fails validation; use the generic default until then.
        let provider_type = provider.map_or("openai_compat", ProviderConfig::provider_type);
        route_capability_defaults(
            provider_type,
            &self.upstream_model,
            &self.compatibility.clone().into_compatibility(),
        )
    }

    pub(super) fn effective_capabilities(
        &self,
        provider: Option<&ProviderConfig>,
    ) -> ProviderCapabilities {
        self.capabilities
            .resolve(self.capability_defaults(provider))
    }

    pub(super) fn validate(
        &self,
        model_id: &str,
        max_reasoning_effort: Option<ReasoningEffort>,
        provider: Option<&ProviderConfig>,
    ) -> anyhow::Result<()> {
        if let Some(id) = &self.id
            && !is_valid_route_id(id)
        {
            bail!(
                "model `{model_id}` route id must contain 1 to 128 ASCII letters, digits, underscores, periods, or hyphens"
            );
        }
        if let Some(context_window_tokens) = self.context_window_tokens
            && context_window_tokens <= 0
        {
            bail!(
                "model `{model_id}` route `{}` context_window_tokens must be positive",
                self.upstream_model
            );
        }
        if let Some(pricing_override) = &self.pricing_override {
            pricing_override.resolve(&format!(
                "model `{model_id}` route `{}` pricing_override",
                self.upstream_model
            ))?;
        }
        enforce_reasoning_effort_map(&self.extra_body, max_reasoning_effort).with_context(
            || {
                format!(
                    "model `{model_id}` route `{}` extra_body violates max_reasoning_effort",
                    self.upstream_model
                )
            },
        )?;

        if provider.is_some_and(|provider| matches!(provider, ProviderConfig::GcpVertex(_))) {
            validate_vertex_upstream_model_format(&self.upstream_model)?;
        }
        if let Some(openrouter) = &self.compatibility.openrouter {
            validate_openrouter_route_compatibility(model_id, self, provider, openrouter)?;
        }
        if self.compatibility.github_copilot.is_some()
            && !provider
                .is_some_and(|provider| matches!(provider, ProviderConfig::GitHubCopilot(_)))
        {
            bail!(
                "model `{model_id}` route for provider `{}` uses compatibility.github_copilot but requires a github_copilot provider",
                self.provider
            );
        }
        let capabilities = self.effective_capabilities(provider);
        if let Some(ProviderConfig::AwsBedrock(provider)) = provider {
            validate_aws_bedrock_route_compatibility(model_id, self, provider, capabilities)?;
        }
        validate_decisions_route_capability(model_id, self, provider, capabilities)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutePricingOverrideConfig {
    #[serde(deserialize_with = "deserialize_yaml_string")]
    pub input_usd_per_million_tokens: String,
    #[serde(deserialize_with = "deserialize_yaml_string")]
    pub output_usd_per_million_tokens: String,
    #[serde(default, deserialize_with = "deserialize_optional_yaml_string")]
    pub cache_read_usd_per_million_tokens: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_yaml_string")]
    pub cache_write_usd_per_million_tokens: Option<String>,
}

fn deserialize_yaml_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    match serde_yaml::Value::deserialize(deserializer)? {
        serde_yaml::Value::String(value) => Ok(value),
        _ => Err(de::Error::custom("expected a quoted decimal string")),
    }
}

fn deserialize_optional_yaml_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<serde_yaml::Value>::deserialize(deserializer)? {
        None | Some(serde_yaml::Value::Null) => Ok(None),
        Some(serde_yaml::Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(de::Error::custom("expected a quoted decimal string")),
    }
}

impl RoutePricingOverrideConfig {
    pub(super) fn resolve(&self, label: &str) -> anyhow::Result<RoutePricingOverride> {
        Ok(RoutePricingOverride {
            input_cost_per_million_tokens: parse_non_negative_rate(
                &self.input_usd_per_million_tokens,
                &format!("{label}.input_usd_per_million_tokens"),
            )?,
            output_cost_per_million_tokens: parse_non_negative_rate(
                &self.output_usd_per_million_tokens,
                &format!("{label}.output_usd_per_million_tokens"),
            )?,
            cache_read_cost_per_million_tokens: self
                .cache_read_usd_per_million_tokens
                .as_deref()
                .map(|value| {
                    parse_non_negative_rate(
                        value,
                        &format!("{label}.cache_read_usd_per_million_tokens"),
                    )
                })
                .transpose()?,
            cache_write_cost_per_million_tokens: self
                .cache_write_usd_per_million_tokens
                .as_deref()
                .map(|value| {
                    parse_non_negative_rate(
                        value,
                        &format!("{label}.cache_write_usd_per_million_tokens"),
                    )
                })
                .transpose()?,
        })
    }
}

fn parse_non_negative_rate(value: &str, label: &str) -> anyhow::Result<Money4> {
    let rate = Money4::from_decimal_str(value)
        .map_err(|error| anyhow::anyhow!("{label} is invalid: {error}"))?;
    if rate.is_negative() {
        bail!("{label} cannot be negative");
    }
    Ok(rate)
}

/// Explicit route capability flags. An omitted flag takes the value implied by the route's
/// provider and compatibility profile (see [`ModelRouteConfig::capability_defaults`]).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteCapabilitiesConfig {
    #[serde(default)]
    pub chat_completions: Option<bool>,
    #[serde(default)]
    pub responses: Option<bool>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub embeddings: Option<bool>,
    #[serde(default)]
    pub decisions: Option<bool>,
    #[serde(default)]
    pub tools: Option<bool>,
    #[serde(default)]
    pub vision: Option<bool>,
    #[serde(default)]
    pub json_schema: Option<bool>,
    #[serde(default)]
    pub developer_role: Option<bool>,
}

impl RouteCapabilitiesConfig {
    fn resolve(&self, defaults: ProviderCapabilities) -> ProviderCapabilities {
        ProviderCapabilities {
            chat_completions: self.chat_completions.unwrap_or(defaults.chat_completions),
            responses: self.responses.unwrap_or(defaults.responses),
            stream: self.stream.unwrap_or(defaults.stream),
            embeddings: self.embeddings.unwrap_or(defaults.embeddings),
            decisions: self.decisions.unwrap_or(defaults.decisions),
            tools: self.tools.unwrap_or(defaults.tools),
            vision: self.vision.unwrap_or(defaults.vision),
            json_schema: self.json_schema.unwrap_or(defaults.json_schema),
            developer_role: self.developer_role.unwrap_or(defaults.developer_role),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteCompatibilityConfig {
    #[serde(default)]
    pub openai_compat: Option<OpenAiCompatRouteCompatibilityConfig>,
    #[serde(default)]
    pub openrouter: Option<OpenRouterRouteCompatibility>,
    #[serde(default)]
    pub aws_bedrock: Option<AwsBedrockRouteCompatibilityConfig>,
    #[serde(default)]
    pub github_copilot: Option<GitHubCopilotRouteCompatibility>,
}

impl RouteCompatibilityConfig {
    pub(super) fn into_compatibility(self) -> RouteCompatibility {
        RouteCompatibility {
            openai_compat: self
                .openai_compat
                .map(OpenAiCompatRouteCompatibilityConfig::into_compatibility),
            openrouter: self.openrouter,
            aws_bedrock: self
                .aws_bedrock
                .map(AwsBedrockRouteCompatibilityConfig::into_compatibility),
            github_copilot: self.github_copilot,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwsBedrockRouteCompatibilityConfig {
    pub api_style: AwsBedrockApiStyle,
    #[serde(default)]
    pub openai_base_path: Option<String>,
    #[serde(default)]
    pub supports_strict_tools: Option<bool>,
}

impl AwsBedrockRouteCompatibilityConfig {
    pub(super) fn into_compatibility(self) -> AwsBedrockRouteCompatibility {
        AwsBedrockRouteCompatibility {
            api_style: self.api_style,
            openai_base_path: self.resolved_openai_base_path(),
            supports_strict_tools: self.supports_strict_tools,
        }
    }

    /// The configured OpenAI base path, or the AWS-documented default for the api_style:
    /// `/openai/v1` on `bedrock-runtime` and `/v1` on `bedrock-mantle`. Some Mantle models are
    /// served under `/openai/v1` instead and must set it explicitly.
    fn resolved_openai_base_path(&self) -> Option<String> {
        if let Some(path) = self
            .openai_base_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            return Some(path.to_string());
        }
        match self.api_style {
            AwsBedrockApiStyle::RuntimeOpenaiChat => Some("/openai/v1".to_string()),
            AwsBedrockApiStyle::MantleOpenaiResponses | AwsBedrockApiStyle::MantleOpenaiChat => {
                Some("/v1".to_string())
            }
            AwsBedrockApiStyle::RuntimeConverse
            | AwsBedrockApiStyle::RuntimeAnthropicInvoke
            | AwsBedrockApiStyle::MantleAnthropicMessages => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAiCompatRouteCompatibilityConfig {
    #[serde(default = "default_enabled")]
    pub supports_store: bool,
    #[serde(default)]
    pub max_tokens_field: OpenAiCompatMaxTokensField,
    #[serde(default)]
    pub developer_role: OpenAiCompatDeveloperRole,
    #[serde(default)]
    pub reasoning_effort: OpenAiCompatReasoningEffort,
    #[serde(default)]
    pub supports_stream_usage: bool,
    #[serde(default)]
    pub empty_tools: OpenAiCompatEmptyTools,
    #[serde(default)]
    pub decisions_api: Option<OpenAiCompatDecisionsApi>,
}

impl OpenAiCompatRouteCompatibilityConfig {
    pub(super) fn into_compatibility(self) -> OpenAiCompatRouteCompatibility {
        OpenAiCompatRouteCompatibility {
            supports_store: self.supports_store,
            max_tokens_field: self.max_tokens_field,
            developer_role: self.developer_role,
            reasoning_effort: self.reasoning_effort,
            supports_stream_usage: self.supports_stream_usage,
            empty_tools: self.empty_tools,
            decisions_api: self.decisions_api,
        }
    }
}

fn validate_vertex_upstream_model_format(value: &str) -> anyhow::Result<()> {
    let mut parts = value.splitn(2, '/');
    let publisher = parts.next().unwrap_or_default();
    let model_id = parts.next().unwrap_or_default();
    if publisher.is_empty() || model_id.is_empty() {
        bail!(
            "gcp_vertex routes require upstream_model in <publisher>/<model_id> format, got `{value}`"
        );
    }
    Ok(())
}

fn validate_openrouter_route_compatibility(
    model_id: &str,
    route: &ModelRouteConfig,
    provider: Option<&ProviderConfig>,
    compatibility: &OpenRouterRouteCompatibility,
) -> anyhow::Result<()> {
    let Some(ProviderConfig::OpenAiCompat(provider)) = provider else {
        bail!(
            "model `{model_id}` route for provider `{}` uses compatibility.openrouter but OpenRouter routing policy requires an openai_compat provider",
            route.provider
        );
    };

    if !is_openrouter_endpoint(&provider.base_url) {
        bail!(
            "model `{model_id}` route for openai_compat provider `{}` uses compatibility.openrouter but provider base_url is not an OpenRouter endpoint",
            provider.id
        );
    }

    if route.extra_body.contains_key("provider") {
        bail!(
            "model `{model_id}` route for OpenRouter provider `{}` cannot set both compatibility.openrouter.provider and extra_body.provider",
            provider.id
        );
    }

    validate_openrouter_provider_routing(
        &compatibility.provider,
        &format!(
            "model `{model_id}` route for OpenRouter provider `{}` compatibility.openrouter.provider",
            provider.id
        ),
        compatibility.api.is_decisions(),
    )
}

fn validate_decisions_route_capability(
    model_id: &str,
    route: &ModelRouteConfig,
    provider: Option<&ProviderConfig>,
    capabilities: ProviderCapabilities,
) -> anyhow::Result<()> {
    let openrouter_decisions = route
        .compatibility
        .openrouter
        .as_ref()
        .is_some_and(|openrouter| openrouter.api.is_decisions());
    let openai_decisions = route
        .compatibility
        .openai_compat
        .as_ref()
        .is_some_and(|compat| compat.decisions_api.is_some());
    let is_typesafe = matches!(provider, Some(ProviderConfig::TypeSafe(_)));

    if openai_decisions {
        if !matches!(provider, Some(ProviderConfig::OpenAiCompat(_))) {
            bail!(
                "model `{model_id}` route for provider `{}` uses compatibility.openai_compat.decisions_api but requires an openai_compat provider",
                route.provider
            );
        }
        if route.compatibility.openrouter.is_some() {
            bail!(
                "model `{model_id}` route for provider `{}` cannot set both compatibility.openai_compat.decisions_api and compatibility.openrouter",
                route.provider
            );
        }
        if !capabilities.decisions {
            bail!(
                "model `{model_id}` route for provider `{}` uses compatibility.openai_compat.decisions_api but capabilities.decisions is not enabled",
                route.provider
            );
        }
    }
    if openrouter_decisions && !capabilities.decisions {
        bail!(
            "model `{model_id}` route for provider `{}` uses compatibility.openrouter.api `decisions` but capabilities.decisions is not enabled",
            route.provider
        );
    }
    if is_typesafe && !capabilities.decisions {
        bail!(
            "model `{model_id}` route for typesafe provider `{}` must enable capabilities.decisions",
            route.provider
        );
    }
    if !capabilities.decisions {
        return Ok(());
    }
    if !is_typesafe && !openrouter_decisions && !openai_decisions {
        bail!(
            "model `{model_id}` route for provider `{}` enables capabilities.decisions but decisions requires a typesafe provider, compatibility.openai_compat.decisions_api, or compatibility.openrouter.api `decisions`",
            route.provider
        );
    }
    for (capability, enabled) in [
        ("chat_completions", capabilities.chat_completions),
        ("responses", capabilities.responses),
        ("embeddings", capabilities.embeddings),
        ("stream", capabilities.stream),
        ("tools", capabilities.tools),
        ("vision", capabilities.vision),
        ("json_schema", capabilities.json_schema),
        ("developer_role", capabilities.developer_role),
    ] {
        if enabled {
            bail!(
                "model `{model_id}` route for provider `{}` enables capabilities.decisions and `{capability}`; decisions routes must disable all other capabilities",
                route.provider
            );
        }
    }
    Ok(())
}

fn is_openrouter_endpoint(base_url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(base_url.trim()) else {
        return false;
    };
    parsed.scheme() == "https" && parsed.host_str() == Some("openrouter.ai")
}

fn validate_openrouter_provider_routing(
    routing: &OpenRouterProviderRouting,
    label: &str,
    allow_empty: bool,
) -> anyhow::Result<()> {
    validate_non_empty_strings(&routing.only, &format!("{label}.only"))?;
    validate_non_empty_strings(&routing.ignore, &format!("{label}.ignore"))?;
    validate_non_empty_strings(&routing.order, &format!("{label}.order"))?;
    validate_unique_strings(&routing.only, &format!("{label}.only"))?;
    validate_unique_strings(&routing.ignore, &format!("{label}.ignore"))?;
    validate_unique_strings(&routing.order, &format!("{label}.order"))?;
    validate_openrouter_only_ignore_overlap(&routing.only, &routing.ignore, label)?;

    if let Some(preference) = &routing.preferred_max_latency {
        validate_openrouter_percentile_preference(
            preference,
            &format!("{label}.preferred_max_latency"),
        )?;
    }

    if let Some(max_price) = &routing.max_price {
        validate_openrouter_max_price(max_price, &format!("{label}.max_price"))?;
    }

    if !allow_empty && routing.is_empty() {
        bail!("{label} must set at least one routing policy field");
    }

    Ok(())
}
fn validate_openrouter_percentile_preference(
    preference: &OpenRouterPercentilePreference,
    label: &str,
) -> anyhow::Result<()> {
    match preference {
        OpenRouterPercentilePreference::Number(value) => validate_positive_f64(*value, label),
        OpenRouterPercentilePreference::Percentiles(percentiles) => {
            validate_openrouter_percentile_cutoffs(percentiles, label)
        }
    }
}

fn validate_openrouter_percentile_cutoffs(
    percentiles: &OpenRouterPercentileCutoffs,
    label: &str,
) -> anyhow::Result<()> {
    let values = [
        ("p50", percentiles.p50),
        ("p75", percentiles.p75),
        ("p90", percentiles.p90),
        ("p99", percentiles.p99),
    ];
    if values.iter().all(|(_, value)| value.is_none()) {
        bail!("{label} percentile object must set at least one of p50, p75, p90, or p99");
    }
    for (name, value) in values {
        if let Some(value) = value {
            validate_positive_f64(value, &format!("{label}.{name}"))?;
        }
    }
    Ok(())
}

fn validate_openrouter_max_price(
    max_price: &OpenRouterMaxPrice,
    label: &str,
) -> anyhow::Result<()> {
    let values = [
        ("prompt", max_price.prompt),
        ("completion", max_price.completion),
        ("request", max_price.request),
        ("image", max_price.image),
    ];
    if values.iter().all(|(_, value)| value.is_none()) {
        bail!("{label} must set at least one of prompt, completion, request, or image");
    }
    for (name, value) in values {
        if let Some(value) = value {
            validate_non_negative_f64(value, &format!("{label}.{name}"))?;
        }
    }
    Ok(())
}

fn validate_non_empty_strings(values: &[String], label: &str) -> anyhow::Result<()> {
    if values.iter().any(|value| value.trim().is_empty()) {
        bail!("{label} entries cannot be empty");
    }
    Ok(())
}

fn validate_unique_strings(values: &[String], label: &str) -> anyhow::Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for value in values {
        if !seen.insert(value.trim()) {
            bail!(
                "{label} entries must be unique; duplicate `{}`",
                value.trim()
            );
        }
    }
    Ok(())
}

fn validate_openrouter_only_ignore_overlap(
    only: &[String],
    ignore: &[String],
    label: &str,
) -> anyhow::Result<()> {
    let only_values = only
        .iter()
        .map(|value| value.trim())
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(overlap) = ignore
        .iter()
        .map(|value| value.trim())
        .find(|value| only_values.contains(value))
    {
        bail!("{label}.only and {label}.ignore cannot both include `{overlap}`");
    }
    Ok(())
}

fn validate_positive_f64(value: f64, label: &str) -> anyhow::Result<()> {
    if !value.is_finite() || value <= 0.0 {
        bail!("{label} must be a positive finite number");
    }
    Ok(())
}

fn validate_non_negative_f64(value: f64, label: &str) -> anyhow::Result<()> {
    if !value.is_finite() || value < 0.0 {
        bail!("{label} must be a non-negative finite number");
    }
    Ok(())
}
fn validate_aws_bedrock_route_compatibility(
    model_id: &str,
    route: &ModelRouteConfig,
    provider: &AwsBedrockProviderConfig,
    capabilities: ProviderCapabilities,
) -> anyhow::Result<()> {
    let Some(compatibility) = route.compatibility.aws_bedrock.as_ref() else {
        bail!(
            "model `{model_id}` route for aws_bedrock provider `{}` requires compatibility.aws_bedrock.api_style",
            provider.id
        );
    };

    let endpoint_matches = match provider.endpoint_kind {
        BedrockEndpointKind::BedrockRuntime => compatibility.api_style.is_runtime(),
        BedrockEndpointKind::BedrockMantle => compatibility.api_style.is_mantle(),
    };
    if !endpoint_matches {
        bail!(
            "model `{model_id}` route for aws_bedrock provider `{}` uses api_style `{:?}` incompatible with endpoint_kind `{}`",
            provider.id,
            compatibility.api_style,
            provider.endpoint_kind.as_config_value()
        );
    }

    if !compatibility.api_style.is_openai_shaped()
        && compatibility
            .openai_base_path
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        bail!(
            "model `{model_id}` route for aws_bedrock provider `{}` api_style `{:?}` cannot set compatibility.aws_bedrock.openai_base_path",
            provider.id,
            compatibility.api_style
        );
    }

    if compatibility.supports_strict_tools.is_some()
        && compatibility.api_style != AwsBedrockApiStyle::RuntimeConverse
    {
        bail!(
            "model `{model_id}` route for aws_bedrock provider `{}` api_style `{:?}` cannot set compatibility.aws_bedrock.supports_strict_tools; the override applies only to `runtime_converse`",
            provider.id,
            compatibility.api_style
        );
    }

    if capabilities.responses
        && compatibility.api_style != AwsBedrockApiStyle::MantleOpenaiResponses
    {
        bail!(
            "model `{model_id}` route for aws_bedrock provider `{}` api_style `{:?}` cannot enable responses capability; responses require api_style `mantle_openai_responses`",
            provider.id,
            compatibility.api_style
        );
    }

    if capabilities.json_schema
        && compatibility.api_style != AwsBedrockApiStyle::MantleOpenaiResponses
    {
        bail!(
            "model `{model_id}` route for aws_bedrock provider `{}` api_style `{:?}` cannot enable json_schema capability; json_schema requires api_style `mantle_openai_responses`",
            provider.id,
            compatibility.api_style
        );
    }

    if compatibility.api_style == AwsBedrockApiStyle::MantleOpenaiResponses
        && capabilities.chat_completions
    {
        bail!(
            "model `{model_id}` route for aws_bedrock provider `{}` api_style `mantle_openai_responses` cannot enable chat_completions capability",
            provider.id
        );
    }

    if compatibility.api_style == AwsBedrockApiStyle::RuntimeAnthropicInvoke && capabilities.stream
    {
        bail!(
            "model `{model_id}` route for aws_bedrock provider `{}` api_style `runtime_anthropic_invoke` cannot enable stream capability",
            provider.id
        );
    }

    Ok(())
}
