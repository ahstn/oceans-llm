use crate::types::ThinkingPolicy;

const SAFE_EFFORT_MODEL_MARKERS: &[&str] = &[
    "claude-mythos-preview",
    "claude-opus-4-6",
    "claude-opus-4-7",
    "claude-opus-4-8",
    "claude-opus-4-9",
    "claude-opus-5",
    "claude-opus-6",
    "claude-sonnet-4-6",
];

const SAFE_EFFORT_EXACT_MODEL_MARKERS: &[&str] = &["claude-fable-5", "claude-sonnet-5"];

/// Infers the thinking policy from candidate identifiers. Callers pass the upstream model
/// first, then aliases and provider metadata. Specific model-family markers (a Claude model
/// that supports effort, a Gemini generation) are resolved across every candidate before the
/// generic `anthropic` / `claude` marker is consulted, so a provider key like
/// `vertex-anthropic-prod` or `anthropic_compat` cannot hide a more specific model alias.
#[must_use]
pub fn infer_thinking_policy(
    values: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<ThinkingPolicy> {
    let values: Vec<String> = values
        .into_iter()
        .map(|value| value.as_ref().to_ascii_lowercase())
        .collect();
    values
        .iter()
        .find_map(|value| classify_model_family(value))
        .or_else(|| {
            values
                .iter()
                .any(|value| value.contains("anthropic") || value.contains("claude"))
                .then_some(ThinkingPolicy::AnthropicManualBudget)
        })
}

fn classify_model_family(value: &str) -> Option<ThinkingPolicy> {
    if SAFE_EFFORT_MODEL_MARKERS
        .iter()
        .any(|marker| value.contains(marker))
        || SAFE_EFFORT_EXACT_MODEL_MARKERS
            .iter()
            .any(|marker| contains_exact_model_marker(value, marker))
    {
        return Some(ThinkingPolicy::AnthropicSafeEffort);
    }
    if let Some(token) = gemini_3_token(value) {
        let is_pro = token.split('-').any(|segment| segment == "pro");
        // `MINIMAL` exists on Flash / Flash-Lite up to 3.6; Pro and 3.7+ start at `LOW`.
        let minor: u32 = token
            .split('-')
            .next()
            .and_then(|version| version.split_once('.'))
            .and_then(|(_, minor)| minor.parse().ok())
            .unwrap_or(0);
        return Some(ThinkingPolicy::GeminiLevel {
            supports_minimal: !is_pro && minor < 7,
            supports_medium: !is_pro,
        });
    }
    if value.contains("gemini-2.5") {
        return Some(ThinkingPolicy::GeminiBudget);
    }
    None
}

/// The model token following `gemini-` for a Gemini 3 id, e.g. `3.1-pro-preview` from
/// `google/gemini-3.1-pro-preview@001`. Tier segments are hyphen-delimited, so
/// `3-production` is not Pro.
fn gemini_3_token(value: &str) -> Option<&str> {
    value.split("gemini-").skip(1).find_map(|rest| {
        rest.split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '/' | ':' | '@'))
            .next()
            .filter(|token| token.split(['.', '-']).next() == Some("3"))
    })
}

fn contains_exact_model_marker(value: &str, marker: &str) -> bool {
    value.split(marker).skip(1).any(|rest| {
        rest.chars().next().is_none_or(|ch| {
            ch.is_ascii_whitespace() || matches!(ch, '/' | ':' | '@' | ',' | ')' | ']' | '-')
        })
    })
}
