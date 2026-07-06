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
    if let Some(cache_read) = money4_to_number(input.cache_read_cost_per_million_tokens_usd_10000) {
        cost.insert("cache_read".to_string(), cache_read);
    }
    Value::Object(cost)
}

pub(crate) fn pi_cost(input: &ClientConfigInput) -> Value {
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
    if let Some(cache_read) = money4_to_number(input.cache_read_cost_per_million_tokens_usd_10000) {
        cost.insert("cacheRead".to_string(), cache_read);
    }
    Value::Object(cost)
}

fn money4_to_number(value: Option<i64>) -> Option<Value> {
    Some(json!((value? as f64) / 10_000.0))
}

fn required_money4_to_number(value: Option<i64>) -> Value {
    money4_to_number(value).unwrap_or_else(|| json!(0))
}
