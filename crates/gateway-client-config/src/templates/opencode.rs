use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::{
    api_style::{ApiFormat, client_api_style, opencode_provider_package_for_style},
    cost::opencode_cost,
    format::to_pretty_json,
    templates::notes::{client_notes_for_inputs, thinking_notes},
    types::{
        ClientConfig, ClientConfigCodeBlock, ClientConfigInput, ClientConfigInputSet,
        ClientConfigSetupItem, ClientConfigTemplate, ReasoningLevel, ThinkingPolicy,
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
                "name": provider_name_for_style(inputs[0], *style, has_multiple_styles),
                "env": [inputs[0].api_key_env_var],
                "package": opencode_provider_package_for_style(*style),
                "settings": {
                    "baseURL": opencode_base_url(inputs[0], *style),
                },
                "models": Value::Object(opencode_models(inputs, *style)),
            });
            providers.insert(provider_id, provider);
        }

        let config = json!({
            "$schema": "https://opencode.ai/config.json",
            "providers": Value::Object(providers),
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

fn opencode_base_url(input: &ClientConfigInput, style: ApiFormat) -> String {
    match style {
        ApiFormat::OpenAiChatCompletions | ApiFormat::OpenAiResponses => {
            input.openai_compatible_client_base_url()
        }
        ApiFormat::AnthropicMessages => input.client_base_url(),
    }
}

fn grouped_models(
    input_set: &ClientConfigInputSet,
) -> BTreeMap<ApiFormat, Vec<&ClientConfigInput>> {
    let mut groups: BTreeMap<ApiFormat, Vec<&ClientConfigInput>> = BTreeMap::new();
    for input in &input_set.models {
        groups
            .entry(client_api_style(input))
            .or_default()
            .push(input);
    }
    groups
}

fn opencode_models(inputs: &[&ClientConfigInput], style: ApiFormat) -> Map<String, Value> {
    inputs
        .iter()
        .map(|input| {
            (
                input.model_id.clone(),
                Value::Object(opencode_model(input, style)),
            )
        })
        .collect()
}

fn opencode_model(input: &ClientConfigInput, style: ApiFormat) -> Map<String, Value> {
    // Always state reasoning support: without it OpenCode generates its own effort variants
    // for unknown models, including levels the gateway rejects.
    let mut model = Map::from_iter([
        ("name".to_string(), json!(input.display_name)),
        (
            "capabilities".to_string(),
            json!({
                "tools": input.capabilities.tool_calling,
                "input": opencode_input_media(input),
                "output": ["text"],
                "reasoning": {"supported": input.thinking_policy.is_some()},
            }),
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

    if let Some(variants) = opencode_variants(input, style) {
        model.insert("variants".to_string(), variants);
    }

    model
}

/// OpenCode matches attached and tool-read files against modality names; only `image` and
/// `pdf` enable file media, so other file types are not representable.
fn opencode_input_media(input: &ClientConfigInput) -> Vec<&'static str> {
    let mut media = input.input_modalities();
    if input.capabilities.attachments {
        media.push("pdf");
    }
    media
}

/// Effort variants for the model's thinking policy, minus any above the gateway ceiling,
/// spelled in the settings the style's OpenCode package reads.
pub(crate) fn opencode_variants(input: &ClientConfigInput, style: ApiFormat) -> Option<Value> {
    let variants: Vec<Value> = policy_efforts(input.thinking_policy?)
        .iter()
        .filter(|(_, effort)| {
            input.max_reasoning_effort.is_none_or(|ceiling| {
                ReasoningLevel::parse(effort).is_none_or(|level| level <= ceiling)
            })
        })
        .map(|(id, effort)| json!({"id": id, "settings": effort_settings(style, effort)}))
        .collect();
    (!variants.is_empty()).then_some(Value::Array(variants))
}

/// `(variant id, effort level)` pairs offered for a thinking policy.
fn policy_efforts(policy: ThinkingPolicy) -> Vec<(&'static str, &'static str)> {
    match policy {
        ThinkingPolicy::AnthropicSafeEffort => vec![("high", "high"), ("max", "xhigh")],
        ThinkingPolicy::AnthropicManualBudget => Vec::new(),
        ThinkingPolicy::GeminiLevel {
            supports_minimal,
            supports_medium,
        } => [
            ("minimal", supports_minimal),
            ("low", true),
            ("medium", supports_medium),
            ("high", true),
        ]
        .into_iter()
        .filter_map(|(level, supported)| supported.then_some((level, level)))
        .collect(),
        ThinkingPolicy::GeminiBudget => ["minimal", "low", "medium", "high"]
            .into_iter()
            .map(|level| (level, level))
            .collect(),
    }
}

/// OpenCode's Anthropic packages read `effort` (sent as `output_config.effort`) with adaptive
/// thinking; the OpenAI-compatible packages read `reasoningEffort`.
fn effort_settings(style: ApiFormat, effort: &str) -> Value {
    match style {
        ApiFormat::AnthropicMessages => json!({
            "thinking": {"type": "adaptive", "display": "summarized"},
            "effort": effort,
        }),
        ApiFormat::OpenAiChatCompletions | ApiFormat::OpenAiResponses => {
            json!({"reasoningEffort": effort})
        }
    }
}

fn provider_id_for_style(
    input: &ClientConfigInput,
    style: ApiFormat,
    has_multiple_styles: bool,
) -> String {
    if !has_multiple_styles {
        return input.provider_id.clone();
    }

    match style {
        ApiFormat::OpenAiChatCompletions => format!("{}-openai-compatible", input.provider_id),
        ApiFormat::OpenAiResponses => format!("{}-openai-responses", input.provider_id),
        ApiFormat::AnthropicMessages => format!("{}-anthropic-messages", input.provider_id),
    }
}

fn provider_name_for_style(
    input: &ClientConfigInput,
    style: ApiFormat,
    has_multiple_styles: bool,
) -> String {
    if !has_multiple_styles {
        return input.provider_name.clone();
    }

    match style {
        ApiFormat::OpenAiChatCompletions => format!("{} OpenAI-compatible", input.provider_name),
        ApiFormat::OpenAiResponses => format!("{} Responses", input.provider_name),
        ApiFormat::AnthropicMessages => format!("{} Anthropic Messages", input.provider_name),
    }
}
