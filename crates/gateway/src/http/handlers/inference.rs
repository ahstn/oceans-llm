//! Shared request finalization and accounting for online inference endpoints.
use super::{
    execution::{AttemptContext, completed_attempts},
    *,
};

#[derive(Clone)]
pub(super) struct InferenceRequest<'a> {
    pub state: &'a AppState,
    pub auth: &'a AuthenticatedApiKey,
    pub resolved: &'a gateway_service::ResolvedGatewayRequest,
    pub request_id: &'a str,
    pub request_started_at: Instant,
    pub stream: bool,
    pub request_log_context: RequestLogContext,
}

pub(super) struct InferenceExecution<'a> {
    pub request: InferenceRequest<'a>,
    pub route: gateway_core::ModelRoute,
    pub provider: Arc<dyn ProviderClient>,
    pub expected_provider_credential_id: Option<uuid::Uuid>,
    pub guard_context: Option<InferenceGuardContext>,
    pub routing_receipt: Option<RoutingReceipt>,
    icon_metadata: RequestLogIconMetadata,
}

impl InferenceRequest<'_> {
    pub async fn unavailable(&self, error: GatewayError) -> AppError {
        let icon_metadata = RequestLogIconMetadata {
            provider_icon_key: resolve_provider_display_from_parts("unavailable", None, None)
                .icon_key,
            model_icon_key: resolve_model_icon_key([
                self.resolved.selection.execution_model.model_key.as_str(),
                self.resolved.selection.requested_model.model_key.as_str(),
            ]),
        };
        self.record_failure("unavailable", icon_metadata, &error, Vec::new())
            .await;
        AppError(error)
    }

    async fn record_failure(
        &self,
        provider_key: &str,
        icon_metadata: RequestLogIconMetadata,
        error: &GatewayError,
        attempts: Vec<RequestAttemptRecord>,
    ) {
        let request = self;
        let labels = ChatMetricLabels {
            requested_model: &self.resolved.selection.requested_model.model_key,
            resolved_model: &self.resolved.selection.execution_model.model_key,
            provider_key,
            stream: self.stream,
        };
        if request.stream {
            best_effort_log_stream_result(
                &request.state.service,
                request.auth,
                &request.request_log_context,
                gateway_service::StreamLogResultInput {
                    provider_key: provider_key.to_string(),
                    icon_metadata: icon_metadata.clone(),
                    latency_ms: latency_ms_since(request.request_started_at),
                    collector: request.state.service.new_stream_response_collector(),
                    failure: Some(gateway_service::StreamFailureSummary {
                        status_code: error.http_status_code().into(),
                        error_code: error.error_code().to_string(),
                    }),
                    attempts,
                },
            )
            .await;
        } else {
            best_effort_log_non_stream_failure(
                &request.state.service,
                request.auth,
                &request.request_log_context,
                provider_key,
                icon_metadata.clone(),
                latency_ms_since(request.request_started_at),
                error,
                attempts,
            )
            .await;
        }
        request
            .state
            .metrics
            .record_chat_request(&ChatRequestMetric {
                labels: labels.clone(),
                status_code: i64::from(error.http_status_code()),
                outcome: error.error_type(),
                latency_seconds: latency_seconds_since(request.request_started_at),
            });
        request.state.metrics.record_tool_cardinality(
            &labels.clone(),
            request.request_log_context.operation,
            &request.request_log_context.tool_cardinality,
        );
    }
}

impl<'a> InferenceExecution<'a> {
    pub fn new(request: InferenceRequest<'a>, selected: SelectedProviderRoute) -> Self {
        let SelectedProviderRoute {
            route,
            provider,
            receipt,
            expected_provider_credential_id,
        } = selected;
        let icon_metadata = request_log_icon_metadata(
            &route,
            request
                .resolved
                .provider_connections
                .get(&route.provider_key),
            &request.resolved.selection.execution_model.model_key,
            &request.resolved.selection.requested_model.model_key,
        );
        record_provider_execution_span_fields(
            &Span::current(),
            &route.provider_key,
            provider.provider_type(),
        );
        Self {
            request,
            route,
            provider,
            expected_provider_credential_id,
            guard_context: None,
            routing_receipt: receipt,
            icon_metadata,
        }
    }

    fn labels(&self) -> ChatMetricLabels<'_> {
        ChatMetricLabels {
            requested_model: &self.request.resolved.selection.requested_model.model_key,
            resolved_model: &self.request.resolved.selection.execution_model.model_key,
            provider_key: &self.route.provider_key,
            stream: self.request.stream,
        }
    }

    pub fn provider_context(
        &self,
        request_headers: BTreeMap<String, String>,
    ) -> ProviderRequestContext {
        let mut context = build_provider_context(
            self.request.request_id,
            &self.request.resolved.selection.requested_model.model_key,
            &self.route,
            self.request.auth,
            request_headers,
        );
        context.expected_provider_credential_id = self.expected_provider_credential_id;
        context
    }

    pub async fn check_budget(&self) -> Result<(), GatewayError> {
        self.request
            .state
            .service
            .enforce_pre_provider_budget(
                self.request.auth,
                self.request.request_id,
                Some(self.request.resolved.selection.execution_model.id),
                Some(self.route.upstream_model.as_str()),
                OffsetDateTime::now_utc(),
            )
            .await
    }

    pub async fn account_usage(&self, usage: Option<Value>) {
        finalize_successful_usage_accounting(
            self.request.state,
            UsageAccountingContext {
                auth: self.request.auth,
                model: &self.request.resolved.selection.execution_model,
                route: &self.route,
                request_id: self.request.request_id,
                labels: self.labels(),
                operation: self.request.request_log_context.operation,
            },
            usage,
        )
        .await;
    }

    pub async fn record_failure(&self, error: &GatewayError, attempts: Vec<RequestAttemptRecord>) {
        self.request
            .record_failure(
                &self.route.provider_key,
                self.icon_metadata.clone(),
                error,
                attempts,
            )
            .await;
    }

    pub async fn stream_response(
        &self,
        stream: ProviderStream,
        attempt: AttemptContext,
        stream_trace: StreamTrace,
        endpoint: RoutingEndpoint,
    ) -> Result<Response, AppError> {
        let request = &self.request;
        let stream = match &self.guard_context {
            Some(guard_context) => {
                enforce_guarded_stream_after_provider(
                    request.state,
                    request.auth,
                    request.resolved,
                    &request.request_log_context,
                    &self.route,
                    self.icon_metadata.clone(),
                    request.request_started_at,
                    &attempt,
                    guard_context,
                    stream,
                )
                .await?
            }
            None => stream,
        };
        let stream = if endpoint == RoutingEndpoint::Messages {
            anthropic_messages_stream_from_openai(
                stream,
                request.resolved.selection.requested_model.model_key.clone(),
            )
        } else {
            stream
        };
        let body_stream = wrap_stream_with_request_logging(LoggingBodyStreamState {
            upstream: stream,
            service: request.state.service.clone(),
            metrics: request.state.metrics.clone(),
            auth: request.auth.clone(),
            request_log_context: request.request_log_context.clone(),
            requested_model_key: request.resolved.selection.requested_model.model_key.clone(),
            resolved_model_key: request.resolved.selection.execution_model.model_key.clone(),
            execution_model: request.resolved.selection.execution_model.clone(),
            route: self.route.clone(),
            routing_receipt: self.routing_receipt.clone(),
            provider_key: self.route.provider_key.clone(),
            icon_metadata: self.icon_metadata.clone(),
            started_at: request.request_started_at,
            attempt_started_at: attempt.started_at,
            attempt_number: attempt.number,
            prior_attempts: attempt.prior_attempts,
            finished: false,
            saw_terminal_event: false,
            collector: request.state.service.new_stream_response_collector(),
            stream_trace,
        });
        Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/event-stream; charset=utf-8")
            .header(CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(body_stream))
            .map_err(|error| {
                AppError(GatewayError::Internal(format!(
                    "failed to build streaming response: {error}"
                )))
            })
    }

    pub async fn complete_response(
        &self,
        value: Value,
        attempt: AttemptContext,
        endpoint: RoutingEndpoint,
    ) -> Result<Response, AppError> {
        let request = &self.request;
        let mut value =
            normalize_response_model(value, &request.resolved.selection.requested_model.model_key);
        self.account_usage(usage_value_from_response(&value)).await;
        let completion = async {
            // Embeddings keep their existing prompt-only guard behavior.
            if matches!(
                endpoint,
                RoutingEndpoint::ChatCompletions
                    | RoutingEndpoint::Responses
                    | RoutingEndpoint::Messages
            ) && let Some(guard_context) = &self.guard_context
            {
                guard_model_response(request.state, guard_context, &mut value).await?;
            }
            if let Some(receipt) = &self.routing_receipt {
                let response_id = (endpoint == RoutingEndpoint::Responses)
                    .then(|| value.get("id").and_then(Value::as_str))
                    .flatten();
                receipt
                    .complete(request.state.store.as_ref(), response_id)
                    .await?;
            }
            Ok::<(), GatewayError>(())
        }
        .await;
        if let Err(error) = completion {
            self.record_failure(
                &error,
                completed_attempts(
                    &attempt.prior_attempts,
                    guarded_failure_attempt(
                        &request.request_log_context,
                        &self.route,
                        attempt.number,
                        attempt.started_at,
                    ),
                ),
            )
            .await;
            return Err(AppError(error));
        }
        let attempts = completed_attempts(
            &attempt.prior_attempts,
            success_attempt(
                &request.request_log_context,
                &self.route,
                false,
                attempt.number,
                attempt.started_at,
            ),
        );
        let tool_cardinality = tool_cardinality_with_invoked(&request.request_log_context, &value);
        best_effort_log_non_stream_success(
            &request.state.service,
            request.auth,
            &request.request_log_context,
            &self.route.provider_key,
            self.icon_metadata.clone(),
            latency_ms_since(request.request_started_at),
            tool_cardinality.invoked_tool_count.unwrap_or(0),
            &value,
            attempts,
        )
        .await;
        request
            .state
            .metrics
            .record_chat_request(&ChatRequestMetric {
                labels: self.labels(),
                status_code: 200,
                outcome: "success",
                latency_seconds: latency_seconds_since(request.request_started_at),
            });
        request.state.metrics.record_tool_cardinality(
            &self.labels(),
            request.request_log_context.operation,
            &tool_cardinality,
        );
        let value = if endpoint == RoutingEndpoint::Messages {
            anthropic_message_from_openai_chat(
                &value,
                &request.resolved.selection.requested_model.model_key,
            )
        } else {
            value
        };
        let mut response = Json(value).into_response();
        if let Ok(request_id_header) = HeaderValue::from_str(request.request_id) {
            response
                .headers_mut()
                .insert("x-request-id", request_id_header);
        }
        Ok(response)
    }
}
