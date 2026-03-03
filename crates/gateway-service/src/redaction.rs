const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
];

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
