use serde_json::{Map, Value, json};

use crate::types::ClientConfigInput;

pub(crate) fn opencode_cost(input: &ClientConfigInput) -> Value {
    let mut cost = Map::from_iter([
        (
            "input".to_string(),
            required_money4_to_number(input.input_cost_per_million_tokens_usd_10000),
        ),
        (
            "output".to_string(),
            required_money4_to_number(input.output_cost_per_million_tokens_usd_10000),
        ),
    ]);
    let cache: Map<String, Value> = [
        ("read", input.cache_read_cost_per_million_tokens_usd_10000),
        ("write", input.cache_write_cost_per_million_tokens_usd_10000),
    ]
    .into_iter()
    .filter_map(|(key, value)| Some((key.to_string(), money4_to_number(value)?)))
    .collect();
    if !cache.is_empty() {
        cost.insert("cache".to_string(), Value::Object(cache));
    }
    Value::Object(cost)
}

pub(crate) fn pi_cost(input: &ClientConfigInput) -> Value {
    let cost = Map::from_iter([
        (
            "input".to_string(),
            required_money4_to_number(input.input_cost_per_million_tokens_usd_10000),
        ),
        (
            "output".to_string(),
            required_money4_to_number(input.output_cost_per_million_tokens_usd_10000),
        ),
        (
            "cacheRead".to_string(),
            required_money4_to_number(input.cache_read_cost_per_million_tokens_usd_10000),
        ),
        (
            "cacheWrite".to_string(),
            required_money4_to_number(input.cache_write_cost_per_million_tokens_usd_10000),
        ),
    ]);
    Value::Object(cost)
}

fn money4_to_number(value: Option<i64>) -> Option<Value> {
    Some(json!((value? as f64) / 10_000.0))
}

fn required_money4_to_number(value: Option<i64>) -> Value {
    money4_to_number(value).unwrap_or_else(|| json!(0))
}
