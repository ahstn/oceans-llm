use std::{collections::BTreeMap, sync::Arc, time::Instant};

use axum::{
    Json,
    body::Body,
    extract::{Extension, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use futures_util::{StreamExt, stream as futures_stream};
use gateway_core::{
    AnthropicMessagesRequest, AuthenticatedApiKey, ChatCompletionsRequest, CoreChatRequest,
    CoreRequestRequirements, EmbeddingsRequest, GatewayError, ModelsListResponse,
    ProviderCapabilities, ProviderClient, ProviderError, ProviderRequestContext, ProviderStream,
    RequestAttemptRecord, RequestAttemptStatus, RequestToolCardinality, ResponsesRequest,
    anthropic_messages_request_to_core, core_chat_request_to_openai, openai_chat_request_to_core,
    openai_embeddings_request_to_core, openai_responses_request_to_core,
    protocol::{anthropic::anthropic_message_from_openai_chat, openai::ModelCard},
    vertex_route_capabilities_for_upstream_model,
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
    error::AppError,
    inference_guardrails::{
        GuardStreamError, InferenceGuardContext, guard_model_response, guard_prompt, guard_stream,
        model_route_key,
    },
    request_tags::extract_request_tags,
    request_tracing::{StreamTrace, provider_operation_span, trace_provider_operation},
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
type SelectedProviderRoute = (gateway_core::ModelRoute, Arc<dyn ProviderClient>);

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

pub async fn v1_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ModelsListResponse>, AppError> {
    let auth = state
        .service
        .authenticate(extract_anthropic_authorization_header(&headers).as_deref())
        .await?;

    let models = state.service.list_models_for_api_key(&auth).await?;
    let data = models
        .into_iter()
        .map(|model| ModelCard {
            id: model.model_key,
            object: "model".to_string(),
            created: 0,
            owned_by: "gateway".to_string(),
        })
        .collect::<Vec<_>>();

    Ok(Json(ModelsListResponse {
        object: "list".to_string(),
        data,
    }))
}

pub async fn v1_messages(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    Json(request): Json<AnthropicMessagesRequest>,
) -> Response {
    match v1_messages_inner(state, request_id, headers, request).await {
        Ok(response) => response,
        Err(error) => anthropic_error_response(error.0),
    }
}

async fn v1_messages_inner(
    state: AppState,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    request: AnthropicMessagesRequest,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_id = canonical_request_id(request_id)?;
    let authorization = extract_anthropic_authorization_header(&headers);
    let auth = state.service.authenticate(authorization.as_deref()).await?;
    let mut core_request = anthropic_messages_request_to_core(&request);
    let log_request = core_chat_request_to_openai(&core_request);
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;

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
    let (eligible_route_count, selected) =
        select_first_eligible_route(&state.providers, &resolved.routes, requirements);

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count,
        stream = core_request.stream,
        required_capabilities = ?requirements.required_capability_names(),
        "anthropic messages request resolved"
    );

    let (route, provider) = match selected {
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

    let context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        &auth,
        request_headers,
    );

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
    if let Err(error) = guard_model_response(&state, &guard_context, &mut openai_value).await {
        record_guarded_non_stream_failure(
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
    response
}

pub async fn v1_chat_completions(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    Json(request): Json<ChatCompletionsRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_id = canonical_request_id(request_id)?;
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await?;
    let mut core_request = openai_chat_request_to_core(&request);
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;

    let request_headers = extract_request_headers(&headers);
    let request_tags = extract_request_tags(&headers)?;
    let mut request_log_context = state.service.begin_chat_request_log(
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
    let (eligible_route_count, selected) =
        select_first_eligible_route(&state.providers, &resolved.routes, requirements);

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count,
        stream = core_request.stream,
        required_capabilities = ?requirements.required_capability_names(),
        "chat completion request resolved"
    );

    let (route, provider) = match selected {
        Some(selection) => selection,
        None => {
            let error = no_compatible_route_error(requirements);
            state.metrics.record_chat_request(&ChatRequestMetric {
                labels: ChatMetricLabels {
                    requested_model: &resolved.selection.requested_model.model_key,
                    resolved_model: &resolved.selection.execution_model.model_key,
                    provider_key: "unavailable",
                    stream: core_request.stream,
                },
                status_code: i64::from(error.http_status_code()),
                outcome: error.error_type(),
                latency_seconds: latency_seconds_since(request_started_at),
            });
            state.metrics.record_tool_cardinality(
                &ChatMetricLabels {
                    requested_model: &resolved.selection.requested_model.model_key,
                    resolved_model: &resolved.selection.execution_model.model_key,
                    provider_key: "unavailable",
                    stream: core_request.stream,
                },
                request_log_context.operation,
                &request_log_context.tool_cardinality,
            );
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
        state.metrics.record_chat_request(&ChatRequestMetric {
            labels: labels.clone(),
            status_code: i64::from(error.http_status_code()),
            outcome: error.error_type(),
            latency_seconds: latency_seconds_since(request_started_at),
        });
        return Err(AppError(error));
    }

    let context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        &auth,
        request_headers,
    );

    if core_request.stream {
        let stream_started_at = Instant::now();
        let mut stream_trace = StreamTrace::new(
            "chat",
            &request_id,
            &route,
            provider.as_ref(),
            stream_started_at,
        );
        let provider_execution_span = provider_operation_span(
            &request_id,
            "chat",
            &auth,
            &resolved,
            &route,
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
                    &guard_context,
                    &request_log_context,
                    &route,
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
                best_effort_log_stream_result(
                    &state.service,
                    &auth,
                    &request_log_context,
                    gateway_service::StreamLogResultInput {
                        provider_key: route.provider_key.clone(),
                        icon_metadata: icon_metadata.clone(),
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
                state.metrics.record_chat_request(&ChatRequestMetric {
                    labels,
                    status_code: i64::from(gateway_error.http_status_code()),
                    outcome: gateway_error.error_type(),
                    latency_seconds: latency_seconds_since(request_started_at),
                });
                state.metrics.record_tool_cardinality(
                    &ChatMetricLabels {
                        requested_model: &resolved.selection.requested_model.model_key,
                        resolved_model: &resolved.selection.execution_model.model_key,
                        provider_key: &route.provider_key,
                        stream: true,
                    },
                    request_log_context.operation,
                    &request_log_context.tool_cardinality,
                );
                return Err(AppError(gateway_error));
            }
        };
        let stream = enforce_guarded_stream_after_provider(
            &state,
            &auth,
            &resolved,
            &request_log_context,
            &route,
            icon_metadata.clone(),
            request_started_at,
            attempt_started_at,
            &guard_context,
            stream,
        )
        .await?;
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
            provider_key: route.provider_key.clone(),
            icon_metadata: icon_metadata.clone(),
            started_at: request_started_at,
            attempt_started_at,
            finished: false,
            collector: state.service.new_stream_response_collector(),
            stream_trace,
        });

        let response = Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(CONTENT_TYPE, "text/event-stream; charset=utf-8")
            .header(CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(body_stream))
            .map_err(|error| {
                AppError(GatewayError::Internal(format!(
                    "failed to build streaming response: {error}"
                )))
            })?;

        return Ok(response);
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
    let mut value = match trace_provider_operation(
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
            state.metrics.record_tool_cardinality(
                &labels,
                request_log_context.operation,
                &request_log_context.tool_cardinality,
            );
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
            operation: "chat_completions",
        },
        usage_value_from_response(&value),
    )
    .await;
    if let Err(error) = guard_model_response(&state, &guard_context, &mut value).await {
        record_guarded_non_stream_failure(
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
    let attempt = success_attempt(&request_log_context, &route, false, attempt_started_at);
    let tool_cardinality = tool_cardinality_with_invoked(&request_log_context, &value);
    best_effort_log_non_stream_success(
        &state.service,
        &auth,
        &request_log_context,
        &route.provider_key,
        icon_metadata,
        latency_ms_since(request_started_at),
        tool_cardinality.invoked_tool_count.unwrap_or(0),
        &value,
        vec![attempt],
    )
    .await;
    state.metrics.record_chat_request(&ChatRequestMetric {
        labels,
        status_code: 200,
        outcome: "success",
        latency_seconds: latency_seconds_since(request_started_at),
    });
    state.metrics.record_tool_cardinality(
        &ChatMetricLabels {
            requested_model: &resolved.selection.requested_model.model_key,
            resolved_model: &resolved.selection.execution_model.model_key,
            provider_key: &route.provider_key,
            stream: false,
        },
        request_log_context.operation,
        &tool_cardinality,
    );
    let mut response = Json(value).into_response();
    if let Ok(request_id_header) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert("x-request-id", request_id_header);
    }
    Ok(response)
}

pub async fn v1_responses(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    Json(request): Json<ResponsesRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_id = canonical_request_id(request_id)?;
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await?;
    let mut core_request = openai_responses_request_to_core(&request);
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;

    let request_headers = extract_request_headers(&headers);
    let request_tags = extract_request_tags(&headers)?;
    let mut request_log_context = state.service.begin_responses_request_log(
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
    let (eligible_route_count, selected) =
        select_first_eligible_route(&state.providers, &resolved.routes, requirements);

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count,
        stream = core_request.stream,
        required_capabilities = ?requirements.required_capability_names(),
        "responses request resolved"
    );

    let (route, provider) = match selected {
        Some(selection) => selection,
        None => {
            let error = no_compatible_route_error(requirements);
            state.metrics.record_chat_request(&ChatRequestMetric {
                labels: ChatMetricLabels {
                    requested_model: &resolved.selection.requested_model.model_key,
                    resolved_model: &resolved.selection.execution_model.model_key,
                    provider_key: "unavailable",
                    stream: core_request.stream,
                },
                status_code: i64::from(error.http_status_code()),
                outcome: error.error_type(),
                latency_seconds: latency_seconds_since(request_started_at),
            });
            state.metrics.record_tool_cardinality(
                &ChatMetricLabels {
                    requested_model: &resolved.selection.requested_model.model_key,
                    resolved_model: &resolved.selection.execution_model.model_key,
                    provider_key: "unavailable",
                    stream: core_request.stream,
                },
                request_log_context.operation,
                &request_log_context.tool_cardinality,
            );
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
        state.metrics.record_chat_request(&ChatRequestMetric {
            labels: labels.clone(),
            status_code: i64::from(error.http_status_code()),
            outcome: error.error_type(),
            latency_seconds: latency_seconds_since(request_started_at),
        });
        return Err(AppError(error));
    }

    let context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        &auth,
        request_headers,
    );

    if core_request.stream {
        let stream_started_at = Instant::now();
        let mut stream_trace = StreamTrace::new(
            "responses",
            &request_id,
            &route,
            provider.as_ref(),
            stream_started_at,
        );
        let provider_execution_span = provider_operation_span(
            &request_id,
            "responses",
            &auth,
            &resolved,
            &route,
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
                    &guard_context,
                    &request_log_context,
                    &route,
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
                best_effort_log_stream_result(
                    &state.service,
                    &auth,
                    &request_log_context,
                    gateway_service::StreamLogResultInput {
                        provider_key: route.provider_key.clone(),
                        icon_metadata: icon_metadata.clone(),
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
                state.metrics.record_chat_request(&ChatRequestMetric {
                    labels: labels.clone(),
                    status_code: i64::from(gateway_error.http_status_code()),
                    outcome: gateway_error.error_type(),
                    latency_seconds: latency_seconds_since(request_started_at),
                });
                state.metrics.record_tool_cardinality(
                    &labels,
                    request_log_context.operation,
                    &request_log_context.tool_cardinality,
                );
                return Err(AppError(gateway_error));
            }
        };
        let stream = enforce_guarded_stream_after_provider(
            &state,
            &auth,
            &resolved,
            &request_log_context,
            &route,
            icon_metadata.clone(),
            request_started_at,
            attempt_started_at,
            &guard_context,
            stream,
        )
        .await?;
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
            provider_key: route.provider_key.clone(),
            icon_metadata: icon_metadata.clone(),
            started_at: request_started_at,
            attempt_started_at,
            finished: false,
            collector: state.service.new_stream_response_collector(),
            stream_trace,
        });

        let response = Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(CONTENT_TYPE, "text/event-stream; charset=utf-8")
            .header(CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(body_stream))
            .map_err(|error| {
                AppError(GatewayError::Internal(format!(
                    "failed to build responses streaming response: {error}"
                )))
            })?;

        return Ok(response);
    }

    let provider_execution_span = provider_operation_span(
        &request_id,
        "responses",
        &auth,
        &resolved,
        &route,
        provider.as_ref(),
        false,
    );
    let attempt_started_at = gateway_service::offset_now();
    let mut value = match trace_provider_operation(
        provider_execution_span,
        provider.responses(&core_request, &context),
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
            state.metrics.record_tool_cardinality(
                &labels,
                request_log_context.operation,
                &request_log_context.tool_cardinality,
            );
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
            operation: "responses",
        },
        usage_value_from_response(&value),
    )
    .await;
    if let Err(error) = guard_model_response(&state, &guard_context, &mut value).await {
        record_guarded_non_stream_failure(
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
    let attempt = success_attempt(&request_log_context, &route, false, attempt_started_at);
    let tool_cardinality = tool_cardinality_with_invoked(&request_log_context, &value);
    best_effort_log_non_stream_success(
        &state.service,
        &auth,
        &request_log_context,
        &route.provider_key,
        icon_metadata,
        latency_ms_since(request_started_at),
        tool_cardinality.invoked_tool_count.unwrap_or(0),
        &value,
        vec![attempt],
    )
    .await;
    state.metrics.record_chat_request(&ChatRequestMetric {
        labels,
        status_code: 200,
        outcome: "success",
        latency_seconds: latency_seconds_since(request_started_at),
    });
    state.metrics.record_tool_cardinality(
        &ChatMetricLabels {
            requested_model: &resolved.selection.requested_model.model_key,
            resolved_model: &resolved.selection.execution_model.model_key,
            provider_key: &route.provider_key,
            stream: false,
        },
        request_log_context.operation,
        &tool_cardinality,
    );
    let mut response = Json(value).into_response();
    if let Ok(request_id_header) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert("x-request-id", request_id_header);
    }
    Ok(response)
}

pub async fn v1_embeddings(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    Json(mut request): Json<EmbeddingsRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_span = Span::current();
    let request_id = canonical_request_id(request_id)?;
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await?;
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
    let (eligible_route_count, selected) =
        select_first_eligible_route(&state.providers, &resolved.routes, requirements);

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count,
        required_capabilities = ?requirements.required_capability_names(),
        "embeddings request resolved"
    );

    let (route, provider) = match selected {
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

    let context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        &auth,
        request_headers,
    );

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

#[tracing::instrument(
    name = "gateway.route.select",
    skip_all,
    fields(gateway.routes.candidate_count = routes.len())
)]
fn select_first_eligible_route(
    providers: &gateway_core::ProviderRegistry,
    routes: &[gateway_core::ModelRoute],
    requirements: CoreRequestRequirements,
) -> (usize, Option<SelectedProviderRoute>) {
    let mut eligible_route_count = 0usize;
    let mut selected = None;

    for route in routes {
        let Some(provider) = providers.get(&route.provider_key) else {
            continue;
        };
        let effective_capabilities =
            route_capabilities_for_request(provider.as_ref(), route, requirements)
                .intersect(route.capabilities);
        if supports_requirements(effective_capabilities, requirements) {
            eligible_route_count += 1;
            if selected.is_none() {
                selected = Some((route.clone(), provider));
            }
        }
    }

    (eligible_route_count, selected)
}

fn route_capabilities_for_request(
    provider: &dyn ProviderClient,
    route: &gateway_core::ModelRoute,
    requirements: CoreRequestRequirements,
) -> ProviderCapabilities {
    let mut capabilities = route_effective_provider_capabilities(provider, route);
    if provider.provider_type() == "github_copilot"
        && requirements.chat_completions
        && route
            .compatibility
            .github_copilot
            .as_ref()
            .is_some_and(|compatibility| {
                compatibility.chat_api
                    == Some(gateway_core::GitHubCopilotChatApi::AnthropicMessages)
            })
    {
        capabilities.json_schema = false;
    }
    capabilities
}

fn route_effective_provider_capabilities(
    provider: &dyn ProviderClient,
    route: &gateway_core::ModelRoute,
) -> ProviderCapabilities {
    if provider.provider_type() == "gcp_vertex" {
        return vertex_route_capabilities_for_upstream_model(Some(&route.upstream_model));
    }
    if provider.provider_type() == "github_copilot" {
        return gateway_core::github_copilot_route_capabilities(
            route.compatibility.github_copilot.as_ref(),
        );
    }

    provider.capabilities()
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

fn supports_requirements(
    capabilities: ProviderCapabilities,
    requirements: CoreRequestRequirements,
) -> bool {
    (!requirements.chat_completions || capabilities.chat_completions)
        && (!requirements.responses || capabilities.responses)
        && (!requirements.stream || capabilities.stream)
        && (!requirements.embeddings || capabilities.embeddings)
        && (!requirements.tools || capabilities.tools)
        && (!requirements.vision || capabilities.vision)
        && (!requirements.json_schema || capabilities.json_schema)
        && (!requirements.developer_role || capabilities.developer_role)
}

fn no_compatible_route_error(requirements: CoreRequestRequirements) -> GatewayError {
    let required = requirements.required_capability_names();
    let required = if required.is_empty() {
        "none".to_string()
    } else {
        required.join(", ")
    };
    GatewayError::InvalidRequest(format!(
        "no configured route supports requested capabilities ({required})"
    ))
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
    provider_key: String,
    icon_metadata: RequestLogIconMetadata,
    started_at: Instant,
    attempt_started_at: OffsetDateTime,
    finished: bool,
    collector: gateway_service::StreamResponseCollector,
    stream_trace: StreamTrace,
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
async fn record_guarded_non_stream_failure(
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
        Err(GuardStreamError { error, collector }) => {
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
        provider_key: route.provider_key.clone(),
        icon_metadata,
        started_at: request_started_at,
        attempt_started_at,
        finished: false,
        collector: state.service.new_stream_response_collector(),
        stream_trace,
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

fn wrap_stream_with_request_logging(
    state: LoggingBodyStreamState,
) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> {
    futures_stream::unfold(state, |mut state| async move {
        if state.finished {
            return None;
        }

        match state.upstream.next().await {
            Some(Ok(chunk)) => {
                let observation = state.collector.observe_chunk(chunk.as_ref());
                let ends_stream = observation.ends_stream;
                state.stream_trace.observe_chunk(chunk.len(), observation);
                if ends_stream {
                    finalize_stream(&mut state).await;
                }

                Some((Ok(chunk), state))
            }
            Some(Err(error)) => {
                state.finished = true;
                state
                    .stream_trace
                    .finish("stream_transport_error", Some("stream_transport_error"));
                let error_message = error.to_string();
                let retryable = error.is_retryable();
                let gateway_error = GatewayError::from(error);
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
                Some((Err(std::io::Error::other(error_message)), state))
            }
            None => {
                finalize_stream(&mut state).await;
                None
            }
        }
    })
}

async fn finalize_stream(state: &mut LoggingBodyStreamState) {
    if state.finished {
        return;
    }
    state.finished = true;
    state.collector.finish();
    let failure = state.collector.failure().cloned();
    state.stream_trace.finish(
        if failure.is_some() {
            "stream_error_event"
        } else {
            "complete"
        },
        failure.as_ref().map(|_| "stream_error_event"),
    );
    // A provider may bill tokens and then emit an error event. Charge whenever
    // usage was reported; only a failure with no usage leaves the ledger alone.
    if failure.is_none() || state.collector.usage().is_some() {
        let labels = state.metric_labels();
        finalize_successful_usage_accounting_from_parts(
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
    }
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
    finalize_successful_usage_accounting_from_parts(
        &state.service,
        &state.metrics,
        context,
        provider_usage,
    )
    .await;
}

async fn finalize_successful_usage_accounting_from_parts(
    service: &std::sync::Arc<AppGatewayService>,
    metrics: &crate::observability::GatewayMetrics,
    context: UsageAccountingContext<'_>,
    provider_usage: Option<Value>,
) {
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
        Ok(usage) => record_usage_metrics_from_ref(metrics, &context.labels, &usage),
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
        }
    }
}

fn extract_authorization_header(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
}

fn extract_anthropic_authorization_header(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = extract_authorization_header(headers) {
        return Some(value.to_string());
    }
    headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .map(|value| format!("Bearer {value}"))
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
    use axum::http::{HeaderMap, HeaderValue};
    use futures_util::{StreamExt, stream};
    use gateway_core::protocol::openai::ChatMessage;
    use gateway_core::{
        ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthenticatedApiKey, BudgetCadence,
        BudgetRepository, ChatCompletionsRequest, CoreChatMessage, CoreChatRequest,
        CoreEmbeddingsRequest, CoreRequestRequirements, CoreResponsesRequest, GatewayError,
        GitHubCopilotChatApi, GitHubCopilotRouteCompatibility, GitHubCopilotUpstreamSupports,
        ModelRoute, Money4, ProviderCapabilities, ProviderClient, ProviderError, ProviderRegistry,
        ProviderRequestContext, ProviderStream, RequestAttemptStatus, RequestTags,
        RouteCompatibility, SeedApiKey, SeedBudget, SeedModel, SeedModelRoute, SeedProvider,
        SeedServiceAccount, SeedTeam, hash_gateway_key_secret,
    };
    use gateway_service::{GatewayService, WeightedRoutePlanner};
    use gateway_store::{
        AnyStore, GatewayStore, StoreConnectionOptions, run_migrations_with_options,
    };
    use serde_json::{Value, json};
    use tower_http::request_id::RequestId;

    use super::{
        LoggingBodyStreamState, anthropic_error_response, api_health, build_provider_context,
        canonical_request_id, extract_anthropic_authorization_header, request_log_icon_metadata,
        route_capabilities_for_request, route_effective_provider_capabilities,
        select_first_eligible_route, split_partial_provider_error,
        wrap_stream_with_request_logging,
    };
    use crate::http::request_tracing::StreamTrace;
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
                provider_key: self.route.provider_key.clone(),
                icon_metadata,
                started_at: Instant::now(),
                attempt_started_at: gateway_service::offset_now(),
                finished: false,
                collector: self.service.new_stream_response_collector(),
                stream_trace: StreamTrace::new(
                    "chat",
                    request_id,
                    &self.route,
                    &self.provider,
                    Instant::now(),
                ),
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
    async fn terminal_stream_forwards_trailing_usage_before_completion() {
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
        assert_eq!(harness.metrics.test_snapshot().requests, 0);
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

    async fn seed_stream_cancellation_test(store: &AnyStore) {
        store
            .seed_from_inputs(
                &[SeedProvider {
                    provider_key: "vertex".to_string(),
                    provider_type: "openai_compat".to_string(),
                    config: json!({}),
                    secrets: None,
                }],
                &[SeedModel {
                    model_key: "fast".to_string(),
                    alias_target_model_key: None,
                    description: None,
                    tags: Vec::new(),
                    rank: 0,
                    routes: vec![SeedModelRoute {
                        provider_key: "vertex".to_string(),
                        upstream_model: "fast-upstream".to_string(),
                        priority: 0,
                        weight: 1.0,
                        enabled: true,
                        context_window_tokens: None,
                        pricing_override: None,
                        extra_headers: Default::default(),
                        extra_body: Default::default(),
                        capabilities: ProviderCapabilities::all_enabled(),
                        compatibility: RouteCompatibility::default(),
                    }],
                    allowlist: None,
                }],
                &[SeedApiKey {
                    name: "Stream Test Key".to_string(),
                    public_id: "streamtest".to_string(),
                    secret_hash: hash_gateway_key_secret("cancel-secret").expect("hash test key"),
                    service_account_key: "stream-test".to_string(),
                    allowed_models: vec!["fast".to_string()],
                }],
                &[SeedServiceAccount {
                    service_account_key: "stream-test".to_string(),
                    service_account_name: "Stream Test".to_string(),
                    team_key: "stream-test".to_string(),
                    tags: None,
                    budget: SeedBudget {
                        cadence: BudgetCadence::Daily,
                        amount_usd: Money4::from_scaled(250_000),
                        hard_limit: true,
                        timezone: "UTC".to_string(),
                    },
                    managed_api_keys: Vec::new(),
                }],
                &[],
                &[],
                &[SeedTeam {
                    team_key: "stream-test".to_string(),
                    team_name: "Stream Test".to_string(),
                    tags: None,
                }],
                &[],
            )
            .await
            .expect("seed cancellation test");
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
                .0
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
            selected.expect("vision-capable route").0.upstream_model,
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
    fn anthropic_authorization_accepts_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("gw-test-key"));

        assert_eq!(
            extract_anthropic_authorization_header(&headers).as_deref(),
            Some("Bearer gw-test-key")
        );
    }

    #[test]
    fn split_partial_provider_error_preserves_usage_accounting_signal() {
        let (source, provider_usage) = split_partial_provider_error(ProviderError::PartialUsage {
            source: Box::new(ProviderError::UpstreamHttp {
                status: 429,
                body: "quota exhausted".to_string(),
            }),
            provider_usage: Some(json!({"prompt_tokens": 4, "total_tokens": 4})),
        });

        assert_eq!(
            provider_usage,
            Some(Some(json!({"prompt_tokens": 4, "total_tokens": 4})))
        );
        match source {
            ProviderError::UpstreamHttp { status, body } => {
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
}
