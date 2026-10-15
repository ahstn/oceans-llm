mod auth;
mod inference;
#[cfg(test)]
mod inference_tests;
#[cfg(test)]
mod model_metadata_tests;
mod routing;
#[cfg(test)]
mod routing_tests;

use auth::InferenceAuth;
use gateway_service::routing::{RoutingEndpoint, RoutingReceipt};
use routing::{SelectedProviderRoute, no_compatible_route_error, select_provider_route};

use std::{collections::BTreeMap, sync::Arc, time::Instant};

use axum::{
    Json,
    body::Body,
    extract::{Extension, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use futures_util::{StreamExt, stream as futures_stream};
use gateway_core::{
    AnthropicMessagesRequest, AuthenticatedApiKey, ChatCompletionsRequest, CoreChatRequest,
    CoreRequestRequirements, CredentialHeaderPolicy, DecisionsRequest, EmbeddingsRequest,
    GatewayError, ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
    RequestAttemptRecord, RequestAttemptStatus, RequestToolCardinality, ResponsesRequest,
    anthropic_messages_request_to_core, core_chat_request_to_openai, enforce_chat_reasoning_effort,
    enforce_responses_reasoning_effort, openai_chat_request_to_core,
    openai_decisions_request_to_core, openai_embeddings_request_to_core,
    openai_responses_request_to_core, protocol::anthropic::anthropic_message_from_openai_chat,
};
use gateway_service::{
    McpAccess, McpTokenOverhead, McpTokenOverheadInput, RequestLogContext, RequestLogIconMetadata,
    ResolvedProviderConnection, resolve_model_icon_key, resolve_provider_display_from_parts,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tower_http::request_id::RequestId;
use tracing::{Span, field};

use crate::http::{
    anthropic_stream::anthropic_messages_stream_from_openai,
    credentials::authenticate_gateway_key,
    error::AppError,
    inference_guardrails::{
        GuardStreamError, InferenceGuardContext, guard_model_response, guard_prompt, guard_stream,
        model_route_key, redact_prompt,
    },
    request_tags::extract_request_tags,
    request_tracing::{StreamTrace, provider_operation_span, trace_provider_operation},
    response_headers,
    state::{AppGatewayService, AppState},
};
use crate::observability::{ChatMetricLabels, ChatRequestMetric};

async fn guard_typed_request<T>(
    state: &AppState,
    request_id: &str,
    route_key: String,
    request: &mut T,
) -> Result<InferenceGuardContext, AppError>
where
    T: Serialize + DeserializeOwned,
{
    let mut guarded = serde_json::to_value(&*request).map_err(|error| {
        AppError(GatewayError::Internal(format!(
            "failed to encode guardrail request: {error}"
        )))
    })?;
    let context = guard_prompt(state, request_id, route_key, &mut guarded)
        .await
        .map_err(AppError)?;
    *request = serde_json::from_value(guarded).map_err(|error| {
        AppError(GatewayError::Internal(format!(
            "guardrail transformation produced an invalid request: {error}"
        )))
    })?;
    Ok(context)
}

/// Redacts prompt secrets in a typed request without running the other prompt evaluators.
async fn redact_typed_request<T>(
    state: &AppState,
    request_id: &str,
    route_key: &str,
    request: &mut T,
) -> Result<(), AppError>
where
    T: Serialize + DeserializeOwned,
{
    let mut redacted = serde_json::to_value(&*request).map_err(|error| {
        AppError(GatewayError::Internal(format!(
            "failed to encode redaction request: {error}"
        )))
    })?;
    redact_prompt(state, request_id, route_key, &mut redacted).await;
    *request = serde_json::from_value(redacted).map_err(|error| {
        AppError(GatewayError::Internal(format!(
            "secret redaction produced an invalid request: {error}"
        )))
    })?;
    Ok(())
}

pub async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn readyz(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    state.service.check_readiness().await?;
    Ok(Json(json!({ "status": "ready" })))
}

pub async fn api_health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "service": "gateway",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn v1_model_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<gateway_service::model_metadata::ModelMetadataResponse>, AppError> {
    let auth =
        authenticate_gateway_key(&state, &headers, CredentialHeaderPolicy::BearerOrXApiKey).await?;
    Ok(Json(state.service.model_metadata_for_api_key(&auth).await?))
}

pub async fn v1_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<gateway_service::model_metadata::ModelsListResponse>, AppError> {
    let auth =
        authenticate_gateway_key(&state, &headers, CredentialHeaderPolicy::BearerOrXApiKey).await?;
    Ok(Json(state.service.models_list_for_api_key(&auth).await?))
}

pub async fn v1_messages(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    InferenceAuth(auth): InferenceAuth,
    Json(request): Json<AnthropicMessagesRequest>,
) -> Response {
    match v1_messages_inner(state, request_id, headers, auth, request).await {
        Ok(response) => response,
        Err(error) => anthropic_error_response(error.0),
    }
}

async fn v1_messages_inner(
    state: AppState,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
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
    enforce_chat_reasoning_effort(&core_request, resolved.selection.max_reasoning_effort)?;

    let request_headers = extract_request_headers(&headers);
    let request_tags = extract_request_tags(&headers)?;
    let mut request_log_context = state.service.begin_chat_request_log(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &resolved.selection.execution_model.model_key,
        &log_request,
        &request_headers,
        request_tags,
    );
    let request_span = Span::current();
    record_request_span_fields(
        &request_span,
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

    let SelectedProviderRoute {
        route,
        provider,
        receipt,
        expected_provider_credential_id,
    } = match selected {
        Some(selection) => selection,
        None => {
            let error = no_compatible_route_error(requirements);
            return Err(AppError(error));
        }
    };

    let icon_metadata = request_log_icon_metadata(
        &route,
        resolved.provider_connections.get(&route.provider_key),
        &resolved.selection.execution_model.model_key,
        &resolved.selection.requested_model.model_key,
    );
    best_effort_record_mcp_request_telemetry(
        &state,
        &auth,
        &mut request_log_context,
        &route,
        resolved.provider_connections.get(&route.provider_key),
    )
    .await;
    let labels = ChatMetricLabels {
        requested_model: &resolved.selection.requested_model.model_key,
        resolved_model: &resolved.selection.execution_model.model_key,
        provider_key: &route.provider_key,
        stream: core_request.stream,
    };
    record_provider_execution_span_fields(
        &request_span,
        &route.provider_key,
        provider.provider_type(),
    );

    let route_key = model_route_key(
        &resolved.selection.execution_model.model_key,
        &route.provider_key,
        &route.upstream_model,
    );
    let guard_context =
        match guard_typed_request(&state, &request_id, route_key, &mut core_request).await {
            Ok(context) => context,
            Err(AppError(error)) => {
                record_guarded_pre_provider_failure(
                    &state,
                    &auth,
                    &request_log_context,
                    &route,
                    icon_metadata,
                    request_started_at,
                    &labels,
                    &error,
                )
                .await;
                return Err(AppError(error));
            }
        };

    if let Err(error) = state
        .service
        .enforce_pre_provider_budget(
            &auth,
            &request_id,
            Some(resolved.selection.execution_model.id),
            Some(route.upstream_model.as_str()),
            OffsetDateTime::now_utc(),
        )
        .await
    {
        return Err(AppError(error));
    }

    let mut context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        &auth,
        request_headers,
    );
    context.expected_provider_credential_id = expected_provider_credential_id;

    if core_request.stream {
        return anthropic_messages_stream_response(
            &state,
            &auth,
            request_started_at,
            &request_id,
            &resolved,
            &request_log_context,
            &route,
            provider,
            &core_request,
            &context,
            icon_metadata,
            requirements,
            &guard_context,
            receipt.as_ref(),
        )
        .await;
    }

    let provider_execution_span = provider_operation_span(
        &request_id,
        "chat",
        &auth,
        &resolved,
        &route,
        provider.as_ref(),
        false,
    );
    let attempt_started_at = gateway_service::offset_now();
    let mut openai_value = match trace_provider_operation(
        provider_execution_span,
        provider.chat_completions(&core_request, &context),
    )
    .await
    {
        Ok(value) => normalize_response_model(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            let (error, attempt) = guarded_provider_error_attempt(
                &state,
                &guard_context,
                &request_log_context,
                &route,
                RequestAttemptStatus::ProviderError,
                false,
                attempt_started_at,
                error,
                requirements,
            )
            .await;
            best_effort_log_non_stream_failure(
                &state.service,
                &auth,
                &request_log_context,
                &route.provider_key,
                icon_metadata.clone(),
                latency_ms_since(request_started_at),
                &error,
                vec![attempt],
            )
            .await;
            state.metrics.record_chat_request(&ChatRequestMetric {
                labels: labels.clone(),
                status_code: i64::from(error.http_status_code()),
                outcome: error.error_type(),
                latency_seconds: latency_seconds_since(request_started_at),
            });
            return Err(AppError(error));
        }
    };
    finalize_successful_usage_accounting(
        &state,
        UsageAccountingContext {
            auth: &auth,
            model: &resolved.selection.execution_model,
            route: &route,
            request_id: &request_id,
            labels: labels.clone(),
            operation: "anthropic_messages",
        },
        usage_value_from_response(&openai_value),
    )
    .await;
    let completion = async {
        guard_model_response(&state, &guard_context, &mut openai_value).await?;
        if let Some(receipt) = &receipt {
            receipt.complete(state.store.as_ref(), None).await?;
        }
        Ok::<(), GatewayError>(())
    }
    .await;
    if let Err(error) = completion {
        record_post_provider_failure(
            &state,
            &auth,
            &request_log_context,
            &route,
            icon_metadata.clone(),
            request_started_at,
            attempt_started_at,
            &labels,
            &error,
        )
        .await;
        return Err(AppError(error));
    }
    let value = anthropic_message_from_openai_chat(
        &openai_value,
        &resolved.selection.requested_model.model_key,
    );
    let attempt = success_attempt(&request_log_context, &route, false, attempt_started_at);
    let tool_cardinality = tool_cardinality_with_invoked(&request_log_context, &openai_value);
    best_effort_log_non_stream_success(
        &state.service,
        &auth,
        &request_log_context,
        &route.provider_key,
        icon_metadata,
        latency_ms_since(request_started_at),
        tool_cardinality.invoked_tool_count.unwrap_or(0),
        &openai_value,
        vec![attempt],
    )
    .await;
    state.metrics.record_chat_request(&ChatRequestMetric {
        labels,
        status_code: 200,
        outcome: "success",
        latency_seconds: latency_seconds_since(request_started_at),
    });
    let mut response = Json(value).into_response();
    if let Ok(request_id_header) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert("x-request-id", request_id_header);
    }
    Ok(response)
}

fn anthropic_error_response(error: GatewayError) -> Response {
    let status =
        StatusCode::from_u16(error.http_status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = Json(json!({
        "type": "error",
        "error": {
            "type": error.error_type(),
            "message": error.to_string(),
            "code": error.error_code(),
        }
    }))
    .into_response();
    *response.status_mut() = status;
    if let Some(retry_after) = crate::http::error::upstream_retry_after(&error) {
        response
            .headers_mut()
            .insert(axum::http::header::RETRY_AFTER, retry_after);
    }
    response
}

pub async fn v1_chat_completions(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    InferenceAuth(auth): InferenceAuth,
    Json(request): Json<ChatCompletionsRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_id = canonical_request_id(request_id)?;
    let mut core_request = openai_chat_request_to_core(&request);
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;
    enforce_chat_reasoning_effort(&core_request, resolved.selection.max_reasoning_effort)?;

    let request_headers = extract_request_headers(&headers);
    let request_tags = extract_request_tags(&headers)?;
    let request_log_context = state.service.begin_chat_request_log(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &resolved.selection.execution_model.model_key,
        &request,
        &request_headers,
        request_tags,
    );
    let request_span = Span::current();
    record_request_span_fields(
        &request_span,
        &auth,
        &resolved,
        core_request.stream,
        "/v1/chat/completions",
    );
    let (eligible_route_count, selected) = select_provider_route(
        &state,
        &resolved,
        requirements,
        &headers,
        &request.extra,
        &request_headers,
        RoutingEndpoint::ChatCompletions,
    )
    .await?;

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count,
        stream = core_request.stream,
        required_capabilities = ?requirements.required_capability_names(),
        "chat completion request resolved"
    );

    let execution = inference::InferenceRequest {
        state: &state,
        auth: &auth,
        resolved: &resolved,
        request_id: &request_id,
        request_started_at,
        stream: core_request.stream,
        request_log_context,
    }
    .prepare(selected, requirements, &mut core_request)
    .await?;
    let route = &execution.route;
    let provider = &execution.provider;
    let guard_context = &execution.guard_context;
    let request_log_context = &execution.request.request_log_context;

    let mut context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        route,
        &auth,
        request_headers,
    );
    context.expected_provider_credential_id = execution.expected_provider_credential_id;

    if core_request.stream {
        let stream_started_at = Instant::now();
        let mut stream_trace = StreamTrace::new(
            "chat",
            &request_id,
            route,
            provider.as_ref(),
            stream_started_at,
        );
        let provider_execution_span = provider_operation_span(
            &request_id,
            "chat",
            &auth,
            &resolved,
            route,
            provider.as_ref(),
            true,
        );
        let attempt_started_at = gateway_service::offset_now();
        let stream = match trace_provider_operation(
            provider_execution_span,
            provider.chat_completions_stream(&core_request, &context),
        )
        .await
        {
            Ok(stream) => stream,
            Err(error) => {
                stream_trace.finish("stream_start_error", Some("stream_start_error"));
                let (gateway_error, attempt) = guarded_provider_error_attempt(
                    &state,
                    guard_context,
                    request_log_context,
                    route,
                    RequestAttemptStatus::StreamStartError,
                    true,
                    attempt_started_at,
                    error,
                    requirements,
                )
                .await;
                tracing::warn!(
                    request_id = %request_id,
                    provider_key = %route.provider_key,
                    termination_reason = "provider_stream_start_error",
                    error_code = %gateway_error.error_code(),
                    "chat completion stream start failed"
                );
                execution
                    .record_stream_start_failure(&gateway_error, attempt, true)
                    .await;
                return Err(AppError(gateway_error));
            }
        };
        return execution
            .stream_response(
                stream,
                attempt_started_at,
                stream_trace,
                "failed to build streaming response",
            )
            .await;
    }

    let provider_execution_span = provider_operation_span(
        &request_id,
        "chat",
        &auth,
        &resolved,
        route,
        provider.as_ref(),
        false,
    );
    let attempt_started_at = gateway_service::offset_now();
    let value = match trace_provider_operation(
        provider_execution_span,
        provider.chat_completions(&core_request, &context),
    )
    .await
    {
        Ok(value) => normalize_response_model(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            return Err(execution
                .non_stream_provider_failure(error, requirements, attempt_started_at)
                .await);
        }
    };
    execution.complete_response(value, attempt_started_at).await
}

pub async fn v1_responses(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    InferenceAuth(auth): InferenceAuth,
    Json(request): Json<ResponsesRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_id = canonical_request_id(request_id)?;
    let mut core_request = openai_responses_request_to_core(&request);
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;
    enforce_responses_reasoning_effort(&core_request, resolved.selection.max_reasoning_effort)?;

    let request_headers = extract_request_headers(&headers);
    let request_tags = extract_request_tags(&headers)?;
    let request_log_context = state.service.begin_responses_request_log(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &resolved.selection.execution_model.model_key,
        &request,
        &request_headers,
        request_tags,
    );
    let request_span = Span::current();
    record_request_span_fields(
        &request_span,
        &auth,
        &resolved,
        core_request.stream,
        "/v1/responses",
    );
    let (eligible_route_count, selected) = select_provider_route(
        &state,
        &resolved,
        requirements,
        &headers,
        &request.extra,
        &request_headers,
        RoutingEndpoint::Responses,
    )
    .await?;

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count,
        stream = core_request.stream,
        required_capabilities = ?requirements.required_capability_names(),
        "responses request resolved"
    );

    let execution = inference::InferenceRequest {
        state: &state,
        auth: &auth,
        resolved: &resolved,
        request_id: &request_id,
        request_started_at,
        stream: core_request.stream,
        request_log_context,
    }
    .prepare(selected, requirements, &mut core_request)
    .await?;
    let route = &execution.route;
    let provider = &execution.provider;
    let guard_context = &execution.guard_context;
    let request_log_context = &execution.request.request_log_context;

    let mut context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        route,
        &auth,
        request_headers,
    );
    context.expected_provider_credential_id = execution.expected_provider_credential_id;

    if core_request.stream {
        let stream_started_at = Instant::now();
        let mut stream_trace = StreamTrace::new(
            "responses",
            &request_id,
            route,
            provider.as_ref(),
            stream_started_at,
        );
        let provider_execution_span = provider_operation_span(
            &request_id,
            "responses",
            &auth,
            &resolved,
            route,
            provider.as_ref(),
            true,
        );
        let attempt_started_at = gateway_service::offset_now();
        let stream = match trace_provider_operation(
            provider_execution_span,
            provider.responses_stream(&core_request, &context),
        )
        .await
        {
            Ok(stream) => stream,
            Err(error) => {
                stream_trace.finish("stream_start_error", Some("stream_start_error"));
                let (gateway_error, attempt) = guarded_provider_error_attempt(
                    &state,
                    guard_context,
                    request_log_context,
                    route,
                    RequestAttemptStatus::StreamStartError,
                    true,
                    attempt_started_at,
                    error,
                    requirements,
                )
                .await;
                tracing::warn!(
                    request_id = %request_id,
                    provider_key = %route.provider_key,
                    termination_reason = "provider_responses_stream_start_error",
                    error_code = %gateway_error.error_code(),
                    "responses stream start failed"
                );
                execution
                    .record_stream_start_failure(&gateway_error, attempt, request.stream)
                    .await;
                return Err(AppError(gateway_error));
            }
        };
        return execution
            .stream_response(
                stream,
                attempt_started_at,
                stream_trace,
                "failed to build responses streaming response",
            )
            .await;
    }

    let provider_execution_span = provider_operation_span(
        &request_id,
        "responses",
        &auth,
        &resolved,
        route,
        provider.as_ref(),
        false,
    );
    let attempt_started_at = gateway_service::offset_now();
    let value = match trace_provider_operation(
        provider_execution_span,
        provider.responses(&core_request, &context),
    )
    .await
    {
        Ok(value) => normalize_response_model(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            return Err(execution
                .non_stream_provider_failure(error, requirements, attempt_started_at)
                .await);
        }
    };
    execution.complete_response(value, attempt_started_at).await
}

pub async fn v1_embeddings(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    InferenceAuth(auth): InferenceAuth,
    Json(mut request): Json<EmbeddingsRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_span = Span::current();
    let request_id = canonical_request_id(request_id)?;
    let core_request = openai_embeddings_request_to_core(&request);
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;
    record_request_span_fields(&request_span, &auth, &resolved, false, "/v1/embeddings");
    let request_headers = extract_request_headers(&headers);
    let request_tags = extract_request_tags(&headers)?;
    let mut request_log_context = state.service.begin_embeddings_request_log(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &resolved.selection.execution_model.model_key,
        &request,
        &request_headers,
        request_tags,
    );
    let (eligible_route_count, selected) = select_provider_route(
        &state,
        &resolved,
        requirements,
        &headers,
        &request.extra,
        &request_headers,
        RoutingEndpoint::Embeddings,
    )
    .await?;

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count,
        required_capabilities = ?requirements.required_capability_names(),
        "embeddings request resolved"
    );

    let SelectedProviderRoute {
        route,
        provider,
        receipt: _,
        expected_provider_credential_id,
    } = match selected {
        Some(selection) => selection,
        None => {
            return Err(AppError(no_compatible_route_error(requirements)));
        }
    };
    record_provider_execution_span_fields(
        &request_span,
        &route.provider_key,
        provider.provider_type(),
    );
    let icon_metadata = request_log_icon_metadata(
        &route,
        resolved.provider_connections.get(&route.provider_key),
        &resolved.selection.execution_model.model_key,
        &resolved.selection.requested_model.model_key,
    );
    best_effort_record_mcp_request_telemetry(
        &state,
        &auth,
        &mut request_log_context,
        &route,
        resolved.provider_connections.get(&route.provider_key),
    )
    .await;
    let labels = ChatMetricLabels {
        requested_model: &resolved.selection.requested_model.model_key,
        resolved_model: &resolved.selection.execution_model.model_key,
        provider_key: &route.provider_key,
        stream: false,
    };
    let route_key = model_route_key(
        &resolved.selection.execution_model.model_key,
        &route.provider_key,
        &route.upstream_model,
    );
    let guard_context =
        match guard_typed_request(&state, &request_id, route_key, &mut request).await {
            Ok(context) => context,
            Err(AppError(error)) => {
                record_guarded_pre_provider_failure(
                    &state,
                    &auth,
                    &request_log_context,
                    &route,
                    icon_metadata,
                    request_started_at,
                    &labels,
                    &error,
                )
                .await;
                return Err(AppError(error));
            }
        };
    let core_request = openai_embeddings_request_to_core(&request);

    state
        .service
        .enforce_pre_provider_budget(
            &auth,
            &request_id,
            Some(resolved.selection.execution_model.id),
            Some(route.upstream_model.as_str()),
            OffsetDateTime::now_utc(),
        )
        .await?;

    let mut context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        &auth,
        request_headers,
    );
    context.expected_provider_credential_id = expected_provider_credential_id;

    let attempt_started_at = gateway_service::offset_now();
    let provider_execution_span = provider_operation_span(
        &request_id,
        "embeddings",
        &auth,
        &resolved,
        &route,
        provider.as_ref(),
        false,
    );
    let value = match trace_provider_operation(
        provider_execution_span,
        provider.embeddings(&core_request, &context),
    )
    .await
    {
        Ok(value) => normalize_response_model(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            let (provider_error, partial_provider_usage) = split_partial_provider_error(error);
            if let Some(provider_usage) = partial_provider_usage {
                finalize_successful_usage_accounting(
                    &state,
                    UsageAccountingContext {
                        auth: &auth,
                        model: &resolved.selection.execution_model,
                        route: &route,
                        request_id: &request_id,
                        labels: labels.clone(),
                        operation: "embeddings",
                    },
                    provider_usage,
                )
                .await;
            }

            let (error, attempt) = guarded_provider_error_attempt(
                &state,
                &guard_context,
                &request_log_context,
                &route,
                RequestAttemptStatus::ProviderError,
                false,
                attempt_started_at,
                provider_error,
                requirements,
            )
            .await;
            best_effort_log_non_stream_failure(
                &state.service,
                &auth,
                &request_log_context,
                &route.provider_key,
                icon_metadata.clone(),
                latency_ms_since(request_started_at),
                &error,
                vec![attempt],
            )
            .await;
            return Err(AppError(error));
        }
    };
    let attempt = success_attempt(&request_log_context, &route, false, attempt_started_at);

    finalize_successful_usage_accounting(
        &state,
        UsageAccountingContext {
            auth: &auth,
            model: &resolved.selection.execution_model,
            route: &route,
            request_id: &request_id,
            labels: labels.clone(),
            operation: "embeddings",
        },
        usage_value_from_response(&value),
    )
    .await;
    best_effort_log_non_stream_success(
        &state.service,
        &auth,
        &request_log_context,
        &route.provider_key,
        icon_metadata,
        latency_ms_since(request_started_at),
        0,
        &value,
        vec![attempt],
    )
    .await;

    let response = Json(value).into_response();
    Ok(response)
}

pub async fn v1_decisions(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    InferenceAuth(auth): InferenceAuth,
    Json(request): Json<DecisionsRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_span = Span::current();
    let request_id = canonical_request_id(request_id)?;
    let mut core_request = openai_decisions_request_to_core(&request);
    if let Some(message) = core_request.validation_error() {
        return Err(AppError(GatewayError::InvalidRequest(message)));
    }
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;
    record_request_span_fields(&request_span, &auth, &resolved, false, "/v1/decisions");
    let request_headers = extract_request_headers(&headers);
    let request_tags = extract_request_tags(&headers)?;
    let mut request_log_context = state.service.begin_decisions_request_log(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &resolved.selection.execution_model.model_key,
        &request,
        &request_headers,
        request_tags,
    );
    let (eligible_route_count, selected) = select_provider_route(
        &state,
        &resolved,
        requirements,
        &headers,
        &request.extra,
        &request_headers,
        RoutingEndpoint::Decisions,
    )
    .await?;

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count,
        required_capabilities = ?requirements.required_capability_names(),
        "decisions request resolved"
    );

    let SelectedProviderRoute {
        route,
        provider,
        receipt: _,
        expected_provider_credential_id,
    } = match selected {
        Some(selection) => selection,
        None => {
            return Err(AppError(no_compatible_route_error(requirements)));
        }
    };
    record_provider_execution_span_fields(
        &request_span,
        &route.provider_key,
        provider.provider_type(),
    );
    let icon_metadata = request_log_icon_metadata(
        &route,
        resolved.provider_connections.get(&route.provider_key),
        &resolved.selection.execution_model.model_key,
        &resolved.selection.requested_model.model_key,
    );
    best_effort_record_mcp_request_telemetry(
        &state,
        &auth,
        &mut request_log_context,
        &route,
        resolved.provider_connections.get(&route.provider_key),
    )
    .await;
    let labels = ChatMetricLabels {
        requested_model: &resolved.selection.requested_model.model_key,
        resolved_model: &resolved.selection.execution_model.model_key,
        provider_key: &route.provider_key,
        stream: false,
    };

    // Decisions routes skip the prompt and response evaluators: `state` and
    // `questions` carry no chat-shaped text for them to inspect. Secrets are
    // still redacted before the request leaves the gateway.
    let route_key = model_route_key(
        &resolved.selection.execution_model.model_key,
        &route.provider_key,
        &route.upstream_model,
    );
    redact_typed_request(&state, &request_id, &route_key, &mut core_request).await?;

    state
        .service
        .enforce_pre_provider_budget(
            &auth,
            &request_id,
            Some(resolved.selection.execution_model.id),
            Some(route.upstream_model.as_str()),
            OffsetDateTime::now_utc(),
        )
        .await?;

    let mut context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        &auth,
        request_headers,
    );
    context.expected_provider_credential_id = expected_provider_credential_id;

    let attempt_started_at = gateway_service::offset_now();
    let provider_execution_span = provider_operation_span(
        &request_id,
        "decisions",
        &auth,
        &resolved,
        &route,
        provider.as_ref(),
        false,
    );
    let value = match trace_provider_operation(
        provider_execution_span,
        provider.decisions(&core_request, &context),
    )
    .await
    {
        Ok(value) => normalize_response_model(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            let (provider_error, partial_provider_usage) = split_partial_provider_error(error);
            if let Some(provider_usage) = partial_provider_usage {
                finalize_successful_usage_accounting(
                    &state,
                    UsageAccountingContext {
                        auth: &auth,
                        model: &resolved.selection.execution_model,
                        route: &route,
                        request_id: &request_id,
                        labels: labels.clone(),
                        operation: "decisions",
                    },
                    provider_usage,
                )
                .await;
            }

            let (error, attempt) = provider_error_attempt(
                &request_log_context,
                &route,
                RequestAttemptStatus::ProviderError,
                false,
                attempt_started_at,
                provider_error,
                requirements,
            );
            best_effort_log_non_stream_failure(
                &state.service,
                &auth,
                &request_log_context,
                &route.provider_key,
                icon_metadata.clone(),
                latency_ms_since(request_started_at),
                &error,
                vec![attempt],
            )
            .await;
            return Err(AppError(error));
        }
    };
    let attempt = success_attempt(&request_log_context, &route, false, attempt_started_at);

    finalize_successful_usage_accounting(
        &state,
        UsageAccountingContext {
            auth: &auth,
            model: &resolved.selection.execution_model,
            route: &route,
            request_id: &request_id,
            labels: labels.clone(),
            operation: "decisions",
        },
        usage_value_from_response(&value),
    )
    .await;
    best_effort_log_non_stream_success(
        &state.service,
        &auth,
        &request_log_context,
        &route.provider_key,
        icon_metadata,
        latency_ms_since(request_started_at),
        0,
        &value,
        vec![attempt],
    )
    .await;

    let response = Json(value).into_response();
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
async fn guarded_provider_error_attempt(
    state: &AppState,
    guard_context: &InferenceGuardContext,
    context: &gateway_service::RequestLogContext,
    route: &gateway_core::ModelRoute,
    status: RequestAttemptStatus,
    stream: bool,
    started_at: OffsetDateTime,
    error: ProviderError,
    requirements: CoreRequestRequirements,
) -> (GatewayError, RequestAttemptRecord) {
    let error = match error {
        ProviderError::UpstreamHttp {
            status: http_status,
            body,
            retry_after,
        } if guard_context.enabled => {
            let mut payload = json!({ "text": body });
            if let Err(error) = guard_model_response(state, guard_context, &mut payload).await {
                let attempt = gateway_service::build_request_attempt(
                    context,
                    route,
                    1,
                    stream,
                    started_at,
                    gateway_service::offset_now(),
                    gateway_service::failed_attempt_outcome(
                        status,
                        &error,
                        false,
                        error.to_string(),
                    ),
                );
                return (error, attempt);
            }
            ProviderError::UpstreamHttp {
                status: http_status,
                body: payload
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("upstream request failed")
                    .to_string(),
                retry_after,
            }
        }
        error => error,
    };
    provider_error_attempt(
        context,
        route,
        status,
        stream,
        started_at,
        error,
        requirements,
    )
}

fn provider_error_attempt(
    context: &gateway_service::RequestLogContext,
    route: &gateway_core::ModelRoute,
    status: RequestAttemptStatus,
    stream: bool,
    started_at: OffsetDateTime,
    error: ProviderError,
    requirements: CoreRequestRequirements,
) -> (GatewayError, RequestAttemptRecord) {
    let retryable = error.is_retryable();
    let detail = error.to_string();
    let gateway_error = map_operation_provider_error(error, requirements);
    let attempt = gateway_service::build_request_attempt(
        context,
        route,
        1,
        stream,
        started_at,
        gateway_service::offset_now(),
        gateway_service::failed_attempt_outcome(status, &gateway_error, retryable, detail),
    );
    (gateway_error, attempt)
}

fn success_attempt(
    context: &gateway_service::RequestLogContext,
    route: &gateway_core::ModelRoute,
    stream: bool,
    started_at: OffsetDateTime,
) -> RequestAttemptRecord {
    gateway_service::build_request_attempt(
        context,
        route,
        1,
        stream,
        started_at,
        gateway_service::offset_now(),
        gateway_service::successful_attempt_outcome(),
    )
}

fn guarded_failure_attempt(
    context: &gateway_service::RequestLogContext,
    route: &gateway_core::ModelRoute,
    started_at: OffsetDateTime,
) -> RequestAttemptRecord {
    let mut outcome = gateway_service::successful_attempt_outcome();
    outcome.produced_final_response = false;
    gateway_service::build_request_attempt(
        context,
        route,
        1,
        false,
        started_at,
        gateway_service::offset_now(),
        outcome,
    )
}

fn stream_failure_attempt(
    context: &gateway_service::RequestLogContext,
    route: &gateway_core::ModelRoute,
    started_at: OffsetDateTime,
    failure: &gateway_service::StreamFailureSummary,
) -> RequestAttemptRecord {
    gateway_service::build_request_attempt(
        context,
        route,
        1,
        true,
        started_at,
        gateway_service::offset_now(),
        gateway_service::RequestAttemptOutcome {
            status: RequestAttemptStatus::StreamError,
            status_code: Some(failure.status_code),
            error_code: Some(failure.error_code.clone()),
            error_detail: None,
            retryable: false,
            produced_final_response: false,
        },
    )
}

fn map_operation_provider_error(
    error: ProviderError,
    requirements: CoreRequestRequirements,
) -> GatewayError {
    match error {
        ProviderError::NotImplemented(_) => no_compatible_route_error(requirements),
        other => GatewayError::Provider(other),
    }
}

fn build_provider_context(
    request_id: &str,
    model_key: &str,
    route: &gateway_core::ModelRoute,
    auth: &AuthenticatedApiKey,
    request_headers: BTreeMap<String, String>,
) -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: request_id.to_string(),
        model_key: model_key.to_string(),
        provider_key: route.provider_key.clone(),
        upstream_model: route.upstream_model.clone(),
        owner_user_id: auth.owner_user_id,
        expected_provider_credential_id: None,
        extra_headers: route.extra_headers.clone(),
        extra_body: route.extra_body.clone(),
        request_headers,
        compatibility: route.compatibility.clone(),
    }
}

struct LoggingBodyStreamState {
    upstream: gateway_core::ProviderStream,
    service: std::sync::Arc<AppGatewayService>,
    metrics: std::sync::Arc<crate::observability::GatewayMetrics>,
    auth: AuthenticatedApiKey,
    request_log_context: gateway_service::RequestLogContext,
    requested_model_key: String,
    resolved_model_key: String,
    execution_model: gateway_core::GatewayModel,
    route: gateway_core::ModelRoute,
    routing_receipt: Option<RoutingReceipt>,
    provider_key: String,
    icon_metadata: RequestLogIconMetadata,
    started_at: Instant,
    attempt_started_at: OffsetDateTime,
    finished: bool,
    saw_terminal_event: bool,
    collector: gateway_service::StreamResponseCollector,
    stream_trace: StreamTrace,
    /// A usage-only chunk held back until a later chunk shows whether it is the last.
    held_usage_chunk: Option<axum::body::Bytes>,
    /// Events with neither output nor usage that arrived after the held usage chunk,
    /// joined in order.
    held_after_usage: bytes::BytesMut,
    /// Buffers an incomplete trailing event, so every chunk handled holds whole events.
    framer: gateway_core::SseEventFramer,
    pending: std::collections::VecDeque<Result<axum::body::Bytes, std::io::Error>>,
    cost_usd: Option<gateway_core::Money4>,
}

impl LoggingBodyStreamState {
    fn metric_labels(&self) -> ChatMetricLabels<'_> {
        ChatMetricLabels {
            requested_model: &self.requested_model_key,
            resolved_model: &self.resolved_model_key,
            provider_key: &self.provider_key,
            stream: true,
        }
    }
}

impl Drop for LoggingBodyStreamState {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.stream_trace
            .finish("client_cancelled", Some("client_cancelled"));
        record_cancelled_stream_metrics(self);
        spawn_cancelled_stream_log(self);
    }
}

fn record_cancelled_stream_metrics(state: &LoggingBodyStreamState) {
    let labels = state.metric_labels();
    state.metrics.record_chat_request(&ChatRequestMetric {
        labels: labels.clone(),
        status_code: 499,
        outcome: "client_cancelled",
        latency_seconds: latency_seconds_since(state.started_at),
    });
    state.metrics.record_tool_cardinality(
        &labels,
        state.request_log_context.operation,
        &RequestToolCardinality {
            invoked_tool_count: Some(state.collector.invoked_tool_count()),
            ..state.request_log_context.tool_cardinality
        },
    );
}

fn spawn_cancelled_stream_log(state: &LoggingBodyStreamState) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::warn!(
            request_id = %state.request_log_context.request_id,
            provider_key = %state.provider_key,
            "cannot persist cancelled stream outside a Tokio runtime"
        );
        return;
    };

    let service = state.service.clone();
    let auth = state.auth.clone();
    let context = state.request_log_context.clone();
    let stream_result = gateway_service::StreamLogResultInput {
        provider_key: state.provider_key.clone(),
        icon_metadata: state.icon_metadata.clone(),
        latency_ms: latency_ms_since(state.started_at),
        collector: state.collector.clone(),
        failure: Some(gateway_service::StreamFailureSummary {
            status_code: 499,
            error_code: "client_cancelled".to_string(),
        }),
        attempts: vec![gateway_service::build_request_attempt(
            &state.request_log_context,
            &state.route,
            1,
            true,
            state.attempt_started_at,
            gateway_service::offset_now(),
            gateway_service::RequestAttemptOutcome {
                status: RequestAttemptStatus::StreamError,
                status_code: Some(499),
                error_code: Some("client_cancelled".to_string()),
                error_detail: None,
                retryable: false,
                produced_final_response: false,
            },
        )],
    };
    runtime.spawn(async move {
        best_effort_log_stream_result(&service, &auth, &context, stream_result).await;
    });
}

struct UsageAccountingContext<'a> {
    auth: &'a AuthenticatedApiKey,
    model: &'a gateway_core::GatewayModel,
    route: &'a gateway_core::ModelRoute,
    request_id: &'a str,
    labels: ChatMetricLabels<'a>,
    operation: &'static str,
}

#[allow(clippy::too_many_arguments)]
async fn record_guarded_pre_provider_failure(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    request_log_context: &RequestLogContext,
    route: &gateway_core::ModelRoute,
    icon_metadata: RequestLogIconMetadata,
    request_started_at: Instant,
    labels: &ChatMetricLabels<'_>,
    error: &GatewayError,
) {
    best_effort_log_non_stream_failure(
        &state.service,
        auth,
        request_log_context,
        &route.provider_key,
        icon_metadata,
        latency_ms_since(request_started_at),
        error,
        Vec::new(),
    )
    .await;
    state.metrics.record_chat_request(&ChatRequestMetric {
        labels: labels.clone(),
        status_code: i64::from(error.http_status_code()),
        outcome: error.error_type(),
        latency_seconds: latency_seconds_since(request_started_at),
    });
    state.metrics.record_tool_cardinality(
        labels,
        request_log_context.operation,
        &request_log_context.tool_cardinality,
    );
}

#[allow(clippy::too_many_arguments)]
async fn record_post_provider_failure(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    request_log_context: &RequestLogContext,
    route: &gateway_core::ModelRoute,
    icon_metadata: RequestLogIconMetadata,
    request_started_at: Instant,
    attempt_started_at: OffsetDateTime,
    labels: &ChatMetricLabels<'_>,
    error: &GatewayError,
) {
    best_effort_log_non_stream_failure(
        &state.service,
        auth,
        request_log_context,
        &route.provider_key,
        icon_metadata,
        latency_ms_since(request_started_at),
        error,
        vec![guarded_failure_attempt(
            request_log_context,
            route,
            attempt_started_at,
        )],
    )
    .await;
    state.metrics.record_chat_request(&ChatRequestMetric {
        labels: labels.clone(),
        status_code: i64::from(error.http_status_code()),
        outcome: error.error_type(),
        latency_seconds: latency_seconds_since(request_started_at),
    });
    state.metrics.record_tool_cardinality(
        labels,
        request_log_context.operation,
        &request_log_context.tool_cardinality,
    );
}

#[allow(clippy::too_many_arguments)]
async fn record_guarded_stream_failure(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    resolved: &gateway_service::ResolvedGatewayRequest,
    request_log_context: &RequestLogContext,
    route: &gateway_core::ModelRoute,
    icon_metadata: RequestLogIconMetadata,
    request_started_at: Instant,
    attempt_started_at: OffsetDateTime,
    error: &GatewayError,
    collector: gateway_service::StreamResponseCollector,
) {
    let labels = ChatMetricLabels {
        requested_model: &resolved.selection.requested_model.model_key,
        resolved_model: &resolved.selection.execution_model.model_key,
        provider_key: &route.provider_key,
        stream: true,
    };
    finalize_successful_usage_accounting(
        state,
        UsageAccountingContext {
            auth,
            model: &resolved.selection.execution_model,
            route,
            request_id: &request_log_context.request_id,
            labels: labels.clone(),
            operation: request_log_context.operation,
        },
        collector.usage().cloned(),
    )
    .await;
    let tool_cardinality = RequestToolCardinality {
        invoked_tool_count: Some(collector.invoked_tool_count()),
        ..request_log_context.tool_cardinality
    };
    let failure = gateway_service::StreamFailureSummary {
        status_code: error.http_status_code().into(),
        error_code: error.error_code().to_string(),
    };
    best_effort_log_stream_result(
        &state.service,
        auth,
        request_log_context,
        gateway_service::StreamLogResultInput {
            provider_key: route.provider_key.clone(),
            icon_metadata,
            latency_ms: latency_ms_since(request_started_at),
            collector,
            failure: Some(failure.clone()),
            attempts: vec![stream_failure_attempt(
                request_log_context,
                route,
                attempt_started_at,
                &failure,
            )],
        },
    )
    .await;
    state.metrics.record_chat_request(&ChatRequestMetric {
        labels: labels.clone(),
        status_code: i64::from(error.http_status_code()),
        outcome: error.error_type(),
        latency_seconds: latency_seconds_since(request_started_at),
    });
    state.metrics.record_tool_cardinality(
        &labels,
        request_log_context.operation,
        &tool_cardinality,
    );
}

#[allow(clippy::too_many_arguments)]
async fn enforce_guarded_stream_after_provider(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    resolved: &gateway_service::ResolvedGatewayRequest,
    request_log_context: &RequestLogContext,
    route: &gateway_core::ModelRoute,
    icon_metadata: RequestLogIconMetadata,
    request_started_at: Instant,
    attempt_started_at: OffsetDateTime,
    guard_context: &InferenceGuardContext,
    stream: ProviderStream,
) -> Result<ProviderStream, AppError> {
    match guard_stream(state, guard_context, stream).await {
        Ok(stream) => Ok(stream),
        Err(failure) => {
            let GuardStreamError { error, collector } = *failure;
            if let Some(collector) = collector {
                record_guarded_stream_failure(
                    state,
                    auth,
                    resolved,
                    request_log_context,
                    route,
                    icon_metadata,
                    request_started_at,
                    attempt_started_at,
                    &error,
                    collector,
                )
                .await;
            }
            Err(AppError(error))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn anthropic_messages_stream_response(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    request_started_at: Instant,
    request_id: &str,
    resolved: &gateway_service::ResolvedGatewayRequest,
    request_log_context: &RequestLogContext,
    route: &gateway_core::ModelRoute,
    provider: Arc<dyn ProviderClient>,
    core_request: &CoreChatRequest,
    context: &ProviderRequestContext,
    icon_metadata: RequestLogIconMetadata,
    requirements: CoreRequestRequirements,
    guard_context: &InferenceGuardContext,
    routing_receipt: Option<&RoutingReceipt>,
) -> Result<Response, AppError> {
    let stream_started_at = Instant::now();
    let mut stream_trace = StreamTrace::new(
        "chat",
        request_id,
        route,
        provider.as_ref(),
        stream_started_at,
    );
    let provider_execution_span = provider_operation_span(
        request_id,
        "chat",
        auth,
        resolved,
        route,
        provider.as_ref(),
        true,
    );
    let attempt_started_at = gateway_service::offset_now();
    let stream = match trace_provider_operation(
        provider_execution_span,
        provider.chat_completions_stream(core_request, context),
    )
    .await
    {
        Ok(stream) => stream,
        Err(error) => {
            stream_trace.finish("stream_start_error", Some("stream_start_error"));
            let (gateway_error, attempt) = guarded_provider_error_attempt(
                state,
                guard_context,
                request_log_context,
                route,
                RequestAttemptStatus::StreamStartError,
                true,
                attempt_started_at,
                error,
                requirements,
            )
            .await;
            best_effort_log_stream_result(
                &state.service,
                auth,
                request_log_context,
                gateway_service::StreamLogResultInput {
                    provider_key: route.provider_key.clone(),
                    icon_metadata,
                    latency_ms: latency_ms_since(request_started_at),
                    collector: state.service.new_stream_response_collector(),
                    failure: Some(gateway_service::StreamFailureSummary {
                        status_code: gateway_error.http_status_code().into(),
                        error_code: gateway_error.error_code().to_string(),
                    }),
                    attempts: vec![attempt],
                },
            )
            .await;
            return Err(AppError(gateway_error));
        }
    };
    let stream = enforce_guarded_stream_after_provider(
        state,
        auth,
        resolved,
        request_log_context,
        route,
        icon_metadata.clone(),
        request_started_at,
        attempt_started_at,
        guard_context,
        stream,
    )
    .await?;
    let stream = anthropic_messages_stream_from_openai(
        stream,
        resolved.selection.requested_model.model_key.clone(),
    );

    let body_stream = wrap_stream_with_request_logging(LoggingBodyStreamState {
        upstream: stream,
        service: state.service.clone(),
        metrics: state.metrics.clone(),
        auth: auth.clone(),
        request_log_context: request_log_context.clone(),
        requested_model_key: resolved.selection.requested_model.model_key.clone(),
        resolved_model_key: resolved.selection.execution_model.model_key.clone(),
        execution_model: resolved.selection.execution_model.clone(),
        route: route.clone(),
        routing_receipt: routing_receipt.cloned(),
        provider_key: route.provider_key.clone(),
        icon_metadata,
        started_at: request_started_at,
        attempt_started_at,
        finished: false,
        saw_terminal_event: false,
        collector: state.service.new_stream_response_collector(),
        stream_trace,
        held_usage_chunk: None,
        held_after_usage: bytes::BytesMut::new(),
        framer: gateway_core::SseEventFramer::default(),
        pending: Default::default(),
        cost_usd: None,
    });

    Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream; charset=utf-8")
        .header(CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(body_stream))
        .map_err(|error| {
            AppError(GatewayError::Internal(format!(
                "failed to build anthropic messages streaming response: {error}"
            )))
        })
}

/// Holds the last usage event of a chunk, when neither it nor anything after it carries
/// output, until a later chunk shows whether it is the last usage report. Events ahead
/// of it in the same chunk are forwarded at once, so output is never delayed. Later
/// events with neither output nor usage, such as a separate `finish_reason` chunk, are
/// held behind it in order, up to `MAX_BYTES_HELD_AFTER_USAGE`; keep-alive comments pass
/// it. Chunks are re-cut at event boundaries first, so an event split across provider
/// chunks is handled whole. When
/// the stream then completes cleanly, the priced cost is added to that usage object. A
/// chunk that ends the stream gets the cost on its own usage event, if it has one.
fn wrap_stream_with_request_logging(
    state: LoggingBodyStreamState,
) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> {
    futures_stream::unfold(state, |mut state| async move {
        loop {
            if let Some(item) = state.pending.pop_front() {
                return Some((item, state));
            }
            if state.finished {
                return None;
            }
            match state.upstream.next().await {
                Some(Ok(chunk)) => {
                    let observation = state.collector.observe_chunk(chunk.as_ref());
                    let ends_stream = observation.ends_stream;
                    state.saw_terminal_event |= observation.has_terminal_event;
                    state.stream_trace.observe_chunk(chunk.len(), observation);
                    // The collector reports the events this chunk completes, which are
                    // exactly the bytes the framer releases.
                    let chunk = state.framer.push(chunk);
                    if chunk.is_empty() {
                        continue;
                    }
                    // A completed Responses event promises a usable continuation ID.
                    // Stop before forwarding it if its origin cannot be persisted.
                    if observation.has_terminal_event
                        && state.collector.failure().is_none()
                        && let Some(receipt) = state.routing_receipt.as_mut()
                        && let Err(error) = receipt
                            .persist_response_origin(
                                state.service.store().as_ref(),
                                state.collector.response_id(),
                            )
                            .await
                    {
                        release_held_usage_chunk(&mut state, false);
                        let result = finalize_stream(&mut state, Some(error))
                            .await
                            .map(|()| chunk)
                            .map_err(|error| std::io::Error::other(error.to_string()));
                        state.pending.push_back(result);
                        continue;
                    }
                    if ends_stream {
                        let result = finalize_stream(&mut state, None).await;
                        // A usage event coalesced with the terminator is the final report.
                        let rewritten = completed_stream_cost(&state, result.is_ok())
                            .filter(|_| observation.has_usage)
                            .and_then(|cost| response_headers::with_stream_cost(&chunk, cost));
                        release_held_usage_chunk(&mut state, result.is_ok() && rewritten.is_none());
                        let chunk = rewritten.unwrap_or(chunk);
                        state.pending.push_back(
                            result
                                .map(|()| chunk)
                                .map_err(|error| std::io::Error::other(error.to_string())),
                        );
                        release_unframed_tail(&mut state);
                        continue;
                    }
                    if state.held_usage_chunk.is_some()
                        && !observation.has_output
                        && !observation.has_usage
                    {
                        if response_headers::is_comment_only(&chunk) {
                            state.pending.push_back(Ok(chunk));
                            continue;
                        }
                        if state.held_after_usage.len() + chunk.len() <= MAX_BYTES_HELD_AFTER_USAGE
                        {
                            state.held_after_usage.extend_from_slice(&chunk);
                            continue;
                        }
                    }
                    release_held_usage_chunk(&mut state, false);
                    if !observation.ends_with_usage_report {
                        state.pending.push_back(Ok(chunk));
                    } else if let Some((before, from_usage)) =
                        response_headers::split_before_usage_event(&chunk)
                    {
                        if !before.is_empty() {
                            state.pending.push_back(Ok(before));
                        }
                        state.held_usage_chunk = Some(from_usage);
                    } else if !observation.has_output {
                        state.held_usage_chunk = Some(chunk);
                    } else {
                        state.pending.push_back(Ok(chunk));
                    }
                }
                Some(Err(error)) => {
                    release_held_usage_chunk(&mut state, false);
                    release_unframed_tail(&mut state);
                    let error = fail_stream_on_transport_error(&mut state, error).await;
                    state.pending.push_back(Err(error));
                }
                None => {
                    let result = finalize_stream(&mut state, None).await;
                    release_held_usage_chunk(&mut state, result.is_ok());
                    release_unframed_tail(&mut state);
                    if let Err(error) = result {
                        state
                            .pending
                            .push_back(Err(std::io::Error::other(error.to_string())));
                    }
                }
            }
        }
    })
}

/// Caps the output-free events that can queue behind a held usage chunk. Past it, the
/// usage is released without the cost rather than holding the stream back further.
const MAX_BYTES_HELD_AFTER_USAGE: usize = 64 * 1024;

/// Forwards bytes of an event the stream never completed, unchanged.
fn release_unframed_tail(state: &mut LoggingBodyStreamState) {
    let tail = state.framer.finish();
    if !tail.is_empty() {
        state.pending.push_back(Ok(tail));
    }
}

/// Forwards the held usage chunk, adding the stream's cost when it completed cleanly,
/// followed by the chunks held behind it.
fn release_held_usage_chunk(state: &mut LoggingBodyStreamState, completed: bool) {
    let Some(chunk) = state.held_usage_chunk.take() else {
        return;
    };
    let chunk = completed_stream_cost(state, completed)
        .and_then(|cost| response_headers::with_stream_cost(&chunk, cost))
        .unwrap_or(chunk);
    state.pending.push_back(Ok(chunk));
    if !state.held_after_usage.is_empty() {
        let held = state.held_after_usage.split().freeze();
        state.pending.push_back(Ok(held));
    }
}

/// The priced cost to report in the stream, once it has completed without failure.
fn completed_stream_cost(
    state: &LoggingBodyStreamState,
    completed: bool,
) -> Option<gateway_core::Money4> {
    state
        .cost_usd
        .filter(|_| completed && state.collector.failure().is_none())
}

/// Logs and accounts a stream that failed in transport, returning the error to forward.
async fn fail_stream_on_transport_error(
    state: &mut LoggingBodyStreamState,
    error: ProviderError,
) -> std::io::Error {
    state.finished = true;
    state.collector.finish();
    state
        .stream_trace
        .finish("stream_transport_error", Some("stream_transport_error"));
    let error_message = error.to_string();
    let retryable = error.is_retryable();
    let gateway_error = GatewayError::from(error);
    // The provider may have billed tokens before the transport failed.
    if state.collector.usage().is_some() {
        account_stream_usage(state).await;
    }
    tracing::warn!(
        request_id = %state.request_log_context.request_id,
        provider_key = %state.provider_key,
        termination_reason = "stream_transport_error",
        "chat completion stream terminated with transport error"
    );
    best_effort_log_stream_result(
        &state.service,
        &state.auth,
        &state.request_log_context,
        gateway_service::StreamLogResultInput {
            provider_key: state.provider_key.clone(),
            icon_metadata: state.icon_metadata.clone(),
            latency_ms: latency_ms_since(state.started_at),
            collector: state.collector.clone(),
            failure: Some(gateway_service::StreamFailureSummary {
                status_code: gateway_error.http_status_code().into(),
                error_code: gateway_error.error_code().to_string(),
            }),
            attempts: vec![gateway_service::build_request_attempt(
                &state.request_log_context,
                &state.route,
                1,
                true,
                state.attempt_started_at,
                gateway_service::offset_now(),
                gateway_service::RequestAttemptOutcome {
                    status: RequestAttemptStatus::StreamError,
                    status_code: Some(gateway_error.http_status_code().into()),
                    error_code: Some(gateway_error.error_code().to_string()),
                    error_detail: Some(error_message.clone()),
                    retryable,
                    produced_final_response: false,
                },
            )],
        },
    )
    .await;
    state.metrics.record_chat_request(&ChatRequestMetric {
        labels: state.metric_labels(),
        status_code: i64::from(gateway_error.http_status_code()),
        outcome: gateway_error.error_type(),
        latency_seconds: latency_seconds_since(state.started_at),
    });
    state.metrics.record_tool_cardinality(
        &state.metric_labels(),
        state.request_log_context.operation,
        &RequestToolCardinality {
            invoked_tool_count: Some(state.collector.invoked_tool_count()),
            ..state.request_log_context.tool_cardinality
        },
    );
    std::io::Error::other(error_message)
}

async fn finalize_stream(
    state: &mut LoggingBodyStreamState,
    origin_error: Option<GatewayError>,
) -> Result<(), GatewayError> {
    if state.finished {
        return Ok(());
    }
    state.finished = true;
    state.collector.finish();
    let mut failure = state.collector.failure().cloned();
    // Failed streams can still report billable usage, but must not refresh affinity.
    if failure.is_none() || state.collector.usage().is_some() {
        account_stream_usage(state).await;
    }
    let routing_error = if origin_error.is_some() {
        origin_error
    } else if failure.is_none() && state.saw_terminal_event {
        match &state.routing_receipt {
            Some(receipt) => receipt
                .complete(
                    state.service.store().as_ref(),
                    state.collector.response_id(),
                )
                .await
                .err(),
            None => None,
        }
    } else {
        None
    };
    if let Some(error) = &routing_error {
        tracing::error!(request_id = %state.request_log_context.request_id,
            error_code = %error.error_code(), "failed to persist successful route binding");
        failure = Some(gateway_service::StreamFailureSummary {
            status_code: error.http_status_code().into(),
            error_code: error.error_code().to_string(),
        });
    }
    state.stream_trace.finish(
        if routing_error.is_some() {
            "routing_state_error"
        } else if failure.is_some() {
            "stream_error_event"
        } else {
            "complete"
        },
        if routing_error.is_some() {
            Some("routing_state_error")
        } else {
            failure.as_ref().map(|_| "stream_error_event")
        },
    );
    tracing::info!(
        request_id = %state.request_log_context.request_id,
        provider_key = %state.provider_key,
        termination_reason = if failure.is_some() { "stream_error_chunk" } else { "complete" },
        "chat completion stream terminated"
    );
    let tool_cardinality = RequestToolCardinality {
        invoked_tool_count: Some(state.collector.invoked_tool_count()),
        ..state.request_log_context.tool_cardinality
    };
    best_effort_log_stream_result(
        &state.service,
        &state.auth,
        &state.request_log_context,
        gateway_service::StreamLogResultInput {
            provider_key: state.provider_key.clone(),
            icon_metadata: state.icon_metadata.clone(),
            latency_ms: latency_ms_since(state.started_at),
            collector: state.collector.clone(),
            failure: failure.clone(),
            attempts: match failure.as_ref() {
                Some(failure) => vec![stream_failure_attempt(
                    &state.request_log_context,
                    &state.route,
                    state.attempt_started_at,
                    failure,
                )],
                None => vec![success_attempt(
                    &state.request_log_context,
                    &state.route,
                    true,
                    state.attempt_started_at,
                )],
            },
        },
    )
    .await;
    let (status_code, outcome) = match failure.as_ref() {
        Some(failure) => (failure.status_code, "upstream_error"),
        None => (200, "success"),
    };
    state.metrics.record_chat_request(&ChatRequestMetric {
        labels: state.metric_labels(),
        status_code,
        outcome,
        latency_seconds: latency_seconds_since(state.started_at),
    });
    state.metrics.record_tool_cardinality(
        &state.metric_labels(),
        state.request_log_context.operation,
        &tool_cardinality,
    );
    match routing_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Records the stream's usage against the ledger and budgets. `collector.usage()`
/// is `None` when the provider reported nothing; the row is then unpriced.
async fn account_stream_usage(state: &mut LoggingBodyStreamState) {
    let labels = state.metric_labels();
    let usage = finalize_successful_usage_accounting_from_parts(
        &state.service,
        &state.metrics,
        UsageAccountingContext {
            auth: &state.auth,
            model: &state.execution_model,
            route: &state.route,
            request_id: &state.request_log_context.request_id,
            labels,
            operation: state.request_log_context.operation,
        },
        state.collector.usage().cloned(),
    )
    .await;
    state.cost_usd = usage.as_ref().and_then(response_headers::billable_cost);
}

#[allow(clippy::too_many_arguments)]
async fn best_effort_log_non_stream_success(
    service: &std::sync::Arc<AppGatewayService>,
    auth: &AuthenticatedApiKey,
    context: &gateway_service::RequestLogContext,
    provider_key: &str,
    icon_metadata: RequestLogIconMetadata,
    latency_ms: i64,
    invoked_tool_count: i64,
    response_body: &Value,
    attempts: Vec<RequestAttemptRecord>,
) {
    if let Err(error) = service
        .log_non_stream_success(
            auth,
            context,
            provider_key,
            icon_metadata,
            latency_ms,
            invoked_tool_count,
            response_body,
            attempts,
        )
        .await
    {
        tracing::warn!(
            request_id = %context.request_id,
            model_key = %context.requested_model_key,
            error = %error,
            "request logging failed"
        );
    }
}

#[allow(clippy::too_many_arguments)]
async fn best_effort_log_non_stream_failure(
    service: &std::sync::Arc<AppGatewayService>,
    auth: &AuthenticatedApiKey,
    context: &gateway_service::RequestLogContext,
    provider_key: &str,
    icon_metadata: RequestLogIconMetadata,
    latency_ms: i64,
    gateway_error: &GatewayError,
    attempts: Vec<RequestAttemptRecord>,
) {
    if let Err(error) = service
        .log_non_stream_failure(
            auth,
            context,
            provider_key,
            icon_metadata,
            latency_ms,
            gateway_error,
            attempts,
        )
        .await
    {
        tracing::warn!(
            request_id = %context.request_id,
            model_key = %context.requested_model_key,
            error = %error,
            "request logging failed"
        );
    }
}

async fn best_effort_log_stream_result(
    service: &std::sync::Arc<AppGatewayService>,
    auth: &AuthenticatedApiKey,
    context: &gateway_service::RequestLogContext,
    stream_result: gateway_service::StreamLogResultInput,
) {
    if let Err(error) = service
        .log_stream_result(auth, context, stream_result)
        .await
    {
        tracing::warn!(
            request_id = %context.request_id,
            model_key = %context.requested_model_key,
            error = %error,
            "request logging failed"
        );
    }
}

#[tracing::instrument(
    name = "gateway.mcp.telemetry",
    skip_all,
    fields(gateway.operation.name = context.operation)
)]
async fn best_effort_record_mcp_request_telemetry(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    context: &mut RequestLogContext,
    route: &gateway_core::ModelRoute,
    provider: Option<&ResolvedProviderConnection>,
) {
    let access = McpAccess::new(state.store.clone());
    let resolution = match access.effective_tools_for_api_key(auth, None).await {
        Ok(resolution) => resolution,
        Err(error) => {
            tracing::warn!(
                request_id = %context.request_id,
                error = %error,
                "failed resolving MCP access for request telemetry"
            );
            return;
        }
    };

    context.tool_cardinality.referenced_mcp_server_count = Some(resolution.referenced_server_count);
    context.tool_cardinality.exposed_tool_count = Some(resolution.allowed_tools.len() as i64);
    context.tool_cardinality.filtered_tool_count = Some(resolution.filtered_tool_count);

    let occurred_at = OffsetDateTime::now_utc();
    let context_window_tokens = match state
        .service
        .resolve_route_metadata_with_provider(route, provider, occurred_at)
        .await
    {
        Ok(metadata) => metadata.limits.context,
        Err(error) => {
            tracing::warn!(
                request_id = %context.request_id,
                route_id = %route.id,
                error = %error,
                "failed resolving effective context for MCP request telemetry"
            );
            None
        }
    };

    let overhead = McpTokenOverhead::new(state.store.clone());
    if let Err(error) = overhead
        .record_request_overhead(McpTokenOverheadInput {
            request_id: context.request_id.clone(),
            request_log_id: None,
            model_key: Some(context.resolved_model_key.clone()),
            provider_family: provider
                .map(|provider| provider.provider_type.clone())
                .unwrap_or_else(|| route.provider_key.clone()),
            model_or_encoding: route.upstream_model.clone(),
            tools: resolution.allowed_tools,
            context_window_tokens,
            protocol_version: None,
            occurred_at,
        })
        .await
    {
        tracing::warn!(
            request_id = %context.request_id,
            error = %error,
            "failed recording MCP token-overhead telemetry"
        );
    }
}

fn tool_cardinality_with_invoked(
    context: &gateway_service::RequestLogContext,
    response_body: &Value,
) -> RequestToolCardinality {
    RequestToolCardinality {
        invoked_tool_count: Some(gateway_service::invoked_tool_count_from_response_body(
            response_body,
        )),
        ..context.tool_cardinality
    }
}

fn request_log_icon_metadata(
    route: &gateway_core::ModelRoute,
    provider: Option<&ResolvedProviderConnection>,
    resolved_model_key: &str,
    requested_model_key: &str,
) -> RequestLogIconMetadata {
    let provider_display = resolve_provider_display_from_parts(
        route.provider_key.as_str(),
        provider.map(|value| value.provider_type.as_str()),
        provider.map(|value| &value.config),
    );
    let model_icon_key = resolve_model_icon_key([
        route.upstream_model.as_str(),
        resolved_model_key,
        requested_model_key,
    ]);

    RequestLogIconMetadata {
        provider_icon_key: provider_display.icon_key,
        model_icon_key,
    }
}

fn normalize_response_model(mut value: Value, model_key: &str) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("model".to_string(), Value::String(model_key.to_string()));
    }
    value
}

fn usage_value_from_response(value: &Value) -> Option<Value> {
    value.get("usage").cloned()
}

fn split_partial_provider_error(error: ProviderError) -> (ProviderError, Option<Option<Value>>) {
    match error {
        ProviderError::PartialUsage {
            source,
            provider_usage,
        } => (*source, Some(provider_usage)),
        error => (error, None),
    }
}

fn latency_ms_since(started_at: Instant) -> i64 {
    i64::try_from(started_at.elapsed().as_millis()).unwrap_or(i64::MAX)
}

fn latency_seconds_since(started_at: Instant) -> f64 {
    started_at.elapsed().as_secs_f64()
}

fn record_request_span_fields(
    span: &Span,
    auth: &AuthenticatedApiKey,
    resolved: &gateway_service::ResolvedGatewayRequest,
    stream: bool,
    route_path: &str,
) {
    span.record(
        "otel.name",
        field::display(format_args!("POST {route_path}")),
    );
    span.record("http.route", field::display(route_path));
    span.record(
        "requested_model",
        field::display(&resolved.selection.requested_model.model_key),
    );
    span.record(
        "resolved_model",
        field::display(&resolved.selection.execution_model.model_key),
    );
    span.record(
        "gen_ai.request.model",
        field::display(&resolved.selection.requested_model.model_key),
    );
    span.record(
        "gen_ai.response.model",
        field::display(&resolved.selection.execution_model.model_key),
    );
    span.record("stream", stream);
    span.record("ownership_kind", field::display(auth.owner_kind.as_str()));
}

fn record_provider_execution_span_fields(span: &Span, provider_key: &str, provider_type: &str) {
    span.record("provider", field::display(provider_key));
    span.record("gateway.provider.key", field::display(provider_key));
    span.record("gen_ai.provider.name", field::display(provider_type));
}

fn record_usage_metrics_from_ref(
    metrics: &crate::observability::GatewayMetrics,
    labels: &ChatMetricLabels<'_>,
    usage: &gateway_service::RecordedChatUsage,
) {
    metrics.record_usage(labels, usage);
}

async fn finalize_successful_usage_accounting(
    state: &AppState,
    context: UsageAccountingContext<'_>,
    provider_usage: Option<Value>,
) {
    if let Some(usage) = finalize_successful_usage_accounting_from_parts(
        &state.service,
        &state.metrics,
        context,
        provider_usage,
    )
    .await
    {
        response_headers::record_cost(&usage);
    }
}

async fn finalize_successful_usage_accounting_from_parts(
    service: &std::sync::Arc<AppGatewayService>,
    metrics: &crate::observability::GatewayMetrics,
    context: UsageAccountingContext<'_>,
    provider_usage: Option<Value>,
) -> Option<gateway_service::RecordedChatUsage> {
    match service
        .record_chat_usage(
            context.auth,
            context.model,
            context.route,
            context.request_id,
            provider_usage,
            OffsetDateTime::now_utc(),
        )
        .await
    {
        Ok(usage) => {
            record_usage_metrics_from_ref(metrics, &context.labels, &usage);
            Some(usage)
        }
        Err(error) => {
            tracing::warn!(
                request_id = %context.request_id,
                requested_model = %context.labels.requested_model,
                resolved_model = %context.labels.resolved_model,
                provider_key = %context.labels.provider_key,
                stream = context.labels.stream,
                operation = context.operation,
                error = %error,
                "post-success usage accounting failed"
            );
            metrics.record_usage_record_failure(&context.labels, context.operation);
            None
        }
    }
}

fn canonical_request_id(request_id: Option<Extension<RequestId>>) -> Result<String, AppError> {
    let Some(Extension(request_id)) = request_id else {
        tracing::error!("canonical request id extension was missing from provider handler");
        return Err(AppError(GatewayError::Internal(
            "canonical request id was not available to the handler".to_string(),
        )));
    };

    request_id
        .header_value()
        .to_str()
        .map(str::to_string)
        .map_err(|error| {
            tracing::warn!(error = %error, "canonical request id extension contained invalid header value");
            AppError(GatewayError::InvalidRequest(
                "x-request-id header must be valid visible ASCII".to_string(),
            ))
        })
}

fn extract_request_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect::<BTreeMap<_, _>>()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::Arc,
        time::{Duration, Instant},
    };

    use async_trait::async_trait;
    use axum::Extension;
    use axum::body::{Bytes, to_bytes};
    use axum::http::HeaderValue;
    use futures_util::{StreamExt, stream};
    use gateway_core::protocol::openai::ChatMessage;
    use gateway_core::{
        ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthenticatedApiKey, BudgetRepository,
        ChatCompletionsRequest, CoreChatMessage, CoreChatRequest, CoreEmbeddingsRequest,
        CoreRequestRequirements, CoreResponsesRequest, GatewayError, GitHubCopilotChatApi,
        GitHubCopilotRouteCompatibility, GitHubCopilotUpstreamSupports, ModelRoute,
        ProviderCapabilities, ProviderClient, ProviderError, ProviderRegistry,
        ProviderRequestContext, ProviderStream, RequestAttemptStatus, RequestTags,
    };
    use gateway_service::{GatewayService, WeightedRoutePlanner};
    use gateway_store::{AnyStore, StoreConnectionOptions, run_migrations_with_options};
    use serde_json::{Value, json};
    use tower_http::request_id::RequestId;

    use super::routing::{
        route_capabilities_for_request, route_effective_provider_capabilities,
        select_first_eligible_route,
    };
    use super::{
        LoggingBodyStreamState, anthropic_error_response, api_health, build_provider_context,
        canonical_request_id, request_log_icon_metadata, split_partial_provider_error,
        wrap_stream_with_request_logging,
    };
    use crate::http::request_tracing::StreamTrace;
    use crate::http::test_support::seed_stream_cancellation_test;
    use crate::observability::GatewayMetrics;

    #[tokio::test]
    async fn api_health_reports_running_gateway_version() {
        let axum::Json(payload) = api_health().await;

        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["service"], "gateway");
        assert_eq!(payload["version"], env!("CARGO_PKG_VERSION"));
    }

    struct StaticProvider {
        provider_type: &'static str,
        capabilities: ProviderCapabilities,
    }

    #[async_trait]
    impl ProviderClient for StaticProvider {
        fn provider_key(&self) -> &str {
            "vertex"
        }

        fn provider_type(&self) -> &str {
            self.provider_type
        }

        fn capabilities(&self) -> ProviderCapabilities {
            self.capabilities
        }

        async fn chat_completions(
            &self,
            _request: &CoreChatRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            Err(ProviderError::NotImplemented("test provider".to_string()))
        }

        async fn chat_completions_stream(
            &self,
            _request: &CoreChatRequest,
            _context: &ProviderRequestContext,
        ) -> Result<ProviderStream, ProviderError> {
            Err(ProviderError::NotImplemented("test provider".to_string()))
        }

        async fn embeddings(
            &self,
            _request: &CoreEmbeddingsRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            Err(ProviderError::NotImplemented("test provider".to_string()))
        }

        async fn responses(
            &self,
            _request: &CoreResponsesRequest,
            _context: &ProviderRequestContext,
        ) -> Result<Value, ProviderError> {
            Err(ProviderError::NotImplemented("test provider".to_string()))
        }

        async fn responses_stream(
            &self,
            _request: &CoreResponsesRequest,
            _context: &ProviderRequestContext,
        ) -> Result<ProviderStream, ProviderError> {
            Err(ProviderError::NotImplemented("test provider".to_string()))
        }
    }

    struct StreamTestHarness {
        _directory: tempfile::TempDir,
        store: AnyStore,
        service: Arc<crate::http::state::AppGatewayService>,
        auth: gateway_core::AuthenticatedApiKey,
        request: ChatCompletionsRequest,
        resolved: gateway_service::ResolvedGatewayRequest,
        route: ModelRoute,
        provider: StaticProvider,
        metrics: Arc<GatewayMetrics>,
    }

    impl StreamTestHarness {
        async fn new() -> Self {
            let directory = tempfile::tempdir().expect("tempdir");
            let options = StoreConnectionOptions::Libsql {
                path: directory.path().join("gateway.db"),
            };
            run_migrations_with_options(&options)
                .await
                .expect("migrations");
            let store = AnyStore::connect(&options).await.expect("store");
            seed_stream_cancellation_test(&store).await;
            let service = Arc::new(GatewayService::new(
                Arc::new(store.clone()),
                Arc::new(WeightedRoutePlanner::default()),
            ));
            let auth = service
                .authenticate(Some("Bearer gwk_streamtest.cancel-secret"))
                .await
                .expect("authenticate test key");
            let request = ChatCompletionsRequest {
                model: "fast".to_string(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: Value::String("hello".to_string()),
                    name: None,
                    extra: BTreeMap::new(),
                }],
                stream: true,
                extra: BTreeMap::new(),
            };
            let resolved = service
                .resolve_request(&auth, &request.model)
                .await
                .expect("resolve request");
            let route = resolved.routes[0].clone();
            Self {
                _directory: directory,
                store,
                service,
                auth,
                request,
                resolved,
                route,
                provider: StaticProvider {
                    provider_type: "openai_compat",
                    capabilities: ProviderCapabilities::all_enabled(),
                },
                metrics: Arc::new(GatewayMetrics::new()),
            }
        }

        fn logging_state(
            &self,
            request_id: &str,
            upstream: ProviderStream,
        ) -> (gateway_service::RequestLogContext, LoggingBodyStreamState) {
            let context = self.service.begin_chat_request_log(
                request_id,
                &self.resolved.selection.requested_model.model_key,
                &self.resolved.selection.execution_model.model_key,
                &self.request,
                &BTreeMap::new(),
                RequestTags::default(),
            );
            let icon_metadata = request_log_icon_metadata(
                &self.route,
                self.resolved
                    .provider_connections
                    .get(&self.route.provider_key),
                &self.resolved.selection.execution_model.model_key,
                &self.resolved.selection.requested_model.model_key,
            );
            let state = LoggingBodyStreamState {
                upstream,
                service: self.service.clone(),
                metrics: self.metrics.clone(),
                auth: self.auth.clone(),
                request_log_context: context.clone(),
                requested_model_key: self.resolved.selection.requested_model.model_key.clone(),
                resolved_model_key: self.resolved.selection.execution_model.model_key.clone(),
                execution_model: self.resolved.selection.execution_model.clone(),
                route: self.route.clone(),
                routing_receipt: None,
                provider_key: self.route.provider_key.clone(),
                icon_metadata,
                started_at: Instant::now(),
                attempt_started_at: gateway_service::offset_now(),
                finished: false,
                saw_terminal_event: false,
                collector: self.service.new_stream_response_collector(),
                stream_trace: StreamTrace::new(
                    "chat",
                    request_id,
                    &self.route,
                    &self.provider,
                    Instant::now(),
                ),
                held_usage_chunk: None,
                held_after_usage: bytes::BytesMut::new(),
                framer: gateway_core::SseEventFramer::default(),
                pending: Default::default(),
                cost_usd: None,
            };
            (context, state)
        }

        fn ownership_scope_key(&self) -> String {
            format!(
                "service_account:{}",
                self.auth
                    .owner_service_account_id
                    .expect("service account owner")
            )
        }
    }

    #[tokio::test]
    async fn dropping_partial_stream_records_cancellation_without_usage() {
        let harness = StreamTestHarness::new().await;
        let request_id = "stream-cancel-test";
        let upstream: ProviderStream = Box::pin(stream::iter([Ok(Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        ))]));
        let (context, state) = harness.logging_state(request_id, upstream);
        let mut body_stream = Box::pin(wrap_stream_with_request_logging(state));

        assert!(body_stream.next().await.is_some());
        drop(body_stream);

        let detail = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(detail) = harness
                    .service
                    .get_request_log_detail(context.request_log_id)
                    .await
                {
                    break detail;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancelled stream log persisted");

        let snapshot = harness.metrics.test_snapshot();
        assert_eq!(snapshot.requests, 1);
        assert_eq!(snapshot.request_outcomes.get("client_cancelled"), Some(&1));
        assert_eq!(detail.log.status_code, Some(499));
        assert_eq!(detail.log.error_code.as_deref(), Some("client_cancelled"));
        assert_eq!(detail.log.prompt_tokens, None);
        assert_eq!(detail.log.completion_tokens, None);
        assert_eq!(detail.log.total_tokens, None);
        assert_eq!(detail.attempts.len(), 1);
        assert_eq!(detail.attempts[0].status, RequestAttemptStatus::StreamError);
        assert_eq!(detail.attempts[0].status_code, Some(499));
        assert_eq!(
            detail.attempts[0].error_code.as_deref(),
            Some("client_cancelled")
        );

        let ledger = harness
            .store
            .get_usage_ledger_by_request_and_scope(request_id, &harness.ownership_scope_key())
            .await
            .expect("read usage ledger");
        assert!(ledger.is_none());
    }

    #[tokio::test]
    async fn terminal_stream_forwards_trailing_usage_after_logging_it() {
        let harness = StreamTestHarness::new().await;
        let request_id = "stream-terminal-test";
        let upstream: ProviderStream = Box::pin(stream::iter([
            Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            )),
            Ok(Bytes::from_static(
                b"data: {\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n",
            )),
            Ok(Bytes::from_static(b"data: [DONE]\n\n")),
        ]));
        let (context, state) = harness.logging_state(request_id, upstream);
        let mut body_stream = Box::pin(wrap_stream_with_request_logging(state));

        let finish_chunk = body_stream
            .next()
            .await
            .expect("finish chunk")
            .expect("finish chunk succeeds");
        assert!(
            finish_chunk
                .windows(b"finish_reason".len())
                .any(|window| { window == b"finish_reason" })
        );
        assert_eq!(harness.metrics.test_snapshot().requests, 0);
        let usage_chunk = body_stream
            .next()
            .await
            .expect("usage chunk")
            .expect("usage chunk succeeds");
        assert!(
            usage_chunk
                .windows(b"prompt_tokens".len())
                .any(|window| { window == b"prompt_tokens" })
        );
        // The final usage chunk waits for the stream to be logged so it can carry the
        // cost. This route is unpriced, so the chunk is forwarded unchanged.
        assert_eq!(harness.metrics.test_snapshot().requests, 1);
        assert_eq!(
            usage_chunk,
            Bytes::from_static(
                b"data: {\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":1,\"total_tokens\":3}}\n\n"
            )
        );
        assert_eq!(
            body_stream
                .next()
                .await
                .expect("done chunk")
                .expect("done chunk succeeds"),
            Bytes::from_static(b"data: [DONE]\n\n")
        );
        drop(body_stream);

        let detail = harness
            .service
            .get_request_log_detail(context.request_log_id)
            .await
            .expect("terminal stream log persisted before chunk delivery");
        let snapshot = harness.metrics.test_snapshot();
        assert_eq!(snapshot.requests, 1);
        assert_eq!(snapshot.request_outcomes.get("success"), Some(&1));
        assert_eq!(detail.log.status_code, Some(200));
        assert_eq!(detail.log.error_code, None);
        assert_eq!(detail.log.prompt_tokens, Some(2));
        assert_eq!(detail.log.completion_tokens, Some(1));
        assert_eq!(detail.log.total_tokens, Some(3));
        assert_eq!(detail.attempts.len(), 1);
        assert_eq!(detail.attempts[0].status, RequestAttemptStatus::Success);

        let ledger = harness
            .store
            .get_usage_ledger_by_request_and_scope(request_id, &harness.ownership_scope_key())
            .await
            .expect("read completed usage ledger");
        assert!(ledger.is_some());
    }

    async fn collect_stream(state: LoggingBodyStreamState) -> Vec<Result<Bytes, String>> {
        wrap_stream_with_request_logging(state)
            .map(|item| item.map_err(|error| error.to_string()))
            .collect()
            .await
    }

    fn priced_route(harness: &mut StreamTestHarness) {
        harness.route.pricing_override = Some(gateway_core::RoutePricingOverride {
            input_cost_per_million_tokens: gateway_core::Money4::from_scaled(30_000),
            output_cost_per_million_tokens: gateway_core::Money4::from_scaled(150_000),
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
        });
    }

    fn sse_json(chunk: &Bytes) -> Value {
        let text = std::str::from_utf8(chunk).expect("utf8 chunk");
        let data = text
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("data line");
        serde_json::from_str(data).expect("json data")
    }

    #[tokio::test]
    async fn priced_stream_adds_the_ledger_cost_to_the_final_usage_chunk() {
        let mut harness = StreamTestHarness::new().await;
        priced_route(&mut harness);
        let request_id = "stream-cost-test";
        let upstream: ProviderStream = Box::pin(stream::iter([
            Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":1,\"total_tokens\":1001}}\n\n",
            )),
            Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            )),
            Ok(Bytes::from_static(
                b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":500,\"total_tokens\":1500}}\n\n",
            )),
            Ok(Bytes::from_static(b"data: [DONE]\n\n")),
        ]));
        let (_context, state) = harness.logging_state(request_id, upstream);
        let chunks = collect_stream(state).await;

        assert_eq!(chunks.len(), 4);
        let chunks: Vec<Bytes> = chunks.into_iter().map(|chunk| chunk.expect("ok")).collect();
        // Chunks with output are never rewritten, even when they carry usage.
        assert_eq!(sse_json(&chunks[0]).pointer("/usage/oceans"), None);
        let ledger = harness
            .store
            .get_usage_ledger_by_request_and_scope(request_id, &harness.ownership_scope_key())
            .await
            .expect("read usage ledger")
            .expect("usage ledger row");
        assert_eq!(
            ledger.pricing_status,
            gateway_core::UsagePricingStatus::Priced
        );
        assert_eq!(ledger.computed_cost_usd.format_4dp(), "0.0105");
        let usage = sse_json(&chunks[2]);
        assert_eq!(usage["usage"]["total_tokens"], 1500);
        assert_eq!(usage["usage"]["oceans"]["cost"], "0.0105");
        assert_eq!(chunks[3], Bytes::from_static(b"data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn priced_stream_adds_cost_to_usage_coalesced_with_other_events() {
        let mut harness = StreamTestHarness::new().await;
        priced_route(&mut harness);
        let upstream: ProviderStream = Box::pin(stream::iter([
            Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":1,\"total_tokens\":1001}}\n\n",
            )),
            Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":500,\"total_tokens\":1500}}\n\ndata: [DONE]\n\n",
            )),
        ]));
        let (_context, state) = harness.logging_state("stream-coalesced-cost-test", upstream);
        let chunks: Vec<Bytes> = collect_stream(state)
            .await
            .into_iter()
            .map(|chunk| chunk.expect("ok"))
            .collect();

        // The content ahead of the first usage event is released at once; the usage
        // event itself is held and released unchanged when more events follow.
        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks[0],
            Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n")
        );
        assert_eq!(sse_json(&chunks[1]).pointer("/usage/oceans"), None);
        let last = std::str::from_utf8(&chunks[2]).expect("utf8");
        assert!(
            last.starts_with("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n")
        );
        assert!(last.contains(r#""total_tokens":1500,"oceans":{"cost":"0.0105"}}"#));
        assert!(last.ends_with("\n\ndata: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn priced_stream_keeps_usage_held_across_chunks_without_output() {
        let mut harness = StreamTestHarness::new().await;
        priced_route(&mut harness);
        let finish = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        );
        let upstream: ProviderStream = Box::pin(stream::iter([
            Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
            )),
            Ok(Bytes::from_static(
                b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":500,\"total_tokens\":1500}}\n\n",
            )),
            Ok(finish.clone()),
            Ok(Bytes::from_static(b": keep-alive\n\n")),
            Ok(Bytes::from_static(b"data: [DONE]\n\n")),
        ]));
        let (_context, state) = harness.logging_state("stream-held-across-test", upstream);
        let chunks: Vec<Bytes> = collect_stream(state)
            .await
            .into_iter()
            .map(|chunk| chunk.expect("ok"))
            .collect();

        // Clients ignore comments, so the keep-alive passes the held usage. The
        // `finish_reason` event stays behind it.
        assert_eq!(chunks.len(), 5);
        assert_eq!(chunks[1], Bytes::from_static(b": keep-alive\n\n"));
        assert_eq!(sse_json(&chunks[2])["usage"]["oceans"]["cost"], "0.0105");
        assert_eq!(chunks[3], finish);
        assert_eq!(chunks[4], Bytes::from_static(b"data: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn priced_stream_adds_cost_to_a_usage_event_split_across_chunks() {
        let mut harness = StreamTestHarness::new().await;
        priced_route(&mut harness);
        let usage = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":500,\"total_tokens\":1500}}\r\r";
        let (head, tail) = usage.split_at(30);
        let upstream: ProviderStream = Box::pin(stream::iter([
            Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r\rdata: {\"cho",
            )),
            Ok(Bytes::from(format!(
                "ices\":[{{\"delta\":{{\"content\":\"!\"}}}}]}}\r\r{head}"
            ))),
            Ok(Bytes::from(tail.to_string())),
            Ok(Bytes::from_static(b"data: [DONE]\r\r")),
        ]));
        let (_context, state) = harness.logging_state("stream-split-usage-test", upstream);
        let chunks: Vec<Bytes> = collect_stream(state)
            .await
            .into_iter()
            .map(|chunk| chunk.expect("ok"))
            .collect();

        // Each chunk forwarded holds whole events, and the usage event gets the cost.
        assert_eq!(chunks.len(), 4);
        assert_eq!(
            chunks[0],
            Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r\r")
        );
        assert_eq!(
            chunks[1],
            Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"!\"}}]}\r\r")
        );
        assert_eq!(
            std::str::from_utf8(&chunks[2]).unwrap(),
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":500,\"total_tokens\":1500,\"oceans\":{\"cost\":\"0.0105\"}}}\r\r"
        );
        assert_eq!(chunks[3], Bytes::from_static(b"data: [DONE]\r\r"));
    }

    #[tokio::test]
    async fn priced_stream_forwards_an_incomplete_trailing_event_unchanged() {
        let mut harness = StreamTestHarness::new().await;
        priced_route(&mut harness);
        let upstream: ProviderStream = Box::pin(stream::iter([
            Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"cho",
            )),
            Err(ProviderError::Transport("reset".to_string())),
        ]));
        let (_context, state) = harness.logging_state("stream-unframed-tail-test", upstream);
        let chunks = collect_stream(state).await;

        let bytes: Vec<u8> = chunks
            .iter()
            .filter_map(|chunk| chunk.as_ref().ok())
            .flat_map(|chunk| chunk.to_vec())
            .collect();
        assert_eq!(
            bytes,
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"cho"
        );
        assert!(chunks.last().expect("error").is_err());
    }

    /// Inference guardrails buffer the upstream stream and emit it as one chunk.
    #[tokio::test]
    async fn priced_stream_adds_cost_when_the_whole_stream_is_one_chunk() {
        let mut harness = StreamTestHarness::new().await;
        priced_route(&mut harness);
        let upstream: ProviderStream = Box::pin(stream::iter([Ok(Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":500,\"total_tokens\":1500}}\n\ndata: [DONE]\n\n",
        ))]));
        let (_context, state) = harness.logging_state("stream-one-chunk-cost-test", upstream);
        let chunks: Vec<Bytes> = collect_stream(state)
            .await
            .into_iter()
            .map(|chunk| chunk.expect("ok"))
            .collect();

        assert_eq!(chunks.len(), 1);
        let body = std::str::from_utf8(&chunks[0]).expect("utf8");
        assert!(body.starts_with("data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n"));
        assert!(body.contains(r#""total_tokens":1500,"oceans":{"cost":"0.0105"}}"#));
        assert!(body.ends_with("\n\ndata: [DONE]\n\n"));
    }

    /// Runs a real `guard_stream` ahead of the logging wrapper, as `stream_response` does
    /// when inference guardrails are enabled, over OpenRouter's stream shape: a comment
    /// line, one event per chunk, and a final usage event that still carries a choice.
    #[tokio::test]
    async fn priced_stream_adds_cost_after_inference_guardrails_buffer_the_stream() {
        let (_guard_directory, mut app_state) = crate::http::test_support::app_state().await;
        Arc::make_mut(&mut app_state.guardrail_config)
            .default
            .enabled = true;
        let mut prompt = json!({"messages": [{"role": "user", "content": "hello"}]});
        let guard_context = crate::http::inference_guardrails::guard_prompt(
            &app_state,
            "stream-guarded-cost-test",
            "fast/openrouter/upstream".into(),
            &mut prompt,
        )
        .await
        .expect("guard prompt");
        assert!(guard_context.enabled);

        let upstream: ProviderStream = Box::pin(stream::iter(
            [
                ": OPENROUTER PROCESSING\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"hi\",\"role\":\"assistant\"},\"finish_reason\":null,\"index\":0}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"\",\"role\":\"assistant\"},\"finish_reason\":\"stop\",\"index\":0,\"native_finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":500,\"total_tokens\":1500,\"cost\":0.00002627,\"is_byok\":false}}\n\n",
                "data: [DONE]\n\n",
            ]
            .map(|event| Ok(Bytes::from_static(event.as_bytes()))),
        ));
        let Ok(guarded) =
            crate::http::inference_guardrails::guard_stream(&app_state, &guard_context, upstream)
                .await
        else {
            panic!("guard stream");
        };

        let mut harness = StreamTestHarness::new().await;
        priced_route(&mut harness);
        let (_context, state) = harness.logging_state("stream-guarded-cost-test", guarded);
        let body = collect_stream(state)
            .await
            .into_iter()
            .map(|chunk| String::from_utf8(chunk.expect("ok").to_vec()).expect("utf8"))
            .collect::<String>();

        let usage = body
            .split("\n\n")
            .filter_map(|event| event.strip_prefix("data: "))
            .filter(|data| *data != "[DONE]")
            .map(|data| serde_json::from_str::<Value>(data).expect("json event"))
            .find(|event| event.get("usage").is_some())
            .expect("usage event");
        assert_eq!(usage["usage"]["total_tokens"], 1500);
        assert_eq!(usage["usage"]["cost"], 0.00002627);
        assert_eq!(usage["usage"]["oceans"]["cost"], "0.0105");
        assert!(body.starts_with(": OPENROUTER PROCESSING\n\n"));
        assert!(body.ends_with("\n\ndata: [DONE]\n\n"));
    }

    #[tokio::test]
    async fn priced_stream_does_not_hold_content_that_carries_usage() {
        let mut harness = StreamTestHarness::new().await;
        priced_route(&mut harness);
        let content_with_usage = Bytes::from_static(
            b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":1,\"total_tokens\":1001}}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
        );
        // The upstream stays open, so a held chunk would never be yielded.
        let upstream: ProviderStream =
            Box::pin(stream::iter([Ok(content_with_usage.clone())]).chain(stream::pending()));
        let (_context, state) = harness.logging_state("stream-no-hold-test", upstream);
        let mut wrapped = std::pin::pin!(wrap_stream_with_request_logging(state));
        let first = tokio::time::timeout(std::time::Duration::from_secs(5), wrapped.next())
            .await
            .expect("output is forwarded without waiting for the next chunk")
            .expect("chunk")
            .expect("ok");
        assert_eq!(first, content_with_usage);
    }

    #[tokio::test]
    async fn priced_messages_stream_adds_cost_to_message_delta() {
        let mut harness = StreamTestHarness::new().await;
        priced_route(&mut harness);
        let upstream: ProviderStream = Box::pin(stream::iter([
            Ok(Bytes::from_static(
                b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1000,\"output_tokens\":1}}}\n\n",
            )),
            Ok(Bytes::from_static(
                b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
            )),
            Ok(Bytes::from_static(
                b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":1000,\"output_tokens\":500}}\n\n",
            )),
            Ok(Bytes::from_static(
                b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            )),
        ]));
        let (_context, state) = harness.logging_state("stream-messages-cost-test", upstream);
        let chunks: Vec<Bytes> = collect_stream(state)
            .await
            .into_iter()
            .map(|chunk| chunk.expect("ok"))
            .collect();

        assert_eq!(chunks.len(), 4);
        // An earlier usage report is released unchanged once more events follow.
        assert_eq!(sse_json(&chunks[0]).pointer("/message/usage/oceans"), None);
        assert!(chunks[2].starts_with(b"event: message_delta\n"));
        assert_eq!(sse_json(&chunks[2])["usage"]["oceans"]["cost"], "0.0105");
        assert!(chunks[3].starts_with(b"event: message_stop\n"));
    }

    /// The SDK compatibility suite in `tests/sdk-compat` parses these fixtures with the
    /// official SDKs. Removing the cost and replaying each fixture through the stream
    /// wrapper must give back the fixture byte for byte, so the SDKs see gateway output.
    /// Set `UPDATE_SDK_COMPAT_FIXTURES=1` to rewrite the fixtures from that output.
    #[tokio::test]
    async fn sdk_compat_fixtures_match_gateway_output() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/sdk-compat/fixtures");
        for name in ["chat_completions", "responses", "messages"] {
            let expected = std::fs::read_to_string(fixtures.join(format!("{name}.sse")))
                .expect("read fixture");
            let upstream = expected
                .replace(r#""oceans":{"cost":"0.0012"},"#, "")
                .replace(r#","oceans":{"cost":"0.0012"}"#, "");
            let events: Vec<Result<Bytes, ProviderError>> = upstream
                .split_inclusive("\n\n")
                .map(|event| Ok(Bytes::from(event.to_string())))
                .collect();

            let mut harness = StreamTestHarness::new().await;
            // 5 input tokens at $200/M plus 2 output tokens at $100/M is $0.0012.
            harness.route.pricing_override = Some(gateway_core::RoutePricingOverride {
                input_cost_per_million_tokens: gateway_core::Money4::from_scaled(2_000_000),
                output_cost_per_million_tokens: gateway_core::Money4::from_scaled(1_000_000),
                cache_read_cost_per_million_tokens: None,
                cache_write_cost_per_million_tokens: None,
            });
            let (_context, state) = harness.logging_state(
                &format!("sdk-compat-{name}"),
                Box::pin(stream::iter(events)),
            );
            let output: String = collect_stream(state)
                .await
                .into_iter()
                .map(|chunk| String::from_utf8(chunk.expect("ok").to_vec()).expect("utf8"))
                .collect();
            assert!(
                output.contains(r#""oceans":{"cost":"0.0012"}"#),
                "{name}: no cost in gateway output"
            );
            if std::env::var_os("UPDATE_SDK_COMPAT_FIXTURES").is_some() {
                std::fs::write(fixtures.join(format!("{name}.sse")), &output)
                    .expect("write fixture");
            } else {
                assert_eq!(output, expected, "{name}");
            }
        }
    }

    #[tokio::test]
    async fn transport_error_releases_held_usage_without_cost() {
        let mut harness = StreamTestHarness::new().await;
        priced_route(&mut harness);
        let usage = b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":500,\"total_tokens\":1500}}\n\n";
        let upstream: ProviderStream = Box::pin(stream::iter([
            Ok(Bytes::from_static(usage)),
            Err(ProviderError::Transport("connection reset".to_string())),
        ]));
        let (_context, state) = harness.logging_state("stream-held-error-test", upstream);
        let chunks = collect_stream(state).await;

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], Ok(Bytes::from_static(usage)));
        assert!(chunks[1].is_err());
    }

    #[tokio::test]
    async fn transport_error_after_usage_still_charges_the_ledger() {
        let harness = StreamTestHarness::new().await;
        let request_id = "stream-transport-error-test";
        let upstream: ProviderStream = Box::pin(stream::iter([
            Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n",
            )),
            Err(ProviderError::Transport("connection reset".to_string())),
        ]));
        let (context, state) = harness.logging_state(request_id, upstream);
        let mut body_stream = Box::pin(wrap_stream_with_request_logging(state));

        body_stream
            .next()
            .await
            .expect("usage chunk")
            .expect("usage chunk succeeds");
        let error = body_stream
            .next()
            .await
            .expect("transport error item")
            .expect_err("transport error surfaces to the client");
        assert!(error.to_string().contains("connection reset"));
        assert!(body_stream.next().await.is_none());
        drop(body_stream);

        let detail = harness
            .service
            .get_request_log_detail(context.request_log_id)
            .await
            .expect("transport error log persisted");
        assert_eq!(detail.log.status_code, Some(502));
        assert_eq!(detail.log.error_code.as_deref(), Some("upstream_transport"));
        assert_eq!(detail.attempts.len(), 1);
        assert_eq!(detail.attempts[0].status, RequestAttemptStatus::StreamError);

        let ledger = harness
            .store
            .get_usage_ledger_by_request_and_scope(request_id, &harness.ownership_scope_key())
            .await
            .expect("read usage ledger")
            .expect("usage billed before the transport failure is charged");
        assert_eq!(ledger.prompt_tokens, Some(5));
        assert_eq!(ledger.completion_tokens, Some(2));
    }

    fn route(upstream_model: &str, capabilities: ProviderCapabilities) -> ModelRoute {
        ModelRoute {
            id: uuid::Uuid::new_v4(),
            model_id: uuid::Uuid::new_v4(),
            provider_key: "vertex".to_string(),
            upstream_model: upstream_model.to_string(),
            priority: 0,
            weight: 1.0,
            enabled: true,
            context_window_tokens: None,
            pricing_override: None,
            extra_headers: Default::default(),
            extra_body: Default::default(),
            capabilities,
            compatibility: Default::default(),
        }
    }

    fn authenticated_key(
        owner_kind: ApiKeyOwnerKind,
        owner_user_id: Option<uuid::Uuid>,
    ) -> AuthenticatedApiKey {
        AuthenticatedApiKey {
            id: uuid::Uuid::new_v4(),
            public_id: "test-key".to_string(),
            name: "Test key".to_string(),
            model_grant_mode: ApiKeyModelGrantMode::All,
            owner_kind,
            owner_user_id,
            owner_team_id: None,
            owner_service_account_id: None,
        }
    }

    #[test]
    fn provider_context_uses_only_the_authenticated_key_owner_user() {
        let user_id = uuid::Uuid::new_v4();
        let route = route("gpt-5.6-luna", ProviderCapabilities::all_enabled());
        let user_context = build_provider_context(
            "req-user",
            "copilot",
            &route,
            &authenticated_key(ApiKeyOwnerKind::User, Some(user_id)),
            BTreeMap::new(),
        );
        let service_account_context = build_provider_context(
            "req-service-account",
            "copilot",
            &route,
            &authenticated_key(ApiKeyOwnerKind::ServiceAccount, None),
            BTreeMap::new(),
        );

        assert_eq!(user_context.owner_user_id, Some(user_id));
        assert_eq!(service_account_context.owner_user_id, None);
    }

    #[test]
    fn vertex_route_effective_capabilities_are_route_aware_for_embeddings() {
        let provider = StaticProvider {
            provider_type: "gcp_vertex",
            capabilities: ProviderCapabilities::all_enabled(),
        };

        let embedding_route = route(
            "google/gemini-embedding-001",
            ProviderCapabilities::all_enabled(),
        );
        let embedding_capabilities =
            route_effective_provider_capabilities(&provider, &embedding_route)
                .intersect(embedding_route.capabilities);
        assert!(embedding_capabilities.embeddings);
        assert!(!embedding_capabilities.chat_completions);
        assert!(!embedding_capabilities.responses);
        assert!(!embedding_capabilities.stream);
        assert!(!embedding_capabilities.tools);

        let chat_route = route(
            "google/gemini-2.0-flash",
            ProviderCapabilities::all_enabled(),
        );
        let chat_capabilities = route_effective_provider_capabilities(&provider, &chat_route)
            .intersect(chat_route.capabilities);
        assert!(chat_capabilities.chat_completions);
        assert!(chat_capabilities.stream);
        assert!(!chat_capabilities.embeddings);
        assert!(chat_capabilities.tools);

        let anthropic_route = route(
            "anthropic/claude-sonnet-4-6",
            ProviderCapabilities::all_enabled(),
        );
        let anthropic_capabilities =
            route_effective_provider_capabilities(&provider, &anthropic_route)
                .intersect(anthropic_route.capabilities);
        assert!(anthropic_capabilities.chat_completions);
        assert!(!anthropic_capabilities.embeddings);
        assert!(anthropic_capabilities.tools);
    }
    #[test]
    fn copilot_capabilities_follow_route_endpoint_metadata() {
        let provider = StaticProvider {
            provider_type: "github_copilot",
            capabilities: ProviderCapabilities::all_enabled(),
        };
        let mut claude_route = route(
            "anthropic/claude-sonnet-4-6",
            ProviderCapabilities::all_enabled(),
        );
        claude_route.compatibility.github_copilot = Some(GitHubCopilotRouteCompatibility {
            chat_api: Some(GitHubCopilotChatApi::AnthropicMessages),
            supports_responses: false,
            supports_embeddings: false,
            upstream_supports: GitHubCopilotUpstreamSupports {
                streaming: true,
                tool_calls: true,
                vision: true,
                ..Default::default()
            },
        });
        let capabilities = route_effective_provider_capabilities(&provider, &claude_route)
            .intersect(claude_route.capabilities);

        assert!(!capabilities.json_schema);
        assert!(capabilities.chat_completions);
        assert!(capabilities.tools);

        let unknown_route = route("unknown", ProviderCapabilities::all_enabled());
        let unknown_capabilities = route_effective_provider_capabilities(&provider, &unknown_route)
            .intersect(unknown_route.capabilities);
        assert!(!unknown_capabilities.chat_completions);
        assert!(!unknown_capabilities.responses);
        assert!(!unknown_capabilities.embeddings);

        let mut responses_route = route("gpt-5", ProviderCapabilities::all_enabled());
        responses_route.compatibility.github_copilot = Some(GitHubCopilotRouteCompatibility {
            chat_api: None,
            supports_responses: true,
            supports_embeddings: false,
            upstream_supports: GitHubCopilotUpstreamSupports {
                streaming: true,
                ..Default::default()
            },
        });
        let responses_capabilities =
            route_effective_provider_capabilities(&provider, &responses_route)
                .intersect(responses_route.capabilities);
        assert!(!responses_capabilities.chat_completions);
        assert!(responses_capabilities.responses);

        let mut mixed_route = route("claude-with-responses", ProviderCapabilities::all_enabled());
        mixed_route.compatibility.github_copilot = Some(GitHubCopilotRouteCompatibility {
            chat_api: Some(GitHubCopilotChatApi::AnthropicMessages),
            supports_responses: true,
            supports_embeddings: false,
            upstream_supports: GitHubCopilotUpstreamSupports {
                structured_outputs: true,
                ..Default::default()
            },
        });
        let response_capabilities = route_capabilities_for_request(
            &provider,
            &mixed_route,
            CoreRequestRequirements {
                responses: true,
                json_schema: true,
                ..Default::default()
            },
        );
        assert!(response_capabilities.json_schema);

        let chat_capabilities = route_capabilities_for_request(
            &provider,
            &mixed_route,
            CoreRequestRequirements {
                chat_completions: true,
                json_schema: true,
                ..Default::default()
            },
        );
        assert!(!chat_capabilities.json_schema);
    }

    #[test]
    fn vertex_embedding_route_selection_only_uses_supported_google_text_embedding_routes() {
        let provider = Arc::new(StaticProvider {
            provider_type: "gcp_vertex",
            capabilities: ProviderCapabilities::all_enabled(),
        });
        let mut providers = ProviderRegistry::new();
        providers.register(provider);
        let routes = vec![
            route("google/gemini-2.5-pro", ProviderCapabilities::all_enabled()),
            route(
                "anthropic/claude-sonnet-4-6",
                ProviderCapabilities::all_enabled(),
            ),
            route(
                "google/text-embedding-005",
                ProviderCapabilities::all_enabled(),
            ),
        ];

        let (eligible_route_count, selected) = select_first_eligible_route(
            &providers,
            &routes,
            CoreRequestRequirements {
                embeddings: true,
                ..Default::default()
            },
        );

        assert_eq!(eligible_route_count, 1);
        assert_eq!(
            selected
                .expect("supported embedding route")
                .route
                .upstream_model,
            "google/text-embedding-005"
        );
    }

    #[test]
    fn chat_file_inputs_only_select_vision_capable_routes() {
        let provider = Arc::new(StaticProvider {
            provider_type: "openai_compat",
            capabilities: ProviderCapabilities::all_enabled(),
        });
        let mut providers = ProviderRegistry::new();
        providers.register(provider);

        let mut text_only_capabilities = ProviderCapabilities::all_enabled();
        text_only_capabilities.vision = false;
        let routes = vec![
            route("text-only", text_only_capabilities),
            route("document-capable", ProviderCapabilities::all_enabled()),
        ];
        let request = CoreChatRequest {
            model: "documents".to_string(),
            messages: vec![CoreChatMessage {
                role: "user".to_string(),
                content: json!([{
                    "type": "file",
                    "file": {
                        "file_data": "data:application/pdf;base64,cGRm",
                        "filename": "document.pdf"
                    }
                }]),
                name: None,
                extra: Default::default(),
            }],
            stream: false,
            extra: Default::default(),
        };

        let (eligible_route_count, selected) =
            select_first_eligible_route(&providers, &routes, request.requirements());

        assert_eq!(eligible_route_count, 1);
        assert_eq!(
            selected.expect("vision-capable route").route.upstream_model,
            "document-capable"
        );
    }

    #[test]
    fn canonical_request_id_returns_gateway_internal_error_when_extension_is_missing() {
        let error = canonical_request_id(None).expect_err("missing extension should fail");

        assert_eq!(error.0.http_status_code(), 500);
        assert_eq!(error.0.error_code(), "internal_error");
        assert!(matches!(error.0, GatewayError::Internal(_)));
    }

    #[test]
    fn canonical_request_id_rejects_invalid_header_value_as_bad_request() {
        let error = canonical_request_id(Some(Extension(RequestId::new(
            HeaderValue::from_bytes(&[0xff]).expect("opaque header value"),
        ))))
        .expect_err("invalid header value should fail");

        assert_eq!(error.0.http_status_code(), 400);
        assert_eq!(error.0.error_code(), "invalid_request");
        assert!(matches!(error.0, GatewayError::InvalidRequest(_)));
    }

    #[test]
    fn canonical_request_id_reads_tower_request_id_extension() {
        let value = match canonical_request_id(Some(Extension(RequestId::new(
            HeaderValue::from_static("req-provided"),
        )))) {
            Ok(value) => value,
            Err(_) => panic!("request id should be available"),
        };

        assert_eq!(value, "req-provided");
    }

    #[test]
    fn split_partial_provider_error_preserves_usage_accounting_signal() {
        let (source, provider_usage) = split_partial_provider_error(ProviderError::PartialUsage {
            source: Box::new(ProviderError::UpstreamHttp {
                status: 429,
                body: "quota exhausted".to_string(),
                retry_after: None,
            }),
            provider_usage: Some(json!({"prompt_tokens": 4, "total_tokens": 4})),
        });

        assert_eq!(
            provider_usage,
            Some(Some(json!({"prompt_tokens": 4, "total_tokens": 4})))
        );
        match source {
            ProviderError::UpstreamHttp { status, body, .. } => {
                assert_eq!(status, 429);
                assert_eq!(body, "quota exhausted");
            }
            other => panic!("unexpected provider error source: {other}"),
        }

        let (source, provider_usage) = split_partial_provider_error(ProviderError::PartialUsage {
            source: Box::new(ProviderError::Transport("invalid json".to_string())),
            provider_usage: None,
        });

        assert_eq!(provider_usage, Some(None));
        match source {
            ProviderError::Transport(message) => assert_eq!(message, "invalid json"),
            other => panic!("unexpected provider error source: {other}"),
        }

        let (source, provider_usage) =
            split_partial_provider_error(ProviderError::Transport("network down".to_string()));

        assert!(matches!(source, ProviderError::Transport(message) if message == "network down"));
        assert_eq!(provider_usage, None);
    }

    #[tokio::test]
    async fn anthropic_error_response_uses_messages_error_shape() {
        let response = anthropic_error_response(GatewayError::InvalidRequest(
            "bad messages request".to_string(),
        ));
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes");
        let value: Value = serde_json::from_slice(&body).expect("json");

        assert_eq!(status.as_u16(), 400);
        assert_eq!(value["type"], "error");
        assert_eq!(value["error"]["type"], "invalid_request_error");
        assert_eq!(value["error"]["code"], "invalid_request");
        assert_eq!(
            value["error"]["message"],
            "invalid request: bad messages request"
        );
    }

    #[tokio::test]
    async fn decisions_request_secrets_are_redacted_before_dispatch() {
        let (_directory, mut state) = crate::http::test_support::app_state().await;
        let policy = &mut Arc::make_mut(&mut state.guardrail_config).default;
        policy.enabled = true;
        policy.secret_redaction.enabled = true;
        // Assembled at runtime so no literal credential lands in the repository.
        let key = format!(
            "sk-ant-api03-{}AA",
            &"aZ3kQ9mB7xR2tW5nL8pJ4vC6yH1dF0gS".repeat(3)[..93]
        );
        let mut request: gateway_core::CoreDecisionsRequest = serde_json::from_value(json!({
            "model": "gpt",
            "state": {"notes": format!("deploy with {key}")},
        }))
        .expect("decisions request");

        let redacted =
            super::redact_typed_request(&state, "request-1", "openai/gpt", &mut request).await;

        assert!(redacted.is_ok());

        assert_eq!(
            request.state["notes"],
            "deploy with [REDACTED:anthropic-api-key]"
        );
        assert_eq!(request.model, "gpt");
    }
}
