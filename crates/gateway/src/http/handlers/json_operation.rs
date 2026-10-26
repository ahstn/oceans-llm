//! The shared request path of the non-streaming JSON operations, embeddings
//! and images. A request is resolved, rate limited, routed, guarded, checked
//! against the budget and sent to one provider; its usage is then accounted
//! and its request log written. Each operation records its own
//! `gateway.<operation>.requests` metric rather than the chat request metric:
//! neither streams nor carries tools.
use gateway_core::RequestTags;

use super::*;
use crate::observability::{GatewayMetrics, OperationRequestMetric};

/// What differs between the operations [`execute_json_operation`] runs. The
/// request is also the prompt guardrail input, so it must round-trip through
/// JSON.
pub(super) trait JsonOperation: Serialize + DeserializeOwned + Send + Sync {
    /// Names the request log, the provider span and usage accounting.
    const OPERATION: &'static str;
    /// The route the request span records, whichever alias was called.
    const ROUTE_PATH: &'static str;
    const ENDPOINT: RoutingEndpoint;

    fn model(&self) -> &str;

    /// The capabilities a route needs to serve this request.
    fn route_requirements(&self) -> CoreRequestRequirements;

    /// The untyped body fields, which route selection reads.
    fn extra(&self) -> &BTreeMap<String, Value>;

    fn begin_request_log(
        &self,
        service: &AppGatewayService,
        request_id: &str,
        resolved: &ResolvedGatewayRequest,
        request_headers: &BTreeMap<String, String>,
        request_tags: RequestTags,
    ) -> RequestLogContext;

    fn call_provider(
        &self,
        provider: &dyn ProviderClient,
        context: &ProviderRequestContext,
    ) -> impl Future<Output = Result<Value, ProviderError>> + Send;

    /// The upstream body as the caller receives it.
    fn caller_response(upstream: Value, requested_model_key: &str) -> Value;

    fn record_request_metric(metrics: &GatewayMetrics, metric: &OperationRequestMetric<'_>);
}

/// Every exit after model resolution records one sample of the operation's
/// request metric, including rejections before any route is chosen.
pub(super) async fn execute_json_operation<O: JsonOperation>(
    state: AppState,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    policy: RequestPolicyContext,
    auth: AuthenticatedApiKey,
    request: O,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_id = canonical_request_id(request_id)?;
    let resolved = state
        .service
        .resolve_request(&auth, request.model())
        .await?;
    let requested_model = resolved.selection.requested_model.model_key.clone();
    let resolved_model = resolved.selection.execution_model.model_key.clone();

    let mut selected_provider = None;
    // A rate-limit rejection is an exit after resolution, so it is counted too.
    let result = match enforce_inference_rate_limits(
        &state,
        &policy,
        &auth,
        &resolved,
        RateLimitOperation::Inference,
    )
    .await
    {
        Ok(rate_limit_team) => {
            dispatch_json_operation(
                &state,
                &headers,
                &auth,
                ResolvedOperation {
                    request_id,
                    request_started_at,
                    rate_limit_team,
                    request,
                    resolved,
                },
                &mut selected_provider,
            )
            .await
        }
        Err(error) => Err(AppError(error)),
    };

    let (status_code, outcome) = match &result {
        Ok(response) => (i64::from(response.status().as_u16()), "success"),
        Err(AppError(error)) => (i64::from(error.http_status_code()), error.error_type()),
    };
    O::record_request_metric(
        &state.metrics,
        &OperationRequestMetric {
            requested_model: &requested_model,
            resolved_model: &resolved_model,
            provider_key: selected_provider.as_deref().unwrap_or("unavailable"),
            status_code,
            outcome,
            latency_seconds: latency_seconds_since(request_started_at),
        },
    );
    result
}

struct ResolvedOperation<O> {
    request_id: String,
    request_started_at: Instant,
    /// The team the request's rate-limit check counted.
    rate_limit_team: RateLimitMemberTeam,
    request: O,
    resolved: ResolvedGatewayRequest,
}

/// Routes and executes a resolved request. `selected_provider` is set once a
/// route is chosen, so the caller can label the request metric on any exit.
/// One linear workflow: each step can end the request, and every step after
/// route selection leaves a request log.
async fn dispatch_json_operation<O: JsonOperation>(
    state: &AppState,
    headers: &HeaderMap,
    auth: &AuthenticatedApiKey,
    request: ResolvedOperation<O>,
    selected_provider: &mut Option<String>,
) -> Result<Response, AppError> {
    let ResolvedOperation {
        request_id,
        request_started_at,
        rate_limit_team,
        mut request,
        resolved,
    } = request;
    let request_span = Span::current();
    let requirements = request.route_requirements();
    record_request_span_fields(&request_span, auth, &resolved, false, O::ROUTE_PATH);
    let request_headers = extract_request_headers(headers);
    let request_tags = extract_request_tags(headers)?;
    let mut request_log_context = request.begin_request_log(
        &state.service,
        &request_id,
        &resolved,
        &request_headers,
        request_tags,
    );
    let (eligible_route_count, selected) = select_provider_route(
        state,
        &resolved,
        requirements,
        headers,
        request.extra(),
        &request_headers,
        O::ENDPOINT,
    )
    .await?;

    tracing::info!(
        request_model = %request.model(),
        resolved_model = %resolved.selection.execution_model.model_key,
        route_count = resolved.routes.len(),
        eligible_route_count,
        required_capabilities = ?requirements.required_capability_names(),
        "{} request resolved",
        O::OPERATION
    );

    let Some(SelectedProviderRoute {
        route,
        provider,
        receipt: _,
        expected_provider_credential_id,
    }) = selected
    else {
        return Err(AppError(no_compatible_route_error(requirements)));
    };
    *selected_provider = Some(route.provider_key.clone());
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
        state,
        auth,
        &mut request_log_context,
        &route,
        resolved.provider_connections.get(&route.provider_key),
    )
    .await;
    // Token, cost and usage-record totals stay on the shared chat usage metrics.
    let labels = ChatMetricLabels {
        requested_model: &resolved.selection.requested_model.model_key,
        resolved_model: &resolved.selection.execution_model.model_key,
        provider_key: &route.provider_key,
        stream: false,
    };
    let usage_context = || UsageAccountingContext {
        auth,
        rate_limit_team,
        model: &resolved.selection.execution_model,
        route: &route,
        request_id: &request_id,
        labels: labels.clone(),
        operation: O::OPERATION,
    };
    let route_key = model_route_key(
        &resolved.selection.execution_model.model_key,
        &route.provider_key,
        &route.upstream_model,
    );
    let guard_context = match guard_typed_request(state, &request_id, route_key, &mut request).await
    {
        Ok(context) => context,
        Err(AppError(error)) => {
            log_pre_provider_failure(
                state,
                auth,
                &request_log_context,
                &route.provider_key,
                icon_metadata,
                request_started_at,
                &error,
            )
            .await;
            return Err(AppError(error));
        }
    };

    if let Err(error) = state
        .service
        .enforce_pre_provider_budget(
            auth,
            &request_id,
            Some(resolved.selection.execution_model.id),
            Some(route.upstream_model.as_str()),
            OffsetDateTime::now_utc(),
        )
        .await
    {
        log_pre_provider_failure(
            state,
            auth,
            &request_log_context,
            &route.provider_key,
            icon_metadata,
            request_started_at,
            &error,
        )
        .await;
        return Err(AppError(error));
    }

    let mut context = build_provider_context(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &route,
        auth,
        request_headers,
    );
    context.expected_provider_credential_id = expected_provider_credential_id;

    let attempt_started_at = gateway_service::offset_now();
    let provider_execution_span = provider_operation_span(
        &request_id,
        O::OPERATION,
        auth,
        &resolved,
        &route,
        provider.as_ref(),
        false,
    );
    let value = match trace_provider_operation(
        provider_execution_span,
        request.call_provider(provider.as_ref(), &context),
    )
    .await
    {
        Ok(value) => O::caller_response(value, &resolved.selection.requested_model.model_key),
        Err(error) => {
            let (provider_error, partial_provider_usage) = split_partial_provider_error(error);
            if let Some(provider_usage) = partial_provider_usage {
                finalize_successful_usage_accounting(state, usage_context(), provider_usage).await;
            }

            let (error, attempt) = guarded_provider_error_attempt(
                state,
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
                auth,
                &request_log_context,
                &route.provider_key,
                icon_metadata,
                latency_ms_since(request_started_at),
                &error,
                vec![attempt],
            )
            .await;
            return Err(AppError(error));
        }
    };
    let attempt = success_attempt(&request_log_context, &route, false, attempt_started_at);

    finalize_successful_usage_accounting(state, usage_context(), usage_value_from_response(&value))
        .await;
    best_effort_log_non_stream_success(
        &state.service,
        auth,
        &request_log_context,
        &route.provider_key,
        icon_metadata,
        latency_ms_since(request_started_at),
        0,
        &value,
        vec![attempt],
    )
    .await;

    Ok(Json(value).into_response())
}

/// A guardrail block or hard-budget rejection: a summary row with no attempts.
/// The request metric is recorded by [`execute_json_operation`], so this skips
/// the chat request and tool-cardinality samples the shared chat helper would
/// add.
async fn log_pre_provider_failure(
    state: &AppState,
    auth: &AuthenticatedApiKey,
    request_log_context: &RequestLogContext,
    provider_key: &str,
    icon_metadata: RequestLogIconMetadata,
    request_started_at: Instant,
    error: &GatewayError,
) {
    best_effort_log_non_stream_failure(
        &state.service,
        auth,
        request_log_context,
        provider_key,
        icon_metadata,
        latency_ms_since(request_started_at),
        error,
        Vec::new(),
    )
    .await;
}
