use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::{
    api_style::{ClientApiStyle, client_api_style, opencode_provider_package_for_style},
    cost::opencode_cost,
    format::to_pretty_json,
    templates::notes::{client_notes_for_inputs, thinking_notes},
    types::{
        AnthropicThinkingPolicy, ClientConfig, ClientConfigCodeBlock, ClientConfigInput,
        ClientConfigInputSet, ClientConfigSetupItem, ClientConfigTemplate,
    },
};

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenCodeConfigTemplate;

const OPENCODE_CONFIG_DOCS_URL: &str = "https://opencode.ai/docs/config/";

impl ClientConfigTemplate for OpenCodeConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig {
        self.render_many(&ClientConfigInputSet::new(vec![input.clone()]))
    }
}

impl OpenCodeConfigTemplate {
    #[must_use]
    pub fn render_many(&self, input_set: &ClientConfigInputSet) -> ClientConfig {
        let first = input_set
            .first()
            .expect("OpenCode config rendering requires at least one model");
        let groups = grouped_models(input_set);
        let has_multiple_styles = groups.len() > 1;
        let default_provider_id =
            provider_id_for_style(first, client_api_style(first), has_multiple_styles);
        let mut providers = Map::new();

        for (style, inputs) in &groups {
            let provider_id = provider_id_for_style(inputs[0], *style, has_multiple_styles);
            let provider = json!({
                "npm": opencode_provider_package_for_style(*style),
                "name": provider_name_for_style(inputs[0], *style, has_multiple_styles),
                "options": {
                    "baseURL": opencode_base_url(inputs[0], *style),
                    "apiKey": format!("{{env:{}}}", inputs[0].api_key_env_var),
                },
                "models": Value::Object(opencode_models(inputs)),
            });
            providers.insert(provider_id, provider);
        }

        let config = json!({
            "$schema": "https://opencode.ai/config.json",
            "provider": Value::Object(providers),
            "model": format!("{}/{}", default_provider_id, first.model_id),
        });

        ClientConfig {
            key: "opencode".to_string(),
            label: "OpenCode".to_string(),
            model_ids: input_set
                .models
                .iter()
                .map(|input| input.model_id.clone())
                .collect(),
            setup: opencode_setup(first),
            blocks: vec![ClientConfigCodeBlock {
                label: "opencode.json".to_string(),
                filename: "opencode.json".to_string(),
                content: to_pretty_json(&config),
            }],
            notes: client_notes_for_inputs(input_set.models.iter())
                .into_iter()
                .chain(input_set.models.iter().flat_map(thinking_notes))
                .collect(),
        }
    }
}

fn opencode_setup(input: &ClientConfigInput) -> Vec<ClientConfigSetupItem> {
    vec![
        ClientConfigSetupItem {
            label: "Configuration".to_string(),
            value: "~/.config/opencode/opencode.json".to_string(),
            href: None,
        },
        ClientConfigSetupItem {
            label: "API key".to_string(),
            value: format!(
                "Set {} to a gateway API key before using this OpenCode configuration.",
                input.api_key_env_var
            ),
            href: None,
        },
        ClientConfigSetupItem {
            label: "Docs".to_string(),
            value: OPENCODE_CONFIG_DOCS_URL.to_string(),
            href: Some(OPENCODE_CONFIG_DOCS_URL.to_string()),
        },
    ]
}

fn opencode_base_url(input: &ClientConfigInput, style: ClientApiStyle) -> String {
    match style {
        ClientApiStyle::OpenAiCompatible => input.openai_compatible_client_base_url(),
        ClientApiStyle::AnthropicMessages => input.client_base_url(),
    }
}

fn grouped_models(
    input_set: &ClientConfigInputSet,
) -> BTreeMap<ClientApiStyle, Vec<&ClientConfigInput>> {
    let mut groups: BTreeMap<ClientApiStyle, Vec<&ClientConfigInput>> = BTreeMap::new();
    for input in &input_set.models {
        groups
            .entry(client_api_style(input))
            .or_default()
            .push(input);
    }
    groups
}

fn opencode_models(inputs: &[&ClientConfigInput]) -> Map<String, Value> {
    inputs
        .iter()
        .map(|input| (input.model_id.clone(), Value::Object(opencode_model(input))))
        .collect()
}

fn opencode_model(input: &ClientConfigInput) -> Map<String, Value> {
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

    model
}

fn provider_id_for_style(
    input: &ClientConfigInput,
    style: ClientApiStyle,
    has_multiple_styles: bool,
) -> String {
    if !has_multiple_styles {
        return input.provider_id.clone();
    }

    match style {
        ClientApiStyle::OpenAiCompatible => format!("{}-openai-compatible", input.provider_id),
        ClientApiStyle::AnthropicMessages => format!("{}-anthropic-messages", input.provider_id),
    }
}

fn provider_name_for_style(
    input: &ClientConfigInput,
    style: ClientApiStyle,
    has_multiple_styles: bool,
) -> String {
    if !has_multiple_styles {
        return input.provider_name.clone();
    }

    match style {
        ClientApiStyle::OpenAiCompatible => format!("{} OpenAI-compatible", input.provider_name),
        ClientApiStyle::AnthropicMessages => format!("{} Anthropic Messages", input.provider_name),
    }
}
