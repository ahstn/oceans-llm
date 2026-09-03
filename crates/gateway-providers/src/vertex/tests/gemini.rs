use gateway_core::ReasoningEffort;

use super::*;

#[test]
fn gemini_3_flash_maps_every_effort_to_a_thinking_level() {
    let model = GeminiModel::parse("gemini-3.7-flash").expect("parse");
    assert_eq!(
        model.thinking_control(ReasoningEffort::Minimal),
        Some(ThinkingControl::Level("MINIMAL"))
    );
    assert_eq!(
        model.thinking_control(ReasoningEffort::Low),
        Some(ThinkingControl::Level("LOW"))
    );
    assert_eq!(
        model.thinking_control(ReasoningEffort::Medium),
        Some(ThinkingControl::Level("MEDIUM"))
    );
    assert_eq!(
        model.thinking_control(ReasoningEffort::High),
        Some(ThinkingControl::Level("HIGH"))
    );
    assert_eq!(
        model.thinking_control(ReasoningEffort::Max),
        Some(ThinkingControl::Level("HIGH"))
    );
}

#[test]
fn gemini_3_pro_collapses_to_low_and_high() {
    let model = GeminiModel::parse("gemini-3.1-pro-preview").expect("parse");
    assert_eq!(
        model.thinking_control(ReasoningEffort::Minimal),
        Some(ThinkingControl::Level("LOW"))
    );
    assert_eq!(
        model.thinking_control(ReasoningEffort::Medium),
        Some(ThinkingControl::Level("HIGH"))
    );
    assert_eq!(
        model.disabled_thinking_control(),
        Some(ThinkingControl::Level("LOW"))
    );
}

#[test]
fn gemini_2_5_uses_budgets_and_can_disable_thinking() {
    let flash = GeminiModel::parse("gemini-2.5-flash").expect("parse");
    assert!(matches!(
        flash.thinking_control(ReasoningEffort::Low),
        Some(ThinkingControl::Budget(budget)) if budget > 0
    ));
    assert_eq!(
        flash.disabled_thinking_control(),
        Some(ThinkingControl::Budget(0))
    );
}

#[test]
fn gemini_2_0_has_no_thinking_controls() {
    let model = GeminiModel::parse("gemini-2.0-flash").expect("parse");
    assert_eq!(model.thinking_control(ReasoningEffort::High), None);
    assert_eq!(model.disabled_thinking_control(), None);
}

#[test]
fn non_gemini_ids_do_not_parse() {
    assert!(GeminiModel::parse("gemma-2-27b-it").is_none());
    assert!(GeminiModel::parse("gemini-").is_none());
}
