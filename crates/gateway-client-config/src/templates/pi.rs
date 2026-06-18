use serde_json::{Map, Value, json};

use crate::{
    api_style::{pi_api_key_env_reference, pi_provider_api, pi_provider_compat},
    cost::pi_cost,
    format::to_pretty_json,
    templates::notes::thinking_notes,
    types::{
        AnthropicThinkingPolicy, ClientConfig, ClientConfigCodeBlock, ClientConfigInput,
        ClientConfigTemplate,
    },
};

#[derive(Debug, Default, Clone, Copy)]
pub struct PiConfigTemplate;

impl ClientConfigTemplate for PiConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig {
        let mut model = Map::from_iter([
            ("id".to_string(), json!(input.model_id)),
            ("name".to_string(), json!(input.display_name)),
            (
                "reasoning".to_string(),
                json!(input.thinking_policy.is_some()),
            ),
            ("input".to_string(), json!(input.input_modalities())),
            ("contextWindow".to_string(), json!(input.context_window())),
            ("maxTokens".to_string(), json!(input.output_window())),
            ("cost".to_string(), pi_cost(input)),
        ]);

        if input.thinking_policy == Some(AnthropicThinkingPolicy::SafeEffort) {
            model.insert(
                "thinkingLevelMap".to_string(),
                json!({
                    "off": null,
                    "minimal": null,
                    "low": "low",
                    "medium": "medium",
                    "high": "high",
                    "xhigh": "xhigh",
                }),
            );
        }

        let mut provider = Map::from_iter([
            ("baseUrl".to_string(), json!(input.gateway_base_url)),
            ("api".to_string(), json!(pi_provider_api(input))),
            ("apiKey".to_string(), json!(pi_api_key_env_reference(input))),
            ("models".to_string(), json!([Value::Object(model)])),
        ]);
        if let Some(compat) = pi_provider_compat(input) {
            provider.insert("compat".to_string(), compat);
        }

        let config = json!({
            "providers": {
                input.provider_id.as_str(): Value::Object(provider),
            },
        });

        ClientConfig {
            key: "pi".to_string(),
            label: "Pi".to_string(),
            blocks: vec![ClientConfigCodeBlock {
                label: "models.json".to_string(),
                filename: "models.json".to_string(),
                content: to_pretty_json(&config),
            }],
            notes: thinking_notes(input),
        }
    }
}
