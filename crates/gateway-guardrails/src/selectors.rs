use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpCall {
    pub server: String,
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JsonPathSegment {
    Key(String),
    Index(usize),
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonPath {
    source: String,
    segments: Vec<JsonPathSegment>,
}

impl JsonPath {
    pub fn parse(source: impl Into<String>) -> Result<Self, JsonPathError> {
        let source = source.into();
        let bytes = source.as_bytes();
        if bytes.first() != Some(&b'$') {
            return Err(JsonPathError::MissingRoot);
        }

        let mut index = 1;
        let mut segments = Vec::new();
        while index < bytes.len() {
            match bytes[index] {
                b'.' => {
                    index += 1;
                    let start = index;
                    while index < bytes.len()
                        && (bytes[index].is_ascii_alphanumeric()
                            || matches!(bytes[index], b'_' | b'-'))
                    {
                        index += 1;
                    }
                    if start == index {
                        return Err(JsonPathError::InvalidSyntax(source));
                    }
                    segments.push(JsonPathSegment::Key(source[start..index].to_string()));
                }
                b'[' => {
                    index += 1;
                    if bytes.get(index) == Some(&b'*') && bytes.get(index + 1) == Some(&b']') {
                        segments.push(JsonPathSegment::Wildcard);
                        index += 2;
                        continue;
                    }
                    if matches!(bytes.get(index), Some(b'\'' | b'"')) {
                        let quote = bytes[index];
                        index += 1;
                        let start = index;
                        while index < bytes.len() && bytes[index] != quote {
                            index += 1;
                        }
                        if index == bytes.len() || bytes.get(index + 1) != Some(&b']') {
                            return Err(JsonPathError::InvalidSyntax(source));
                        }
                        segments.push(JsonPathSegment::Key(source[start..index].to_string()));
                        index += 2;
                        continue;
                    }
                    let start = index;
                    while index < bytes.len() && bytes[index].is_ascii_digit() {
                        index += 1;
                    }
                    if start == index || bytes.get(index) != Some(&b']') {
                        return Err(JsonPathError::InvalidSyntax(source));
                    }
                    let value = source[start..index]
                        .parse::<usize>()
                        .map_err(|_| JsonPathError::InvalidSyntax(source.clone()))?;
                    segments.push(JsonPathSegment::Index(value));
                    index += 1;
                }
                _ => return Err(JsonPathError::InvalidSyntax(source)),
            }
        }

        Ok(Self { source, segments })
    }

    pub fn as_str(&self) -> &str {
        &self.source
    }

    pub fn select<'a>(&self, root: &'a Value) -> Vec<&'a Value> {
        let mut current = vec![root];
        for segment in &self.segments {
            let mut next = Vec::new();
            for value in current {
                match segment {
                    JsonPathSegment::Key(key) => {
                        if let Some(selected) = value.get(key) {
                            next.push(selected);
                        }
                    }
                    JsonPathSegment::Index(index) => {
                        if let Some(selected) = value.get(*index) {
                            next.push(selected);
                        }
                    }
                    JsonPathSegment::Wildcard => match value {
                        Value::Array(values) => next.extend(values),
                        Value::Object(values) => next.extend(values.values()),
                        _ => {}
                    },
                }
            }
            current = next;
        }
        current
    }
}

impl FromStr for JsonPath {
    type Err = JsonPathError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for JsonPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.source)
    }
}

impl Serialize for JsonPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.source)
    }
}

impl<'de> Deserialize<'de> for JsonPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JsonPathError {
    #[error("JSON path must start with `$`")]
    MissingRoot,
    #[error("invalid JSON path syntax `{0}`")]
    InvalidSyntax(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonPredicateOp {
    Exists,
    Equals(Value),
    NotEquals(Value),
    OneOf(Vec<Value>),
    Contains(String),
    Truthy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonPredicate {
    pub path: JsonPath,
    pub op: JsonPredicateOp,
}

impl JsonPredicate {
    pub fn matches(&self, root: &Value) -> bool {
        let selected = self.path.select(root);
        match &self.op {
            JsonPredicateOp::Exists => !selected.is_empty(),
            JsonPredicateOp::Equals(expected) => selected.contains(&expected),
            JsonPredicateOp::NotEquals(expected) => {
                !selected.is_empty() && selected.iter().all(|value| *value != expected)
            }
            JsonPredicateOp::OneOf(expected) => selected
                .iter()
                .any(|value| expected.iter().any(|candidate| *value == candidate)),
            JsonPredicateOp::Contains(fragment) => selected
                .iter()
                .any(|value| value.as_str().is_some_and(|value| value.contains(fragment))),
            JsonPredicateOp::Truthy => selected.iter().any(|value| match value {
                Value::Bool(value) => *value,
                Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
                Value::String(value) => !value.is_empty(),
                Value::Array(value) => !value.is_empty(),
                Value::Object(value) => !value.is_empty(),
                Value::Null => false,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpSelector {
    #[serde(default)]
    pub servers: Vec<String>,
    pub tools: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub predicates: Vec<JsonPredicate>,
}

impl McpSelector {
    pub fn matches(&self, call: &McpCall) -> bool {
        let server_matches = self.servers.is_empty()
            || self
                .servers
                .iter()
                .any(|server| identity_matches(server, &call.server));
        let tool_matches = self
            .tools
            .iter()
            .chain(&self.aliases)
            .any(|tool| identity_matches(tool, &call.tool));

        server_matches
            && tool_matches
            && self
                .predicates
                .iter()
                .all(|predicate| predicate.matches(&call.arguments))
    }
}

fn identity_matches(pattern: &str, actual: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix("*.") {
        actual
            .to_ascii_lowercase()
            .ends_with(&format!(".{suffix}").to_ascii_lowercase())
    } else {
        pattern.eq_ignore_ascii_case(actual)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn selects_nested_values_and_wildcards() {
        let value = json!({"operations": [{"action": "keep"}, {"action": "delete"}]});
        let path = JsonPath::parse("$.operations[*].action").unwrap();
        assert_eq!(path.select(&value), vec![&json!("keep"), &json!("delete")]);
    }

    #[test]
    fn selector_is_independent_of_json_key_order() {
        let selector = McpSelector {
            servers: vec!["notion".into()],
            tools: vec!["pages.update".into()],
            aliases: vec!["update_page".into()],
            predicates: vec![JsonPredicate {
                path: JsonPath::parse("$.archived").unwrap(),
                op: JsonPredicateOp::Equals(json!(true)),
            }],
        };
        let first = McpCall {
            server: "notion".into(),
            tool: "update_page".into(),
            arguments: json!({"page_id": "p", "archived": true}),
        };
        let second = McpCall {
            arguments: json!({"archived": true, "page_id": "p"}),
            ..first.clone()
        };
        assert!(selector.matches(&first));
        assert!(selector.matches(&second));
    }
}
