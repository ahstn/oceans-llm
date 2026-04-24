use serde_json::{Map, Value, json};

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "anthropic-api-key",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-goog-api-key",
    "x-api-key",
];

const SENSITIVE_JSON_KEYS: &[&str] = &[
    "authorization",
    "proxy_authorization",
    "cookie",
    "set_cookie",
    "x_api_key",
    "api_key",
    "anthropic_api_key",
    "client_secret",
    "credentials",
    "private_key",
    "token",
    "access_token",
    "refresh_token",
    "secret",
    "password",
];

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

const DEFAULT_REQUEST_MAX_BYTES: usize = 64 * 1024;
const DEFAULT_RESPONSE_MAX_BYTES: usize = 64 * 1024;
const DEFAULT_STREAM_MAX_EVENTS: usize = 128;
const PAYLOAD_POLICY_VERSION: &str = "builtin:v1";
const SECRET_MASK: &str = "********";
const REDACTED_VALUE: &str = "[REDACTED]";
const LARGE_FIELD_PREVIEW_BYTES: usize = 96;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RequestLogPayloadCaptureMode {
    Disabled,
    SummaryOnly,
    #[default]
    RedactedPayloads,
}

impl RequestLogPayloadCaptureMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::SummaryOnly => "summary_only",
            Self::RedactedPayloads => "redacted_payloads",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestLogPayloadPolicy {
    pub capture_mode: RequestLogPayloadCaptureMode,
    pub request_max_bytes: usize,
    pub response_max_bytes: usize,
    pub stream_max_events: usize,
    redaction_paths: Vec<PayloadPath>,
}

impl Default for RequestLogPayloadPolicy {
    fn default() -> Self {
        Self {
            capture_mode: RequestLogPayloadCaptureMode::default(),
            request_max_bytes: DEFAULT_REQUEST_MAX_BYTES,
            response_max_bytes: DEFAULT_RESPONSE_MAX_BYTES,
            stream_max_events: DEFAULT_STREAM_MAX_EVENTS,
            redaction_paths: Vec::new(),
        }
    }
}

impl RequestLogPayloadPolicy {
    #[must_use]
    pub fn new(
        capture_mode: RequestLogPayloadCaptureMode,
        request_max_bytes: usize,
        response_max_bytes: usize,
        stream_max_events: usize,
        redaction_paths: Vec<PayloadPath>,
    ) -> Self {
        Self {
            capture_mode,
            request_max_bytes,
            response_max_bytes,
            stream_max_events,
            redaction_paths,
        }
    }

    #[must_use]
    pub fn metadata_value(&self) -> Value {
        json!({
            "capture_mode": self.capture_mode.as_str(),
            "request_max_bytes": self.request_max_bytes,
            "response_max_bytes": self.response_max_bytes,
            "stream_max_events": self.stream_max_events,
            "version": PAYLOAD_POLICY_VERSION,
        })
    }

    #[must_use]
    pub fn should_capture_payloads(&self) -> bool {
        matches!(
            self.capture_mode,
            RequestLogPayloadCaptureMode::RedactedPayloads
        )
    }

    fn redacts_path(&self, path: &[PathSegment]) -> bool {
        self.redaction_paths
            .iter()
            .any(|candidate| candidate.matches(path))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadPath {
    segments: Vec<PathSegment>,
}

impl PayloadPath {
    #[must_use]
    pub fn as_string(&self) -> String {
        self.segments
            .iter()
            .map(PathSegment::as_str)
            .collect::<Vec<_>>()
            .join(".")
    }

    fn matches(&self, path: &[PathSegment]) -> bool {
        self.segments.len() == path.len()
            && self
                .segments
                .iter()
                .zip(path)
                .all(|(expected, actual)| expected.matches(actual))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PathSegment {
    Key(String),
    Wildcard,
}

impl PathSegment {
    fn as_str(&self) -> &str {
        match self {
            Self::Key(value) => value.as_str(),
            Self::Wildcard => "*",
        }
    }

    fn matches(&self, actual: &Self) -> bool {
        matches!(self, Self::Wildcard) || self == actual
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuiltInPathSegment {
    Key(&'static str),
    Wildcard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuiltInPayloadPath {
    segments: &'static [BuiltInPathSegment],
}

impl BuiltInPayloadPath {
    const fn new(segments: &'static [BuiltInPathSegment]) -> Self {
        Self { segments }
    }

    fn matches(&self, path: &[PathSegment]) -> bool {
        self.segments.len() == path.len()
            && self
                .segments
                .iter()
                .zip(path)
                .all(|(expected, actual)| expected.matches(actual))
    }
}

impl BuiltInPathSegment {
    fn matches(&self, actual: &PathSegment) -> bool {
        match (self, actual) {
            (Self::Wildcard, _) => true,
            (Self::Key(expected), PathSegment::Key(actual)) => *expected == actual,
            (Self::Key(_), PathSegment::Wildcard) => false,
        }
    }
}

fn normalize_key(value: &str) -> String {
    value.to_ascii_lowercase().replace('-', "_")
}

pub fn parse_payload_path(value: &str) -> Result<PayloadPath, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("path cannot be empty".to_string());
    }

    let mut segments = Vec::new();
    for segment in trimmed.split('.') {
        if segment.is_empty() {
            return Err(format!("path `{value}` contains an empty segment"));
        }
        if segment == "*" {
            segments.push(PathSegment::Wildcard);
            continue;
        }
        if !segment.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        }) {
            return Err(format!(
                "path `{value}` segment `{segment}` must use ASCII letters, numbers, `_`, `-`, or `*`"
            ));
        }
        segments.push(PathSegment::Key(segment.to_string()));
    }

    Ok(PayloadPath { segments })
}

#[must_use]
pub fn is_sensitive_header(header_name: &str) -> bool {
    let lower = header_name.to_ascii_lowercase();
    SENSITIVE_HEADERS
        .iter()
        .any(|candidate| *candidate == lower)
}

#[must_use]
pub fn is_sensitive_json_key(key: &str) -> bool {
    let normalized = normalize_key(key);
    SENSITIVE_JSON_KEYS
        .iter()
        .any(|candidate| *candidate == normalized)
}

#[must_use]
pub fn redact_header_value(header_name: &str, header_value: &str) -> String {
    if is_sensitive_header(header_name) {
        REDACTED_VALUE.to_string()
    } else {
        header_value.to_string()
    }
}

#[must_use]
pub fn redact_json_value(value: &Value) -> Value {
    redact_json_value_with_policy(value, &RequestLogPayloadPolicy::default())
}

#[must_use]
pub fn redact_json_value_with_policy(value: &Value, policy: &RequestLogPayloadPolicy) -> Value {
    redact_json_value_at_path(value, policy, &mut Vec::new())
}

fn redact_json_value_at_path(
    value: &Value,
    policy: &RequestLogPayloadPolicy,
    path: &mut Vec<PathSegment>,
) -> Value {
    if policy.redacts_path(path) {
        return Value::String(REDACTED_VALUE.to_string());
    }

    match value {
        Value::Array(values) => {
            path.push(PathSegment::Wildcard);
            let redacted = values
                .iter()
                .map(|value| redact_json_value_at_path(value, policy, path))
                .collect();
            path.pop();
            Value::Array(redacted)
        }
        Value::Object(values) => {
            let mut redacted = Map::with_capacity(values.len());
            for (key, value) in values {
                if is_sensitive_json_key(key) {
                    redacted.insert(key.clone(), Value::String(REDACTED_VALUE.to_string()));
                } else {
                    path.push(PathSegment::Key(key.clone()));
                    redacted.insert(key.clone(), redact_json_value_at_path(value, policy, path));
                    path.pop();
                }
            }
            Value::Object(redacted)
        }
        _ => value.clone(),
    }
}

#[must_use]
pub fn truncate_large_payload_fields(value: &Value) -> Value {
    truncate_large_fields_at_path(value, LARGE_FIELD_PATHS, &mut Vec::new())
}

fn truncate_large_fields_at_path(
    value: &Value,
    paths: &[BuiltInPayloadPath],
    path: &mut Vec<PathSegment>,
) -> Value {
    if paths.iter().any(|candidate| candidate.matches(path))
        && let Some(text) = value.as_str()
        && should_truncate_known_large_field(text)
    {
        return json!({
            "truncated": true,
            "size_bytes": text.len(),
            "preview": safe_preview(text, LARGE_FIELD_PREVIEW_BYTES),
        });
    }

    match value {
        Value::Array(values) => {
            path.push(PathSegment::Wildcard);
            let truncated = values
                .iter()
                .map(|value| truncate_large_fields_at_path(value, paths, path))
                .collect();
            path.pop();
            Value::Array(truncated)
        }
        Value::Object(values) => {
            let mut truncated = Map::with_capacity(values.len());
            for (key, value) in values {
                path.push(PathSegment::Key(key.clone()));
                truncated.insert(
                    key.clone(),
                    truncate_large_fields_at_path(value, paths, path),
                );
                path.pop();
            }
            Value::Object(truncated)
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

#[must_use]
pub fn mask_secret_leaf_values(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(mask_secret_leaf_values).collect()),
        Value::Object(values) => {
            let mut masked = Map::with_capacity(values.len());
            for (key, value) in values {
                masked.insert(key.clone(), mask_secret_leaf_values(value));
            }
            Value::Object(masked)
        }
        Value::Null => Value::Null,
        Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            Value::String(SECRET_MASK.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        RequestLogPayloadCaptureMode, RequestLogPayloadPolicy, is_sensitive_json_key,
        mask_secret_leaf_values, parse_payload_path, redact_header_value, redact_json_value,
        redact_json_value_with_policy, truncate_large_payload_fields,
    };

    #[test]
    fn redacts_nested_sensitive_json_keys() {
        let input = json!({
            "token": "raw",
            "nested": {
                "password": "secret",
                "keep": "value"
            }
        });

        let redacted = redact_json_value(&input);
        assert_eq!(redacted["token"], "[REDACTED]");
        assert_eq!(redacted["nested"]["password"], "[REDACTED]");
        assert_eq!(redacted["nested"]["keep"], "value");
    }

    #[test]
    fn header_redaction_keeps_non_sensitive_values() {
        assert_eq!(redact_header_value("x-trace-id", "trace-1"), "trace-1");
        assert_eq!(redact_header_value("authorization", "secret"), "[REDACTED]");
    }

    #[test]
    fn mask_secret_leaf_values_preserves_shape_with_asterisked_scalars() {
        let input = json!({
            "api_key": "raw-key",
            "service_account": {
                "client_email": "svc@example.com",
                "nested": [
                    {"private_key": "-----BEGIN PRIVATE KEY-----"},
                    42,
                    true,
                    null
                ]
            }
        });

        let masked = mask_secret_leaf_values(&input);

        assert_eq!(masked["api_key"], "********");
        assert_eq!(masked["service_account"]["client_email"], "********");
        assert_eq!(
            masked["service_account"]["nested"][0]["private_key"],
            "********"
        );
        assert_eq!(masked["service_account"]["nested"][1], "********");
        assert_eq!(masked["service_account"]["nested"][2], "********");
        assert_eq!(
            masked["service_account"]["nested"][3],
            serde_json::Value::Null
        );
    }

    #[test]
    fn sensitive_json_key_check_normalizes_separators() {
        assert!(is_sensitive_json_key("x-api-key"));
        assert!(is_sensitive_json_key("refresh_token"));
    }

    #[test]
    fn parses_payload_paths_with_wildcards() {
        let path = parse_payload_path("body.messages.*.content").expect("path parses");
        assert_eq!(path.as_string(), "body.messages.*.content");
        assert!(parse_payload_path("body..messages").is_err());
        assert!(parse_payload_path("body.messages[0]").is_err());
    }

    #[test]
    fn redacts_operator_configured_paths() {
        let policy = RequestLogPayloadPolicy::new(
            RequestLogPayloadCaptureMode::RedactedPayloads,
            1024,
            1024,
            10,
            vec![parse_payload_path("body.messages.*.metadata.internal").expect("path")],
        );
        let input = json!({
            "body": {
                "messages": [
                    {"metadata": {"internal": "secret", "public": "kept"}}
                ]
            }
        });

        let redacted = redact_json_value_with_policy(&input, &policy);

        assert_eq!(
            redacted["body"]["messages"][0]["metadata"]["internal"],
            "[REDACTED]"
        );
        assert_eq!(
            redacted["body"]["messages"][0]["metadata"]["public"],
            "kept"
        );
    }

    #[test]
    fn truncates_known_large_provider_fields_without_changing_shape() {
        let input = json!({
            "body": {
                "messages": [
                    {
                        "content": [
                            {
                                "type": "input_audio",
                                "input_audio": {
                                    "data": "a".repeat(400),
                                    "format": "wav"
                                }
                            }
                        ]
                    }
                ]
            }
        });

        let truncated = truncate_large_payload_fields(&input);

        assert_eq!(
            truncated["body"]["messages"][0]["content"][0]["input_audio"]["data"]["truncated"],
            true
        );
        assert_eq!(
            truncated["body"]["messages"][0]["content"][0]["input_audio"]["format"],
            "wav"
        );
    }

    #[test]
    fn leaves_normal_remote_image_urls_unchanged() {
        let input = json!({
            "body": {
                "messages": [
                    {
                        "content": [
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": "https://example.com/image.png"
                                }
                            }
                        ]
                    }
                ]
            }
        });

        let truncated = truncate_large_payload_fields(&input);

        assert_eq!(
            truncated["body"]["messages"][0]["content"][0]["image_url"]["url"],
            "https://example.com/image.png"
        );
    }

    #[test]
    fn truncates_vertex_gemini_inline_data_fields() {
        let input = json!({
            "body": {
                "contents": [
                    {
                        "parts": [
                            {
                                "inlineData": {
                                    "mimeType": "image/png",
                                    "data": "a".repeat(400)
                                }
                            }
                        ]
                    }
                ]
            }
        });

        let truncated = truncate_large_payload_fields(&input);

        assert_eq!(
            truncated["body"]["contents"][0]["parts"][0]["inlineData"]["data"]["truncated"],
            true
        );
        assert_eq!(
            truncated["body"]["contents"][0]["parts"][0]["inlineData"]["mimeType"],
            "image/png"
        );
    }

    #[test]
    fn truncates_vertex_anthropic_base64_source_data_fields() {
        let input = json!({
            "body": {
                "messages": [
                    {
                        "content": [
                            {
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": "image/jpeg",
                                    "data": "a".repeat(400)
                                }
                            }
                        ]
                    }
                ]
            }
        });

        let truncated = truncate_large_payload_fields(&input);

        assert_eq!(
            truncated["body"]["messages"][0]["content"][0]["source"]["data"]["truncated"],
            true
        );
        assert_eq!(
            truncated["body"]["messages"][0]["content"][0]["source"]["media_type"],
            "image/jpeg"
        );
    }
}
