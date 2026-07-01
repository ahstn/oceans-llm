use crate::types::AnthropicThinkingPolicy;

#[must_use]
pub fn infer_anthropic_thinking_policy(
    values: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<AnthropicThinkingPolicy> {
    let joined = values
        .into_iter()
        .map(|value| value.as_ref().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    if joined.contains("claude-mythos-preview")
        || joined.contains("claude-opus-4-7")
        || joined.contains("claude-opus-4-8")
        || joined.contains("claude-opus-4-9")
        || joined.contains("claude-opus-5")
        || joined.contains("claude-opus-6")
        || joined.contains("claude-sonnet-5")
        || joined.contains("claude-sonnet-4-6")
        || joined.contains("claude-fable-5")
        || joined.contains("claude-opus-4-6")
    {
        return Some(AnthropicThinkingPolicy::SafeEffort);
    }

    if joined.contains("anthropic") || joined.contains("claude") {
        return Some(AnthropicThinkingPolicy::ManualBudget);
    }

    None
}
