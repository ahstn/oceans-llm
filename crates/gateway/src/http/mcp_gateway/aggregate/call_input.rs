//! The `call_tool` input as clients send it.
//!
//! A client in OpenAI strict mode can't send an object with arbitrary keys:
//! strict mode closes the free-form `arguments` object, so the model can only
//! send `{}`. Such a client sends the target tool's arguments as a JSON string
//! in `arguments_json` instead, and this module decodes it into
//! [`CallMcpToolInput::arguments`].

use gateway_service::CallMcpToolInput;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallToolParams {
    address: String,
    #[serde(default)]
    arguments: Value,
    /// Kept as a raw value so a non-string gets an error that names the field.
    #[serde(default)]
    arguments_json: Value,
    #[serde(default)]
    schema_hash: Option<String>,
}

/// Parses `call_tool` params. `arguments_json`, when set to a non-empty
/// string, must hold a JSON object and replaces `arguments`, which must then
/// be absent, `null` or `{}`. The error is an invalid-params message.
pub(super) fn parse_call_tool_input(params: Value) -> Result<CallMcpToolInput, String> {
    let params: CallToolParams =
        serde_json::from_value(params).map_err(|error| error.to_string())?;
    let arguments = match params.arguments_json {
        Value::Null => params.arguments,
        Value::String(encoded) if encoded.is_empty() => params.arguments,
        Value::String(encoded) => {
            if !is_empty_arguments(&params.arguments) {
                return Err(
                    "set the target tool's arguments in either arguments or arguments_json, not both"
                        .to_string(),
                );
            }
            let decoded: Value = serde_json::from_str(&encoded)
                .map_err(|error| format!("arguments_json is not valid JSON: {error}"))?;
            if !decoded.is_object() {
                return Err(format!(
                    "arguments_json must encode a JSON object, not {}",
                    json_kind(&decoded)
                ));
            }
            decoded
        }
        other => {
            return Err(format!(
                "arguments_json must be a string that holds a JSON object, not {}",
                json_kind(&other)
            ));
        }
    };
    Ok(CallMcpToolInput {
        address: params.address,
        arguments,
        schema_hash: params.schema_hash,
    })
}

/// Strict clients send `{}` or `null` in `arguments` next to `arguments_json`.
fn is_empty_arguments(arguments: &Value) -> bool {
    match arguments {
        Value::Null => true,
        Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ADDRESS: &str = "mcp://repository/tools/search";

    fn parsed_arguments(params: Value) -> Value {
        parse_call_tool_input(params)
            .expect("valid call_tool params")
            .arguments
    }

    fn parse_error(params: Value) -> String {
        parse_call_tool_input(params).expect_err("invalid call_tool params")
    }

    #[test]
    fn arguments_object_passes_through() {
        let input = parse_call_tool_input(json!({
            "address": ADDRESS,
            "arguments": {"query": "oceans"},
            "schema_hash": "search-v1",
        }))
        .expect("valid call_tool params");
        assert_eq!(input.address, ADDRESS);
        assert_eq!(input.arguments, json!({"query": "oceans"}));
        assert_eq!(input.schema_hash.as_deref(), Some("search-v1"));
    }

    #[test]
    fn absent_null_or_empty_arguments_json_keeps_arguments() {
        for arguments_json in [None, Some(Value::Null), Some(json!(""))] {
            let mut params = json!({"address": ADDRESS, "arguments": {"query": "oceans"}});
            if let Some(arguments_json) = arguments_json {
                params["arguments_json"] = arguments_json;
            }
            assert_eq!(parsed_arguments(params), json!({"query": "oceans"}));
        }
        assert_eq!(parsed_arguments(json!({"address": ADDRESS})), Value::Null);
    }

    #[test]
    fn arguments_json_alone_supplies_the_arguments() {
        assert_eq!(
            parsed_arguments(json!({
                "address": ADDRESS,
                "arguments_json": r#"{"query":"oceans","limit":3}"#,
            })),
            json!({"query": "oceans", "limit": 3})
        );
    }

    /// Strict-mode clients send every property, so `arguments` arrives empty.
    #[test]
    fn arguments_json_with_empty_or_null_arguments_is_accepted() {
        for arguments in [json!({}), Value::Null] {
            assert_eq!(
                parsed_arguments(json!({
                    "address": ADDRESS,
                    "arguments": arguments,
                    "arguments_json": r#"{"query":"oceans"}"#,
                    "schema_hash": null,
                })),
                json!({"query": "oceans"})
            );
        }
    }

    #[test]
    fn arguments_and_arguments_json_together_conflict() {
        for arguments in [json!({"query": "other"}), json!("query")] {
            let error = parse_error(json!({
                "address": ADDRESS,
                "arguments": arguments,
                "arguments_json": r#"{"query":"oceans"}"#,
            }));
            assert!(error.contains("arguments_json, not both"), "{error}");
        }
    }

    #[test]
    fn malformed_arguments_json_is_rejected() {
        let error = parse_error(json!({"address": ADDRESS, "arguments_json": "{\"query\":"}));
        assert!(
            error.starts_with("arguments_json is not valid JSON"),
            "{error}"
        );
    }

    #[test]
    fn arguments_json_must_encode_an_object() {
        for (encoded, kind) in [("[1]", "an array"), ("\"x\"", "a string"), ("null", "null")] {
            let error = parse_error(json!({"address": ADDRESS, "arguments_json": encoded}));
            assert_eq!(
                error,
                format!("arguments_json must encode a JSON object, not {kind}")
            );
        }
    }

    #[test]
    fn arguments_json_must_be_a_string() {
        let error = parse_error(json!({"address": ADDRESS, "arguments_json": {"query": "x"}}));
        assert_eq!(
            error,
            "arguments_json must be a string that holds a JSON object, not an object"
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let error = parse_error(json!({"address": ADDRESS, "argument": {}}));
        assert!(error.contains("unknown field `argument`"), "{error}");
    }
}
