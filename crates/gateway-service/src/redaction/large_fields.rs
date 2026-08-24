use serde_json::{Map, Value, json};

use super::{BuiltInPathSegment, BuiltInPayloadPath, PathSegment};

const LARGE_FIELD_PATHS: &[BuiltInPayloadPath] = &[
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("messages"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("image_url"),
        BuiltInPathSegment::Key("url"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("messages"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("input_audio"),
        BuiltInPathSegment::Key("data"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("messages"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("file"),
        BuiltInPathSegment::Key("file_data"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("contents"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("parts"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("inlineData"),
        BuiltInPathSegment::Key("data"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("contents"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("parts"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("inline_data"),
        BuiltInPathSegment::Key("data"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("messages"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("source"),
        BuiltInPathSegment::Key("data"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("input"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("image_url"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("input"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("file_url"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("input"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("file_data"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("events"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("choices"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("delta"),
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("image_url"),
        BuiltInPathSegment::Key("url"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("events"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("choices"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("delta"),
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("input_audio"),
        BuiltInPathSegment::Key("data"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("events"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("choices"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("delta"),
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("file"),
        BuiltInPathSegment::Key("file_data"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("events"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("source"),
        BuiltInPathSegment::Key("data"),
    ]),
];

const LARGE_FIELD_PREVIEW_BYTES: usize = 96;

#[must_use]
pub fn truncate_large_payload_fields(value: &Value) -> Value {
    truncate_large_payload_fields_with_count(value).0
}

#[must_use]
pub fn truncate_large_payload_fields_with_count(value: &Value) -> (Value, usize) {
    let mut truncated_field_count = 0_usize;
    let value = truncate_large_fields_at_path(value, &mut Vec::new(), &mut truncated_field_count);
    (value, truncated_field_count)
}

fn truncate_large_fields_at_path(
    value: &Value,
    path: &mut Vec<PathSegment>,
    truncated_field_count: &mut usize,
) -> Value {
    if LARGE_FIELD_PATHS
        .iter()
        .any(|candidate| candidate.matches(path))
        && let Some(text) = value.as_str()
        && should_truncate_known_large_field(text)
    {
        *truncated_field_count = truncated_field_count.saturating_add(1);
        return json!({
            "truncated": true,
            "size_bytes": text.len(),
            "preview": safe_preview(text, LARGE_FIELD_PREVIEW_BYTES),
        });
    }

    match value {
        Value::Array(values) => {
            path.push(PathSegment::Wildcard);
            let bounded_values = values
                .iter()
                .map(|value| truncate_large_fields_at_path(value, path, truncated_field_count))
                .collect();
            path.pop();
            Value::Array(bounded_values)
        }
        Value::Object(values) => {
            let mut bounded_values = Map::with_capacity(values.len());
            for (key, value) in values {
                path.push(PathSegment::Key(key.clone()));
                bounded_values.insert(
                    key.clone(),
                    truncate_large_fields_at_path(value, path, truncated_field_count),
                );
                path.pop();
            }
            Value::Object(bounded_values)
        }
        _ => value.clone(),
    }
}

fn should_truncate_known_large_field(value: &str) -> bool {
    value.starts_with("data:") || is_probably_base64_payload(value)
}

fn is_probably_base64_payload(value: &str) -> bool {
    value.len() > 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'\n' | b'\r')
        })
}

fn safe_preview(value: &str, max_bytes: usize) -> String {
    value
        .char_indices()
        .take_while(|(index, _)| *index < max_bytes)
        .map(|(_, character)| character)
        .collect()
}
