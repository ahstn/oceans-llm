mod bedrock;
mod model_armor;

pub use bedrock::{
    BedrockApplyGuardrail, BedrockApplyGuardrailConfig, BedrockAuth, validate_guardrail_identifier,
    validate_guardrail_version,
};
pub use model_armor::{
    BearerTokenProvider, ModelArmor, ModelArmorConfig, StaticBearerTokenProvider,
    validate_template_resource_name,
};

use crate::EvaluationInput;

fn input_text(input: &EvaluationInput) -> String {
    input.payload.inspection_text()
}
