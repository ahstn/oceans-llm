use std::{collections::BTreeMap, sync::Arc, time::Instant};

use axum::{
    Json,
    body::Body,
    extract::{Extension, State},
    http::{
        HeaderMap, HeaderValue,
        header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use futures_util::{StreamExt, stream};
use gateway_core::{
    AuthenticatedApiKey, ChatCompletionsRequest, CoreRequestRequirements, EmbeddingsRequest,
    GatewayError, ModelsListResponse, ProviderCapabilities, ProviderClient, ProviderError,
    ProviderRequestContext, RequestAttemptRecord, RequestAttemptStatus, RequestToolCardinality,
    ResponsesRequest, openai_chat_request_to_core, openai_embeddings_request_to_core,
    openai_responses_request_to_core, protocol::openai::ModelCard,
};
use gateway_service::{
    RequestLogIconMetadata, ResolvedProviderConnection, resolve_model_icon_key,
    resolve_provider_display_from_parts,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tower_http::request_id::RequestId;
use tracing::{Instrument, Span, field};

use crate::http::{
    error::AppError,
    request_tags::extract_request_tags,
    state::{AppGatewayService, AppState},
};
use crate::observability::{ChatMetricLabels, ChatRequestMetric};

type SelectedProviderRoute = (gateway_core::ModelRoute, Arc<dyn ProviderClient>);

pub async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn readyz(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    state.service.check_readiness().await?;
    Ok(Json(json!({ "status": "ready" })))
}

pub async fn api_health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "gateway" }))
}

pub async fn v1_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ModelsListResponse>, AppError> {
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
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
    let core_request = openai_chat_request_to_core(&request);
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;

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
    let labels = ChatMetricLabels {
        requested_model: &resolved.selection.requested_model.model_key,
        resolved_model: &resolved.selection.execution_model.model_key,
        provider_key: &route.provider_key,
        stream: core_request.stream,
    };
    record_provider_execution_span_fields(&request_span, &route.provider_key);

    if let Err(error) = state
        .service
        .enforce_pre_provider_budget(&auth, &request_id, OffsetDateTime::now_utc())
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
        request_headers,
    );

    if core_request.stream {
        let provider_execution_span = tracing::info_span!(
            "provider_execution",
            request_id = %request_id,
            requested_model = %resolved.selection.requested_model.model_key,
            resolved_model = %resolved.selection.execution_model.model_key,
            provider = %route.provider_key,
            stream = true,
            ownership_kind = %auth.owner_kind.as_str(),
        );
        let attempt_started_at = gateway_service::offset_now();
        let stream = match provider
            .chat_completions_stream(&core_request, &context)
            .instrument(provider_execution_span)
            .await
        {
            Ok(stream) => stream,
            Err(error) => {
                let (gateway_error, attempt) = provider_error_attempt(
                    &request_log_context,
                    &route,
                    RequestAttemptStatus::StreamStartError,
                    true,
                    attempt_started_at,
                    error,
                    requirements,
                );
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

    let provider_execution_span = tracing::info_span!(
        "provider_execution",
        request_id = %request_id,
        requested_model = %resolved.selection.requested_model.model_key,
        resolved_model = %resolved.selection.execution_model.model_key,
        provider = %route.provider_key,
        stream = false,
        ownership_kind = %auth.owner_kind.as_str(),
    );
    let attempt_started_at = gateway_service::offset_now();
    let value = match provider
        .chat_completions(&core_request, &context)
        .instrument(provider_execution_span)
        .await
    {
        Ok(value) => normalize_response_model(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            let (error, attempt) = provider_error_attempt(
                &request_log_context,
                &route,
                RequestAttemptStatus::ProviderError,
                false,
                attempt_started_at,
                error,
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
    let attempt = success_attempt(&request_log_context, &route, false, attempt_started_at);
    let tool_cardinality = tool_cardinality_with_invoked(&request_log_context, &value);
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
    let core_request = openai_responses_request_to_core(&request);
    let requirements = core_request.requirements();
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
        .await?;

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
    let labels = ChatMetricLabels {
        requested_model: &resolved.selection.requested_model.model_key,
        resolved_model: &resolved.selection.execution_model.model_key,
        provider_key: &route.provider_key,
        stream: core_request.stream,
    };
    record_provider_execution_span_fields(&request_span, &route.provider_key);

    if let Err(error) = state
        .service
        .enforce_pre_provider_budget(&auth, &request_id, OffsetDateTime::now_utc())
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
        request_headers,
    );

    if core_request.stream {
        let provider_execution_span = tracing::info_span!(
            "provider_execution",
            request_id = %request_id,
            requested_model = %resolved.selection.requested_model.model_key,
            resolved_model = %resolved.selection.execution_model.model_key,
            provider = %route.provider_key,
            stream = true,
            ownership_kind = %auth.owner_kind.as_str(),
        );
        let attempt_started_at = gateway_service::offset_now();
        let stream = match provider
            .responses_stream(&core_request, &context)
            .instrument(provider_execution_span)
            .await
        {
            Ok(stream) => stream,
            Err(error) => {
                let (gateway_error, attempt) = provider_error_attempt(
                    &request_log_context,
                    &route,
                    RequestAttemptStatus::StreamStartError,
                    true,
                    attempt_started_at,
                    error,
                    requirements,
                );
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

    let provider_execution_span = tracing::info_span!(
        "provider_execution",
        request_id = %request_id,
        requested_model = %resolved.selection.requested_model.model_key,
        resolved_model = %resolved.selection.execution_model.model_key,
        provider = %route.provider_key,
        stream = false,
        ownership_kind = %auth.owner_kind.as_str(),
    );
    let attempt_started_at = gateway_service::offset_now();
    let value = match provider
        .responses(&core_request, &context)
        .instrument(provider_execution_span)
        .await
    {
        Ok(value) => normalize_response_model(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            let (error, attempt) = provider_error_attempt(
                &request_log_context,
                &route,
                RequestAttemptStatus::ProviderError,
                false,
                attempt_started_at,
                error,
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
    let attempt = success_attempt(&request_log_context, &route, false, attempt_started_at);
    let tool_cardinality = tool_cardinality_with_invoked(&request_log_context, &value);
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
    Json(request): Json<EmbeddingsRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
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
    let request_headers = extract_request_headers(&headers);
    let request_tags = extract_request_tags(&headers)?;
    let request_log_context = state.service.begin_embeddings_request_log(
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
    let icon_metadata = request_log_icon_metadata(
        &route,
        resolved.provider_connections.get(&route.provider_key),
        &resolved.selection.execution_model.model_key,
        &resolved.selection.requested_model.model_key,
    );
    let labels = ChatMetricLabels {
        requested_model: &resolved.selection.requested_model.model_key,
        resolved_model: &resolved.selection.execution_model.model_key,
        provider_key: &route.provider_key,
        stream: false,
    };

    state
        .service
        .enforce_pre_provider_budget(&auth, &request_id, OffsetDateTime::now_utc())
        .await?;

    let context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        request_headers,
    );

    let attempt_started_at = gateway_service::offset_now();
    let value = match provider.embeddings(&core_request, &context).await {
        Ok(value) => normalize_response_model(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            let (error, attempt) = provider_error_attempt(
                &request_log_context,
                &route,
                RequestAttemptStatus::ProviderError,
                false,
                attempt_started_at,
                error,
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
        let effective_capabilities = provider.capabilities().intersect(route.capabilities);
        if supports_requirements(effective_capabilities, requirements) {
            eligible_route_count += 1;
            if selected.is_none() {
                selected = Some((route.clone(), provider));
            }
        }
    }

    (eligible_route_count, selected)
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
    request_headers: BTreeMap<String, String>,
) -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: request_id.to_string(),
        model_key: model_key.to_string(),
        provider_key: route.provider_key.clone(),
        upstream_model: route.upstream_model.clone(),
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
}

struct UsageAccountingContext<'a> {
    auth: &'a AuthenticatedApiKey,
    model: &'a gateway_core::GatewayModel,
    route: &'a gateway_core::ModelRoute,
    request_id: &'a str,
    labels: ChatMetricLabels<'a>,
    operation: &'static str,
}

fn wrap_stream_with_request_logging(
    state: LoggingBodyStreamState,
) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> {
    stream::unfold(state, |mut state| async move {
        if state.finished {
            return None;
        }

        match state.upstream.next().await {
            Some(Ok(chunk)) => {
                state.collector.observe_chunk(chunk.as_ref());

                Some((Ok(chunk), state))
            }
            Some(Err(error)) => {
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
                    labels: ChatMetricLabels {
                        requested_model: &state.requested_model_key,
                        resolved_model: &state.resolved_model_key,
                        provider_key: &state.provider_key,
                        stream: true,
                    },
                    status_code: i64::from(gateway_error.http_status_code()),
                    outcome: gateway_error.error_type(),
                    latency_seconds: latency_seconds_since(state.started_at),
                });
                state.metrics.record_tool_cardinality(
                    &ChatMetricLabels {
                        requested_model: &state.requested_model_key,
                        resolved_model: &state.resolved_model_key,
                        provider_key: &state.provider_key,
                        stream: true,
                    },
                    state.request_log_context.operation,
                    &RequestToolCardinality {
                        invoked_tool_count: Some(state.collector.invoked_tool_count()),
                        ..state.request_log_context.tool_cardinality
                    },
                );
                state.finished = true;
                Some((Err(std::io::Error::other(error_message)), state))
            }
            None => {
                state.collector.finish();
                let failure = state.collector.failure().cloned();
                if failure.is_none() {
                    let labels = ChatMetricLabels {
                        requested_model: &state.requested_model_key,
                        resolved_model: &state.resolved_model_key,
                        provider_key: &state.provider_key,
                        stream: true,
                    };
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
                        collector: state.collector,
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
                    labels: ChatMetricLabels {
                        requested_model: &state.requested_model_key,
                        resolved_model: &state.resolved_model_key,
                        provider_key: &state.provider_key,
                        stream: true,
                    },
                    status_code,
                    outcome,
                    latency_seconds: latency_seconds_since(state.started_at),
                });
                state.metrics.record_tool_cardinality(
                    &ChatMetricLabels {
                        requested_model: &state.requested_model_key,
                        resolved_model: &state.resolved_model_key,
                        provider_key: &state.provider_key,
                        stream: true,
                    },
                    state.request_log_context.operation,
                    &tool_cardinality,
                );
                None
            }
        }
    })
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
    span.record("http.route", field::display(route_path));
    span.record(
        "requested_model",
        field::display(&resolved.selection.requested_model.model_key),
    );
    span.record(
        "resolved_model",
        field::display(&resolved.selection.execution_model.model_key),
    );
    span.record("stream", stream);
    span.record("ownership_kind", field::display(auth.owner_kind.as_str()));
}

fn record_provider_execution_span_fields(span: &Span, provider_key: &str) {
    span.record("provider", field::display(provider_key));
}

fn record_usage_metrics_from_ref(
    metrics: &crate::observability::GatewayMetrics,
    labels: &ChatMetricLabels<'_>,
    usage: &gateway_service::RecordedChatUsage,
) {
    if matches!(
        usage.disposition,
        gateway_service::budget_guard::BudgetGuardDisposition::Inserted
    ) {
        metrics.record_usage(
            labels,
            usage.pricing_status.as_str(),
            usage.prompt_tokens,
            usage.completion_tokens,
            usage.total_tokens,
            usage.cost_usd,
        );
    }
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
    use axum::Extension;
    use axum::http::HeaderValue;
    use gateway_core::GatewayError;
    use tower_http::request_id::RequestId;

    use super::canonical_request_id;

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
}
