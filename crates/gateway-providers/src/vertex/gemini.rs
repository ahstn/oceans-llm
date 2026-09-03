use gateway_core::ReasoningEffort;

/// Version and tier of a Gemini model id such as `gemini-3.7-flash` or `gemini-3.1-pro-preview`.
///
/// Thinking controls differ by generation: Gemini 3.x takes `thinkingConfig.thinkingLevel`,
/// Gemini 2.5 takes `thinkingConfig.thinkingBudget`, and older models have no thinking at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GeminiModel {
    major: u32,
    minor: u32,
    tier: GeminiTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeminiTier {
    Flash,
    FlashLite,
    Pro,
    Other,
}

/// Thinking control the model accepts; carries the wire value for one requested effort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThinkingControl {
    Level(&'static str),
    Budget(i64),
}

impl GeminiModel {
    pub(super) fn parse(model_id: &str) -> Option<Self> {
        let rest = model_id.strip_prefix("gemini-")?;
        let (version, tier) = rest.split_once('-').unwrap_or((rest, ""));
        let (major, minor) = version.split_once('.').unwrap_or((version, "0"));
        let major = major.parse().ok()?;
        let minor = minor.parse().ok()?;
        let tier = if tier.starts_with("flash-lite") {
            GeminiTier::FlashLite
        } else if tier.starts_with("flash") {
            GeminiTier::Flash
        } else if tier.starts_with("pro") {
            GeminiTier::Pro
        } else {
            GeminiTier::Other
        };
        Some(Self { major, minor, tier })
    }

    pub(super) const fn supports_thinking(self) -> bool {
        self.major >= 3 || (self.major == 2 && self.minor >= 5)
    }

    const fn uses_thinking_level(self) -> bool {
        self.major >= 3
    }

    const fn is_at_least(self, major: u32, minor: u32) -> bool {
        self.major > major || (self.major == major && self.minor >= minor)
    }

    /// Gemini 3.5+ accepts `functionCall.id` / `functionResponse.id`; earlier Vertex models
    /// reject them.
    pub(super) const fn supports_function_ids(self) -> bool {
        self.is_at_least(3, 5)
    }

    /// Gemini 3.7+ ignores `temperature`/`topP`/`topK` and rejects `presencePenalty`,
    /// `frequencyPenalty`, and `candidateCount` with an API error.
    pub(super) const fn deprecates_sampling(self) -> bool {
        self.is_at_least(3, 7)
    }

    /// `MINIMAL` is a Flash / Flash-Lite level up to Gemini 3.6; Pro and 3.7+ start at `LOW`.
    const fn supports_minimal_level(self) -> bool {
        matches!(self.tier, GeminiTier::Flash | GeminiTier::FlashLite) && !self.is_at_least(3, 7)
    }

    /// Wire value for one categorical effort, or `None` when the model has no thinking.
    pub(super) fn thinking_control(self, effort: ReasoningEffort) -> Option<ThinkingControl> {
        if !self.supports_thinking() {
            return None;
        }
        if self.uses_thinking_level() {
            return Some(ThinkingControl::Level(self.thinking_level(effort)));
        }
        Some(ThinkingControl::Budget(self.thinking_budget(effort)))
    }

    /// Wire value that suppresses thinking as far as the model allows. Gemini 2.5 Pro cannot
    /// disable thinking and enforces a 128-token floor, so it gets the minimum budget instead.
    pub(super) fn disabled_thinking_control(self) -> Option<ThinkingControl> {
        if !self.supports_thinking() {
            return None;
        }
        if self.uses_thinking_level() {
            return Some(ThinkingControl::Level(
                self.thinking_level(ReasoningEffort::Minimal),
            ));
        }
        if matches!(self.tier, GeminiTier::Pro) {
            return Some(ThinkingControl::Budget(
                self.thinking_budget(ReasoningEffort::Minimal),
            ));
        }
        Some(ThinkingControl::Budget(0))
    }

    /// Gemini 3 Pro only exposes `LOW` and `HIGH`; Flash and Flash-Lite expose `LOW`..`HIGH`
    /// plus `MINIMAL` before 3.7.
    fn thinking_level(self, effort: ReasoningEffort) -> &'static str {
        let pro = matches!(self.tier, GeminiTier::Pro);
        match effort {
            ReasoningEffort::Minimal if self.supports_minimal_level() => "MINIMAL",
            ReasoningEffort::Minimal | ReasoningEffort::Low => "LOW",
            ReasoningEffort::Medium if pro => "HIGH",
            ReasoningEffort::Medium => "MEDIUM",
            ReasoningEffort::High | ReasoningEffort::XHigh | ReasoningEffort::Max => "HIGH",
        }
    }

    /// Token budgets for Gemini 2.5, mirroring the values Pi uses for the same tiers.
    fn thinking_budget(self, effort: ReasoningEffort) -> i64 {
        let pro = matches!(self.tier, GeminiTier::Pro);
        match effort {
            ReasoningEffort::Minimal => 128,
            ReasoningEffort::Low => 2048,
            ReasoningEffort::Medium => 8192,
            ReasoningEffort::High | ReasoningEffort::XHigh | ReasoningEffort::Max if pro => 32_768,
            ReasoningEffort::High | ReasoningEffort::XHigh | ReasoningEffort::Max => 24_576,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_generation_and_tier() {
        let flash = GeminiModel::parse("gemini-3.8-flash").expect("flash");
        assert_eq!(
            (flash.major, flash.minor, flash.tier),
            (3, 8, GeminiTier::Flash)
        );
        let lite = GeminiModel::parse("gemini-3.1-flash-lite").expect("lite");
        assert_eq!(lite.tier, GeminiTier::FlashLite);
        let pro = GeminiModel::parse("gemini-3-pro-preview").expect("pro");
        assert_eq!((pro.major, pro.minor, pro.tier), (3, 0, GeminiTier::Pro));
        assert!(GeminiModel::parse("gemini-embedding-2").is_none());
        assert!(GeminiModel::parse("text-embedding-005").is_none());
    }

    #[test]
    fn flash_levels_pass_through_and_pro_collapses() {
        let flash = GeminiModel::parse("gemini-3.6-flash").expect("flash");
        assert_eq!(
            flash.thinking_control(ReasoningEffort::Minimal),
            Some(ThinkingControl::Level("MINIMAL"))
        );
        assert_eq!(
            flash.thinking_control(ReasoningEffort::Medium),
            Some(ThinkingControl::Level("MEDIUM"))
        );
        assert_eq!(
            flash.thinking_control(ReasoningEffort::XHigh),
            Some(ThinkingControl::Level("HIGH"))
        );

        let pro = GeminiModel::parse("gemini-3.1-pro-preview").expect("pro");
        assert_eq!(
            pro.thinking_control(ReasoningEffort::Minimal),
            Some(ThinkingControl::Level("LOW"))
        );
        assert_eq!(
            pro.thinking_control(ReasoningEffort::Medium),
            Some(ThinkingControl::Level("HIGH"))
        );
    }

    #[test]
    fn gemini_3_7_and_later_drop_minimal_and_deprecate_sampling() {
        for id in [
            "gemini-3.7-flash",
            "gemini-3.8-flash",
            "gemini-4-flash-lite",
        ] {
            let model = GeminiModel::parse(id).expect(id);
            assert_eq!(
                model.thinking_control(ReasoningEffort::Minimal),
                Some(ThinkingControl::Level("LOW")),
                "{id}"
            );
            assert_eq!(
                model.disabled_thinking_control(),
                Some(ThinkingControl::Level("LOW")),
                "{id}"
            );
            assert_eq!(
                model.thinking_control(ReasoningEffort::Medium),
                Some(ThinkingControl::Level("MEDIUM")),
                "{id}"
            );
            assert!(model.deprecates_sampling(), "{id}");
        }
        for id in [
            "gemini-3.6-flash",
            "gemini-3.1-pro-preview",
            "gemini-2.5-flash",
        ] {
            assert!(
                !GeminiModel::parse(id).expect(id).deprecates_sampling(),
                "{id}"
            );
        }
    }

    #[test]
    fn gemini_2_5_uses_budgets_and_2_0_has_none() {
        let flash = GeminiModel::parse("gemini-2.5-flash").expect("flash");
        assert_eq!(
            flash.thinking_control(ReasoningEffort::High),
            Some(ThinkingControl::Budget(24_576))
        );
        assert_eq!(
            flash.disabled_thinking_control(),
            Some(ThinkingControl::Budget(0))
        );
        let pro = GeminiModel::parse("gemini-2.5-pro").expect("pro");
        assert_eq!(
            pro.thinking_control(ReasoningEffort::Max),
            Some(ThinkingControl::Budget(32_768))
        );
        let legacy = GeminiModel::parse("gemini-2.0-flash").expect("legacy");
        assert_eq!(legacy.thinking_control(ReasoningEffort::Low), None);
        assert!(!legacy.supports_thinking());
    }

    #[test]
    fn function_ids_require_gemini_3_5() {
        assert!(
            GeminiModel::parse("gemini-3.5-flash")
                .unwrap()
                .supports_function_ids()
        );
        assert!(
            GeminiModel::parse("gemini-3.8-flash")
                .unwrap()
                .supports_function_ids()
        );
        assert!(
            !GeminiModel::parse("gemini-3.1-pro-preview")
                .unwrap()
                .supports_function_ids()
        );
        assert!(
            !GeminiModel::parse("gemini-2.5-flash")
                .unwrap()
                .supports_function_ids()
        );
    }
}
