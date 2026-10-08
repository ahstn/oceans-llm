use serde_json::Value;

use crate::ProviderError;

/// Failure classes used to decide whether another provider attempt is safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFailureKind {
    Transient,
    Quota,
    Credential,
    Terminal,
}

impl ProviderError {
    #[must_use]
    pub fn failure_kind(&self, provider_type: &str) -> ProviderFailureKind {
        match self {
            // Replaying any request with possible billed usage can charge it twice.
            Self::PartialUsage { .. } | Self::InvalidRequest(_) | Self::NotImplemented(_) => {
                ProviderFailureKind::Terminal
            }
            Self::Timeout | Self::Transport(_) => ProviderFailureKind::Transient,
            Self::UpstreamHttp { status, body, .. } => {
                classify_http_failure(provider_type, *status, body)
            }
        }
    }
}

fn classify_http_failure(provider_type: &str, status: u16, body: &str) -> ProviderFailureKind {
    let payload = serde_json::from_str::<Value>(body).ok();
    let codes = payload.as_ref().map(error_codes).unwrap_or_default();
    if codes.iter().any(|code| is_policy_denial(code)) {
        return ProviderFailureKind::Terminal;
    }

    // Copilot's HTTP 402 and quota error codes are defined by Microsoft's client:
    // https://github.com/microsoft/vscode/blob/3bf9306bd0df5c518f4bd35e0c7e6097dfa44c3c/extensions/copilot/src/extension/prompt/node/chatMLFetcher.ts
    // Keep this provider-specific: unknown authorization failures must not bypass policy.
    if provider_type == "github_copilot"
        && (status == 402 || codes.iter().any(|code| is_copilot_quota(code)))
    {
        return ProviderFailureKind::Quota;
    }

    match status {
        401 => ProviderFailureKind::Credential,
        408 | 429 | 500..=599 => ProviderFailureKind::Transient,
        _ => ProviderFailureKind::Terminal,
    }
}

fn error_codes(payload: &Value) -> Vec<&str> {
    // Only structured fields count. Error messages and arbitrary nested payloads
    // can contain user-controlled text and must not change retry decisions.
    ["/error/code", "/error/type", "/code", "/type"]
        .into_iter()
        .filter_map(|path| payload.pointer(path).and_then(Value::as_str))
        .collect()
}

fn is_policy_denial(code: &str) -> bool {
    matches!(
        code,
        "content_filter"
            | "content_filtered"
            | "content_policy_violation"
            | "policy_violation"
            | "policy_error"
            | "refusal"
            | "off_topic"
            | "extension_blocked"
    )
}

fn is_copilot_quota(code: &str) -> bool {
    matches!(
        code,
        "quota_exceeded"
            | "free_quota_exceeded"
            | "additional_spend_limit_reached"
            | "overage_limit_reached"
            | "billing_not_configured"
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::{ProviderError, ProviderFailureKind};

    fn http_error(status: u16, body: impl Into<String>) -> ProviderError {
        ProviderError::UpstreamHttp {
            status,
            body: body.into(),
            retry_after: None,
        }
    }

    #[test]
    fn generic_http_statuses_have_conservative_failure_classes() {
        for status in [408, 429, 500, 502, 503, 599] {
            assert_eq!(
                http_error(status, "upstream unavailable").failure_kind("openai_compat"),
                ProviderFailureKind::Transient,
                "status {status}"
            );
        }
        assert_eq!(
            http_error(401, "unauthorized").failure_kind("openai_compat"),
            ProviderFailureKind::Credential
        );
        for status in [400, 402, 403, 404, 409, 413, 422] {
            assert_eq!(
                http_error(status, "unrecognized error").failure_kind("openai_compat"),
                ProviderFailureKind::Terminal,
                "status {status}"
            );
        }
    }

    #[test]
    fn transport_errors_are_transient_but_request_errors_are_terminal() {
        assert_eq!(
            ProviderError::Timeout.failure_kind("github_copilot"),
            ProviderFailureKind::Transient
        );
        assert_eq!(
            ProviderError::Transport("connection failed".to_string()).failure_kind("openai_compat"),
            ProviderFailureKind::Transient
        );
        for error in [
            ProviderError::InvalidRequest("invalid model".to_string()),
            ProviderError::NotImplemented("embeddings".to_string()),
        ] {
            assert_eq!(
                error.failure_kind("github_copilot"),
                ProviderFailureKind::Terminal
            );
            assert!(!error.is_retryable());
        }
    }

    #[test]
    fn copilot_quota_classification_uses_documented_status_and_exact_codes() {
        // Constructed fixtures from Microsoft's client, not captured user traffic.
        // https://github.com/microsoft/vscode/blob/3bf9306bd0df5c518f4bd35e0c7e6097dfa44c3c/extensions/copilot/src/platform/chat/common/commonTypes.ts
        for code in [
            "quota_exceeded",
            "free_quota_exceeded",
            "additional_spend_limit_reached",
            "overage_limit_reached",
            "billing_not_configured",
        ] {
            for status in [402, 429] {
                let body = json!({"error": {"code": code, "message": "limit reached"}});
                assert_eq!(
                    http_error(status, body.to_string()).failure_kind("github_copilot"),
                    ProviderFailureKind::Quota,
                    "status {status}, code {code}"
                );
            }
        }
        assert_eq!(
            http_error(402, "unrecognized or non-JSON quota response")
                .failure_kind("github_copilot"),
            ProviderFailureKind::Quota
        );
        assert_eq!(
            http_error(
                429,
                json!({"error": {"code": "quota_exceeded"}}).to_string()
            )
            .failure_kind("openai_compat"),
            ProviderFailureKind::Transient
        );
    }

    #[test]
    fn policy_denial_overrides_retryable_status_and_quota_markers() {
        for status in [401, 402, 429, 503] {
            for code in [
                "content_filter",
                "refusal",
                "off_topic",
                "extension_blocked",
            ] {
                let body = json!({"error": {"code": code, "type": "rate_limit_error"}});
                assert_eq!(
                    http_error(status, body.to_string()).failure_kind("github_copilot"),
                    ProviderFailureKind::Terminal,
                    "status {status}, code {code}"
                );
            }
        }
        let body = json!({"error": {"code": "quota_exceeded", "type": "policy_error"}});
        let error = http_error(429, body.to_string());
        assert_eq!(
            error.failure_kind("github_copilot"),
            ProviderFailureKind::Terminal
        );
        assert!(!error.is_retryable());
    }

    #[test]
    fn flat_codes_are_supported_but_messages_are_not_classification_inputs() {
        assert_eq!(
            http_error(429, json!({"code": "extension_blocked"}).to_string())
                .failure_kind("github_copilot"),
            ProviderFailureKind::Terminal
        );
        for body in [
            "quota_exceeded",
            r#"{"error":{"message":"quota_exceeded"}}"#,
            r#"{"error":{"code":"quota_exceeded_unknown"}}"#,
            r#"{"data":{"code":"quota_exceeded"}}"#,
        ] {
            assert_eq!(
                http_error(403, body).failure_kind("github_copilot"),
                ProviderFailureKind::Terminal
            );
        }
        assert_eq!(
            http_error(503, r#"{"error":{"message":"content_filter"}}"#)
                .failure_kind("github_copilot"),
            ProviderFailureKind::Transient
        );
    }

    #[test]
    fn partial_usage_is_terminal_even_when_the_source_is_transient_or_quota() {
        for provider_usage in [None, Some(json!({"total_tokens": 5}))] {
            for source in [
                ProviderError::Timeout,
                http_error(429, "rate limited"),
                http_error(402, "quota exceeded"),
            ] {
                let error = ProviderError::PartialUsage {
                    source: Box::new(source),
                    provider_usage: provider_usage.clone(),
                };
                assert_eq!(
                    error.failure_kind("github_copilot"),
                    ProviderFailureKind::Terminal
                );
                assert!(!error.is_retryable());
            }
        }
    }

    #[test]
    fn retry_after_metadata_does_not_change_classification() {
        let error = ProviderError::UpstreamHttp {
            status: 403,
            body: "unknown authorization restriction".to_string(),
            retry_after: Some(Duration::from_secs(17)),
        };
        assert_eq!(error.retry_after(), Some(Duration::from_secs(17)));
        assert_eq!(
            error.failure_kind("github_copilot"),
            ProviderFailureKind::Terminal
        );
        assert_eq!(ProviderError::Timeout.retry_after(), None);
    }
}
