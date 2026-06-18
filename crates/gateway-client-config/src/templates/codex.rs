use std::collections::BTreeMap;

use serde::Serialize;

use crate::{
    format::to_pretty_toml,
    templates::notes::codex_notes,
    types::{ClientConfig, ClientConfigCodeBlock, ClientConfigInput, ClientConfigTemplate},
};

const CODEX_WIRE_API_RESPONSES: &str = "responses";

#[derive(Debug, Default, Clone, Copy)]
pub struct CodexConfigTemplate;

impl ClientConfigTemplate for CodexConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig {
        let mut model_providers = BTreeMap::new();
        model_providers.insert(
            input.provider_id.clone(),
            CodexModelProviderConfig {
                name: input.provider_name.clone(),
                base_url: input.gateway_base_url.clone(),
                env_key: input.api_key_env_var.clone(),
                wire_api: CODEX_WIRE_API_RESPONSES,
            },
        );

        let config = CodexConfigToml {
            model: input.model_id.clone(),
            model_provider: input.provider_id.clone(),
            model_providers,
        };

        ClientConfig {
            key: "codex".to_string(),
            label: "Codex".to_string(),
            blocks: vec![ClientConfigCodeBlock {
                label: "config.toml".to_string(),
                filename: "config.toml".to_string(),
                content: to_pretty_toml(&config),
            }],
            notes: codex_notes(input),
        }
    }
}

#[derive(Debug, Serialize)]
struct CodexConfigToml {
    model: String,
    model_provider: String,
    model_providers: BTreeMap<String, CodexModelProviderConfig>,
}

#[derive(Debug, Serialize)]
struct CodexModelProviderConfig {
    name: String,
    base_url: String,
    env_key: String,
    wire_api: &'static str,
}
