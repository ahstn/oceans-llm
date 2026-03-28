use std::{collections::BTreeMap, sync::Arc, time::Instant};

use axum::{
    Json,
    body::Body,
    extract::State,
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
    ProviderRequestContext, RequestLogRecord, RequestTags, openai_chat_request_to_core,
    openai_embeddings_request_to_core, protocol::openai::ModelCard,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use tracing::{Instrument, Span, field};
use uuid::Uuid;

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
    headers: HeaderMap,
    Json(request): Json<ChatCompletionsRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
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

    let request_id = extract_request_id(&headers);
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
    record_request_span_fields(&request_span, &auth, &resolved, core_request.stream);
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
                fallback_used: false,
                latency_seconds: latency_seconds_since(request_started_at),
            });
            return Err(AppError(error));
        }
    };
    let labels = ChatMetricLabels {
        requested_model: &resolved.selection.requested_model.model_key,
        resolved_model: &resolved.selection.execution_model.model_key,
        provider_key: &route.provider_key,
        stream: core_request.stream,
    };
    if let Err(error) = state
        .service
        .enforce_pre_provider_budget(&auth, &request_id, OffsetDateTime::now_utc())
        .await
    {
        state.metrics.record_chat_request(&ChatRequestMetric {
            labels: labels.clone(),
            status_code: i64::from(error.http_status_code()),
            outcome: error.error_type(),
            fallback_used: false,
            latency_seconds: latency_seconds_since(request_started_at),
        });
        return Err(AppError(error));
    }

    state.metrics.record_provider_attempt(&labels);
    record_attempt_span_fields(&request_span, &route.provider_key, 1, false);

    let context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        request_headers,
    );

    if core_request.stream {
        let attempt_span = tracing::info_span!(
            "provider_attempt",
            request_id = %request_id,
            requested_model = %resolved.selection.requested_model.model_key,
            resolved_model = %resolved.selection.execution_model.model_key,
            provider = %route.provider_key,
            stream = true,
            attempt_count = 1_i64,
            fallback_used = false,
            ownership_kind = %auth.owner_kind.as_str(),
        );
        let stream = match provider
            .chat_completions_stream(&core_request, &context)
            .instrument(attempt_span)
            .await
        {
            Ok(stream) => stream,
            Err(error) => {
                let gateway_error = map_operation_provider_error(error, requirements);
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
                        attempt_count: 1,
                        latency_ms: latency_ms_since(request_started_at),
                        collector: state.service.new_stream_response_collector(),
                        failure: Some(gateway_service::StreamFailureSummary {
                            status_code: gateway_error.http_status_code().into(),
                            error_code: gateway_error.error_code().to_string(),
                        }),
                    },
                )
                .await;
                state.metrics.record_chat_request(&ChatRequestMetric {
                    labels,
                    status_code: i64::from(gateway_error.http_status_code()),
                    outcome: gateway_error.error_type(),
                    fallback_used: false,
                    latency_seconds: latency_seconds_since(request_started_at),
                });
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
            started_at: request_started_at,
            finished: false,
            attempt_count: 1,
            collector: state.service.new_stream_response_collector(),
        });

        let mut response = Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(CONTENT_TYPE, "text/event-stream; charset=utf-8")
            .header(CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(body_stream))
            .map_err(|error| {
                AppError(GatewayError::Internal(format!(
                    "failed to build streaming response: {error}"
                )))
            })?;

        if let Ok(request_id_header) = HeaderValue::from_str(&request_id) {
            response
                .headers_mut()
                .insert("x-request-id", request_id_header);
        }

        return Ok(response);
    }

    let attempt_span = tracing::info_span!(
        "provider_attempt",
        request_id = %request_id,
        requested_model = %resolved.selection.requested_model.model_key,
        resolved_model = %resolved.selection.execution_model.model_key,
        provider = %route.provider_key,
        stream = false,
        attempt_count = 1_i64,
        fallback_used = false,
        ownership_kind = %auth.owner_kind.as_str(),
    );
    let value = provider
        .chat_completions(&core_request, &context)
        .instrument(attempt_span)
        .await
        .map_err(|error| map_operation_provider_error(error, requirements));
    let value = match value {
        Ok(value) => normalize_response_model(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            best_effort_log_non_stream_failure(
                &state.service,
                &auth,
                &request_log_context,
                &route.provider_key,
                1,
                latency_ms_since(request_started_at),
                &error,
            )
            .await;
            state.metrics.record_chat_request(&ChatRequestMetric {
                labels,
                status_code: i64::from(error.http_status_code()),
                outcome: error.error_type(),
                fallback_used: false,
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
        1,
        latency_ms_since(request_started_at),
        &value,
    )
    .await;
    state.metrics.record_chat_request(&ChatRequestMetric {
        labels,
        status_code: 200,
        outcome: "success",
        fallback_used: false,
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

pub async fn v1_embeddings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<EmbeddingsRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
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
    let request_id = extract_request_id(&headers);
    let request_headers = extract_request_headers(&headers);
    let request_tags = extract_request_tags(&headers)?;
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

    let value = provider
        .embeddings(&core_request, &context)
        .await
        .map_err(|error| map_operation_provider_error(error, requirements));
    let value = match value {
        Ok(value) => normalize_response_model(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            best_effort_log_request(
                &state.service,
                &auth,
                &request_id,
                &resolved.selection.requested_model.model_key,
                &resolved.selection.execution_model.model_key,
                &request_tags,
                RequestLogSummary::failure(
                    RequestOperation::Embeddings,
                    route.provider_key.clone(),
                    false,
                    latency_ms_since(request_started_at),
                    error.http_status_code().into(),
                    error.error_code().to_string(),
                ),
            )
            .await;
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
            operation: "embeddings",
        },
        usage_value_from_response(&value),
    )
    .await;
    best_effort_log_request(
        &state.service,
        &auth,
        &request_id,
        &resolved.selection.requested_model.model_key,
        &resolved.selection.execution_model.model_key,
        &request_tags,
        RequestLogSummary::success(
            RequestOperation::Embeddings,
            route.provider_key.clone(),
            false,
            latency_ms_since(request_started_at),
            usage_from_response(&value),
        ),
    )
    .await;

    let mut response = Json(value).into_response();
    if let Ok(request_id_header) = HeaderValue::from_str(&request_id) {
        response
            .headers_mut()
            .insert("x-request-id", request_id_header);
    }
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
    }
}

#[derive(Debug, Clone, Default)]
struct UsageSummary {
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

#[derive(Debug, Clone)]
enum RequestOperation {
    Embeddings,
}

impl RequestOperation {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Embeddings => "embeddings",
        }
    }
}

#[derive(Debug, Clone)]
struct RequestLogSummary {
    operation: RequestOperation,
    provider_key: String,
    stream: bool,
    status_code: i64,
    error_code: Option<String>,
    latency_ms: i64,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

impl RequestLogSummary {
    fn success(
        operation: RequestOperation,
        provider_key: String,
        stream: bool,
        latency_ms: i64,
        usage: UsageSummary,
    ) -> Self {
        Self {
            operation,
            provider_key,
            stream,
            status_code: 200,
            error_code: None,
            latency_ms,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
        }
    }

    fn failure(
        operation: RequestOperation,
        provider_key: String,
        stream: bool,
        latency_ms: i64,
        status_code: i64,
        error_code: String,
    ) -> Self {
        Self {
            operation,
            provider_key,
            stream,
            status_code,
            error_code: Some(error_code),
            latency_ms,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        }
    }
}

struct LoggingBodyStreamState {
    upstream: gateway_core::ProviderStream,
    service: std::sync::Arc<AppGatewayService>,
    metrics: std::sync::Arc<crate::observability::GatewayMetrics>,
    auth: AuthenticatedApiKey,
    request_log_context: gateway_service::ChatRequestLogContext,
    requested_model_key: String,
    resolved_model_key: String,
    execution_model: gateway_core::GatewayModel,
    route: gateway_core::ModelRoute,
    provider_key: String,
    started_at: Instant,
    finished: bool,
    attempt_count: usize,
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
                        attempt_count: state.attempt_count,
                        latency_ms: latency_ms_since(state.started_at),
                        collector: state.collector.clone(),
                        failure: Some(gateway_service::StreamFailureSummary {
                            status_code: gateway_error.http_status_code().into(),
                            error_code: gateway_error.error_code().to_string(),
                        }),
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
                    fallback_used: state.attempt_count > 1,
                    latency_seconds: latency_seconds_since(state.started_at),
                });
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
                            operation: "chat_completions",
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
                best_effort_log_stream_result(
                    &state.service,
                    &state.auth,
                    &state.request_log_context,
                    gateway_service::StreamLogResultInput {
                        provider_key: state.provider_key.clone(),
                        attempt_count: state.attempt_count,
                        latency_ms: latency_ms_since(state.started_at),
                        collector: state.collector,
                        failure: failure.clone(),
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
                    fallback_used: state.attempt_count > 1,
                    latency_seconds: latency_seconds_since(state.started_at),
                });
                None
            }
        }
    })
}

async fn best_effort_log_non_stream_success(
    service: &std::sync::Arc<AppGatewayService>,
    auth: &AuthenticatedApiKey,
    context: &gateway_service::ChatRequestLogContext,
    provider_key: &str,
    attempt_count: usize,
    latency_ms: i64,
    response_body: &Value,
) {
    if let Err(error) = service
        .log_non_stream_success(
            auth,
            context,
            provider_key,
            attempt_count,
            latency_ms,
            response_body,
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

async fn best_effort_log_non_stream_failure(
    service: &std::sync::Arc<AppGatewayService>,
    auth: &AuthenticatedApiKey,
    context: &gateway_service::ChatRequestLogContext,
    provider_key: &str,
    attempt_count: usize,
    latency_ms: i64,
    gateway_error: &GatewayError,
) {
    if let Err(error) = service
        .log_non_stream_failure(
            auth,
            context,
            provider_key,
            attempt_count,
            latency_ms,
            gateway_error,
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
    context: &gateway_service::ChatRequestLogContext,
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

async fn best_effort_log_request(
    service: &std::sync::Arc<AppGatewayService>,
    auth: &AuthenticatedApiKey,
    request_id: &str,
    model_key: &str,
    resolved_model_key: &str,
    request_tags: &RequestTags,
    summary: RequestLogSummary,
) {
    let metadata = request_log_metadata(summary.stream, summary.operation);
    let log = RequestLogRecord {
        request_log_id: Uuid::new_v4(),
        request_id: request_id.to_string(),
        api_key_id: auth.id,
        user_id: None,
        team_id: None,
        model_key: model_key.to_string(),
        resolved_model_key: resolved_model_key.to_string(),
        provider_key: summary.provider_key,
        status_code: Some(summary.status_code),
        latency_ms: Some(summary.latency_ms),
        prompt_tokens: summary.prompt_tokens,
        completion_tokens: summary.completion_tokens,
        total_tokens: summary.total_tokens,
        has_payload: false,
        request_payload_truncated: false,
        response_payload_truncated: false,
        request_tags: request_tags.clone(),
        error_code: summary.error_code,
        metadata,
        occurred_at: OffsetDateTime::now_utc(),
    };

    if let Err(error) = service.log_request_if_enabled(auth, log).await {
        tracing::warn!(
            request_id = %request_id,
            model_key = %model_key,
            error = %error,
            "request logging failed"
        );
    }
}

fn request_log_metadata(stream: bool, operation: RequestOperation) -> Map<String, Value> {
    let mut metadata = Map::new();
    metadata.insert(
        "operation".to_string(),
        Value::String(operation.as_str().to_string()),
    );
    metadata.insert("stream".to_string(), Value::Bool(stream));
    metadata
}

fn normalize_response_model(mut value: Value, model_key: &str) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("model".to_string(), Value::String(model_key.to_string()));
    }
    value
}

fn usage_from_response(value: &Value) -> UsageSummary {
    usage_summary_from_value(value.get("usage"))
}

fn usage_value_from_response(value: &Value) -> Option<Value> {
    value.get("usage").cloned()
}

fn usage_summary_from_value(value: Option<&Value>) -> UsageSummary {
    let Some(usage) = value.and_then(Value::as_object) else {
        return UsageSummary::default();
    };

    let prompt_tokens = usage.get("prompt_tokens").and_then(Value::as_i64);
    let completion_tokens = usage.get("completion_tokens").and_then(Value::as_i64);
    let total_tokens = match usage.get("total_tokens").and_then(Value::as_i64) {
        some @ Some(_) => some,
        None => match (prompt_tokens, completion_tokens) {
            (Some(prompt), Some(completion)) => prompt.checked_add(completion),
            _ => None,
        },
    };

    UsageSummary {
        prompt_tokens,
        completion_tokens,
        total_tokens,
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
) {
    span.record("http.route", field::display("/v1/chat/completions"));
    span.record(
        "requested_model",
        field::display(&resolved.selection.requested_model.model_key),
    );
    span.record(
        "resolved_model",
        field::display(&resolved.selection.execution_model.model_key),
    );
    span.record("stream", stream);
    span.record("fallback_used", false);
    span.record("ownership_kind", field::display(auth.owner_kind.as_str()));
}

fn record_attempt_span_fields(
    span: &Span,
    provider_key: &str,
    attempt_count: usize,
    fallback_used: bool,
) {
    span.record("provider", field::display(provider_key));
    span.record(
        "attempt_count",
        i64::try_from(attempt_count).unwrap_or(i64::MAX),
    );
    span.record("fallback_used", fallback_used);
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

fn extract_request_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string())
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
