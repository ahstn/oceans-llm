//! Messages request decoding and native-versus-translated provider dispatch.
use super::inference::{InferenceExecution, InferenceRequest, InferenceResponseFormat};
use super::*;

pub async fn v1_messages(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    policy: RequestPolicyContext,
    InferenceAuth(auth): InferenceAuth,
    Json(request): Json<AnthropicMessagesRequest>,
) -> Response {
    match v1_messages_inner(state, request_id, headers, policy, auth, request).await {
        Ok(response) => response,
        Err(error) => anthropic_error_response(error.0),
    }
}

pub(super) async fn v1_messages_inner(
    state: AppState,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    policy: RequestPolicyContext,
    auth: AuthenticatedApiKey,
    request: AnthropicMessagesRequest,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_id = canonical_request_id(request_id)?;
    let mut core_request = anthropic_messages_request_to_core(&request);
    let log_request = core_chat_request_to_openai(&core_request);
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;
    let rate_limit_team = enforce_chat_rate_limits(
        &state,
        &policy,
        &auth,
        &resolved,
        core_request.stream,
        request_started_at,
    )
    .await?;
    enforce_chat_reasoning_effort(&core_request, resolved.selection.max_reasoning_effort)
        .inspect_err(|rejection| resolved.selection.log_reasoning_effort_rejection(rejection))
        .map_err(GatewayError::from)?;

    let request_headers = extract_request_headers(&headers);
    let request_tags = extract_request_tags(&headers)?;
    let request_log_context = state.service.begin_chat_request_log(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &resolved.selection.execution_model.model_key,
        &log_request,
        &request_headers,
        request_tags,
    );
    record_request_span_fields(
        &Span::current(),
        &auth,
        &resolved,
        core_request.stream,
        "/v1/messages",
    );
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

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count,
        stream = core_request.stream,
        required_capabilities = ?requirements.required_capability_names(),
        "anthropic messages request resolved"
    );

    let execution = InferenceRequest {
        state: &state,
        auth: &auth,
        rate_limit_team,
        resolved: &resolved,
        request_id: &request_id,
        request_started_at,
        stream: core_request.stream,
        endpoint: RoutingEndpoint::Messages,
        request_log_context,
    }
    .prepare(selected, requirements, &mut core_request)
    .await?;
    let mut context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &execution.route,
        &auth,
        request_headers,
    );
    context.chat_protocol = gateway_core::ChatRequestProtocol::AnthropicMessages;
    context.expected_provider_credential_id = execution.expected_provider_credential_id;
    let native_messages = execution.provider.supports_native_messages(&context);
    if !native_messages && let Some(system) = core_request.extra.remove("system") {
        core_request.messages.insert(
            0,
            gateway_core::CoreChatMessage {
                role: "system".to_string(),
                content: system,
                name: None,
                extra: Default::default(),
            },
        );
    }

    if core_request.stream {
        return stream_response(
            &execution,
            &core_request,
            &context,
            requirements,
            native_messages,
        )
        .await;
    }

    let provider_execution_span = provider_operation_span(
        &request_id,
        "chat",
        &auth,
        &resolved,
        &execution.route,
        execution.provider.as_ref(),
        false,
    );
    let attempt_started_at = gateway_service::offset_now();
    let response = match trace_provider_operation(provider_execution_span, async {
        if native_messages {
            execution.provider.messages(&core_request, &context).await
        } else {
            execution
                .provider
                .chat_completions(&core_request, &context)
                .await
                .map(|body| gateway_core::NativeMessagesResponse {
                    body,
                    headers: Default::default(),
                })
        }
    })
    .await
    {
        Ok(response) => response,
        Err(error) => {
            return Err(execution
                .non_stream_provider_failure(error, requirements, attempt_started_at)
                .await);
        }
    };
    let format = if native_messages {
        InferenceResponseFormat::NativeMessages(response.headers)
    } else {
        InferenceResponseFormat::TranslatedMessages
    };
    execution
        .complete_response(
            normalize_response_model(response.body, &resolved.selection.requested_model.model_key),
            attempt_started_at,
            format,
        )
        .await
}

async fn stream_response(
    execution: &InferenceExecution<'_>,
    core_request: &CoreChatRequest,
    context: &ProviderRequestContext,
    requirements: CoreRequestRequirements,
    native_messages: bool,
) -> Result<Response, AppError> {
    let request = &execution.request;
    let mut stream_trace = StreamTrace::new(
        "chat",
        request.request_id,
        &execution.route,
        execution.provider.as_ref(),
        Instant::now(),
    );
    let provider_execution_span = provider_operation_span(
        request.request_id,
        "chat",
        request.auth,
        request.resolved,
        &execution.route,
        execution.provider.as_ref(),
        true,
    );
    let attempt_started_at = gateway_service::offset_now();
    let response = match trace_provider_operation(provider_execution_span, async {
        if native_messages {
            execution
                .provider
                .messages_stream(core_request, context)
                .await
        } else {
            execution
                .provider
                .chat_completions_stream(core_request, context)
                .await
                .map(|body| gateway_core::NativeMessagesResponse {
                    body,
                    headers: Default::default(),
                })
        }
    })
    .await
    {
        Ok(response) => response,
        Err(error) => {
            stream_trace.finish("stream_start_error", Some("stream_start_error"));
            let (gateway_error, attempt) = guarded_provider_error_attempt(
                request.state,
                &execution.guard_context,
                &request.request_log_context,
                &execution.route,
                RequestAttemptStatus::StreamStartError,
                true,
                attempt_started_at,
                error,
                requirements,
            )
            .await;
            execution
                .record_stream_start_failure(&gateway_error, attempt, true)
                .await;
            return Err(AppError(gateway_error));
        }
    };
    let format = if native_messages {
        InferenceResponseFormat::NativeMessages(response.headers)
    } else {
        InferenceResponseFormat::TranslatedMessages
    };
    execution
        .stream_response(
            response.body,
            attempt_started_at,
            stream_trace,
            "failed to build anthropic messages streaming response",
            format,
        )
        .await
}
