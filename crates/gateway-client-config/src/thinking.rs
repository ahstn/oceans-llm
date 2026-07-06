use crate::types::AnthropicThinkingPolicy;

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
pub fn infer_anthropic_thinking_policy(
    values: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<AnthropicThinkingPolicy> {
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
        return Some(AnthropicThinkingPolicy::SafeEffort);
    }

    if joined.contains("anthropic") || joined.contains("claude") {
        return Some(AnthropicThinkingPolicy::ManualBudget);
    }

    None
}

fn contains_exact_model_marker(value: &str, marker: &str) -> bool {
    value.split(marker).skip(1).any(|rest| {
        rest.chars().next().is_none_or(|ch| {
            ch.is_ascii_whitespace() || matches!(ch, '/' | ':' | '@' | ',' | ')' | ']')
        })
    })
}
