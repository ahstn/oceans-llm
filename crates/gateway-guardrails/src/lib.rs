//! Protocol-neutral guardrail policy, evaluation, and managed-service adapters.
//!
//! HTTP routing, authentication, configuration loading, persistence, and tool
//! transport stay in their owning gateway crates.

mod command;
mod evaluation;
mod managed;
mod model;
mod packs;
mod policy;
mod selectors;

pub use evaluation::{
    DeterministicEvaluator, EvaluationError, GuardrailEngine, ManagedEvaluator, ManagedOutcome,
};
pub use managed::{
    BearerTokenProvider, BedrockApplyGuardrail, BedrockApplyGuardrailConfig, BedrockAuth,
    ModelArmor, ModelArmorConfig, StaticBearerTokenProvider, validate_guardrail_identifier,
    validate_guardrail_version, validate_template_resource_name,
};
pub use model::{
    ContentTransformation, DecisionAction, DecisionId, DecisionRecord, EffectiveScope,
    EvaluationInput, EvaluationPayload, FailureDisposition, GuardPhase, GuardrailEvaluation,
    ManagedDecisionMetadata, ManagedService, MatchedRule, PolicyMode, ReasonCode,
};
pub use packs::{BUILT_IN_PACK_IDS, BuiltInEvaluator, PackId, PackMetadata, PackRegistry};
pub use policy::{
    BedrockManagedAuthConfig, BedrockManagedConfig, EffectivePolicy, GuardrailConfig,
    GuardrailConfigError, ManagedCheckConfig, ManagedCheckKind, ModelArmorAuthConfig,
    ModelArmorManagedConfig, PolicyConfig, PolicyOverride, PolicyResolver, PolicyTarget,
};
pub use selectors::{
    JsonPath, JsonPathError, JsonPredicate, JsonPredicateOp, McpCall, McpSelector,
};

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
