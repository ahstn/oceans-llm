//! Messages token counting.
//!
//! Requests authenticate, resolve and route exactly like `/v1/messages`, so a
//! count reflects the route the next Messages call would use. Counting generates
//! nothing, so it writes no usage-ledger row, charges no budget and creates no
//! request log. It is visible in traces as the `count_tokens` provider operation.
use super::*;
use axum::extract::rejection::JsonRejection;

use crate::http::request_body::DEFAULT_MAX_BYTES;

const COUNT_TOKENS_ROUTE: &str = "/v1/messages/count_tokens";
/// Generation-only controls. They never change the count, and the inference
/// mappers reject some of their values, so they are dropped before mapping.
const GENERATION_FIELDS: &[&str] = &[
    "temperature",
    "top_p",
    "top_k",
    "stop",
    "stop_sequences",
    "store",
    "stream_options",
    "metadata",
    "service_tier",
    "user",
];
/// Baseline `max_tokens` sent with every count. Upstream count APIs ignore it, but
/// Bedrock wraps a full InvokeModel body, which requires a valid one.
const COUNT_MAX_TOKENS: u64 = 4096;

pub async fn v1_messages_count_tokens(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    InferenceAuth(auth): InferenceAuth,
    request: Result<Json<AnthropicMessagesRequest>, JsonRejection>,
) -> Response {
    // Anthropic clients parse every failure as an error envelope, including a body
    // that is not valid JSON or has the wrong shape.
    let request = match request {
        Ok(Json(request)) => request,
        Err(rejection) => return anthropic_error_response(json_rejection_error(&rejection)),
    };
    match count_tokens_inner(state, request_id, headers, auth, request).await {
        Ok(response) => response,
        Err(error) => anthropic_error_response(error.0),
    }
}

fn json_rejection_error(rejection: &JsonRejection) -> GatewayError {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        GatewayError::PayloadTooLarge {
            limit_bytes: DEFAULT_MAX_BYTES,
        }
    } else {
        GatewayError::InvalidRequest(rejection.body_text())
    }
}

async fn count_tokens_inner(
    state: AppState,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    auth: AuthenticatedApiKey,
    request: AnthropicMessagesRequest,
) -> Result<Response, AppError> {
    let request_id = canonical_request_id(request_id)?;
    let mut core_request = anthropic_messages_request_to_core(&request);
    prepare_count_request(&mut core_request);
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;
    enforce_chat_reasoning_effort(&core_request, resolved.selection.max_reasoning_effort)?;

    let request_headers = extract_request_headers(&headers);
    let request_span = Span::current();
    record_request_span_fields(&request_span, &auth, &resolved, false, COUNT_TOKENS_ROUTE);
    let (eligible_route_count, selected) = select_provider_route(
        &state,
        &resolved,
        requirements,
        &headers,
        &request.extra,
        &request_headers,
        RoutingEndpoint::Messages,
    )
    .await?;
    // The receipt is dropped: a count is not a completed response, so it must not
    // refresh session affinity or record a response origin.
    let Some(SelectedProviderRoute {
        route,
        provider,
        expected_provider_credential_id,
        ..
    }) = selected
    else {
        return Err(AppError(no_compatible_route_error(requirements)));
    };
    record_provider_execution_span_fields(
        &request_span,
        &route.provider_key,
        provider.provider_type(),
    );

    // The prompt guard runs exactly as for `/v1/messages`, so a count cannot send
    // upstream a prompt that Messages would deny. Its decisions carry this request
    // ID; the response-guard context is unused because a count generates nothing.
    let route_key = model_route_key(
        &resolved.selection.execution_model.model_key,
        &route.provider_key,
        &route.upstream_model,
    );
    let guard_context =
        guard_typed_request(&state, &request_id, route_key, &mut core_request).await?;

    let mut context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        &auth,
        request_headers,
    );
    context.expected_provider_credential_id = expected_provider_credential_id;
    let span = provider_operation_span(
        &request_id,
        "count_tokens",
        &auth,
        &resolved,
        &route,
        provider.as_ref(),
        false,
    );
    let value = trace_provider_operation(span, provider.count_tokens(&core_request, &context))
        .await
        .map_err(|error| AppError(count_tokens_error(error, &route, provider.provider_type())))?;

    let input_tokens = value
        .get("input_tokens")
        .and_then(serde_json::Value::as_u64);
    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        provider_key = %route.provider_key,
        eligible_route_count,
        guardrails_enabled = guard_context.enabled,
        input_tokens,
        "anthropic count_tokens request completed"
    );

    let mut response = Json(value).into_response();
    if let Ok(request_id_header) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert("x-request-id", request_id_header);
    }
    Ok(response)
}

/// Shapes a Messages request so every count mapper accepts it.
///
/// A count never streams, so a stray `stream: true` must not narrow eligibility.
/// Generation controls are dropped, and the caller's output limit is replaced: a
/// limit cannot change the count, but Bedrock validates it, so `0` or a value below
/// a manual thinking budget would fail. The replacement leaves room for that budget.
fn prepare_count_request(request: &mut CoreChatRequest) {
    request.stream = false;
    for field in GENERATION_FIELDS
        .iter()
        .chain(&["max_tokens", "max_completion_tokens"])
    {
        request.extra.remove(*field);
    }
    let thinking_budget = request
        .extra
        .get("thinking")
        .and_then(|thinking| thinking.get("budget_tokens"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let max_tokens = COUNT_MAX_TOKENS.max(thinking_budget.saturating_add(1));
    request
        .extra
        .insert("max_tokens".to_string(), json!(max_tokens));
}

/// A route without an upstream count API returns a typed 501 naming the route,
/// never a guessed number.
fn count_tokens_error(
    error: ProviderError,
    route: &gateway_core::ModelRoute,
    provider_type: &str,
) -> GatewayError {
    match error {
        ProviderError::NotImplemented(reason) => GatewayError::NotImplemented(format!(
            "token counting is not available for route `{}` ({provider_type}, upstream model `{}`): {reason}",
            route.provider_key, route.upstream_model
        )),
        other => GatewayError::Provider(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(extra: serde_json::Value) -> CoreChatRequest {
        CoreChatRequest {
            model: "fast".to_string(),
            messages: Vec::new(),
            stream: true,
            extra: serde_json::from_value(extra).unwrap(),
        }
    }

    #[test]
    fn prepare_sets_max_tokens_above_a_thinking_budget() {
        let mut without = request(json!({"temperature": 0.2}));
        prepare_count_request(&mut without);
        assert!(!without.stream);
        assert_eq!(
            serde_json::to_value(&without.extra).unwrap(),
            json!({"max_tokens": COUNT_MAX_TOKENS})
        );

        let mut thinking = request(json!({
            "max_tokens": 2048,
            "thinking": {"type": "enabled", "budget_tokens": 10_000},
        }));
        prepare_count_request(&mut thinking);
        assert_eq!(thinking.extra["max_tokens"], 10_001);
    }

    #[test]
    fn prepare_replaces_unusable_caller_limits() {
        for extra in [
            json!({"max_tokens": 0}),
            json!({"max_completion_tokens": 0}),
            json!({"max_tokens": 64, "max_completion_tokens": 512}),
        ] {
            let mut request = request(extra);
            prepare_count_request(&mut request);
            assert_eq!(
                serde_json::to_value(&request.extra).unwrap(),
                json!({"max_tokens": COUNT_MAX_TOKENS})
            );
        }
    }
}
