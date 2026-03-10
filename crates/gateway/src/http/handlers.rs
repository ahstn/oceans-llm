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
    AuthenticatedApiKey, ChatCompletionsRequest, EmbeddingsRequest, GatewayError,
    ModelsListResponse, ProviderError, ProviderRequestContext, RequestLogRecord,
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
    let resolved = state.service.resolve_request(&auth, &request.model).await?;

    let idempotency_key = extract_idempotency_key(&headers).map(str::to_string);
    let request_id = extract_request_id(&headers).to_string();
    let request_headers = extract_request_headers(&headers);
    let allow_fallback = !request.stream && idempotency_key.is_some();

    let mut eligible = Vec::new();
    for route in &resolved.routes {
        let Some(provider) = state.providers.get(&route.provider_key) else {
            continue;
        };
        let caps = provider.capabilities();
        if request.stream {
            if caps.chat_completions_stream {
                eligible.push((route.clone(), provider));
            }
        } else if caps.chat_completions {
            eligible.push((route.clone(), provider));
        }
    }

    tracing::info!(
        request_model = %request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count = eligible.len(),
        stream = request.stream,
        fallback_allowed = allow_fallback,
        "chat completion request resolved"
    );

    if eligible.is_empty() {
        return Err(AppError(GatewayError::Provider(
            ProviderError::NotImplemented(
                "no registered provider supports chat completions for resolved routes".to_string(),
            ),
        )));
    }

    if request.stream || !allow_fallback {
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

        if request.stream {
            let stream = match provider.chat_completions_stream(&request, &context).await {
                Ok(stream) => stream,
                Err(error) => {
                    let gateway_error = GatewayError::from(error);
                    best_effort_log_request(
                        &state.service,
                        &auth,
                        &request_id,
                        &resolved.selection.requested_model.model_key,
                        &resolved.selection.execution_model.model_key,
                        ChatCompletionLogSummary::failure(
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
                model_key: resolved.selection.requested_model.model_key.clone(),
                resolved_model_key: resolved.selection.execution_model.model_key.clone(),
                provider_key: route.provider_key.clone(),
                started_at: request_started_at,
                finished: false,
                failure: None,
                attempt_count: 1,
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
            .chat_completions(&request, &context)
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
                    ChatCompletionLogSummary::failure(
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
        best_effort_log_request(
            &state.service,
            &auth,
            &request_id,
            &resolved.selection.requested_model.model_key,
            &resolved.selection.execution_model.model_key,
            ChatCompletionLogSummary::success(
                route.provider_key.clone(),
                1,
                false,
                latency_ms_since(request_started_at),
                usage_from_response(&value),
            ),
        )
        .await;
        return Ok(Json(value).into_response());
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

        match provider.chat_completions(&request, &context).await {
            Ok(value) => {
                best_effort_log_request(
                    &state.service,
                    &auth,
                    &request_id,
                    &resolved.selection.requested_model.model_key,
                    &resolved.selection.execution_model.model_key,
                    ChatCompletionLogSummary::success(
                        route.provider_key.clone(),
                        attempt_count,
                        false,
                        latency_ms_since(request_started_at),
                        usage_from_response(&value),
                    ),
                )
                .await;
                return Ok(Json(normalize_response_model(
                    value,
                    &resolved.selection.requested_model.model_key,
                ))
                .into_response());
            }
            Err(error) => {
                tracing::warn!(
                    provider_key = %route.provider_key,
                    request_model = %request.model,
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
                        ChatCompletionLogSummary::failure(
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
        ChatCompletionLogSummary::failure(
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
) -> Result<Json<serde_json::Value>, AppError> {
    let auth = state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await?;
    let resolved = state.service.resolve_request(&auth, &request.model).await?;

    let has_adapter = resolved
        .routes
        .first()
        .and_then(|route| state.providers.get(&route.provider_key))
        .is_some();

    tracing::info!(
        request_model = %request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        provider_adapter_available = has_adapter,
        "embeddings request resolved"
    );

    for route in &resolved.routes {
        let Some(provider) = state.providers.get(&route.provider_key) else {
            continue;
        };

        if !provider.capabilities().embeddings {
            return Err(AppError(GatewayError::Provider(
                ProviderError::NotImplemented(format!(
                    "provider `{}` does not implement embeddings in this slice",
                    route.provider_key
                )),
            )));
        }

        return Err(AppError(GatewayError::NotImplemented(
            "embeddings execution is intentionally deferred in this foundation phase".to_string(),
        )));
    }

    Err(AppError(GatewayError::Provider(
        ProviderError::NotImplemented(
            "no registered provider supports embeddings for resolved routes".to_string(),
        ),
    )))
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
struct ChatCompletionLogSummary {
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

impl ChatCompletionLogSummary {
    fn success(
        provider_key: String,
        attempt_count: usize,
        stream: bool,
        latency_ms: i64,
        usage: UsageSummary,
    ) -> Self {
        Self {
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
        provider_key: String,
        attempt_count: usize,
        stream: bool,
        latency_ms: i64,
        status_code: i64,
        error_code: String,
    ) -> Self {
        Self {
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
    model_key: String,
    resolved_model_key: String,
    provider_key: String,
    started_at: Instant,
    finished: bool,
    failure: Option<StreamFailure>,
    attempt_count: usize,
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

                Some((Ok(chunk), state))
            }
            Some(Err(error)) => {
                let error_message = error.to_string();
                let gateway_error = GatewayError::from(error);
                best_effort_log_request(
                    &state.service,
                    &state.auth,
                    &state.request_id,
                    &state.model_key,
                    &state.resolved_model_key,
                    ChatCompletionLogSummary::failure(
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
                    Some(failure) => ChatCompletionLogSummary::failure(
                        state.provider_key.clone(),
                        state.attempt_count,
                        true,
                        latency_ms_since(state.started_at),
                        failure.status_code,
                        failure.error_code.clone(),
                    ),
                    None => ChatCompletionLogSummary::success(
                        state.provider_key.clone(),
                        state.attempt_count,
                        true,
                        latency_ms_since(state.started_at),
                        UsageSummary::default(),
                    ),
                };
                best_effort_log_request(
                    &state.service,
                    &state.auth,
                    &state.request_id,
                    &state.model_key,
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
    summary: ChatCompletionLogSummary,
) {
    let metadata = request_log_metadata(summary.attempt_count, summary.stream);
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

fn request_log_metadata(attempt_count: usize, stream: bool) -> Map<String, Value> {
    let mut metadata = Map::new();
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
    let Some(usage) = value.get("usage").and_then(Value::as_object) else {
        return UsageSummary::default();
    };

    UsageSummary {
        prompt_tokens: usage.get("prompt_tokens").and_then(Value::as_i64),
        completion_tokens: usage.get("completion_tokens").and_then(Value::as_i64),
        total_tokens: usage.get("total_tokens").and_then(Value::as_i64),
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

fn extract_request_id(headers: &HeaderMap) -> &str {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing-request-id")
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
