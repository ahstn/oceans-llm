use serde_json::{Map, Value};

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
];

const SENSITIVE_JSON_KEYS: &[&str] = &[
    "authorization",
    "proxy_authorization",
    "cookie",
    "set_cookie",
    "x_api_key",
    "api_key",
    "token",
    "access_token",
    "refresh_token",
    "secret",
    "password",
];

const SECRET_MASK: &str = "********";

fn normalize_key(value: &str) -> String {
    value.to_ascii_lowercase().replace('-', "_")
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
        "[REDACTED]".to_string()
    } else {
        header_value.to_string()
    }
}

#[must_use]
pub fn redact_json_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(redact_json_value).collect()),
        Value::Object(values) => {
            let mut redacted = Map::with_capacity(values.len());
            for (key, value) in values {
                if is_sensitive_json_key(key) {
                    redacted.insert(key.clone(), Value::String("[REDACTED]".to_string()));
                } else {
                    redacted.insert(key.clone(), redact_json_value(value));
                }
            }
            Value::Object(redacted)
        }
        _ => value.clone(),
    }
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
        is_sensitive_json_key, mask_secret_leaf_values, redact_header_value, redact_json_value,
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
}
