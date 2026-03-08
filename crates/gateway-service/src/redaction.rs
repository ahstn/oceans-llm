use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
];

const SENSITIVE_JSON_KEYS: &[&str] = &[
    "api_key",
    "authorization",
    "client_secret",
    "password",
    "refresh_token",
    "secret",
    "token",
    "access_token",
];

const MAX_STRING_BYTES: usize = 8 * 1024;
const MAX_DOCUMENT_BYTES: usize = 64 * 1024;
const PREVIEW_CHARS: usize = 160;

#[derive(Debug, Clone)]
pub struct SanitizedPayload {
    pub value: Value,
    pub bytes: i64,
    pub truncated: bool,
    pub sha256: String,
}

#[must_use]
pub fn is_sensitive_header(header_name: &str) -> bool {
    let lower = header_name.to_ascii_lowercase();
    SENSITIVE_HEADERS
        .iter()
        .any(|candidate| *candidate == lower)
}

#[must_use]
pub fn redact_header_value(header_name: &str, header_value: &str) -> String {
    if is_sensitive_header(header_name) {
        "[REDACTED]".to_string()
    } else {
        header_value.to_string()
    }
}

#[must_use]
pub fn sanitize_headers(headers: &BTreeMap<String, String>) -> Map<String, Value> {
    headers
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                Value::String(redact_header_value(name, value)),
            )
        })
        .collect()
}

#[must_use]
pub fn sanitize_json_payload(value: &Value) -> SanitizedPayload {
    let original_serialized = serialize_json(value);
    let sanitized = sanitize_value(None, value);
    let sanitized_serialized = serialize_json(&sanitized);
    let sanitized_bytes = sanitized_serialized.len();
    let sanitized_truncated = sanitized_bytes > MAX_DOCUMENT_BYTES;

    let final_value = if sanitized_truncated {
        json!({
            "kind": "document_truncated",
            "original_bytes": sanitized_bytes,
            "preview": preview_string(&sanitized_serialized),
            "sha256": sha256_hex(sanitized_serialized.as_bytes()),
        })
    } else {
        sanitized
    };

    let final_serialized = serialize_json(&final_value);

    SanitizedPayload {
        value: final_value,
        bytes: i64::try_from(original_serialized.len()).unwrap_or(i64::MAX),
        truncated: sanitized_truncated || final_serialized.len() > sanitized_bytes,
        sha256: sha256_hex(original_serialized.as_bytes()),
    }
}

fn sanitize_value(key: Option<&str>, value: &Value) -> Value {
    if key.is_some_and(is_sensitive_json_key) {
        return Value::String("[REDACTED]".to_string());
    }

    match value {
        Value::Object(object) => {
            let mut sanitized = Map::new();
            for (child_key, child_value) in object {
                sanitized.insert(child_key.clone(), sanitize_value(Some(child_key), child_value));
            }
            Value::Object(sanitized)
        }
        Value::Array(array) => Value::Array(
            array
                .iter()
                .map(|item| sanitize_value(None, item))
                .collect::<Vec<_>>(),
        ),
        Value::String(string) => sanitize_string_value(string),
        _ => value.clone(),
    }
}

fn sanitize_string_value(value: &str) -> Value {
    let is_binary_like = value.starts_with("data:")
        || value.len() > MAX_STRING_BYTES
        || looks_like_base64(value);
    if !is_binary_like {
        return Value::String(value.to_string());
    }

    json!({
        "kind": "omitted_string",
        "bytes": value.len(),
        "preview": preview_string(value),
        "sha256": sha256_hex(value.as_bytes()),
    })
}

fn is_sensitive_json_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    let tokens = normalized
        .split(|ch: char| !(ch.is_ascii_alphanumeric()))
        .filter(|part| !part.is_empty())
        .collect::<BTreeSet<_>>();

    SENSITIVE_JSON_KEYS.iter().any(|candidate| {
        let candidate_tokens = candidate.split('_').collect::<Vec<_>>();
        candidate_tokens.iter().all(|token| tokens.contains(token))
    })
}

fn looks_like_base64(value: &str) -> bool {
    if value.len() < 512 || !value.len().is_multiple_of(4) {
        return false;
    }

    value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'-' | b'_')
    })
}

fn preview_string(value: &str) -> String {
    value.chars().take(PREVIEW_CHARS).collect()
}

fn serialize_json(value: &Value) -> String {
    serde_json::to_string(value).expect("JSON payload should serialize")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::{sanitize_headers, sanitize_json_payload};

    #[test]
    fn redacts_sensitive_headers_and_keys() {
        let mut headers = BTreeMap::new();
        headers.insert("authorization".to_string(), "Bearer secret".to_string());
        headers.insert("x-team".to_string(), "platform".to_string());

        let headers = sanitize_headers(&headers);
        assert_eq!(headers["authorization"], "[REDACTED]");
        assert_eq!(headers["x-team"], "platform");

        let sanitized = sanitize_json_payload(&json!({
            "token": "secret",
            "nested": {"client_secret": "very-secret"},
            "message": "hello"
        }));

        assert_eq!(sanitized.value["token"], "[REDACTED]");
        assert_eq!(sanitized.value["nested"]["client_secret"], "[REDACTED]");
        assert_eq!(sanitized.value["message"], "hello");
    }

    #[test]
    fn truncates_binary_like_strings() {
        let big = "a".repeat(9000);
        let sanitized = sanitize_json_payload(&json!({
            "image": big
        }));

        assert_eq!(sanitized.value["image"]["kind"], "omitted_string");
        assert!(sanitized.value["image"]["bytes"].as_u64().unwrap_or_default() > 0);
    }
}
