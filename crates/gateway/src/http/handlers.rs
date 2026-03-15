use std::{collections::BTreeMap, time::Instant};

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
    GatewayError, ModelsListResponse, ProviderCapabilities, ProviderError, ProviderRequestContext,
    RequestLogRecord, openai_chat_request_to_core, openai_embeddings_request_to_core,
    protocol::openai::ModelCard,
};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::http::{
    error::AppError,
    state::{AppGatewayService, AppState},
};

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

    let idempotency_key = extract_idempotency_key(&headers).map(str::to_string);
    let request_id = extract_request_id(&headers);
    let request_headers = extract_request_headers(&headers);
    let allow_fallback = !core_request.stream && idempotency_key.is_some();

    let mut eligible = Vec::new();
    for route in &resolved.routes {
        let Some(provider) = state.providers.get(&route.provider_key) else {
            continue;
        };
        let effective_capabilities = provider.capabilities().intersect(route.capabilities);
        if supports_requirements(effective_capabilities, requirements) {
            eligible.push((route.clone(), provider));
        }
    }

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count = eligible.len(),
        stream = core_request.stream,
        required_capabilities = ?requirements.required_capability_names(),
        fallback_allowed = allow_fallback,
        "chat completion request resolved"
    );

    if eligible.is_empty() {
        return Err(AppError(no_compatible_route_error(requirements)));
    }

    if core_request.stream || !allow_fallback {
        let (route, provider) = eligible
            .into_iter()
            .next()
            .expect("eligible routes checked as non-empty");

        let context = build_provider_context(
            &request_id,
            &resolved.selection.requested_model.model_key,
            &route,
            idempotency_key.clone(),
            request_headers,
        );

        if core_request.stream {
            let stream = match provider
                .chat_completions_stream(&core_request, &context)
                .await
            {
                Ok(stream) => stream,
                Err(error) => {
                    let gateway_error = GatewayError::from(error);
                    best_effort_log_request(
                        &state.service,
                        &auth,
                        &request_id,
                        &resolved.selection.requested_model.model_key,
                        &resolved.selection.execution_model.model_key,
                        RequestLogSummary::failure(
                            RequestOperation::ChatCompletions,
                            route.provider_key.clone(),
                            1,
                            true,
                            latency_ms_since(request_started_at),
                            gateway_error.http_status_code().into(),
                            gateway_error.error_code().to_string(),
                        ),
                    )
                    .await;
                    return Err(AppError(gateway_error));
                }
            };
            let body_stream = wrap_stream_with_request_logging(LoggingBodyStreamState {
                upstream: stream,
                service: state.service.clone(),
                auth: auth.clone(),
                request_id: request_id.clone(),
                requested_model_key: resolved.selection.requested_model.model_key.clone(),
                resolved_model_key: resolved.selection.execution_model.model_key.clone(),
                execution_model: resolved.selection.execution_model.clone(),
                route: route.clone(),
                provider_key: route.provider_key.clone(),
                started_at: request_started_at,
                finished: false,
                failure: None,
                attempt_count: 1,
                usage: None,
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

        let value = provider
            .chat_completions(&core_request, &context)
            .await
            .map_err(GatewayError::from);
        let value = match value {
            Ok(value) => {
                normalize_response_model(value, &resolved.selection.requested_model.model_key)
            }
            Err(error) => {
                best_effort_log_request(
                    &state.service,
                    &auth,
                    &request_id,
                    &resolved.selection.requested_model.model_key,
                    &resolved.selection.execution_model.model_key,
                    RequestLogSummary::failure(
                        RequestOperation::ChatCompletions,
                        route.provider_key.clone(),
                        1,
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
        state
            .service
            .record_chat_usage(
                &auth,
                &resolved.selection.execution_model,
                &route,
                &request_id,
                usage_value_from_response(&value),
                OffsetDateTime::now_utc(),
            )
            .await?;
        best_effort_log_request(
            &state.service,
            &auth,
            &request_id,
            &resolved.selection.requested_model.model_key,
            &resolved.selection.execution_model.model_key,
            RequestLogSummary::success(
                RequestOperation::ChatCompletions,
                route.provider_key.clone(),
                1,
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
        return Ok(response);
    }

    let mut first_failure: Option<GatewayError> = None;
    let mut first_failure_provider_key: Option<String> = None;
    let mut attempt_count = 0usize;

    for (route, provider) in eligible {
        attempt_count += 1;
        let context = build_provider_context(
            &request_id,
            &resolved.selection.requested_model.model_key,
            &route,
            idempotency_key.clone(),
            request_headers.clone(),
        );

        match provider.chat_completions(&core_request, &context).await {
            Ok(value) => {
                best_effort_log_request(
                    &state.service,
                    &auth,
                    &request_id,
                    &resolved.selection.requested_model.model_key,
                    &resolved.selection.execution_model.model_key,
                    RequestLogSummary::success(
                        RequestOperation::ChatCompletions,
                        route.provider_key.clone(),
                        attempt_count,
                        false,
                        latency_ms_since(request_started_at),
                        usage_from_response(&value),
                    ),
                )
                .await;
                state
                    .service
                    .record_chat_usage(
                        &auth,
                        &resolved.selection.execution_model,
                        &route,
                        &request_id,
                        usage_value_from_response(&value),
                        OffsetDateTime::now_utc(),
                    )
                    .await?;
                let mut response = Json(normalize_response_model(
                    value,
                    &resolved.selection.requested_model.model_key,
                ))
                .into_response();
                if let Ok(request_id_header) = HeaderValue::from_str(&request_id) {
                    response
                        .headers_mut()
                        .insert("x-request-id", request_id_header);
                }
                return Ok(response);
            }
            Err(error) => {
                tracing::warn!(
                    provider_key = %route.provider_key,
                    request_model = %core_request.model,
                    retryable = error.is_retryable(),
                    "chat completion attempt failed"
                );

                if !error.is_retryable() {
                    let gateway_error = GatewayError::from(error);
                    best_effort_log_request(
                        &state.service,
                        &auth,
                        &request_id,
                        &resolved.selection.requested_model.model_key,
                        &resolved.selection.execution_model.model_key,
                        RequestLogSummary::failure(
                            RequestOperation::ChatCompletions,
                            route.provider_key.clone(),
                            attempt_count,
                            false,
                            latency_ms_since(request_started_at),
                            gateway_error.http_status_code().into(),
                            gateway_error.error_code().to_string(),
                        ),
                    )
                    .await;
                    return Err(AppError(gateway_error));
                }

                if first_failure.is_none() {
                    first_failure = Some(error.into());
                    first_failure_provider_key = Some(route.provider_key.clone());
                }
            }
        }
    }

    let final_error = first_failure.unwrap_or_else(|| {
        GatewayError::Provider(ProviderError::Transport(
            "all fallback routes failed without a terminal error".to_string(),
        ))
    });
    best_effort_log_request(
        &state.service,
        &auth,
        &request_id,
        &resolved.selection.requested_model.model_key,
        &resolved.selection.execution_model.model_key,
        RequestLogSummary::failure(
            RequestOperation::ChatCompletions,
            first_failure_provider_key.unwrap_or_else(|| "unknown".to_string()),
            attempt_count,
            false,
            latency_ms_since(request_started_at),
            final_error.http_status_code().into(),
            final_error.error_code().to_string(),
        ),
    )
    .await;

    Err(AppError(final_error))
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
    let idempotency_key = extract_idempotency_key(&headers).map(str::to_string);
    let request_id = extract_request_id(&headers);
    let request_headers = extract_request_headers(&headers);
    let allow_fallback = idempotency_key.is_some();

    let mut eligible = Vec::new();
    for route in &resolved.routes {
        let Some(provider) = state.providers.get(&route.provider_key) else {
            continue;
        };
        let effective_capabilities = provider.capabilities().intersect(route.capabilities);
        if supports_requirements(effective_capabilities, requirements) {
            eligible.push((route.clone(), provider));
        }
    }

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count = eligible.len(),
        required_capabilities = ?requirements.required_capability_names(),
        fallback_allowed = allow_fallback,
        "embeddings request resolved"
    );

    if eligible.is_empty() {
        return Err(AppError(no_compatible_route_error(requirements)));
    }

    if !allow_fallback {
        let (route, provider) = eligible
            .into_iter()
            .next()
            .expect("eligible routes checked as non-empty");
        let context = build_provider_context(
            &request_id,
            &resolved.selection.requested_model.model_key,
            &route,
            idempotency_key.clone(),
            request_headers,
        );

        let value = provider
            .embeddings(&core_request, &context)
            .await
            .map_err(GatewayError::from);
        let value = match value {
            Ok(value) => {
                normalize_response_model(value, &resolved.selection.requested_model.model_key)
            }
            Err(error) => {
                best_effort_log_request(
                    &state.service,
                    &auth,
                    &request_id,
                    &resolved.selection.requested_model.model_key,
                    &resolved.selection.execution_model.model_key,
                    RequestLogSummary::failure(
                        RequestOperation::Embeddings,
                        route.provider_key.clone(),
                        1,
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

        state
            .service
            .record_chat_usage(
                &auth,
                &resolved.selection.execution_model,
                &route,
                &request_id,
                usage_value_from_response(&value),
                OffsetDateTime::now_utc(),
            )
            .await?;
        best_effort_log_request(
            &state.service,
            &auth,
            &request_id,
            &resolved.selection.requested_model.model_key,
            &resolved.selection.execution_model.model_key,
            RequestLogSummary::success(
                RequestOperation::Embeddings,
                route.provider_key.clone(),
                1,
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
        return Ok(response);
    }

    let mut first_failure: Option<GatewayError> = None;
    let mut first_failure_provider_key: Option<String> = None;
    let mut attempt_count = 0usize;

    for (route, provider) in eligible {
        attempt_count += 1;
        let context = build_provider_context(
            &request_id,
            &resolved.selection.requested_model.model_key,
            &route,
            idempotency_key.clone(),
            request_headers.clone(),
        );

        match provider.embeddings(&core_request, &context).await {
            Ok(value) => {
                let value =
                    normalize_response_model(value, &resolved.selection.requested_model.model_key);
                state
                    .service
                    .record_chat_usage(
                        &auth,
                        &resolved.selection.execution_model,
                        &route,
                        &request_id,
                        usage_value_from_response(&value),
                        OffsetDateTime::now_utc(),
                    )
                    .await?;
                best_effort_log_request(
                    &state.service,
                    &auth,
                    &request_id,
                    &resolved.selection.requested_model.model_key,
                    &resolved.selection.execution_model.model_key,
                    RequestLogSummary::success(
                        RequestOperation::Embeddings,
                        route.provider_key.clone(),
                        attempt_count,
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
                return Ok(response);
            }
            Err(error) => {
                tracing::warn!(
                    provider_key = %route.provider_key,
                    request_model = %core_request.model,
                    retryable = error.is_retryable(),
                    attempt_count = attempt_count,
                    fallback_allowed = allow_fallback,
                    "embeddings attempt failed"
                );

                if !error.is_retryable() {
                    let gateway_error = GatewayError::from(error);
                    best_effort_log_request(
                        &state.service,
                        &auth,
                        &request_id,
                        &resolved.selection.requested_model.model_key,
                        &resolved.selection.execution_model.model_key,
                        RequestLogSummary::failure(
                            RequestOperation::Embeddings,
                            route.provider_key.clone(),
                            attempt_count,
                            false,
                            latency_ms_since(request_started_at),
                            gateway_error.http_status_code().into(),
                            gateway_error.error_code().to_string(),
                        ),
                    )
                    .await;
                    return Err(AppError(gateway_error));
                }

                if first_failure.is_none() {
                    first_failure = Some(error.into());
                    first_failure_provider_key = Some(route.provider_key.clone());
                }
            }
        }
    }

    let final_error = first_failure.unwrap_or_else(|| {
        GatewayError::Provider(ProviderError::Transport(
            "all fallback routes failed without a terminal error".to_string(),
        ))
    });
    best_effort_log_request(
        &state.service,
        &auth,
        &request_id,
        &resolved.selection.requested_model.model_key,
        &resolved.selection.execution_model.model_key,
        RequestLogSummary::failure(
            RequestOperation::Embeddings,
            first_failure_provider_key.unwrap_or_else(|| "unknown".to_string()),
            attempt_count,
            false,
            latency_ms_since(request_started_at),
            final_error.http_status_code().into(),
            final_error.error_code().to_string(),
        ),
    )
    .await;

    Err(AppError(final_error))
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
    idempotency_key: Option<String>,
    request_headers: BTreeMap<String, String>,
) -> ProviderRequestContext {
    ProviderRequestContext {
        request_id: request_id.to_string(),
        model_key: model_key.to_string(),
        provider_key: route.provider_key.clone(),
        upstream_model: route.upstream_model.clone(),
        extra_headers: route.extra_headers.clone(),
        extra_body: route.extra_body.clone(),
        idempotency_key,
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
    ChatCompletions,
    Embeddings,
}

impl RequestOperation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Embeddings => "embeddings",
        }
    }
}

#[derive(Debug, Clone)]
struct RequestLogSummary {
    operation: RequestOperation,
    provider_key: String,
    attempt_count: usize,
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
        attempt_count: usize,
        stream: bool,
        latency_ms: i64,
        usage: UsageSummary,
    ) -> Self {
        Self {
            operation,
            provider_key,
            attempt_count,
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
        attempt_count: usize,
        stream: bool,
        latency_ms: i64,
        status_code: i64,
        error_code: String,
    ) -> Self {
        Self {
            operation,
            provider_key,
            attempt_count,
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

struct StreamFailure {
    status_code: i64,
    error_code: String,
}

struct LoggingBodyStreamState {
    upstream: gateway_core::ProviderStream,
    service: std::sync::Arc<AppGatewayService>,
    auth: AuthenticatedApiKey,
    request_id: String,
    requested_model_key: String,
    resolved_model_key: String,
    execution_model: gateway_core::GatewayModel,
    route: gateway_core::ModelRoute,
    provider_key: String,
    started_at: Instant,
    finished: bool,
    failure: Option<StreamFailure>,
    attempt_count: usize,
    usage: Option<Value>,
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
                if state.failure.is_none()
                    && let Some(error_code) = extract_stream_error_code(chunk.as_ref())
                {
                    state.failure = Some(StreamFailure {
                        status_code: 502,
                        error_code,
                    });
                }
                if state.failure.is_none()
                    && let Some(usage) = extract_stream_usage(chunk.as_ref())
                {
                    state.usage = Some(usage);
                }

                Some((Ok(chunk), state))
            }
            Some(Err(error)) => {
                let error_message = error.to_string();
                let gateway_error = GatewayError::from(error);
                tracing::warn!(
                    request_id = %state.request_id,
                    provider_key = %state.provider_key,
                    termination_reason = "stream_transport_error",
                    "chat completion stream terminated with transport error"
                );
                best_effort_log_request(
                    &state.service,
                    &state.auth,
                    &state.request_id,
                    &state.requested_model_key,
                    &state.resolved_model_key,
                    RequestLogSummary::failure(
                        RequestOperation::ChatCompletions,
                        state.provider_key.clone(),
                        state.attempt_count,
                        true,
                        latency_ms_since(state.started_at),
                        gateway_error.http_status_code().into(),
                        gateway_error.error_code().to_string(),
                    ),
                )
                .await;
                state.finished = true;
                Some((Err(std::io::Error::other(error_message)), state))
            }
            None => {
                let summary = match &state.failure {
                    Some(failure) => RequestLogSummary::failure(
                        RequestOperation::ChatCompletions,
                        state.provider_key.clone(),
                        state.attempt_count,
                        true,
                        latency_ms_since(state.started_at),
                        failure.status_code,
                        failure.error_code.clone(),
                    ),
                    None => {
                        let usage_summary = usage_summary_from_value(state.usage.as_ref());
                        if let Err(error) = state
                            .service
                            .record_chat_usage(
                                &state.auth,
                                &state.execution_model,
                                &state.route,
                                &state.request_id,
                                state.usage.clone(),
                                OffsetDateTime::now_utc(),
                            )
                            .await
                        {
                            tracing::warn!(
                                request_id = %state.request_id,
                                model_key = %state.execution_model.model_key,
                                error = %error,
                                "usage ledger write failed after stream completion"
                            );
                        }
                        RequestLogSummary::success(
                            RequestOperation::ChatCompletions,
                            state.provider_key.clone(),
                            state.attempt_count,
                            true,
                            latency_ms_since(state.started_at),
                            usage_summary,
                        )
                    }
                };
                tracing::info!(
                    request_id = %state.request_id,
                    provider_key = %state.provider_key,
                    termination_reason = if state.failure.is_some() { "stream_error_chunk" } else { "complete" },
                    "chat completion stream terminated"
                );
                best_effort_log_request(
                    &state.service,
                    &state.auth,
                    &state.request_id,
                    &state.requested_model_key,
                    &state.resolved_model_key,
                    summary,
                )
                .await;
                None
            }
        }
    })
}

async fn best_effort_log_request(
    service: &std::sync::Arc<AppGatewayService>,
    auth: &AuthenticatedApiKey,
    request_id: &str,
    model_key: &str,
    resolved_model_key: &str,
    summary: RequestLogSummary,
) {
    let metadata = request_log_metadata(summary.attempt_count, summary.stream, summary.operation);
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

fn request_log_metadata(
    attempt_count: usize,
    stream: bool,
    operation: RequestOperation,
) -> Map<String, Value> {
    let mut metadata = Map::new();
    metadata.insert(
        "operation".to_string(),
        Value::String(operation.as_str().to_string()),
    );
    metadata.insert("stream".to_string(), Value::Bool(stream));
    metadata.insert("fallback_used".to_string(), Value::Bool(attempt_count > 1));
    metadata.insert(
        "attempt_count".to_string(),
        Value::Number(i64::try_from(attempt_count).unwrap_or(i64::MAX).into()),
    );
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

fn extract_stream_error_code(chunk: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(chunk).ok()?;
    for line in text.lines() {
        let Some(payload) = line.strip_prefix("data: ") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }

        let value: Value = serde_json::from_str(payload).ok()?;
        let Some(error) = value.get("error").and_then(Value::as_object) else {
            continue;
        };

        return error
            .get("code")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| Some("stream_error".to_string()));
    }

    None
}

fn extract_authorization_header(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
}

fn extract_idempotency_key(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("idempotency-key")
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

fn extract_stream_usage(chunk: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(chunk).ok()?;
    for line in text.lines() {
        let Some(payload) = line.strip_prefix("data: ") else {
            continue;
        };
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }

        let value: Value = serde_json::from_str(payload).ok()?;
        if let Some(usage) = value.get("usage") {
            return Some(usage.clone());
        }
    }

    None
}
