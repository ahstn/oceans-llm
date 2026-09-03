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

/// Infers the thinking policy from candidate identifiers, most specific first: callers pass
/// the upstream model, then gateway model aliases, then provider metadata. The first candidate
/// naming a model family (a Claude model that supports effort, a Gemini generation) wins;
/// the generic `anthropic` / `claude` marker is consulted only after every candidate, so a
/// provider key like `vertex-anthropic-prod` or `anthropic_compat` cannot hide a more specific
/// model alias.
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
    let (major, minor, is_pro) = gemini_generation(value)?;
    // Mirrors the Vertex adapter's `GeminiModel`: 3.x and later take `thinkingLevel`, 2.5 takes
    // `thinkingBudget`, older models have no thinking. `MINIMAL` exists on Flash / Flash-Lite up
    // to 3.6; Pro and 3.7+ start at `LOW`, and Pro offers no `MEDIUM`.
    if major >= 3 {
        Some(ThinkingPolicy::GeminiLevel {
            supports_minimal: !is_pro && (major, minor) < (3, 7),
            supports_medium: !is_pro,
        })
    } else if (major, minor) == (2, 5) {
        Some(ThinkingPolicy::GeminiBudget)
    } else {
        None
    }
}

/// `(major, minor, is_pro)` of the first `gemini-<major>[.<minor>][-<tier>...]` id in `value`,
/// e.g. `(3, 1, true)` from `google/gemini-3.1-pro-preview@001`. Tier segments are
/// hyphen-delimited, so `gemini-3-production` is not Pro.
fn gemini_generation(value: &str) -> Option<(u32, u32, bool)> {
    value.split("gemini-").skip(1).find_map(|rest| {
        let token = rest
            .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '/' | ':' | '@'))
            .next()?;
        let mut segments = token.split('-');
        let version = segments.next()?;
        let (major, minor) = version.split_once('.').unwrap_or((version, "0"));
        let major = major.parse().ok()?;
        let minor = minor.parse().ok()?;
        let is_pro = segments.any(|segment| segment == "pro");
        Some((major, minor, is_pro))
    })
}

fn contains_exact_model_marker(value: &str, marker: &str) -> bool {
    value.split(marker).skip(1).any(|rest| {
        rest.chars().next().is_none_or(|ch| {
            ch.is_ascii_whitespace() || matches!(ch, '/' | ':' | '@' | ',' | ')' | ']' | '-')
        })
    })
}
