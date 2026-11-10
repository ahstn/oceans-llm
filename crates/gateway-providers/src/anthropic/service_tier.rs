//! Claude API speed selection on Messages bodies.

use gateway_core::{
    ProviderError, ProviderRequestContext, ServiceTier,
    claude_models::{contains_exact_claude_model_marker, normalize_claude_model},
};
use serde_json::{Map, Value};

use super::beta::{normalized_body_betas, put_body_betas};
use crate::service_tier::{DEFAULT_PRIORITY, checked_service_tier};

/// Research-preview beta the Claude API requires before Opus 5 and Opus 4.8 accept fast mode.
const FAST_MODE_BETA: &str = "fast-mode-2026-02-01";

/// Writes the effective tier the way the Claude API takes it. Priority is fast mode,
/// `speed: "fast"`. An explicit default pins standard capacity with
/// `service_tier: "standard_only"`, so an organization's Priority Tier commitment never
/// serves it.
pub(crate) fn apply_claude_api_service_tier(
    body: &mut Value,
    context: &ProviderRequestContext,
) -> Result<(), ProviderError> {
    let (Some(tier), Some(object)) = (
        checked_service_tier(context, DEFAULT_PRIORITY)?,
        body.as_object_mut(),
    ) else {
        return Ok(());
    };
    if tier == ServiceTier::Default {
        object.insert("service_tier".to_string(), Value::from("standard_only"));
        return Ok(());
    }
    object.insert("speed".to_string(), Value::from("fast"));
    if needs_fast_mode_beta(&context.upstream_model) {
        add_body_beta(object, FAST_MODE_BETA)?;
    }
    Ok(())
}

/// Opus 5 (not a later Opus 5 minor version) and Opus 4.8 serve fast mode only as a
/// research preview behind [`FAST_MODE_BETA`].
fn needs_fast_mode_beta(upstream_model: &str) -> bool {
    let model = normalize_claude_model(upstream_model);
    contains_exact_claude_model_marker(&model, "claude-opus-4-8") || is_claude_opus_5_0(&model)
}

/// `claude-opus-5` followed by a separator, a date stamp, or a deployment version, but not a
/// one- or two-digit minor version such as `claude-opus-5-5`.
fn is_claude_opus_5_0(model: &str) -> bool {
    model
        .split("claude-opus-5")
        .skip(1)
        .any(|rest| match rest.strip_prefix('-') {
            Some(suffix) => {
                let digits = suffix.chars().take_while(char::is_ascii_digit).count();
                !(1..=2).contains(&digits)
            }
            None => rest.chars().next().is_none_or(|ch| {
                ch.is_ascii_whitespace() || matches!(ch, '/' | ':' | '@' | ',' | ')' | ']')
            }),
        })
}

fn add_body_beta(body: &mut Map<String, Value>, beta: &str) -> Result<(), ProviderError> {
    let mut betas = normalized_body_betas(body.get("anthropic_beta"))?;
    if !betas.iter().any(|existing| existing == beta) {
        betas.push(beta.to_string());
    }
    body.remove("anthropic_beta");
    put_body_betas(body, betas);
    Ok(())
}
