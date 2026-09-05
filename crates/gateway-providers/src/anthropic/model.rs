//! Shared Claude model classification; request and beta handling stay in each transport.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeThinkingPolicy {
    AdaptiveOnly,
    AdaptivePreferred,
    ManualWithEffort,
    ManualOnly,
    MythosPreview,
}

#[must_use]
pub fn claude_thinking_policy(upstream_model: &str) -> ClaudeThinkingPolicy {
    let model = upstream_model.to_ascii_lowercase();
    if model.contains("claude-mythos-preview") {
        ClaudeThinkingPolicy::MythosPreview
    } else if is_adaptive_only_claude(&model) {
        ClaudeThinkingPolicy::AdaptiveOnly
    } else if model.contains("claude-opus-4-6") || model.contains("claude-sonnet-4-6") {
        ClaudeThinkingPolicy::AdaptivePreferred
    } else if model.contains("claude-opus-4-5") {
        ClaudeThinkingPolicy::ManualWithEffort
    } else {
        ClaudeThinkingPolicy::ManualOnly
    }
}

pub fn is_adaptive_only_claude(model: &str) -> bool {
    is_opus_4_7_or_later(model)
        || contains_exact_claude_model_marker(model, "claude-fable-5")
        || contains_exact_claude_model_marker(model, "claude-sonnet-5")
}

pub fn contains_exact_claude_model_marker(model: &str, marker: &str) -> bool {
    model.split(marker).skip(1).any(|rest| {
        rest.chars().next().is_none_or(|ch| {
            ch.is_ascii_whitespace() || matches!(ch, '/' | ':' | '@' | ',' | ')' | ']' | '-')
        })
    })
}

fn is_opus_4_7_or_later(model: &str) -> bool {
    let Some(rest) = model.split("claude-opus-4-").nth(1) else {
        return false;
    };
    rest.split(|ch: char| !ch.is_ascii_digit())
        .next()
        .and_then(|minor| minor.parse::<u16>().ok())
        .is_some_and(|minor| minor >= 7)
}

#[cfg(test)]
mod tests {
    use super::{ClaudeThinkingPolicy::*, claude_thinking_policy};

    #[test]
    fn classifies_direct_and_transport_qualified_models() {
        for (model, expected) in [
            ("claude-mythos-preview", MythosPreview),
            ("claude-opus-4-7", AdaptiveOnly),
            ("claude-fable-5-1", AdaptiveOnly),
            ("claude-sonnet-5", AdaptiveOnly),
            ("claude-opus-4-6", AdaptivePreferred),
            ("claude-sonnet-4-6", AdaptivePreferred),
            ("claude-opus-4-5", ManualWithEffort),
            ("claude-sonnet-4-5", ManualOnly),
            ("claude-3-7-sonnet", ManualOnly),
            ("unknown-model", ManualOnly),
        ] {
            for qualified in [
                model.to_string(),
                format!("us.anthropic.{model}-v1:0"),
                format!("anthropic/{model}@20260101"),
                format!("arn:aws:bedrock:us-east-1::foundation-model/anthropic.{model}-v1:0"),
                model.to_ascii_uppercase(),
            ] {
                assert_eq!(claude_thinking_policy(&qualified), expected, "{qualified}");
            }
        }
    }

    #[test]
    fn adaptive_model_markers_require_a_suffix_boundary() {
        for marker in ["claude-fable-5", "claude-sonnet-5"] {
            for suffix in [
                "", "-1", ":0", "@latest", "/alias", ",", ")", "]", " ", "\t",
            ] {
                let model = format!("anthropic.{marker}{suffix}");
                assert_eq!(claude_thinking_policy(&model), AdaptiveOnly, "{model}");
            }
            for suffix in ["0", "1", "x", "_1", ".1"] {
                let model = format!("anthropic.{marker}{suffix}");
                assert_eq!(claude_thinking_policy(&model), ManualOnly, "{model}");
            }
        }
    }

    #[test]
    fn preserves_opus_minor_parsing_and_classification_precedence() {
        for (model, expected) in [
            ("claude-opus-4-10-v1:0", AdaptiveOnly),
            ("claude-opus-4-007", AdaptiveOnly),
            ("claude-opus-4-65535", AdaptiveOnly),
            ("claude-opus-4-70000", ManualOnly),
            ("claude-opus-4-", ManualOnly),
            ("claude-opus-4-x7", ManualOnly),
            // Keep the existing substring fallback, including overflowing minor versions.
            ("claude-opus-4-65536", AdaptivePreferred),
            ("claude-opus-4-6-preview", AdaptivePreferred),
            ("claude-opus-4-5-preview", ManualWithEffort),
            ("claude-opus-4-7/claude-mythos-preview", MythosPreview),
            ("claude-opus-4-5/claude-sonnet-5", AdaptiveOnly),
        ] {
            assert_eq!(claude_thinking_policy(model), expected, "{model}");
        }
    }
}
