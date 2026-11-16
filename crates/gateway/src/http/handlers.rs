mod auth;
mod count_tokens;
mod decisions;
#[cfg(test)]
mod decisions_tests;
mod embeddings;
#[cfg(test)]
mod embeddings_tests;
mod images;
#[cfg(test)]
mod images_tests;
mod inference;
#[cfg(test)]
mod inference_tests;
mod json_operation;
mod messages;
#[cfg(test)]
mod model_group_info_tests;
#[cfg(test)]
mod model_metadata_tests;
mod native_messages;
#[cfg(test)]
mod route_tests;
mod routing;
#[cfg(test)]
mod routing_tests;
mod service_tier;
#[cfg(test)]
mod service_tier_tests;

use auth::InferenceAuth;
pub use count_tokens::{CountTokensLimiter, v1_messages_count_tokens};
pub use decisions::{v1_decisions, v1_systemone};
pub use embeddings::v1_embeddings;
use gateway_service::routing::{RoutingEndpoint, RoutingReceipt};
pub use images::v1_images;
pub use messages::v1_messages;
use routing::{SelectedProviderRoute, no_compatible_route_error, select_provider_route};
use service_tier::{enforce_requested_service_tier, service_tier_outcome};

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
    CoreRequestRequirements, GatewayError, McpToolProjection, ProviderClient, ProviderError,
    ProviderRequestContext, ProviderStream, RateLimitOperation, RequestAttemptRecord,
    RequestAttemptStatus, RequestToolCardinality, ResponsesRequest,
    anthropic_messages_request_to_core, core_chat_request_to_openai, enforce_chat_reasoning_effort,
    enforce_responses_reasoning_effort, openai_chat_request_to_core,
    openai_responses_request_to_core, protocol::anthropic::anthropic_message_from_openai_chat,
    take_requested_service_tier,
};
use gateway_service::{
    McpAccess, McpTokenOverhead, McpTokenOverheadInput, RateLimitSubject, RequestLogContext,
    RequestLogIconMetadata, ResolvedGatewayRequest, ResolvedProviderConnection,
    rate_limits::RateLimitMemberTeam, resolve_model_icon_key, resolve_provider_display_from_parts,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tower_http::request_id::RequestId;
use tracing::{Instrument as _, Span, field};

use crate::http::{
    anthropic_stream::anthropic_messages_stream_from_openai,
    error::AppError,
    inference_guardrails::{
        GuardStreamError, InferenceGuardContext, guard_model_response, guard_prompt, guard_stream,
        model_route_key, redact_prompt,
    },
    request_policy::{RequestPolicyContext, enforce_rate_limits},
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

/// Ready while the database answers. A draining gateway reports 503 so load
/// balancers outside Kubernetes stop sending it work too.
pub async fn readyz(State(state): State<AppState>) -> Result<Response, AppError> {
    if state.shutdown.is_draining() {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "status": "draining" })),
        )
            .into_response());
    }
    state.service.check_readiness().await?;
    Ok(Json(json!({ "status": "ready" })).into_response())
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
    let auth = state
        .service
        .authenticate_for_scope(
            extract_anthropic_authorization_header(&headers).as_deref(),
            gateway_core::SCOPE_INFERENCE,
        )
        .await?;
    Ok(Json(
        state
            .service
            .model_metadata_for_api_key(&auth, &state.providers)
            .await?,
    ))
}

pub async fn v1_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<gateway_service::model_metadata::ModelsListResponse>, AppError> {
    let auth = state
        .service
        .authenticate_for_scope(
            extract_anthropic_authorization_header(&headers).as_deref(),
            gateway_core::SCOPE_INFERENCE,
        )
        .await?;
    Ok(Json(
        state
            .service
            .models_list_for_api_key(&auth, &state.providers)
            .await?,
    ))
}

/// LiteLLM's `GET /model_group/info`, so LiteLLM-aware clients can discover
/// models with their limits, pricing, and capabilities.
pub async fn v1_model_group_info(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<gateway_service::model_metadata::ModelGroupInfoResponse>, AppError> {
    let auth = state
        .service
        .authenticate_for_scope(
            extract_anthropic_authorization_header(&headers).as_deref(),
            gateway_core::SCOPE_INFERENCE,
        )
        .await?;
    Ok(Json(
        state
            .service
            .model_group_info_for_api_key(&auth, &state.providers)
            .await?,
    ))
}

fn anthropic_error_response(error: GatewayError) -> Response {
    let mut response = native_messages::upstream_error_response(&error).unwrap_or_else(|| {
        let status = StatusCode::from_u16(error.http_status_code())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
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
    });
    super::traffic::mark_upstream_failure(&mut response, &error);
    response
}

pub async fn v1_chat_completions(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    policy: RequestPolicyContext,
    InferenceAuth(auth): InferenceAuth,
    Json(mut request): Json<ChatCompletionsRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_id = canonical_request_id(request_id)?;
    let requested_service_tier = take_requested_service_tier(&mut request.extra)?;
    let mut core_request = openai_chat_request_to_core(&request);
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
        .inspect_err(|rejection| {
            resolved
                .selection
                .log_reasoning_effort_rejection(Some(&request_id), rejection);
        })
        .map_err(GatewayError::from)?;
    enforce_requested_service_tier(&resolved, requested_service_tier)?;

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
        requested_service_tier,
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
        rate_limit_team,
        resolved: &resolved,
        request_id: &request_id,
        request_started_at,
        stream: core_request.stream,
        endpoint: RoutingEndpoint::ChatCompletions,
        requested_service_tier,
        request_log_context,
    }
    .prepare(selected, &mut core_request)
    .await?;
    let route = &execution.route;
    let provider = &execution.provider;
    let guard_context = &execution.guard_context;
    let request_log_context = &execution.request.request_log_context;
    let context = execution.provider_context(request_headers);

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
                inference::InferenceResponseFormat::OpenAi,
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
    execution
        .complete_response(
            value,
            attempt_started_at,
            inference::InferenceResponseFormat::OpenAi,
        )
        .await
}

pub async fn v1_responses(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    policy: RequestPolicyContext,
    InferenceAuth(auth): InferenceAuth,
    Json(mut request): Json<ResponsesRequest>,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_id = canonical_request_id(request_id)?;
    let requested_service_tier = take_requested_service_tier(&mut request.extra)?;
    let mut core_request = openai_responses_request_to_core(&request);
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
    enforce_responses_reasoning_effort(&core_request, resolved.selection.max_reasoning_effort)
        .inspect_err(|rejection| {
            resolved
                .selection
                .log_reasoning_effort_rejection(Some(&request_id), rejection);
        })
        .map_err(GatewayError::from)?;
    enforce_requested_service_tier(&resolved, requested_service_tier)?;

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
        requested_service_tier,
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
        rate_limit_team,
        resolved: &resolved,
        request_id: &request_id,
        request_started_at,
        stream: core_request.stream,
        endpoint: RoutingEndpoint::Responses,
        requested_service_tier,
        request_log_context,
    }
    .prepare(selected, &mut core_request)
    .await?;
    let route = &execution.route;
    let provider = &execution.provider;
    let guard_context = &execution.guard_context;
    let request_log_context = &execution.request.request_log_context;
    let context = execution.provider_context(request_headers);

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
                inference::InferenceResponseFormat::OpenAi,
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
    execution
        .complete_response(
            value,
            attempt_started_at,
            inference::InferenceResponseFormat::OpenAi,
        )
        .await
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
    let native_headers = match &error {
        ProviderError::NativeMessagesHttp { headers, .. } => Some(headers.clone()),
        _ => None,
    };
    let error = match error {
        ProviderError::UpstreamHttp {
            status: http_status,
            body,
        }
        | ProviderError::NativeMessagesHttp {
            status: http_status,
            body,
            ..
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
            let body = payload
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("upstream request failed")
                .to_string();
            if let Some(headers) = native_headers {
                ProviderError::NativeMessagesHttp {
                    status: http_status,
                    body,
                    headers,
                }
            } else {
                ProviderError::UpstreamHttp {
                    status: http_status,
                    body,
                }
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
        chat_protocol: Default::default(),
        extra_headers: route.extra_headers.clone(),
        extra_body: route.extra_body.clone(),
        request_headers,
        compatibility: route.compatibility.clone(),
        service_tier: None,
    }
}

struct LoggingBodyStreamState {
    upstream: gateway_core::ProviderStream,
    finalization: Option<StreamFinalization>,
    /// Ends the stream at the shutdown cutoff and tracks finalization, so the
    /// gateway waits for usage and logging before it exits.
    shutdown: crate::shutdown::Shutdown,
}

/// Moved exactly once into an owned task before the first finalization await.
/// The task never owns or drains the upstream HTTP stream.
struct StreamFinalization {
    service: std::sync::Arc<AppGatewayService>,
    metrics: std::sync::Arc<crate::observability::GatewayMetrics>,
    auth: AuthenticatedApiKey,
    /// The team the request's rate-limit check counted.
    rate_limit_team: RateLimitMemberTeam,
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
    saw_terminal_event: bool,
    requires_message_stop: bool,
    collector: gateway_service::StreamResponseCollector,
    stream_trace: StreamTrace,
    #[cfg(test)]
    test_gate: Option<StreamFinalizationTestGate>,
}

#[cfg(test)]
struct StreamFinalizationTestGate {
    after_accounting: bool,
    entered: tokio::sync::oneshot::Sender<()>,
    resume: tokio::sync::oneshot::Receiver<()>,
}

#[cfg(test)]
async fn pause_stream_finalization(
    gate: &mut Option<StreamFinalizationTestGate>,
    after_accounting: bool,
) {
    if gate
        .as_ref()
        .is_some_and(|gate| gate.after_accounting == after_accounting)
    {
        let gate = gate.take().expect("matching finalization checkpoint");
        let _ = gate.entered.send(());
        let _ = gate.resume.await;
    }
}

impl StreamFinalization {
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
        let Some(finalization) = self.finalization.take() else {
            return;
        };
        let termination = if self.shutdown.is_cut_off() {
            StreamTermination::ServerShutdown
        } else {
            record_cancelled_stream_metrics(&finalization);
            StreamTermination::ClientCancelled
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                request_id = %finalization.request_log_context.request_id,
                provider_key = %finalization.provider_key,
                "cannot persist cancelled stream outside a Tokio runtime"
            );
            return;
        };
        if matches!(termination, StreamTermination::ClientCancelled) {
            // Register the charge synchronously: an immediate retry must wait
            // even when this client cancels before persistence starts.
            spawn_cancelled_stream_token_charge(&finalization);
        }
        self.shutdown.tasks().spawn_on(
            finalize_stream(finalization, termination).in_current_span(),
            &runtime,
        );
    }
}

/// A cancelled stream skips usage accounting, but usage the provider already
/// reported still counts against token limits, so cancelling a stream cannot
/// be used to dodge them.
fn spawn_cancelled_stream_token_charge(state: &StreamFinalization) {
    let usage = gateway_service::request_logging::usage_summary_from_value(state.collector.usage());
    let Some(total_tokens) = chargeable_tokens(
        usage.total_tokens,
        usage.prompt_tokens,
        usage.completion_tokens,
    ) else {
        return;
    };
    let model_keys = [
        state.requested_model_key.as_str(),
        state.resolved_model_key.as_str(),
    ];
    let subject = RateLimitSubject::new(RateLimitOperation::Inference, &state.auth)
        .with_models(&model_keys)
        .with_member_team(state.rate_limit_team);
    // Nothing can await here: the task runs on, and this replica's next
    // check for the same counters waits for it.
    drop(
        state
            .service
            .rate_limiter()
            .record_tokens_detached(&subject, total_tokens),
    );
}

/// The tokens a token limit charges: the reported total, or prompt plus
/// completion when no total is reported. `None` when there is nothing to charge.
fn chargeable_tokens(
    total_tokens: Option<i64>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
) -> Option<i64> {
    total_tokens
        .or_else(|| Some(prompt_tokens.unwrap_or(0) + completion_tokens.unwrap_or(0)))
        .filter(|tokens| *tokens > 0)
}

fn record_cancelled_stream_metrics(state: &StreamFinalization) {
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

struct UsageAccountingContext<'a> {
    auth: &'a AuthenticatedApiKey,
    /// The team the request's rate-limit check counted.
    rate_limit_team: RateLimitMemberTeam,
    model: &'a gateway_core::GatewayModel,
    route: &'a gateway_core::ModelRoute,
    request_id: &'a str,
    labels: ChatMetricLabels<'a>,
    operation: &'static str,
    service_tiers: gateway_core::ServiceTierOutcome,
    /// Sizes that estimate the request's tokens when the provider reports none.
    usage_estimate: gateway_service::UsageEstimateInput,
}

/// Records a request rejected after routing but before any provider call, such as a
/// guardrail block or a hard-budget rejection: a summary row with no attempts.
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
    best_effort_log_failure(
        &state.service,
        auth,
        request_log_context,
        &route.provider_key,
        icon_metadata,
        labels.stream,
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
    rate_limit_team: RateLimitMemberTeam,
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
            rate_limit_team,
            model: &resolved.selection.execution_model,
            route,
            request_id: &request_log_context.request_id,
            labels: labels.clone(),
            operation: request_log_context.operation,
            service_tiers: service_tier_outcome(
                request_log_context,
                collector.served_service_tier(),
            ),
            // A provider may not bill a stream that failed, so only reported usage is charged.
            usage_estimate: gateway_service::UsageEstimateInput::default(),
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
    rate_limit_team: RateLimitMemberTeam,
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
                    rate_limit_team,
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

fn wrap_stream_with_request_logging(
    state: LoggingBodyStreamState,
) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> {
    futures_stream::unfold(state, |mut state| async move {
        state.finalization.as_ref()?;

        // Upstream chunks win over the cutoff, so a stream that is finishing
        // when the drain limit passes still completes.
        let next = tokio::select! {
            biased;
            item = state.upstream.next() => Some(item),
            () = state.shutdown.cut_off_reached() => None,
        };
        let Some(next) = next else {
            let error = state
                .finalize(StreamTermination::ServerShutdown)
                .await
                .expect_err("shutdown cutoff");
            return Some((Err(std::io::Error::other(error.to_string())), state));
        };
        match next {
            Some(Ok(chunk)) => {
                let finalization = state.finalization.as_mut().expect("active stream");
                let observation = finalization.collector.observe_chunk(chunk.as_ref());
                finalization.saw_terminal_event |= observation.has_terminal_event;
                finalization
                    .stream_trace
                    .observe_chunk(chunk.len(), observation);
                if observation.ends_stream
                    && let Err(error) = state.finalize(StreamTermination::Complete).await
                {
                    return Some((Err(std::io::Error::other(error.to_string())), state));
                }
                Some((Ok(chunk), state))
            }
            Some(Err(error)) => {
                let error = state
                    .finalize(StreamTermination::Transport(error))
                    .await
                    .expect_err("transport failure");
                Some((Err(std::io::Error::other(error.to_string())), state))
            }
            None => match state.finalize(StreamTermination::Complete).await {
                Ok(()) => None,
                Err(error) => Some((Err(std::io::Error::other(error.to_string())), state)),
            },
        }
    })
}

enum StreamTermination {
    Complete,
    Transport(ProviderError),
    ClientCancelled,
    /// The shutdown drain limit passed before the stream finished.
    ServerShutdown,
}

impl LoggingBodyStreamState {
    fn finalize(
        &mut self,
        termination: StreamTermination,
    ) -> impl Future<Output = Result<(), GatewayError>> + use<> {
        let finalization = self.finalization.take().expect("stream finalized once");
        // Dropping the join handle detaches, rather than cancels, this bounded
        // persistence task. No body-owned future can interrupt billing or logging.
        let task = self
            .shutdown
            .tasks()
            .spawn(finalize_stream(finalization, termination).in_current_span());
        async move {
            task.await.map_err(|error| {
                GatewayError::Internal(format!("stream finalization task failed: {error}"))
            })?
        }
    }
}

async fn finalize_stream(
    mut state: StreamFinalization,
    termination: StreamTermination,
) -> Result<(), GatewayError> {
    state.collector.finish();
    let cancelled = matches!(termination, StreamTermination::ClientCancelled);
    let shut_down = matches!(termination, StreamTermination::ServerShutdown);
    let (transport, transport_outcome) = match termination {
        StreamTermination::Transport(error) => {
            let detail = error.to_string();
            let retryable = error.is_retryable();
            (Some(GatewayError::from(error)), Some((detail, retryable)))
        }
        StreamTermination::Complete
        | StreamTermination::ClientCancelled
        | StreamTermination::ServerShutdown => (None, None),
    };
    let mut failure = if cancelled {
        Some(gateway_service::StreamFailureSummary {
            status_code: 499,
            error_code: "client_cancelled".to_string(),
        })
    } else if shut_down {
        Some(gateway_service::StreamFailureSummary {
            status_code: 503,
            error_code: SERVER_SHUTDOWN.to_string(),
        })
    } else if let Some(error) = &transport {
        Some(gateway_service::StreamFailureSummary {
            status_code: error.http_status_code().into(),
            error_code: error.error_code().to_string(),
        })
    } else {
        state.collector.failure().cloned()
    };
    let completion_error = (state.requires_message_stop
        && !state.saw_terminal_event
        && failure.is_none())
    .then(|| {
        GatewayError::Provider(ProviderError::Transport(
            "upstream Anthropic stream ended prematurely before message_stop event".to_string(),
        ))
    });
    if let Some(error) = &completion_error {
        failure = Some(gateway_service::StreamFailureSummary {
            status_code: error.http_status_code().into(),
            error_code: error.error_code().to_string(),
        });
    }
    #[cfg(test)]
    pause_stream_finalization(&mut state.test_gate, false).await;
    // Cancelled partial streams keep their existing no-ledger semantics. Failed
    // streams charge only reported usage and never refresh session affinity.
    if !cancelled && (failure.is_none() || state.collector.usage().is_some()) {
        account_stream_usage(&state, failure.is_none()).await;
    }
    #[cfg(test)]
    pause_stream_finalization(&mut state.test_gate, true).await;
    let routing_error = if failure.is_none() {
        match &state.routing_receipt {
            Some(receipt) => receipt
                .complete(
                    state.service.store().as_ref(),
                    if state.saw_terminal_event {
                        state.collector.response_id()
                    } else {
                        None
                    },
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
    let termination_reason = if cancelled {
        "client_cancelled"
    } else if shut_down {
        SERVER_SHUTDOWN
    } else if transport.is_some() {
        "stream_transport_error"
    } else if routing_error.is_some() {
        "routing_state_error"
    } else if failure.is_some() {
        "stream_error_event"
    } else {
        "complete"
    };
    state.stream_trace.finish(
        termination_reason,
        failure.as_ref().map(|_| termination_reason),
    );
    tracing::info!(
        request_id = %state.request_log_context.request_id,
        provider_key = %state.provider_key,
        termination_reason,
        "inference stream terminated"
    );
    let tool_cardinality = RequestToolCardinality {
        invoked_tool_count: Some(state.collector.invoked_tool_count()),
        ..state.request_log_context.tool_cardinality
    };
    let mut attempt = match failure.as_ref() {
        Some(failure) => stream_failure_attempt(
            &state.request_log_context,
            &state.route,
            state.attempt_started_at,
            failure,
        ),
        None => success_attempt(
            &state.request_log_context,
            &state.route,
            true,
            state.attempt_started_at,
        ),
    };
    if let Some((detail, retryable)) = transport_outcome {
        attempt.error_detail = Some(detail);
        attempt.retryable = retryable;
    }
    let (status_code, outcome) = match failure.as_ref() {
        Some(_) if transport.is_some() => {
            let error = transport.as_ref().expect("transport failure");
            (i64::from(error.http_status_code()), error.error_type())
        }
        Some(_) if shut_down => (503, SERVER_SHUTDOWN),
        Some(failure) => (failure.status_code, "upstream_error"),
        None => (200, "success"),
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
            failure,
            attempts: vec![attempt],
        },
    )
    .await;
    if !cancelled {
        let labels = ChatMetricLabels {
            requested_model: &state.requested_model_key,
            resolved_model: &state.resolved_model_key,
            provider_key: &state.provider_key,
            stream: true,
        };
        state.metrics.record_chat_request(&ChatRequestMetric {
            labels: labels.clone(),
            status_code,
            outcome,
            latency_seconds: latency_seconds_since(state.started_at),
        });
        state.metrics.record_tool_cardinality(
            &labels,
            state.request_log_context.operation,
            &tool_cardinality,
        );
    }
    if shut_down {
        return Err(GatewayError::Internal(
            "the gateway shut down before the stream finished".to_string(),
        ));
    }
    match transport.or(routing_error).or(completion_error) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Error code, request-log error code and stream termination reason for a
/// request the shutdown cutoff ended.
pub(super) const SERVER_SHUTDOWN: &str = "server_shutdown";

/// Records usage against the ledger and budgets. Only successful streams may
/// estimate missing usage; failed streams charge what the provider reported.
async fn account_stream_usage(state: &StreamFinalization, successful: bool) {
    let labels = state.metric_labels();
    finalize_successful_usage_accounting_from_parts(
        &state.service,
        &state.metrics,
        UsageAccountingContext {
            auth: &state.auth,
            rate_limit_team: state.rate_limit_team,
            model: &state.execution_model,
            route: &state.route,
            request_id: &state.request_log_context.request_id,
            labels,
            operation: state.request_log_context.operation,
            service_tiers: service_tier_outcome(
                &state.request_log_context,
                state.collector.served_service_tier(),
            ),
            usage_estimate: if successful {
                state
                    .request_log_context
                    .usage_estimate(Some(state.collector.output_text_bytes()))
            } else {
                gateway_service::UsageEstimateInput::default()
            },
        },
        state.collector.usage().cloned(),
    )
    .await;
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
    best_effort_log_failure(
        service,
        auth,
        context,
        provider_key,
        icon_metadata,
        false,
        latency_ms,
        gateway_error,
        attempts,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn best_effort_log_failure(
    service: &std::sync::Arc<AppGatewayService>,
    auth: &AuthenticatedApiKey,
    context: &gateway_service::RequestLogContext,
    provider_key: &str,
    icon_metadata: RequestLogIconMetadata,
    stream: bool,
    latency_ms: i64,
    gateway_error: &GatewayError,
    attempts: Vec<RequestAttemptRecord>,
) {
    if let Err(error) = service
        .log_failure(
            auth,
            context,
            provider_key,
            icon_metadata,
            stream,
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

/// How long a credential's MCP tool set is reused for request telemetry.
/// A grant change shows in the counts within this long.
pub const MCP_TELEMETRY_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);

/// Token-overhead estimates running at once. They are best effort, so when
/// the store falls behind, a request past this records its counts without
/// one instead of queueing more work.
const MCP_TELEMETRY_TASKS: usize = 64;
static MCP_TELEMETRY_PERMITS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(MCP_TELEMETRY_TASKS);

/// A credential's MCP tool set, as inference request telemetry counts it.
#[derive(Debug)]
pub struct McpTelemetrySnapshot {
    referenced_server_count: i64,
    filtered_tool_count: i64,
    tools: Arc<[gateway_core::ExternalMcpToolRecord]>,
}

/// Records the caller's MCP tool counts on the request log and estimates
/// the tokens their definitions would take.
///
/// This runs before dispatch on every inference request, so it stays cheap:
/// the tool set is cached per credential for [`MCP_TELEMETRY_CACHE_TTL`], and
/// the token-overhead estimate and its writes run in a background task, at
/// most [`MCP_TELEMETRY_TASKS`] at once.
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
    // A key without an MCP scope can reach no tools, so there is nothing to record.
    if !auth.has_scope(gateway_core::SCOPE_MCP_AGGREGATE)
        && !auth.has_scope(gateway_core::SCOPE_MCP_PROXY)
    {
        return;
    }
    let snapshot = match mcp_telemetry_snapshot(state, auth).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            tracing::warn!(
                request_id = %context.request_id,
                error = %error,
                "failed resolving MCP access for request telemetry"
            );
            return;
        }
    };

    context.tool_cardinality.referenced_mcp_server_count = Some(snapshot.referenced_server_count);
    context.tool_cardinality.exposed_tool_count = Some(snapshot.tools.len() as i64);
    context.tool_cardinality.filtered_tool_count = Some(snapshot.filtered_tool_count);

    let Ok(permit) = MCP_TELEMETRY_PERMITS.try_acquire() else {
        tracing::debug!(
            request_id = %context.request_id,
            "skipping the MCP token-overhead estimate: too many in flight"
        );
        return;
    };
    let state = state.clone();
    let request_id = context.request_id.clone();
    let model_key = context.resolved_model_key.clone();
    let route = route.clone();
    let provider = provider.cloned();
    tokio::spawn(
        async move {
            record_mcp_token_overhead(&state, request_id, model_key, &route, provider, snapshot)
                .await;
            drop(permit);
        }
        .in_current_span(),
    );
}

/// The credential's tool set, from the cache or resolved once for
/// concurrent requests.
pub(crate) async fn mcp_telemetry_snapshot(
    state: &AppState,
    auth: &AuthenticatedApiKey,
) -> Result<Arc<McpTelemetrySnapshot>, GatewayError> {
    state
        .mcp_telemetry_cache
        .get_or_load(auth.id, || async {
            let resolution = McpAccess::new(state.store.clone())
                .effective_tools_for_api_key(auth, None, McpToolProjection::Full)
                .await?;
            let resolution = restrict_to_listed_servers(state, auth, resolution).await?;
            Ok(Arc::new(McpTelemetrySnapshot {
                referenced_server_count: resolution.referenced_server_count,
                filtered_tool_count: resolution.filtered_tool_count,
                tools: resolution.allowed_tools.into(),
            }))
        })
        .await
        .map(|(snapshot, _)| snapshot)
        .map_err(|(error, _)| error)
}

async fn record_mcp_token_overhead(
    state: &AppState,
    request_id: String,
    model_key: String,
    route: &gateway_core::ModelRoute,
    provider: Option<ResolvedProviderConnection>,
    snapshot: Arc<McpTelemetrySnapshot>,
) {
    let occurred_at = OffsetDateTime::now_utc();
    let context_window_tokens = match state
        .service
        .resolve_route_metadata_with_provider(route, provider.as_ref(), occurred_at)
        .await
    {
        Ok(metadata) => metadata.limits.context,
        Err(error) => {
            tracing::warn!(
                request_id = %request_id,
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
            request_id: request_id.clone(),
            request_log_id: None,
            model_key: Some(model_key),
            provider_family: provider
                .map(|provider| provider.provider_type)
                .unwrap_or_else(|| route.provider_key.clone()),
            model_or_encoding: route.upstream_model.clone(),
            tools: snapshot.tools.clone(),
            context_window_tokens,
            protocol_version: None,
            occurred_at,
        })
        .await
    {
        tracing::warn!(
            request_id = %request_id,
            error = %error,
            "failed recording MCP token-overhead telemetry"
        );
    }
}

/// Drops tools on servers outside the key's MCP server list, so telemetry
/// describes only what the key can reach, and recomputes the derived counts.
pub(crate) async fn restrict_to_listed_servers(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    mut resolution: gateway_core::McpAccessResolution,
) -> Result<gateway_core::McpAccessResolution, GatewayError> {
    let Some(server_keys) = auth.access.mcp_server_keys.as_deref() else {
        return Ok(resolution);
    };
    // One query for the listed keys, rather than one per key.
    let listed = gateway_core::McpRegistryRepository::list_external_mcp_servers_by_keys(
        state.store.as_ref(),
        server_keys,
    )
    .await?
    .into_iter()
    .map(|server| server.mcp_server_id)
    .collect::<std::collections::HashSet<_>>();
    resolution
        .allowed_tools
        .retain(|tool| listed.contains(&tool.mcp_server_id));
    resolution.referenced_server_count = resolution
        .allowed_tools
        .iter()
        .map(|tool| tool.mcp_server_id)
        .collect::<std::collections::HashSet<_>>()
        .len() as i64;
    resolution.filtered_tool_count = resolution
        .exposed_tool_count
        .saturating_sub(resolution.allowed_tools.len() as i64);
    Ok(resolution)
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
    // Token limits count what the provider reported, whatever happens to the
    // ledger and budget accounting below. The charge starts before anything
    // here awaits, on its own task, so a client that drops a finished stream
    // mid-accounting cannot cancel it, and the drop path never charges it a
    // second time. It is awaited below, so it has landed before the response
    // completes, as a caller's next request on another replica needs.
    let token_charge = charge_rate_limit_tokens(
        service,
        &context,
        &gateway_service::request_logging::usage_summary_from_value(provider_usage.as_ref()),
    );
    match service
        .record_chat_usage(
            context.auth,
            context.model,
            context.route,
            context.request_id,
            provider_usage,
            context.service_tiers,
            context.usage_estimate,
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
    if let Some(token_charge) = token_charge {
        let _ = token_charge.await;
    }
}

/// Token limits are checked before a request and charged here, once the
/// provider has reported usage. Returns the charge's task, for the caller to
/// await.
fn charge_rate_limit_tokens(
    service: &AppGatewayService,
    context: &UsageAccountingContext<'_>,
    usage: &gateway_service::request_logging::UsageSummary,
) -> Option<tokio::task::JoinHandle<()>> {
    let total_tokens = chargeable_tokens(
        usage.total_tokens,
        usage.prompt_tokens,
        usage.completion_tokens,
    )?;
    let model_keys = [
        context.labels.requested_model,
        context.labels.resolved_model,
    ];
    let subject = RateLimitSubject::new(RateLimitOperation::Inference, context.auth)
        .with_models(&model_keys)
        .with_member_team(context.rate_limit_team);
    service
        .rate_limiter()
        .record_tokens_detached(&subject, total_tokens)
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

/// [`enforce_inference_rate_limits`] for Chat Completions, Responses and
/// Messages. A rejection still records one `gateway.chat.requests` sample, as
/// every other exit after model resolution does.
async fn enforce_chat_rate_limits(
    state: &AppState,
    policy: &RequestPolicyContext,
    auth: &AuthenticatedApiKey,
    resolved: &ResolvedGatewayRequest,
    stream: bool,
    request_started_at: Instant,
) -> Result<RateLimitMemberTeam, GatewayError> {
    let result =
        enforce_inference_rate_limits(state, policy, auth, resolved, RateLimitOperation::Inference)
            .await;
    if let Err(error) = &result {
        state.metrics.record_chat_request(&ChatRequestMetric {
            labels: ChatMetricLabels {
                requested_model: &resolved.selection.requested_model.model_key,
                resolved_model: &resolved.selection.execution_model.model_key,
                provider_key: "unavailable",
                stream,
            },
            status_code: i64::from(error.http_status_code()),
            outcome: error.error_type(),
            latency_seconds: latency_seconds_since(request_started_at),
        });
    }
    result
}

/// Rejects the request with `429 rate_limited` when a matching policy is
/// exhausted. Runs after model resolution and before the request log,
/// guardrails, budget and provider call, so a rejection costs nothing upstream.
/// Returns the team the check counted, for the request's token charge.
async fn enforce_inference_rate_limits(
    state: &AppState,
    policy: &RequestPolicyContext,
    auth: &AuthenticatedApiKey,
    resolved: &ResolvedGatewayRequest,
    operation: RateLimitOperation,
) -> Result<RateLimitMemberTeam, GatewayError> {
    let model_keys = [
        resolved.selection.requested_model.model_key.as_str(),
        resolved.selection.execution_model.model_key.as_str(),
    ];
    let subject = RateLimitSubject::new(operation, auth).with_models(&model_keys);
    enforce_rate_limits(state, policy, &subject).await
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
    let mut extracted = headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect::<BTreeMap<_, _>>();
    // Anthropic permits repeated beta headers, but the provider context stores
    // one value per name. Preserve every opt-in before transport-specific mapping.
    let betas = headers
        .get_all("anthropic-beta")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>();
    if !betas.is_empty() {
        extracted.insert("anthropic-beta".to_string(), betas.join(","));
    }
    extracted
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashMap},
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
        ApiKeyModelGrantMode, ApiKeyOwnerKind, AuthenticatedApiKey, BudgetRepository,
        ChatCompletionsRequest, CoreChatMessage, CoreChatRequest, CoreEmbeddingsRequest,
        CoreRequestRequirements, CoreResponsesRequest, GatewayError, GitHubCopilotChatApi,
        GitHubCopilotRouteCompatibility, GitHubCopilotUpstreamSupports, ModelRoute,
        PartialCapabilities, ProviderCapabilities, ProviderClient, ProviderError, ProviderRegistry,
        ProviderRequestContext, ProviderStream, RequestAttemptStatus, RequestTags,
        effective_route_capabilities,
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
        canonical_request_id, extract_anthropic_authorization_header, request_log_icon_metadata,
        split_partial_provider_error, wrap_stream_with_request_logging,
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

    #[test]
    fn request_headers_preserve_repeated_anthropic_beta_opt_ins() {
        let mut headers = HeaderMap::new();
        headers.append(
            "anthropic-beta",
            HeaderValue::from_static("thinking-display-updates-2026-08-18"),
        );
        headers.append(
            "Anthropic-Beta",
            HeaderValue::from_static("mid-conversation-output-config-2026-07-01"),
        );
        headers.insert("x-request-id", HeaderValue::from_static("request_1"));

        let extracted = super::extract_request_headers(&headers);
        assert_eq!(
            extracted["anthropic-beta"],
            "thinking-display-updates-2026-08-18,mid-conversation-output-config-2026-07-01"
        );
        assert_eq!(extracted["x-request-id"], "request_1");
        assert!(super::extract_request_headers(&HeaderMap::new()).is_empty());
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
        shutdown: crate::shutdown::Shutdown,
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
                shutdown: crate::shutdown::Shutdown::new(),
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
                finalization: Some(super::StreamFinalization {
                    service: self.service.clone(),
                    metrics: self.metrics.clone(),
                    auth: self.auth.clone(),
                    rate_limit_team: Default::default(),
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
                    saw_terminal_event: false,
                    requires_message_stop: false,
                    collector: self.service.new_stream_response_collector(),
                    stream_trace: StreamTrace::new(
                        "chat",
                        request_id,
                        &self.route,
                        &self.provider,
                        Instant::now(),
                    ),
                    test_gate: None,
                }),
                shutdown: self.shutdown.clone(),
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
    async fn shutdown_cutoff_ends_an_open_stream_with_an_error_and_logs_it() {
        let harness = StreamTestHarness::new().await;
        let request_id = "stream-shutdown-test";
        let upstream: ProviderStream = Box::pin(
            stream::iter([Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            ))])
            .chain(stream::pending()),
        );
        let (context, state) = harness.logging_state(request_id, upstream);
        let mut body_stream = Box::pin(wrap_stream_with_request_logging(state));

        assert!(matches!(body_stream.next().await, Some(Ok(_))));
        harness.shutdown.cut_off();
        let ended = body_stream.next().await.expect("an item at the cutoff");
        assert!(
            ended.is_err(),
            "the client must see an error, not a clean end"
        );
        assert!(body_stream.next().await.is_none());
        assert!(
            harness.shutdown.finish_tasks().await,
            "shutdown waits for the stream's finalization"
        );

        let detail = harness
            .service
            .get_request_log_detail(context.request_log_id)
            .await
            .expect("request log persisted before shutdown returned");
        assert_eq!(detail.log.status_code, Some(503));
        assert_eq!(detail.log.error_code.as_deref(), Some("server_shutdown"));
        let snapshot = harness.metrics.test_snapshot();
        assert_eq!(snapshot.request_outcomes.get("server_shutdown"), Some(&1));
    }

    #[tokio::test]
    async fn shutdown_joins_backpressured_stream_before_waiting_for_usage_and_logs() {
        use tokio::io::AsyncWriteExt;

        let harness = StreamTestHarness::new().await;
        let (writing_tx, writing_rx) = tokio::sync::oneshot::channel();
        let upstream: ProviderStream = Box::pin(
            stream::iter([Ok(Bytes::from_static(
                b"data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n",
            ))])
            .chain(stream::once(async move {
                writing_tx.send(()).unwrap();
                // Much larger than the socket buffers: a client that never
                // reads keeps hyper from polling the body again at cutoff.
                Ok(Bytes::from(vec![b'x'; 32 * 1024 * 1024]))
            }))
            .chain(stream::pending()),
        );
        let (context, mut state) = harness.logging_state("backpressured-shutdown", upstream);
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        state.finalization.as_mut().unwrap().test_gate = Some(super::StreamFinalizationTestGate {
            after_accounting: false,
            entered: entered_tx,
            resume: resume_rx,
        });
        let state = Arc::new(std::sync::Mutex::new(Some(state)));
        let app = axum::Router::new().route("/", axum::routing::get(move || {
            let state = state.lock().unwrap().take().unwrap();
            async move { axum::body::Body::from_stream(wrap_stream_with_request_logging(state)) }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let serving = tokio::spawn({
            let shutdown = harness.shutdown.clone();
            async move { shutdown.serve(listener, app).await }
        });
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        writing_rx.await.expect("hyper polled the large response");
        harness.shutdown.begin_drain();
        harness.shutdown.cut_off();
        tokio::time::timeout(Duration::from_secs(10), serving)
            .await
            .expect("connections are forcibly closed")
            .unwrap()
            .unwrap();
        entered_rx
            .await
            .expect("dropping the connection registered finalization");
        assert!(!harness.shutdown.tasks().is_empty());
        let finishing = tokio::spawn({
            let shutdown = harness.shutdown.clone();
            async move { shutdown.finish_tasks().await }
        });
        resume_tx
            .send(())
            .expect("usage write survives connection teardown");
        assert!(finishing.await.unwrap());
        let detail = harness
            .service
            .get_request_log_detail(context.request_log_id)
            .await
            .unwrap();
        assert_eq!(detail.log.status_code, Some(503));
        assert_eq!(detail.log.error_code.as_deref(), Some("server_shutdown"));
        let ledger = harness
            .store
            .get_usage_ledger_by_request_and_scope(
                "backpressured-shutdown",
                &harness.ownership_scope_key(),
            )
            .await
            .unwrap()
            .expect("reported usage was charged before shutdown finished");
        assert_eq!(ledger.total_tokens, Some(7));
        drop(client);
    }

    #[tokio::test]
    async fn cancelled_stream_charges_reported_usage_to_token_limits() {
        let harness = StreamTestHarness::new().await;
        let limiter = Arc::new(gateway_service::RateLimiter::new(
            Arc::new(harness.store.clone()),
            gateway_service::RateLimitSettings {
                config_policies: vec![gateway_service::ConfiguredRateLimitPolicy {
                    name: "key tokens".to_string(),
                    operation: gateway_core::RateLimitOperation::Inference,
                    scope_kind: gateway_core::RateLimitScopeKind::ApiKey,
                    model_key: None,
                    mcp_server_key: None,
                    metric: gateway_core::RateLimitMetric::Tokens,
                    period: gateway_core::RateLimitPeriod::Hour,
                    limit: 1_000,
                }],
                ..Default::default()
            },
        ));
        let service = Arc::new(
            GatewayService::new(
                Arc::new(harness.store.clone()),
                Arc::new(WeightedRoutePlanner::default()),
            )
            .with_rate_limiter(limiter.clone()),
        );
        // Usage arrives early, as Anthropic-style streams report input usage
        // up front; the client then disconnects.
        let upstream: ProviderStream = Box::pin(stream::iter([Ok(Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}],\"usage\":{\"prompt_tokens\":40,\"completion_tokens\":2,\"total_tokens\":42}}\n\n",
        ))]));
        let (_, mut state) = harness.logging_state("stream-cancel-tokens", upstream);
        state.finalization.as_mut().unwrap().service = service;
        let mut body_stream = Box::pin(wrap_stream_with_request_logging(state));
        assert!(body_stream.next().await.is_some());
        drop(body_stream);

        let models = ["fast"];
        let subject = gateway_service::RateLimitSubject::new(
            gateway_core::RateLimitOperation::Inference,
            &harness.auth,
        )
        .with_models(&models);
        // The next check waits for the detached charge, so a client that
        // cancels and retries at once already sees the usage.
        let decision = limiter.check(&subject).await.expect("check");
        let remaining = decision.status().map(|status| status.remaining);
        assert_eq!(remaining, Some(958));
    }

    async fn assert_finalization_survives_body_cancellation(
        harness: &StreamTestHarness,
        protocol: &str,
        after_accounting: bool,
        expected_charged_tokens: i64,
    ) {
        use gateway_core::AttributionRepository as _;

        let request_uuid = uuid::Uuid::new_v4();
        let request_id = request_uuid.to_string();
        let usage = Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}\n\n",
        );
        let frames = match protocol {
            "response.completed" | "response.incomplete" | "response.failed" => {
                vec![Ok(Bytes::from(format!(
                    "event: {protocol}\ndata: {{\"type\":\"{protocol}\",\"response\":{{\"id\":\"resp_finalization\",\"usage\":{{\"input_tokens\":5,\"output_tokens\":2,\"total_tokens\":7}}}}}}\n\n"
                )))]
            }
            "native_messages" => vec![Ok(Bytes::from_static(
                b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":5}}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            ))],
            "transport_error" => vec![
                Ok(usage),
                Err(ProviderError::Transport("connection reset".to_string())),
            ],
            "chat" | "translated_messages" => vec![
                Ok(usage),
                Ok(Bytes::from_static(b"data: [DONE]\n\n")),
            ],
            _ => panic!("unexpected protocol {protocol}"),
        };
        // The provider deliberately never closes. A terminal-event consumer
        // must not need EOF, nor leave an upstream-draining task behind.
        let upstream: ProviderStream = Box::pin(stream::iter(frames).chain(stream::pending()));
        let upstream = if protocol == "translated_messages" {
            super::anthropic_messages_stream_from_openai(upstream, "fast".to_string())
        } else {
            upstream
        };
        let (context, mut state) = harness.logging_state(&request_id, upstream);
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        let finalization = state.finalization.as_mut().unwrap();
        finalization.requires_message_stop = protocol == "native_messages";
        finalization.test_gate = Some(super::StreamFinalizationTestGate {
            after_accounting,
            entered: entered_tx,
            resume: resume_rx,
        });
        let requests_before = harness.metrics.test_snapshot().requests;
        let client = tokio::spawn(async move {
            let body = axum::body::Body::from_stream(wrap_stream_with_request_logging(state));
            let mut chunks = body.into_data_stream();
            while chunks.next().await.is_some() {}
        });
        tokio::time::timeout(Duration::from_secs(5), entered_rx)
            .await
            .expect("protocol terminal starts finalization without EOF")
            .expect("finalizer reached persistence checkpoint");
        assert!(!client.is_finished());
        assert!(
            harness
                .service
                .get_request_log_detail(context.request_log_id)
                .await
                .is_err(),
            "checkpoint precedes request logging"
        );
        let ledger = harness
            .store
            .get_usage_ledger_by_request_and_scope(&request_id, &harness.ownership_scope_key())
            .await
            .unwrap();
        assert_eq!(ledger.is_some(), after_accounting);

        client.abort();
        assert!(client.await.unwrap_err().is_cancelled());
        // The client hook can arrive while persistence is still pending. Its
        // association must join the eventual request and usage, not be lost.
        let now = gateway_service::offset_now();
        let session_hash = format!("{:064x}", request_uuid.as_u128());
        harness
            .store
            .apply_attribution_request_session(&gateway_core::AttributionRequestSessionWrite {
                owner: gateway_core::AttributionOwner {
                    ownership_scope_key: harness.ownership_scope_key(),
                    owner_user_id: harness.auth.owner_user_id,
                    owner_service_account_id: harness.auth.owner_service_account_id,
                },
                request_id: request_uuid,
                session_hash: session_hash.clone(),
                freshness: gateway_core::AttributionFreshness {
                    installation_id: "stream-finalization-test".to_string(),
                    sequence: 1,
                    observed_at: now,
                    late_sequenced: false,
                },
                received_at: now,
            })
            .await
            .expect("correlate request while its finalizer is pending");
        resume_tx
            .send(())
            .expect("finalizer outlives response body");
        tokio::time::timeout(Duration::from_secs(5), async {
            while harness.metrics.test_snapshot().requests == requests_before {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("owned finalizer completes after body cancellation");

        let failed = matches!(protocol, "response.failed" | "transport_error");
        let detail = harness
            .service
            .get_request_log_detail(context.request_log_id)
            .await
            .expect("request log survives cancellation");
        assert_eq!(detail.log.status_code, Some(if failed { 502 } else { 200 }));
        assert_eq!(
            detail.log.error_code.as_deref(),
            match protocol {
                "response.failed" => Some("stream_error"),
                "transport_error" => Some("upstream_transport"),
                _ => None,
            }
        );
        assert_eq!(
            (detail.log.prompt_tokens, detail.log.completion_tokens),
            if failed {
                (None, None)
            } else {
                (Some(5), Some(2))
            },
            "reported usage survives cancellation for {protocol}"
        );
        assert_eq!(detail.attempts.len(), 1);
        assert_eq!(
            detail.attempts[0].status,
            if failed {
                RequestAttemptStatus::StreamError
            } else {
                RequestAttemptStatus::Success
            }
        );
        let logs = harness
            .service
            .list_request_logs(&gateway_core::RequestLogQuery {
                request_id: Some(request_id.clone()),
                page: 1,
                page_size: 10,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(logs.total, 1);
        assert_eq!(
            harness.metrics.test_snapshot().requests,
            requests_before + 1
        );
        assert_eq!(
            harness
                .metrics
                .test_snapshot()
                .request_outcomes
                .get("client_cancelled"),
            None,
            "completed or failed finalization is never reclassified as cancellation"
        );
        let ledger = harness
            .store
            .get_usage_ledger_by_request_and_scope(&request_id, &harness.ownership_scope_key())
            .await
            .unwrap()
            .expect("usage survives response body cancellation");
        assert_eq!(ledger.total_tokens, Some(7));
        let facts = harness
            .store
            .list_attribution_request_facts(&[session_hash], gateway_service::offset_now())
            .await
            .expect("session correlation can join the persisted request and usage");
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].request_id, request_id);
        assert_eq!(facts[0].ownership_scope_key, harness.ownership_scope_key());
        assert!(facts[0].pricing_status.is_some());
        let models = ["fast"];
        let subject = gateway_service::RateLimitSubject::new(
            gateway_core::RateLimitOperation::Inference,
            &harness.auth,
        )
        .with_models(&models);
        let decision = harness
            .service
            .rate_limiter()
            .check(&subject)
            .await
            .unwrap();
        assert_eq!(
            decision.status().map(|status| status.remaining),
            Some(1_000 - expected_charged_tokens),
            "each stream charges token limits exactly once"
        );
    }

    #[tokio::test]
    async fn stream_finalization_survives_body_cancellation_at_persistence_checkpoints() {
        let mut harness = StreamTestHarness::new().await;
        let limiter = Arc::new(gateway_service::RateLimiter::new(
            Arc::new(harness.store.clone()),
            gateway_service::RateLimitSettings {
                config_policies: vec![gateway_service::ConfiguredRateLimitPolicy {
                    name: "key tokens".to_string(),
                    operation: gateway_core::RateLimitOperation::Inference,
                    scope_kind: gateway_core::RateLimitScopeKind::ApiKey,
                    model_key: None,
                    mcp_server_key: None,
                    metric: gateway_core::RateLimitMetric::Tokens,
                    period: gateway_core::RateLimitPeriod::Hour,
                    limit: 1_000,
                }],
                ..Default::default()
            },
        ));
        harness.service = Arc::new(
            GatewayService::new(
                Arc::new(harness.store.clone()),
                Arc::new(WeightedRoutePlanner::default()),
            )
            .with_rate_limiter(limiter),
        );
        let mut charged_tokens = 0;
        for protocol in [
            "chat",
            "response.completed",
            "response.incomplete",
            "response.failed",
            "native_messages",
            "translated_messages",
            "transport_error",
        ] {
            for after_accounting in [false, true] {
                charged_tokens += 7;
                assert_finalization_survives_body_cancellation(
                    &harness,
                    protocol,
                    after_accounting,
                    charged_tokens,
                )
                .await;
            }
        }
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
            capabilities: PartialCapabilities::from_capabilities(capabilities),
            compatibility: Default::default(),
            service_tier: None,
        }
    }

    /// What routing reads for `route` when no request-specific restriction applies.
    fn effective_capabilities(
        provider: &StaticProvider,
        route: &ModelRoute,
    ) -> ProviderCapabilities {
        effective_route_capabilities(
            route,
            provider.provider_type(),
            PartialCapabilities::default(),
            route_effective_provider_capabilities(provider, route),
        )
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
            access: Default::default(),
            oauth: None,
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
        let embedding_capabilities = effective_capabilities(&provider, &embedding_route);
        assert!(embedding_capabilities.embeddings);
        assert!(!embedding_capabilities.chat_completions);
        assert!(!embedding_capabilities.responses);
        assert!(!embedding_capabilities.stream);
        assert!(!embedding_capabilities.tools);

        let chat_route = route(
            "google/gemini-2.0-flash",
            ProviderCapabilities::all_enabled(),
        );
        let chat_capabilities = effective_capabilities(&provider, &chat_route);
        assert!(chat_capabilities.chat_completions);
        assert!(chat_capabilities.stream);
        assert!(!chat_capabilities.embeddings);
        assert!(chat_capabilities.tools);

        let anthropic_route = route(
            "anthropic/claude-sonnet-4-6",
            ProviderCapabilities::all_enabled(),
        );
        let anthropic_capabilities = effective_capabilities(&provider, &anthropic_route);
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
        let capabilities = effective_capabilities(&provider, &claude_route);

        assert!(!capabilities.json_schema);
        assert!(capabilities.chat_completions);
        assert!(capabilities.tools);

        let unknown_route = route("unknown", ProviderCapabilities::all_enabled());
        let unknown_capabilities = effective_capabilities(&provider, &unknown_route);
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
        let responses_capabilities = effective_capabilities(&provider, &responses_route);
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
            &HashMap::new(),
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

        let (eligible_route_count, selected) = select_first_eligible_route(
            &providers,
            &routes,
            &HashMap::new(),
            request.requirements(),
        );

        assert_eq!(eligible_route_count, 1);
        assert_eq!(
            selected.expect("vision-capable route").route.upstream_model,
            "document-capable"
        );
    }

    #[test]
    fn image_only_catalog_hints_drop_chat_unless_the_route_declares_it() {
        let provider = Arc::new(StaticProvider {
            provider_type: "openai_compat",
            capabilities: ProviderCapabilities::all_enabled(),
        });
        let mut providers = ProviderRegistry::new();
        providers.register(provider);
        let mut hinted_only = route("image-model", ProviderCapabilities::all_enabled());
        hinted_only.capabilities = PartialCapabilities::default();
        let mut explicit_chat = route("image-model", ProviderCapabilities::all_enabled());
        explicit_chat.capabilities = PartialCapabilities {
            chat_completions: Some(true),
            ..Default::default()
        };
        // What the catalog says for a model whose `api_endpoints` list only images.
        let images_only = PartialCapabilities {
            chat_completions: Some(false),
            responses: Some(false),
            embeddings: Some(false),
            images: Some(true),
            ..Default::default()
        };
        let hints = HashMap::from([
            (hinted_only.id, images_only),
            (explicit_chat.id, images_only),
        ]);
        let explicit_chat_id = explicit_chat.id;
        let routes = vec![hinted_only, explicit_chat];

        let (eligible_route_count, selected) = select_first_eligible_route(
            &providers,
            &routes,
            &hints,
            CoreRequestRequirements {
                chat_completions: true,
                ..Default::default()
            },
        );
        assert_eq!(eligible_route_count, 1);
        assert_eq!(
            selected.expect("explicit chat route").route.id,
            explicit_chat_id
        );

        let (eligible_route_count, _) = select_first_eligible_route(
            &providers,
            &routes,
            &hints,
            CoreRequestRequirements {
                images: true,
                ..Default::default()
            },
        );
        assert_eq!(
            eligible_route_count, 2,
            "hints turn on the unset images flag"
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
        let system_one: gateway_core::SystemOneDecisionsRequest = serde_json::from_value(json!({
            "model": "gpt",
            "state": {"notes": format!("deploy with {key}")},
            "questions": {"ship": {"type": "noul", "instructions": "Ship it?"}},
        }))
        .expect("decisions request");
        let mut request = gateway_core::system_one_decisions_request_to_core(&system_one);

        let redacted =
            super::redact_typed_request(&state, "request-1", "openai/gpt", &mut request).await;

        assert!(redacted.is_ok());

        let gateway_core::CoreDecisionsInput::State(state_value) = &request.input else {
            panic!("System One input stays state");
        };
        assert_eq!(
            state_value["notes"],
            "deploy with [REDACTED:anthropic-api-key]"
        );
        assert_eq!(request.questions[0].key, "ship");
        assert_eq!(request.model, "gpt");
    }
}
