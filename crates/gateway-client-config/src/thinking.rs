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

#[must_use]
pub fn infer_thinking_policy(
    values: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<ThinkingPolicy> {
    let joined = values
        .into_iter()
        .map(|value| value.as_ref().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    if SAFE_EFFORT_MODEL_MARKERS
        .iter()
        .any(|marker| joined.contains(marker))
        || SAFE_EFFORT_EXACT_MODEL_MARKERS
            .iter()
            .any(|marker| contains_exact_model_marker(&joined, marker))
    {
        return Some(ThinkingPolicy::AnthropicSafeEffort);
    }

    if joined.contains("anthropic") || joined.contains("claude") {
        return Some(ThinkingPolicy::AnthropicManualBudget);
    }

    if joined.contains("gemini-3") {
        let is_pro = is_gemini_3_pro(&joined);
        return Some(ThinkingPolicy::GeminiLevel {
            supports_minimal: !is_pro,
            supports_medium: !is_pro,
        });
    }

    if joined.contains("gemini-2.5") {
        return Some(ThinkingPolicy::GeminiBudget);
    }

    None
}

/// Inspects only the `gemini-3…` model token so provider labels such as
/// `vertex-prod` cannot masquerade as a Pro model.
fn is_gemini_3_pro(value: &str) -> bool {
    value.split("gemini-3").skip(1).any(|rest| {
        rest.split(|ch: char| ch.is_ascii_whitespace())
            .next()
            .is_some_and(|token| token.contains("-pro"))
    })
}

fn contains_exact_model_marker(value: &str, marker: &str) -> bool {
    value.split(marker).skip(1).any(|rest| {
        rest.chars().next().is_none_or(|ch| {
            ch.is_ascii_whitespace() || matches!(ch, '/' | ':' | '@' | ',' | ')' | ']' | '-')
        })
    })
}
