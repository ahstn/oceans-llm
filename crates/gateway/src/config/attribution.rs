//! `attribution`: which tabs of the admin Attribution page are shown. Ingestion and the report
//! endpoint are unaffected; the flags only decide what the admin UI renders.

use serde::Deserialize;

const fn enabled() -> bool {
    true
}

/// Each Attribution tab, on by default except two. `repositories` is off until line survival
/// makes its readiness quadrant meaningful, and `concepts` is the design mockup on sample data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributionPagesConfig {
    #[serde(default = "enabled")]
    pub impact: bool,
    #[serde(default = "enabled")]
    pub delivery: bool,
    #[serde(default = "enabled")]
    pub pull_requests: bool,
    /// Off by default: without line survival its quadrant has no durability axis.
    #[serde(default)]
    pub repositories: bool,
    #[serde(default = "enabled")]
    pub durability: bool,
    #[serde(default = "enabled")]
    pub my_work: bool,
    /// The concept mockups at `/observability/attribution/concepts`, on generated sample data.
    #[serde(default)]
    pub concepts: bool,
}

impl Default for AttributionPagesConfig {
    fn default() -> Self {
        Self {
            impact: true,
            delivery: true,
            pull_requests: true,
            repositories: false,
            durability: true,
            my_work: true,
            concepts: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributionConfig {
    #[serde(default)]
    pub pages: AttributionPagesConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tab_but_repositories_and_the_concepts_defaults_on() {
        let config: AttributionConfig = serde_yaml::from_str("{}").expect("empty config parses");
        assert_eq!(config.pages, AttributionPagesConfig::default());
        assert!(config.pages.impact && config.pages.my_work);
        assert!(!config.pages.repositories);
        assert!(!config.pages.concepts);
    }

    #[test]
    fn a_tab_can_be_turned_off_alone() {
        let config: AttributionConfig =
            serde_yaml::from_str("pages:\n  durability: false\n  concepts: true\n")
                .expect("pages parse");
        assert!(!config.pages.durability);
        assert!(config.pages.concepts);
        assert!(config.pages.delivery);
    }

    #[test]
    fn unknown_tabs_are_rejected() {
        let error = serde_yaml::from_str::<AttributionConfig>("pages:\n  agents: false\n")
            .expect_err("unknown tab");
        assert!(error.to_string().contains("unknown field"));
    }
}
