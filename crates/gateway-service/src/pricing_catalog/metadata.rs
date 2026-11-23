//! Descriptive catalog fields. These fields never participate in spend accounting.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CatalogModelMetadata {
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub structured_output: Option<bool>,
    pub attachment: Option<bool>,
    pub temperature: Option<bool>,
    pub knowledge: Option<String>,
    #[serde(default)]
    pub reasoning_options: Vec<Value>,
    /// The gateway API surfaces a source says the model serves; `None` when the source
    /// does not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_endpoints: Option<CatalogApiEndpoints>,
}

impl CatalogModelMetadata {
    /// The values of the catalog's `effort` reasoning option, or `None` when the catalog
    /// lists no effort option for the model.
    #[must_use]
    pub fn effort_values(&self) -> Option<Vec<&str>> {
        self.reasoning_options
            .iter()
            .find(|option| option.get("type").and_then(Value::as_str) == Some("effort"))
            .map(|option| {
                option
                    .get("values")
                    .and_then(Value::as_array)
                    .map(|values| values.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default()
            })
    }
}

/// API surfaces a catalog source lists for a model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogApiEndpoints {
    pub chat_completions: bool,
    pub responses: bool,
    pub embeddings: bool,
    pub images: bool,
}

#[cfg(test)]
mod tests {
    use super::CatalogModelMetadata;
    use crate::pricing_catalog::sources::models_dev;
    use serde_json::json;

    #[test]
    fn effort_values_read_only_the_effort_option() {
        let metadata = |options: serde_json::Value| CatalogModelMetadata {
            reasoning_options: serde_json::from_value(options).unwrap(),
            ..CatalogModelMetadata::default()
        };
        assert_eq!(
            metadata(json!([{"type": "toggle"}, {"type": "effort", "values": ["high"]}]))
                .effort_values(),
            Some(vec!["high"])
        );
        assert_eq!(
            metadata(json!([{"type": "budget_tokens"}])).effort_values(),
            None
        );
        assert_eq!(metadata(json!([])).effort_values(), None);
        // An effort option without values says the model takes no effort level.
        assert_eq!(
            metadata(json!([{"type": "effort"}])).effort_values(),
            Some(Vec::new())
        );
    }

    #[test]
    fn projection_preserves_unknown_false_reasoning_options_and_conditional_rates() {
        let body = json!({"openai":{"name":"OpenAI","models":{"example":{
            "name":"Example","tool_call":false,
            "reasoning_options":[{"type":"effort","values":["low","high"]}],
            "cost":{"input":0,"context_over_200k":{"input":3,"output":12}}
        }}}});
        let providers = models_dev::project(body.to_string().as_bytes()).unwrap();
        let model = &providers["openai"].models["example"];
        assert_eq!(model.metadata.tool_call, Some(false));
        assert_eq!(model.metadata.reasoning, None);
        assert_eq!(
            model.metadata.reasoning_options[0]["values"],
            json!(["low", "high"])
        );
        assert_eq!(model.cost.input.as_deref(), Some("0.0000"));
        assert_eq!(model.cost.conditions["context_over_200k"]["input"], 3);
        assert_eq!(model.metadata.api_endpoints, None);
    }
}
