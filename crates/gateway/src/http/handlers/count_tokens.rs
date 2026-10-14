//! Messages token counting.
//!
//! Requests authenticate, resolve and route exactly like `/v1/messages`, so a
//! count reflects the route the next Messages call would use. Counting generates
//! nothing, so it writes no usage-ledger row, charges no budget and creates no
//! request log. It is visible in traces as the `count_tokens` provider operation.
use super::*;
use crate::http::inference_guardrails::redact_prompt_body;

const COUNT_TOKENS_ROUTE: &str = "/v1/messages/count_tokens";
const SAMPLING_FIELDS: &[&str] = &["temperature", "top_p", "top_k", "stop_sequences"];

pub async fn v1_messages_count_tokens(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    InferenceAuth(auth): InferenceAuth,
    Json(request): Json<AnthropicMessagesRequest>,
) -> Response {
    match count_tokens_inner(state, request_id, headers, auth, request).await {
        Ok(response) => response,
        Err(error) => anthropic_error_response(error.0),
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
    // A count never streams, so a stray `stream: true` must not narrow eligibility.
    core_request.stream = false;
    // Sampling settings never change the count, and some models reject them during mapping.
    for field in SAMPLING_FIELDS {
        core_request.extra.remove(*field);
    }
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

    // Secrets are still redacted before the prompt leaves the gateway. Decisions are
    // not persisted because there is no request log for them to attach to.
    let route_key = model_route_key(
        &resolved.selection.execution_model.model_key,
        &route.provider_key,
        &route.upstream_model,
    );
    let redaction_count = redact_count_tokens_request(&state, &route_key, &mut core_request)?;

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
        redaction_count,
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

fn redact_count_tokens_request(
    state: &AppState,
    route_key: &str,
    request: &mut CoreChatRequest,
) -> Result<usize, AppError> {
    let mut redacted = serde_json::to_value(&*request).map_err(|error| {
        AppError(GatewayError::Internal(format!(
            "failed to encode redaction request: {error}"
        )))
    })?;
    let decisions = redact_prompt_body(state, route_key, &mut redacted);
    *request = serde_json::from_value(redacted).map_err(|error| {
        AppError(GatewayError::Internal(format!(
            "secret redaction produced an invalid request: {error}"
        )))
    })?;
    Ok(decisions.len())
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
