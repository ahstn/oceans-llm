use serde_json::{Map, Value, json};

use crate::{
    api_style::opencode_provider_package,
    cost::opencode_cost,
    format::to_pretty_json,
    templates::notes::thinking_notes,
    types::{
        AnthropicThinkingPolicy, ClientConfig, ClientConfigCodeBlock, ClientConfigInput,
        ClientConfigTemplate,
    },
};

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenCodeConfigTemplate;

impl ClientConfigTemplate for OpenCodeConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig {
        let mut model = Map::from_iter([
            ("name".to_string(), json!(input.display_name)),
            (
                "reasoning".to_string(),
                json!(input.thinking_policy.is_some()),
            ),
            (
                "tool_call".to_string(),
                json!(input.capabilities.tool_calling),
            ),
            (
                "limit".to_string(),
                json!({
                    "context": input.context_window(),
                    "output": input.output_window(),
                }),
            ),
            ("cost".to_string(), opencode_cost(input)),
        ]);

        if input.capabilities.attachments || input.capabilities.vision {
            model.insert("attachment".to_string(), json!(true));
        }
        if input.thinking_policy == Some(AnthropicThinkingPolicy::SafeEffort) {
            model.insert(
                "variants".to_string(),
                json!({
                    "high": {
                        "reasoningEffort": "high",
                    },
                    "max": {
                        "reasoningEffort": "xhigh",
                    },
                }),
            );
        }

        let config = json!({
            "$schema": "https://opencode.ai/config.json",
            "provider": {
                input.provider_id.as_str(): {
                    "npm": opencode_provider_package(input),
                    "name": input.provider_name,
                    "options": {
                        "baseURL": input.gateway_base_url,
                        "apiKey": format!("{{env:{}}}", input.api_key_env_var),
                    },
                    "models": {
                        input.model_id.as_str(): Value::Object(model),
                    },
                },
            },
            "model": format!("{}/{}", input.provider_id, input.model_id),
        });

        ClientConfig {
            key: "opencode".to_string(),
            label: "OpenCode".to_string(),
            blocks: vec![ClientConfigCodeBlock {
                label: "opencode.json".to_string(),
                filename: "opencode.json".to_string(),
                content: to_pretty_json(&config),
            }],
            notes: thinking_notes(input),
        }
    }
}
