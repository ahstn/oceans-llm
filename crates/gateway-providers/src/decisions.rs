//! Shared transport helpers for the Decisions API family.
//!
//! Upstreams speak one of two wire formats: System One (one `state` evaluated
//! against named typed questions; native TypeSafe and the alpha
//! `openai_compat` route) or OpenAI Decisions (`input` plus ordered
//! questions). Adapters own URL routing and auth; request translation and
//! response shape checks live here so every transport stays identical.

use std::collections::BTreeMap;

use gateway_core::{
    CoreDecisionsRequest, DecisionsWireFormat, ProviderDecisionsResponse, ProviderError,
    ProviderRequestContext, core_decisions_request_to_openai, core_decisions_request_to_system_one,
};
use serde_json::Value;

/// Translate a core request into the upstream wire format, pin the upstream
/// model, and merge route `extra_body` overrides. Requests the format cannot
/// carry (image inputs or `safety_identifier` for System One, for example)
/// fail as invalid requests before any upstream call.
pub(crate) fn base_decisions_request_body(
    format: DecisionsWireFormat,
    request: &CoreDecisionsRequest,
    context: &ProviderRequestContext,
) -> Result<Value, ProviderError> {
    let mut body = match format {
        DecisionsWireFormat::SystemOne => core_decisions_request_to_system_one(request)
            .map_err(ProviderError::InvalidRequest)
            .and_then(|wire| serde_json::to_value(wire).map_err(transport_error))?,
        DecisionsWireFormat::OpenAi => core_decisions_request_to_openai(request)
            .map_err(ProviderError::InvalidRequest)
            .and_then(|wire| serde_json::to_value(wire).map_err(transport_error))?,
    };
    let Some(object) = body.as_object_mut() else {
        return Err(ProviderError::Transport(
            "decisions request must serialize to a JSON object".to_string(),
        ));
    };
    object.insert(
        "model".to_string(),
        Value::String(context.upstream_model.clone()),
    );
    for (key, value) in &context.extra_body {
        object.insert(key.clone(), value.clone());
    }
    Ok(body)
}

fn transport_error(error: serde_json::Error) -> ProviderError {
    ProviderError::Transport(error.to_string())
}

/// POST builder with default headers, route extra headers, and `x-request-id`.
/// Callers apply their own bearer auth before building.
pub(crate) fn decisions_request_builder(
    client: &reqwest::Client,
    url: String,
    body: &Value,
    default_headers: &BTreeMap<String, String>,
    context: &ProviderRequestContext,
) -> reqwest::RequestBuilder {
    let mut builder = client.post(url).json(body);
    for (name, value) in default_headers {
        builder = builder.header(name, value);
    }
    for (name, value) in &context.extra_headers {
        if let Some(value) = value.as_str() {
            builder = builder.header(name, value);
        }
    }
    builder.header("x-request-id", &context.request_id)
}

/// Trimmed base root plus its host. Rejects query parameters and fragments so
/// adapters can safely append their own Decisions path.
pub(crate) fn base_root_and_host(
    base_url: &str,
) -> Result<(String, Option<String>), ProviderError> {
    let trimmed = base_url.trim_end_matches('/');
    let parsed = url::Url::parse(trimmed)
        .map_err(|error| ProviderError::Transport(format!("invalid base_url: {error}")))?;
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(ProviderError::InvalidRequest(
            "base_url cannot contain query parameters or fragments".to_string(),
        ));
    }
    Ok((trimmed.to_string(), parsed.host_str().map(str::to_string)))
}

/// Reject upstream shape drift loudly: the response must satisfy the
/// format's contract, and every requested question needs a matching valid
/// answer. The upstream has already billed a rejected response, so the error
/// carries its `usage` for accounting.
pub(crate) fn parse_decisions_response(
    format: DecisionsWireFormat,
    body: Value,
    request: &CoreDecisionsRequest,
) -> Result<ProviderDecisionsResponse, ProviderError> {
    let parsed =
        format
            .parse_response(&body, request)
            .map_err(|message| ProviderError::PartialUsage {
                source: Box::new(ProviderError::Transport(message)),
                provider_usage: body.get("usage").cloned(),
            })?;
    Ok(ProviderDecisionsResponse {
        format,
        body,
        parsed,
    })
}

/// Shared fixture: a System One caller's request translated to core.
#[cfg(test)]
pub(crate) fn system_one_test_request() -> CoreDecisionsRequest {
    let request: gateway_core::SystemOneDecisionsRequest =
        serde_json::from_value(serde_json::json!({
            "model": "jev",
            "state": "Help! My payouts have been failing for 3 days.",
            "questions": {
                "is_urgent": {"type": "noul", "instructions": "Does this convey urgency?"},
                "department": {
                    "type": "choice",
                    "instructions": "Which team?",
                    "criteria": {"billing": "Payments", "technical": null}
                },
                "frustration": {
                    "type": "score",
                    "instructions": "How frustrated?",
                    "criteria": ["Calm", "Frustrated"]
                }
            }
        }))
        .expect("valid System One request");
    gateway_core::system_one_decisions_request_to_core(&request)
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::*;

    fn context() -> ProviderRequestContext {
        ProviderRequestContext {
            request_id: "req-decisions".to_string(),
            model_key: "jev".to_string(),
            provider_key: "decisions".to_string(),
            upstream_model: "upstream-jev".to_string(),
            owner_user_id: None,
            expected_provider_credential_id: None,
            extra_headers: Map::new(),
            extra_body: Map::from_iter([("seed".to_string(), json!(7))]),
            request_headers: BTreeMap::new(),
            compatibility: Default::default(),
        }
    }

    #[test]
    fn builds_body_in_the_upstream_format() {
        let request = system_one_test_request();
        let system_one =
            base_decisions_request_body(DecisionsWireFormat::SystemOne, &request, &context())
                .expect("System One body");
        assert_eq!(system_one["model"], "upstream-jev");
        assert_eq!(system_one["seed"], 7);
        assert_eq!(system_one["questions"]["is_urgent"]["type"], "noul");

        let openai = base_decisions_request_body(DecisionsWireFormat::OpenAi, &request, &context())
            .expect("OpenAI body");
        assert_eq!(openai["model"], "upstream-jev");
        assert_eq!(
            openai["input"],
            "Help! My payouts have been failing for 3 days."
        );
        assert_eq!(openai["questions"][0]["name"], "department");
    }

    #[test]
    fn safety_identifier_is_rejected_before_system_one_upstreams() {
        let mut request = system_one_test_request();
        request.safety_identifier = Some("user-123".to_string());

        let error =
            base_decisions_request_body(DecisionsWireFormat::SystemOne, &request, &context())
                .expect_err("System One cannot carry safety_identifier");
        assert!(
            matches!(error, ProviderError::InvalidRequest(_)),
            "{error:?}"
        );

        let openai = base_decisions_request_body(DecisionsWireFormat::OpenAi, &request, &context())
            .expect("OpenAI carries safety_identifier");
        assert_eq!(openai["safety_identifier"], "user-123");
    }

    #[test]
    fn rejects_malformed_answers_keeping_billed_usage() {
        let request = system_one_test_request();
        let valid = json!({
            "model": "jev",
            "answers": {
                "is_urgent": {"type": "noul", "noul": 0.9},
                "department": {"type": "choice", "choice": "billing"},
                "frustration": {"type": "score", "score": 0.8}
            }
        });
        let parsed =
            parse_decisions_response(DecisionsWireFormat::SystemOne, valid.clone(), &request)
                .expect("valid");
        assert_eq!(parsed.body, valid);
        assert_eq!(parsed.format, DecisionsWireFormat::SystemOne);

        let mut billed = valid;
        billed["usage"] = json!({"input_tokens": 12, "output_tokens": 3});
        let error = parse_decisions_response(DecisionsWireFormat::OpenAi, billed, &request)
            .expect_err("wrong format");
        let ProviderError::PartialUsage {
            source,
            provider_usage,
        } = error
        else {
            panic!("expected partial usage, got {error:?}");
        };
        assert!(matches!(*source, ProviderError::Transport(_)), "{source:?}");
        assert_eq!(
            provider_usage,
            Some(json!({"input_tokens": 12, "output_tokens": 3}))
        );
    }
}
