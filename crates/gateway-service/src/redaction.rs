use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::agent_analysis::SESSION_ANALYSIS_DIAGNOSTIC_HEADERS;

mod large_fields;

pub use large_fields::{truncate_large_payload_fields, truncate_large_payload_fields_with_count};

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "anthropic-api-key",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-goog-api-key",
    "x-api-key",
];

const CODEX_TURN_METADATA_FIELDS: &[&str] = &[
    "forked_from_thread_id",
    "parent_thread_id",
    "session_id",
    "thread_id",
    "turn_id",
];

const SENSITIVE_JSON_KEYS: &[&str] = &[
    "authorization",
    "proxy_authorization",
    "cookie",
    "set_cookie",
    "x_api_key",
    "api_key",
    "raw_key",
    "generated_key",
    "key_material",
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

const MEDIA_URL_PATHS: &[BuiltInPayloadPath] = &[
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
        BuiltInPathSegment::Key("video_url"),
        BuiltInPathSegment::Key("url"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("messages"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("input_video"),
        BuiltInPathSegment::Key("url"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("messages"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("file"),
        BuiltInPathSegment::Key("url"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("messages"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("input_file"),
        BuiltInPathSegment::Key("url"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("messages"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("document"),
        BuiltInPathSegment::Key("url"),
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
        BuiltInPathSegment::Key("video_url"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("input"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("content"),
        BuiltInPathSegment::Wildcard,
        BuiltInPathSegment::Key("audio_url"),
    ]),
];

const ERROR_TEXT_PATHS: &[BuiltInPayloadPath] = &[
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("error"),
    ]),
    BuiltInPayloadPath::new(&[
        BuiltInPathSegment::Key("body"),
        BuiltInPathSegment::Key("error"),
        BuiltInPathSegment::Key("message"),
    ]),
];

const DEFAULT_REQUEST_MAX_BYTES: usize = 128 * 1024;
const DEFAULT_RESPONSE_MAX_BYTES: usize = 64 * 1024;
const DEFAULT_STREAM_MAX_EVENTS: usize = 128;
const PAYLOAD_POLICY_VERSION: &str = "builtin:v2";
const SECRET_MASK: &str = "********";
pub const MAX_INLINE_REQUEST_BYTES: usize = 256 * 1024;
pub(crate) const REDACTED_VALUE: &str = "[REDACTED]";

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
            request_max_bytes: request_max_bytes.min(MAX_INLINE_REQUEST_BYTES),
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
pub fn sanitize_diagnostic_headers(headers: &BTreeMap<String, String>) -> Map<String, Value> {
    headers
        .iter()
        .filter(|(name, _)| is_diagnostic_header(name))
        .filter_map(|(name, value)| {
            sanitize_diagnostic_header_value(name, value)
                .map(|value| (name.clone(), Value::String(value)))
        })
        .collect()
}

fn is_diagnostic_header(header_name: &str) -> bool {
    SESSION_ANALYSIS_DIAGNOSTIC_HEADERS
        .iter()
        .any(|candidate| header_name.eq_ignore_ascii_case(candidate))
}

fn sanitize_diagnostic_header_value(header_name: &str, header_value: &str) -> Option<String> {
    if !header_name.eq_ignore_ascii_case("x-codex-turn-metadata") {
        return Some(redact_header_value(header_name, header_value));
    }

    let metadata = serde_json::from_str::<Value>(header_value.trim()).ok()?;
    let metadata = metadata.as_object()?;
    let retained = CODEX_TURN_METADATA_FIELDS
        .iter()
        .filter_map(|field| {
            metadata
                .get(*field)
                .and_then(Value::as_str)
                .map(|value| ((*field).to_string(), Value::String(value.to_string())))
        })
        .collect::<Map<_, _>>();
    serde_json::to_string(&retained).ok()
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

    if MEDIA_URL_PATHS
        .iter()
        .any(|candidate| candidate.matches(path))
        && let Some(url) = value.as_str()
    {
        return Value::String(redact_url_query(url));
    }

    if ERROR_TEXT_PATHS
        .iter()
        .any(|candidate| candidate.matches(path))
        && let Some(message) = value.as_str()
    {
        return Value::String(redact_https_url_queries(message));
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

fn redact_url_query(url: &str) -> String {
    let (base, has_query) = url
        .split_once('?')
        .map_or((url, false), |(base, _)| (base, true));
    let sanitized_base = url::Url::parse(base)
        .ok()
        .filter(|parsed| !parsed.username().is_empty() || parsed.password().is_some())
        .and_then(|mut parsed| {
            parsed.set_username("").ok()?;
            parsed.set_password(None).ok()?;
            Some(parsed.to_string())
        })
        .unwrap_or_else(|| base.to_string());

    if has_query {
        format!("{sanitized_base}?<redacted>")
    } else {
        sanitized_base
    }
}

fn redact_https_url_queries(text: &str) -> String {
    let lowercase = text.to_ascii_lowercase();
    let mut redacted = String::with_capacity(text.len());
    let mut cursor = 0;

    while let Some(relative_start) = lowercase[cursor..].find("https://") {
        let start = cursor + relative_start;
        let end = text[start..]
            .find(|character: char| {
                character.is_ascii_whitespace() || matches!(character, '"' | '\'' | '`' | '<' | '>')
            })
            .map_or(text.len(), |relative_end| start + relative_end);
        redacted.push_str(&text[cursor..start]);
        redacted.push_str(&redact_url_query(&text[start..end]));
        cursor = end;
    }

    redacted.push_str(&text[cursor..]);
    redacted
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
mod tests;
