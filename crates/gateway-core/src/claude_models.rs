//! Claude model classification shared by the provider adapters and the client-config
//! generator, so both agree on what each model generation accepts. Request and beta handling
//! stay in each transport.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeThinkingPolicy {
    /// Manual thinking budgets are unsupported; defaults and thinking-off rules vary by model.
    AdaptiveOnly,
    AdaptivePreferred,
    ManualWithEffort,
    ManualOnly,
    MythosPreview,
}

#[must_use]
pub fn claude_thinking_policy(upstream_model: &str) -> ClaudeThinkingPolicy {
    let model = normalize_claude_model(upstream_model);
    if model.contains("claude-mythos-preview") {
        ClaudeThinkingPolicy::MythosPreview
    } else if is_normalized_adaptive_only_claude(&model) {
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
    is_normalized_adaptive_only_claude(&normalize_claude_model(model))
}

/// Expects a model already passed through `normalize_claude_model`.
fn is_normalized_adaptive_only_claude(model: &str) -> bool {
    is_rolling_claude_alias(model)
        || is_opus_4_7_or_later(model)
        || contains_exact_claude_model_marker(model, "claude-fable-5")
        || contains_exact_claude_model_marker(model, "claude-opus-5")
        || contains_exact_claude_model_marker(model, "claude-sonnet-5")
        || contains_exact_claude_model_marker(model, "claude-haiku-5")
        || contains_exact_claude_model_marker(model, "claude-mythos-5")
}

/// Rolling identities describe a family, not a fixed generation. Chat compatibility
/// aliases can request adaptive thinking, while native controls are validated upstream.
#[must_use]
pub fn is_rolling_claude_alias(model: &str) -> bool {
    matches!(
        model.to_ascii_lowercase().rsplit('/').next(),
        Some("claude-opus-latest" | "claude-sonnet-latest" | "claude-haiku-latest")
    )
}

#[must_use]
pub fn is_openrouter_claude_model(model: &str) -> bool {
    model
        .strip_prefix("~anthropic/")
        .or_else(|| model.strip_prefix("anthropic/"))
        .is_some_and(|model| model.starts_with("claude-"))
}

/// `None` means the model's native thinking contract is unknown. Never infer legacy
/// restrictions from a rolling alias, private deployment name, or a new model family.
#[must_use]
pub fn known_claude_thinking_policy(model: &str) -> Option<ClaudeThinkingPolicy> {
    if is_rolling_claude_alias(model) {
        return None;
    }
    let model = normalize_claude_model(model);
    [
        "claude-opus-5-5",
        "claude-opus-5",
        "claude-opus-4-8",
        "claude-opus-4-7",
        "claude-opus-4-6",
        "claude-opus-4-5",
        "claude-opus-4-1",
        "claude-opus-4",
        "claude-sonnet-5-5",
        "claude-sonnet-5",
        "claude-sonnet-4-6",
        "claude-sonnet-4-5",
        "claude-sonnet-4",
        "claude-haiku-5-5",
        "claude-haiku-4-5",
        "claude-fable-5-1",
        "claude-fable-5",
        "claude-mythos-5-1",
        "claude-mythos-5",
        "claude-mythos-preview",
        "claude-3-7-sonnet",
        "claude-3-5-sonnet",
        "claude-3-5-haiku",
        "claude-3-opus",
        "claude-3-sonnet",
        "claude-3-haiku",
    ]
    .iter()
    .any(|marker| model.split(marker).skip(1).any(documented_model_suffix))
    .then(|| claude_thinking_policy(&model))
}

fn documented_model_suffix(rest: &str) -> bool {
    if rest.is_empty() || rest.starts_with(['/', ':', '@', ',', ')', ']', ' ', '\t']) {
        return true;
    }
    // Provider deployment versions and dated snapshots retain the documented profile.
    // A new short numeric minor version or an arbitrary alias does not.
    rest.starts_with("-v")
        || rest
            .strip_prefix('-')
            .is_some_and(|suffix| suffix.chars().take_while(char::is_ascii_digit).count() >= 6)
}

/// Lowercases `model` and rewrites dotted versions such as `claude-haiku-5.5` (the OpenRouter
/// spelling) to Anthropic's dashed form. Only a dot between two digits changes, so prefixes
/// like `us.anthropic.` keep theirs.
#[must_use]
pub fn normalize_claude_model(model: &str) -> String {
    let model = model.to_ascii_lowercase();
    let bytes = model.as_bytes();
    model
        .char_indices()
        .map(|(index, ch)| {
            let between_digits = index > 0
                && bytes[index - 1].is_ascii_digit()
                && bytes.get(index + 1).is_some_and(u8::is_ascii_digit);
            if ch == '.' && between_digits { '-' } else { ch }
        })
        .collect()
}

/// How a Claude model accepts a caller's request to turn thinking off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeThinkingOff {
    /// `thinking.type: disabled` is accepted at every effort level.
    Disabled,
    /// `thinking.type: disabled` is accepted at effort `high` or below.
    DisabledUpToHighEffort,
    /// `disabled` is rejected; `thinking.type: between_tools` is the thinking-off setting and
    /// is accepted at effort `high` or below.
    BetweenTools,
    /// Thinking is always on.
    Unsupported,
}

/// Only documented model generations are listed; anything else keeps `disabled` and lets
/// the upstream decide.
#[must_use]
pub fn claude_thinking_off(upstream_model: &str) -> ClaudeThinkingOff {
    let model = normalize_claude_model(upstream_model);
    if model.contains("claude-mythos-preview")
        || contains_exact_claude_model_marker(&model, "claude-fable-5")
        || contains_exact_claude_model_marker(&model, "claude-mythos-5")
        || claude_minor_version(&model, "claude-opus-5") == Some(5)
    {
        ClaudeThinkingOff::Unsupported
    } else if claude_minor_version(&model, "claude-sonnet-5") == Some(5) {
        ClaudeThinkingOff::BetweenTools
    } else if claude_minor_version(&model, "claude-opus-5") == Some(0)
        || claude_minor_version(&model, "claude-haiku-5") == Some(5)
    {
        ClaudeThinkingOff::DisabledUpToHighEffort
    } else {
        ClaudeThinkingOff::Disabled
    }
}

/// Forced `tool_choice` (`any`, `tool`, or the OpenAI `required` and named function forms)
/// returns a 400 upstream on these models; other Claude models accept it.
#[must_use]
pub fn claude_rejects_forced_tool_choice(upstream_model: &str) -> bool {
    let model = normalize_claude_model(upstream_model);
    claude_minor_version(&model, "claude-fable-5") == Some(1)
        || claude_minor_version(&model, "claude-mythos-5") == Some(1)
        || claude_minor_version(&model, "claude-opus-5") == Some(5)
        || claude_minor_version(&model, "claude-sonnet-5") == Some(5)
}

pub fn contains_exact_claude_model_marker(model: &str, marker: &str) -> bool {
    model
        .split(marker)
        .skip(1)
        .any(|rest| rest.chars().next().is_none_or(is_model_marker_boundary))
}

fn is_model_marker_boundary(ch: char) -> bool {
    ch.is_ascii_whitespace() || matches!(ch, '/' | ':' | '@' | ',' | ')' | ']' | '-')
}

/// Minor version of the first `marker` (a `claude-<family>-<major>` id) in `model`: `0` for
/// the bare major release, so `claude-sonnet-5-v1:0` is `0` and `claude-sonnet-5-5@001` is `5`.
/// A segment longer than two digits is a date stamp, not a minor version.
fn claude_minor_version(model: &str, marker: &str) -> Option<u16> {
    model.split(marker).skip(1).find_map(|rest| {
        if !rest.chars().next().is_none_or(is_model_marker_boundary) {
            return None;
        }
        let digits = rest
            .strip_prefix('-')
            .and_then(|rest| rest.split(|ch: char| !ch.is_ascii_digit()).next())
            .unwrap_or_default();
        Some(if (1..=2).contains(&digits.len()) {
            digits.parse().unwrap_or(0)
        } else {
            0
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
    use super::{
        ClaudeThinkingOff, ClaudeThinkingPolicy::*, claude_rejects_forced_tool_choice,
        claude_thinking_off, claude_thinking_policy,
    };

    #[test]
    fn rolling_and_unrecognized_models_defer_native_policy_to_upstream() {
        for model in [
            "~anthropic/claude-opus-latest",
            "anthropic/claude-sonnet-latest",
            "claude-haiku-latest",
            "private-deployment",
            "claude-opus-9-1",
            "claude-sonnet-4-9",
            "claude-opus-5-9",
            "claude-opus-5-custom",
        ] {
            assert_eq!(super::known_claude_thinking_policy(model), None, "{model}");
        }
        for model in [
            "claude-opus-5-5",
            "anthropic/claude-opus-5.5",
            "claude-sonnet-4-6",
            "us.anthropic.claude-opus-5-5-v1:0",
            "claude-haiku-4-5-20251001",
        ] {
            assert_eq!(
                super::known_claude_thinking_policy(model),
                Some(claude_thinking_policy(model)),
                "{model}"
            );
        }
        assert_eq!(
            claude_thinking_policy("~anthropic/claude-opus-latest"),
            AdaptiveOnly
        );
    }

    #[test]
    fn classifies_direct_and_transport_qualified_models() {
        for (model, expected) in [
            ("claude-mythos-preview", MythosPreview),
            ("claude-opus-4-7", AdaptiveOnly),
            ("claude-fable-5-1", AdaptiveOnly),
            ("claude-sonnet-5", AdaptiveOnly),
            ("claude-sonnet-5-5", AdaptiveOnly),
            ("claude-opus-5", AdaptiveOnly),
            ("claude-opus-5-5", AdaptiveOnly),
            ("claude-haiku-5-5", AdaptiveOnly),
            ("claude-mythos-5", AdaptiveOnly),
            ("claude-mythos-5-1", AdaptiveOnly),
            ("claude-haiku-4-5", ManualOnly),
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
        for marker in [
            "claude-fable-5",
            "claude-opus-5",
            "claude-sonnet-5",
            "claude-haiku-5",
        ] {
            for suffix in [
                "", "-1", ".1", ":0", "@latest", "/alias", ",", ")", "]", " ", "\t",
            ] {
                let model = format!("anthropic.{marker}{suffix}");
                assert_eq!(claude_thinking_policy(&model), AdaptiveOnly, "{model}");
            }
            for suffix in ["0", "1", "x", "_1", ".x"] {
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

    #[test]
    fn classifies_dotted_model_versions_like_dashed_ones() {
        for (model, expected) in [
            ("anthropic/claude-sonnet-4.6", AdaptivePreferred),
            ("anthropic/claude-opus-4.7", AdaptiveOnly),
            ("anthropic/claude-haiku-5.5", AdaptiveOnly),
            ("anthropic/claude-haiku-4.5", ManualOnly),
            ("us.anthropic.claude-opus-4-5-v1:0", ManualWithEffort),
        ] {
            assert_eq!(claude_thinking_policy(model), expected, "{model}");
        }
        for (model, expected) in [
            ("anthropic/claude-opus-4.8", ClaudeThinkingOff::Disabled),
            (
                "anthropic/claude-haiku-5.5",
                ClaudeThinkingOff::DisabledUpToHighEffort,
            ),
            (
                "anthropic/claude-sonnet-5.5",
                ClaudeThinkingOff::BetweenTools,
            ),
            ("anthropic/claude-opus-5.5", ClaudeThinkingOff::Unsupported),
            ("anthropic/claude-fable-5.1", ClaudeThinkingOff::Unsupported),
        ] {
            assert_eq!(claude_thinking_off(model), expected, "{model}");
        }
    }

    #[test]
    fn classifies_how_each_model_turns_thinking_off() {
        for (model, expected) in [
            ("claude-opus-4-7", ClaudeThinkingOff::Disabled),
            ("claude-opus-4-8", ClaudeThinkingOff::Disabled),
            ("claude-sonnet-5", ClaudeThinkingOff::Disabled),
            ("claude-sonnet-4-6", ClaudeThinkingOff::Disabled),
            ("claude-opus-5", ClaudeThinkingOff::DisabledUpToHighEffort),
            (
                "claude-haiku-5-5",
                ClaudeThinkingOff::DisabledUpToHighEffort,
            ),
            ("claude-sonnet-5-5", ClaudeThinkingOff::BetweenTools),
            ("claude-opus-5-5", ClaudeThinkingOff::Unsupported),
            ("claude-fable-5", ClaudeThinkingOff::Unsupported),
            ("claude-fable-5-1", ClaudeThinkingOff::Unsupported),
            ("claude-mythos-5", ClaudeThinkingOff::Unsupported),
            ("claude-mythos-5-1", ClaudeThinkingOff::Unsupported),
            ("claude-mythos-preview", ClaudeThinkingOff::Unsupported),
        ] {
            for qualified in [
                model.to_string(),
                format!("us.anthropic.{model}-v1:0"),
                format!("anthropic/{model}@20260101"),
                format!("{model}-20260415"),
                model.to_ascii_uppercase(),
            ] {
                assert_eq!(claude_thinking_off(&qualified), expected, "{qualified}");
            }
        }
    }

    #[test]
    fn classifies_forced_tool_choice_support() {
        for (model, rejects) in [
            ("claude-fable-5-1", true),
            ("claude-mythos-5-1", true),
            ("claude-opus-5-5", true),
            ("claude-sonnet-5-5", true),
            ("claude-fable-5", false),
            ("claude-opus-5", false),
            ("claude-sonnet-5", false),
            ("claude-haiku-5-5", false),
            ("claude-opus-4-7", false),
            ("claude-mythos-preview", false),
        ] {
            for qualified in [
                model.to_string(),
                format!("us.anthropic.{model}-v1:0"),
                format!("anthropic/{model}@20260101"),
                format!("{model}-20260415"),
                model.to_ascii_uppercase(),
                model.replacen("-5-", "-5.", 1),
            ] {
                assert_eq!(
                    claude_rejects_forced_tool_choice(&qualified),
                    rejects,
                    "{qualified}"
                );
            }
        }
    }
}
