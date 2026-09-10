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
}

#[cfg(test)]
mod tests {
    use crate::pricing_catalog::project_models_dev_snapshot;
    use serde_json::json;
    use time::OffsetDateTime;

    #[test]
    fn projection_preserves_unknown_false_reasoning_options_and_conditional_rates() {
        let body = json!({"openai":{"name":"OpenAI","models":{"example":{
            "name":"Example","tool_call":false,
            "reasoning_options":[{"type":"effort","values":["low","high"]}],
            "cost":{"input":0,"context_over_200k":{"input":3,"output":12}}
        }}}});
        let snapshot = project_models_dev_snapshot(
            &body.to_string(),
            "test",
            None,
            OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap();
        let model = &snapshot.document.providers["openai"].models["example"];
        assert_eq!(model.metadata.tool_call, Some(false));
        assert_eq!(model.metadata.reasoning, None);
        assert_eq!(
            model.metadata.reasoning_options[0]["values"],
            json!(["low", "high"])
        );
        assert_eq!(model.cost.input.as_deref(), Some("0.0000"));
        assert_eq!(model.cost.conditions["context_over_200k"]["input"], 3);
    }
}
