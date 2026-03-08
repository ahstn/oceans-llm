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
    ModelsListResponse, OpenAiErrorEnvelope, ProviderError, ProviderRequestContext,
    RequestLogBundle, RequestLogPayloadRecord, RequestLogRecord, protocol::openai::ModelCard,
};
use gateway_service::redaction::{sanitize_headers, sanitize_json_payload};
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
    let request_id = extract_request_id(&headers)
        .map(str::to_string)
        .unwrap_or_else(generate_request_id);
    let idempotency_key = extract_idempotency_key(&headers).map(str::to_string);
    let request_headers = extract_request_headers(&headers);
    let allow_fallback = !request.stream && idempotency_key.is_some();
    let auth = match state
        .service
        .authenticate(extract_authorization_header(&headers))
        .await
    {
        Ok(auth) => auth,
        Err(error) => return Ok(error_response_with_request_id(error, &request_id)),
    };
    let resolved = match state.service.resolve_request(&auth, &request.model).await {
        Ok(resolved) => resolved,
        Err(error) => return Ok(error_response_with_request_id(error, &request_id)),
    };

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
        resolved_model = %resolved.model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count = eligible.len(),
        stream = request.stream,
        fallback_allowed = allow_fallback,
        "chat completion request resolved"
    );

    if eligible.is_empty() {
        return Ok(error_response_with_request_id(
            GatewayError::Provider(ProviderError::NotImplemented(
                "no registered provider supports chat completions for resolved routes".to_string(),
            )),
            &request_id,
        ));
    }

    if request.stream || !allow_fallback {
        let (route, provider) = eligible
            .into_iter()
            .next()
            .expect("eligible routes checked as non-empty");
        let request_payload =
            build_request_payload(&request, &request_headers, &resolved.model.model_key, &route);

        let context = build_provider_context(
            &request_id,
            &resolved.model.model_key,
            &route,
            idempotency_key.clone(),
            request_headers.clone(),
        );

        if request.stream {
            let stream = match provider.chat_completions_stream(&request, &context).await {
                Ok(stream) => stream,
                Err(error) => {
                    let gateway_error = GatewayError::from(error);
                    let response_payload = build_error_response_payload(&request_id, &gateway_error);
                    best_effort_log_request(
                        &state.service,
                        &auth,
                        &request_id,
                        &resolved.model.model_key,
                        &route.upstream_model,
                        ChatCompletionLogSummary::failure(
                            route.provider_key.clone(),
                            1,
                            true,
                            latency_ms_since(request_started_at),
                            gateway_error.http_status_code().into(),
                            gateway_error.error_code().to_string(),
                        ),
                        request_payload,
                        Some(response_payload),
                    )
                    .await;
                    return Ok(error_response_with_request_id(gateway_error, &request_id));
                }
            };
            let body_stream = wrap_stream_with_request_logging(LoggingBodyStreamState {
                upstream: stream,
                service: state.service.clone(),
                auth: auth.clone(),
                request_id: request_id.clone(),
                model_key: resolved.model.model_key.clone(),
                provider_key: route.provider_key.clone(),
                upstream_model: route.upstream_model.clone(),
                started_at: request_started_at,
                finished: false,
                failure: None,
                attempt_count: 1,
                request_payload,
                transcript: StreamTranscript::default(),
            });

            return streaming_response_with_request_id(&request_id, body_stream);
        }

        let value = provider
            .chat_completions(&request, &context)
            .await
            .map_err(GatewayError::from);
        let value = match value {
            Ok(value) => value,
            Err(error) => {
                let response_payload = build_error_response_payload(&request_id, &error);
                best_effort_log_request(
                    &state.service,
                    &auth,
                    &request_id,
                    &resolved.model.model_key,
                    &route.upstream_model,
                    ChatCompletionLogSummary::failure(
                        route.provider_key.clone(),
                        1,
                        false,
                        latency_ms_since(request_started_at),
                        error.http_status_code().into(),
                        error.error_code().to_string(),
                    ),
                    request_payload,
                    Some(response_payload),
                )
                .await;
                return Ok(error_response_with_request_id(error, &request_id));
            }
        };
        let response_payload = build_json_response_payload(&request_id, 200, &value);
        best_effort_log_request(
            &state.service,
            &auth,
            &request_id,
            &resolved.model.model_key,
            &route.upstream_model,
            ChatCompletionLogSummary::success(
                route.provider_key.clone(),
                1,
                false,
                latency_ms_since(request_started_at),
                usage_from_response(&value),
            ),
            request_payload,
            Some(response_payload),
        )
        .await;
        return Ok(json_response_with_request_id(&request_id, value));
    }

    let mut first_failure: Option<GatewayError> = None;
    let mut first_failure_provider_key: Option<String> = None;
    let mut first_failure_upstream_model: Option<String> = None;
    let mut first_failure_request_payload: Option<PayloadSnapshot> = None;
    let mut attempt_count = 0usize;

    for (route, provider) in eligible {
        attempt_count += 1;
        let request_payload =
            build_request_payload(&request, &request_headers, &resolved.model.model_key, &route);
        let context = build_provider_context(
            &request_id,
            &resolved.model.model_key,
            &route,
            idempotency_key.clone(),
            request_headers.clone(),
        );

        match provider.chat_completions(&request, &context).await {
            Ok(value) => {
                let response_payload = build_json_response_payload(&request_id, 200, &value);
                best_effort_log_request(
                    &state.service,
                    &auth,
                    &request_id,
                    &resolved.model.model_key,
                    &route.upstream_model,
                    ChatCompletionLogSummary::success(
                        route.provider_key.clone(),
                        attempt_count,
                        false,
                        latency_ms_since(request_started_at),
                        usage_from_response(&value),
                    ),
                    request_payload,
                    Some(response_payload),
                )
                .await;
                return Ok(json_response_with_request_id(&request_id, value));
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
                    let response_payload = build_error_response_payload(&request_id, &gateway_error);
                    best_effort_log_request(
                        &state.service,
                        &auth,
                        &request_id,
                        &resolved.model.model_key,
                        &route.upstream_model,
                        ChatCompletionLogSummary::failure(
                            route.provider_key.clone(),
                            attempt_count,
                            false,
                            latency_ms_since(request_started_at),
                            gateway_error.http_status_code().into(),
                            gateway_error.error_code().to_string(),
                        ),
                        request_payload,
                        Some(response_payload),
                    )
                    .await;
                    return Ok(error_response_with_request_id(gateway_error, &request_id));
                }

                if first_failure.is_none() {
                    first_failure = Some(error.into());
                    first_failure_provider_key = Some(route.provider_key.clone());
                    first_failure_upstream_model = Some(route.upstream_model.clone());
                    first_failure_request_payload = Some(request_payload.clone());
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
        &resolved.model.model_key,
        &first_failure_upstream_model.unwrap_or_default(),
        ChatCompletionLogSummary::failure(
            first_failure_provider_key.unwrap_or_else(|| "unknown".to_string()),
            attempt_count,
            false,
            latency_ms_since(request_started_at),
            final_error.http_status_code().into(),
            final_error.error_code().to_string(),
        ),
        first_failure_request_payload.unwrap_or_else(|| {
            build_request_payload(
                &request,
                &request_headers,
                &resolved.model.model_key,
                &resolved.routes[0],
            )
        }),
        Some(build_error_response_payload(&request_id, &final_error)),
    )
    .await;

    Ok(error_response_with_request_id(final_error, &request_id))
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
        resolved_model = %resolved.model.model_key,
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
struct PayloadSnapshot {
    value: Value,
    bytes: i64,
    truncated: bool,
    sha256: String,
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
    provider_key: String,
    upstream_model: String,
    started_at: Instant,
    finished: bool,
    failure: Option<StreamFailure>,
    attempt_count: usize,
    request_payload: PayloadSnapshot,
    transcript: StreamTranscript,
}

#[derive(Debug, Clone, Default)]
struct StreamTranscript {
    events: Vec<Value>,
    event_count: usize,
    done_seen: bool,
    truncated: bool,
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
                state.transcript.push_chunk(chunk.as_ref());
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
                    &state.upstream_model,
                    ChatCompletionLogSummary::failure(
                        state.provider_key.clone(),
                        state.attempt_count,
                        true,
                        latency_ms_since(state.started_at),
                        gateway_error.http_status_code().into(),
                        gateway_error.error_code().to_string(),
                    ),
                    state.request_payload.clone(),
                    Some(build_stream_response_payload(
                        &state.request_id,
                        gateway_error.http_status_code().into(),
                        &state.transcript,
                    )),
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
                    &state.upstream_model,
                    summary,
                    state.request_payload.clone(),
                    Some(build_stream_response_payload(
                        &state.request_id,
                        state.failure.as_ref().map_or(200, |failure| failure.status_code),
                        &state.transcript,
                    )),
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
    upstream_model: &str,
    summary: ChatCompletionLogSummary,
    request_payload: PayloadSnapshot,
    response_payload: Option<PayloadSnapshot>,
) {
    let request_log_id = Uuid::new_v4();
    let payload = response_payload.map(|response_payload| RequestLogPayloadRecord {
        request_log_id,
        request_json: request_payload.value,
        response_json: response_payload.value,
        request_bytes: request_payload.bytes,
        response_bytes: response_payload.bytes,
        request_truncated: request_payload.truncated,
        response_truncated: response_payload.truncated,
        request_sha256: request_payload.sha256,
        response_sha256: response_payload.sha256,
        occurred_at: OffsetDateTime::now_utc(),
    });

    let summary_record = RequestLogRecord {
        request_log_id,
        request_id: request_id.to_string(),
        api_key_id: auth.id,
        user_id: None,
        team_id: None,
        model_key: model_key.to_string(),
        provider_key: summary.provider_key,
        upstream_model: upstream_model.to_string(),
        status_code: Some(summary.status_code),
        latency_ms: Some(summary.latency_ms),
        stream: summary.stream,
        fallback_used: summary.attempt_count > 1,
        attempt_count: i64::try_from(summary.attempt_count).unwrap_or(i64::MAX),
        prompt_tokens: summary.prompt_tokens,
        completion_tokens: summary.completion_tokens,
        total_tokens: summary.total_tokens,
        payload_available: payload.is_some(),
        error_code: summary.error_code,
        metadata: request_log_metadata(),
        occurred_at: OffsetDateTime::now_utc(),
    };

    let bundle = RequestLogBundle {
        summary: summary_record,
        payload,
    };

    if let Err(error) = service.log_request_if_enabled(auth, bundle).await {
        tracing::warn!(
            request_id = %request_id,
            model_key = %model_key,
            error = %error,
            "request logging failed"
        );
    }
}

fn request_log_metadata() -> Map<String, Value> {
    Map::new()
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

fn build_request_payload(
    request: &ChatCompletionsRequest,
    request_headers: &BTreeMap<String, String>,
    model_key: &str,
    route: &gateway_core::ModelRoute,
) -> PayloadSnapshot {
    let body = serde_json::to_value(request).expect("chat request should serialize");
    let sanitized = sanitize_json_payload(&json!({
        "headers": sanitize_headers(request_headers),
        "body": body,
        "routing": {
            "model_key": model_key,
            "provider_key": route.provider_key,
            "upstream_model": route.upstream_model,
        }
    }));

    PayloadSnapshot {
        value: sanitized.value,
        bytes: sanitized.bytes,
        truncated: sanitized.truncated,
        sha256: sanitized.sha256,
    }
}

fn build_json_response_payload(request_id: &str, status_code: i64, body: &Value) -> PayloadSnapshot {
    let sanitized = sanitize_json_payload(&json!({
        "status_code": status_code,
        "headers": default_json_response_headers(request_id),
        "body": body,
    }));

    PayloadSnapshot {
        value: sanitized.value,
        bytes: sanitized.bytes,
        truncated: sanitized.truncated,
        sha256: sanitized.sha256,
    }
}

fn build_error_response_payload(request_id: &str, error: &GatewayError) -> PayloadSnapshot {
    let body = serde_json::to_value(OpenAiErrorEnvelope::from_gateway_error(error))
        .expect("error envelope should serialize");
    build_json_response_payload(request_id, error.http_status_code().into(), &body)
}

fn build_stream_response_payload(
    request_id: &str,
    status_code: i64,
    transcript: &StreamTranscript,
) -> PayloadSnapshot {
    let sanitized = sanitize_json_payload(&json!({
        "status_code": status_code,
        "headers": {
            "content-type": "text/event-stream; charset=utf-8",
            "cache-control": "no-cache",
            "x-request-id": request_id,
        },
        "body": {
            "kind": "sse_transcript",
            "event_count": transcript.event_count,
            "done_seen": transcript.done_seen,
            "truncated": transcript.truncated,
            "events": transcript.events,
        }
    }));

    PayloadSnapshot {
        value: sanitized.value,
        bytes: sanitized.bytes,
        truncated: sanitized.truncated || transcript.truncated,
        sha256: sanitized.sha256,
    }
}

fn default_json_response_headers(request_id: &str) -> Map<String, Value> {
    let mut headers = Map::new();
    headers.insert(
        "content-type".to_string(),
        Value::String("application/json".to_string()),
    );
    headers.insert(
        "x-request-id".to_string(),
        Value::String(request_id.to_string()),
    );
    headers
}

impl StreamTranscript {
    fn push_chunk(&mut self, chunk: &[u8]) {
        const MAX_EVENTS: usize = 128;

        let Ok(text) = std::str::from_utf8(chunk) else {
            self.push_event(json!({"kind": "non_utf8_chunk", "bytes": chunk.len()}), MAX_EVENTS);
            return;
        };

        for line in text.lines() {
            let Some(payload) = line.strip_prefix("data: ") else {
                continue;
            };

            let payload = payload.trim();
            self.event_count += 1;
            if payload == "[DONE]" {
                self.done_seen = true;
                self.push_event(json!({"kind": "done"}), MAX_EVENTS);
                continue;
            }

            let value = serde_json::from_str::<Value>(payload)
                .unwrap_or_else(|_| json!({"kind": "raw_event", "data": payload}));
            self.push_event(value, MAX_EVENTS);
        }
    }

    fn push_event(&mut self, event: Value, max_events: usize) {
        if self.events.len() >= max_events {
            self.truncated = true;
            return;
        }
        self.events.push(event);
    }
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

fn extract_request_id(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
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

fn generate_request_id() -> String {
    format!("req_{}", Uuid::new_v4().simple())
}

fn json_response_with_request_id(request_id: &str, value: Value) -> Response {
    let mut response = Json(value).into_response();
    if let Ok(request_id_header) = HeaderValue::from_str(request_id) {
        response
            .headers_mut()
            .insert("x-request-id", request_id_header);
    }
    response
}

fn error_response_with_request_id(error: GatewayError, request_id: &str) -> Response {
    let mut response = AppError(error).into_response();
    if let Ok(request_id_header) = HeaderValue::from_str(request_id) {
        response
            .headers_mut()
            .insert("x-request-id", request_id_header);
    }
    response
}

fn streaming_response_with_request_id(
    request_id: &str,
    body_stream: impl futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> + Send + 'static,
) -> Result<Response, AppError> {
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

    if let Ok(request_id_header) = HeaderValue::from_str(request_id) {
        response
            .headers_mut()
            .insert("x-request-id", request_id_header);
    }

    Ok(response)
}
