use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::{
    api_style::{
        ClientApiStyle, client_api_style, pi_api_key_env_reference, pi_provider_api_for_style,
        pi_provider_compat,
    },
    cost::pi_cost,
    format::to_pretty_json,
    templates::notes::thinking_notes,
    types::{
        AnthropicThinkingPolicy, ClientConfig, ClientConfigCodeBlock, ClientConfigInput,
        ClientConfigInputSet, ClientConfigTemplate,
    },
};

#[derive(Debug, Default, Clone, Copy)]
pub struct PiConfigTemplate;

impl ClientConfigTemplate for PiConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig {
        self.render_many(&ClientConfigInputSet::new(vec![input.clone()]))
    }
}

impl PiConfigTemplate {
    #[must_use]
    pub fn render_many(&self, input_set: &ClientConfigInputSet) -> ClientConfig {
        let groups = grouped_models(input_set);
        let has_multiple_styles = groups.len() > 1;
        let mut providers = Map::new();

        for (style, inputs) in &groups {
            let provider_id = provider_id_for_style(inputs[0], *style, has_multiple_styles);
            let mut provider = Map::from_iter([
                ("baseUrl".to_string(), json!(inputs[0].gateway_base_url)),
                ("api".to_string(), json!(pi_provider_api_for_style(*style))),
                (
                    "apiKey".to_string(),
                    json!(pi_api_key_env_reference(inputs[0])),
                ),
                ("models".to_string(), Value::Array(pi_models(inputs))),
            ]);
            if let Some(compat) = pi_group_compat(*style, inputs) {
                provider.insert("compat".to_string(), compat);
            }
            providers.insert(provider_id, Value::Object(provider));
        }

        let config = json!({
            "providers": Value::Object(providers),
        });

        ClientConfig {
            key: "pi".to_string(),
            label: "Pi".to_string(),
            model_ids: input_set
                .models
                .iter()
                .map(|input| input.model_id.clone())
                .collect(),
            blocks: vec![ClientConfigCodeBlock {
                label: "models.json".to_string(),
                filename: "models.json".to_string(),
                content: to_pretty_json(&config),
            }],
            notes: input_set.models.iter().flat_map(thinking_notes).collect(),
        }
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

fn pi_models(inputs: &[&ClientConfigInput]) -> Vec<Value> {
    inputs
        .iter()
        .map(|input| Value::Object(pi_model(input)))
        .collect()
}

fn pi_model(input: &ClientConfigInput) -> Map<String, Value> {
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

    model
}

fn pi_group_compat(style: ClientApiStyle, inputs: &[&ClientConfigInput]) -> Option<Value> {
    match style {
        ClientApiStyle::OpenAiCompatible => pi_provider_compat(inputs[0]),
        ClientApiStyle::AnthropicMessages => inputs
            .iter()
            .find(|input| input.thinking_policy == Some(AnthropicThinkingPolicy::SafeEffort))
            .and_then(|input| pi_provider_compat(input)),
    }
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
