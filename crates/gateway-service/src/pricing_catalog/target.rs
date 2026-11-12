use gateway_core::{ModelRoute, PricingUnpricedReason, ProviderConnection};
use serde_json::Value;

pub(super) const AMAZON_BEDROCK_PRICING_PROVIDER_ID: &str = "amazon-bedrock";
pub(super) const GITHUB_COPILOT_PRICING_PROVIDER_ID: &str = "github-copilot";
pub(super) const GOOGLE_VERTEX_PRICING_PROVIDER_ID: &str = "google-vertex";
pub(super) const GOOGLE_VERTEX_ANTHROPIC_PRICING_PROVIDER_ID: &str = "google-vertex-anthropic";
pub(super) const OPENAI_PRICING_PROVIDER_ID: &str = "openai";
pub(super) const OPENROUTER_PRICING_PROVIDER_ID: &str = "openrouter";
const OPENCODE_PRICING_PROVIDER_ID: &str = "opencode";
const BEDROCK_GPT_OSS_120B_PRICING_MODEL_ID: &str = "openai.gpt-oss-120b-1:0";
const BEDROCK_GPT_OSS_20B_PRICING_MODEL_ID: &str = "openai.gpt-oss-20b-1:0";

pub const SUPPORTED_PRICING_PROVIDER_IDS: [&str; 7] = [
    AMAZON_BEDROCK_PRICING_PROVIDER_ID,
    GITHUB_COPILOT_PRICING_PROVIDER_ID,
    GOOGLE_VERTEX_PRICING_PROVIDER_ID,
    GOOGLE_VERTEX_ANTHROPIC_PRICING_PROVIDER_ID,
    OPENAI_PRICING_PROVIDER_ID,
    OPENROUTER_PRICING_PROVIDER_ID,
    OPENCODE_PRICING_PROVIDER_ID,
];

#[derive(Debug, Clone)]
pub(super) enum PricingTarget {
    Exact {
        pricing_provider_id: String,
        model_id: String,
    },
    Unpriced(PricingUnpricedReason),
}

#[must_use]
pub fn is_supported_pricing_provider_id(value: &str) -> bool {
    SUPPORTED_PRICING_PROVIDER_IDS.contains(&value)
}

pub(super) fn pricing_target_for_route(
    provider: &ProviderConnection,
    route: &ModelRoute,
) -> PricingTarget {
    let target = catalog_identity_for_route(provider, route);
    if let Some(reason) = unsupported_vertex_location(provider, &target) {
        return PricingTarget::Unpriced(reason);
    }
    target
}

/// Whether the provider bills each request itself and reports the cost in its usage
/// object (OpenRouter's `usage.cost`), so no catalog rate is needed at any service tier.
pub(super) fn route_reports_request_cost(
    provider: &ProviderConnection,
    route: &ModelRoute,
) -> bool {
    matches!(
        catalog_identity_for_route(provider, route),
        PricingTarget::Exact { pricing_provider_id, .. }
            if pricing_provider_id == OPENROUTER_PRICING_PROVIDER_ID
    )
}

pub(crate) fn catalog_metadata_target_for_route(
    provider: &ProviderConnection,
    route: &ModelRoute,
) -> Option<(String, String)> {
    match catalog_identity_for_route(provider, route) {
        PricingTarget::Exact {
            pricing_provider_id,
            model_id,
        } => Some((pricing_provider_id, model_id)),
        PricingTarget::Unpriced(_) => None,
    }
}

/// Whether the catalog prices `pricing_provider_id` in `provider`'s location: Vertex
/// Anthropic is priced only in the global location.
pub(crate) fn catalog_pricing_location_supported(
    provider: &ProviderConnection,
    pricing_provider_id: &str,
) -> bool {
    unsupported_vertex_location_for_pricing_provider_id(provider, pricing_provider_id).is_none()
}

fn catalog_identity_for_route(provider: &ProviderConnection, route: &ModelRoute) -> PricingTarget {
    match provider.provider_type.as_str() {
        "openai_compat"
        | "gcp_cloud_run_openai_compat"
        | "github_copilot"
        | "anthropic_compat"
        | "typesafe" => openai_compatible_pricing_target(provider, route),
        "gcp_vertex" => vertex_catalog_target(route),
        "aws_bedrock" => PricingTarget::Exact {
            pricing_provider_id: AMAZON_BEDROCK_PRICING_PROVIDER_ID.to_string(),
            model_id: normalize_bedrock_pricing_model_id(&route.upstream_model),
        },
        other => PricingTarget::Unpriced(PricingUnpricedReason::UnsupportedPricingProviderId(
            other.to_string(),
        )),
    }
}

fn openai_compatible_pricing_target(
    provider: &ProviderConnection,
    route: &ModelRoute,
) -> PricingTarget {
    let Some(pricing_provider_id) = provider
        .config
        .get("pricing_provider_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    else {
        return PricingTarget::Unpriced(PricingUnpricedReason::ProviderPricingSourceMissing);
    };

    if !is_supported_pricing_provider_id(&pricing_provider_id) {
        return PricingTarget::Unpriced(PricingUnpricedReason::UnsupportedPricingProviderId(
            pricing_provider_id,
        ));
    }

    PricingTarget::Exact {
        pricing_provider_id,
        model_id: route.upstream_model.clone(),
    }
}

fn vertex_catalog_target(route: &ModelRoute) -> PricingTarget {
    let mut parts = route.upstream_model.splitn(2, '/');
    let publisher = parts.next().unwrap_or_default();
    let model_id = parts.next().unwrap_or_default();
    if publisher.is_empty() || model_id.is_empty() {
        return PricingTarget::Unpriced(PricingUnpricedReason::UnsupportedVertexPublisher(
            route.upstream_model.clone(),
        ));
    }

    let pricing_provider_id = match publisher {
        "google" => GOOGLE_VERTEX_PRICING_PROVIDER_ID,
        "anthropic" => GOOGLE_VERTEX_ANTHROPIC_PRICING_PROVIDER_ID,
        other => {
            return PricingTarget::Unpriced(PricingUnpricedReason::UnsupportedVertexPublisher(
                other.to_string(),
            ));
        }
    };

    PricingTarget::Exact {
        pricing_provider_id: pricing_provider_id.to_string(),
        model_id: model_id.to_string(),
    }
}

/// Second catalog id to try when the exact target id is missing.
///
/// models.dev publishes Vertex Anthropic models under a versioned id
/// (`claude-sonnet-4-6@default`, `claude-haiku-4-5@20251001`), while routes usually
/// name the unversioned model. Lookups try the exact id first, then `<id>@default`,
/// so new families resolve without a hard-coded list.
pub(crate) fn catalog_model_id_fallback(
    pricing_provider_id: &str,
    model_id: &str,
) -> Option<String> {
    (pricing_provider_id == GOOGLE_VERTEX_ANTHROPIC_PRICING_PROVIDER_ID && !model_id.contains('@'))
        .then(|| format!("{model_id}@default"))
}

pub(super) fn normalize_bedrock_pricing_model_id(upstream_model: &str) -> String {
    let model_id = upstream_model
        .strip_prefix("arn:")
        .and_then(|_| upstream_model.rsplit('/').next())
        .unwrap_or(upstream_model);

    match model_id {
        "gpt-oss-120b" => return BEDROCK_GPT_OSS_120B_PRICING_MODEL_ID.to_string(),
        "gpt-oss-20b" => return BEDROCK_GPT_OSS_20B_PRICING_MODEL_ID.to_string(),
        _ => {}
    }

    strip_bedrock_default_version_suffix(model_id)
        .unwrap_or(model_id)
        .to_string()
}

fn strip_bedrock_default_version_suffix(model_id: &str) -> Option<&str> {
    if !(model_id.contains("claude-sonnet-4-6")
        || model_id.contains("claude-opus-4-6")
        || model_id.contains("claude-opus-4-7"))
    {
        return None;
    }

    let (base, version) = model_id.rsplit_once("-v")?;
    if version == "1:0" { Some(base) } else { None }
}

fn unsupported_vertex_location(
    provider: &ProviderConnection,
    target: &PricingTarget,
) -> Option<PricingUnpricedReason> {
    let PricingTarget::Exact {
        pricing_provider_id,
        ..
    } = target
    else {
        return None;
    };
    unsupported_vertex_location_for_pricing_provider_id(provider, pricing_provider_id)
}

fn unsupported_vertex_location_for_pricing_provider_id(
    provider: &ProviderConnection,
    pricing_provider_id: &str,
) -> Option<PricingUnpricedReason> {
    if pricing_provider_id != GOOGLE_VERTEX_ANTHROPIC_PRICING_PROVIDER_ID {
        return None;
    }

    let location = provider
        .config
        .get("location")
        .and_then(Value::as_str)
        .unwrap_or("global");
    (location != "global")
        .then(|| PricingUnpricedReason::UnsupportedVertexLocation(location.to_string()))
}
