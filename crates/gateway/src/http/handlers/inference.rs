//! Shared request preparation and accounting for OpenAI inference endpoints.
use super::*;

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
    pub guard_context: InferenceGuardContext,
    icon_metadata: RequestLogIconMetadata,
}

impl<'a> InferenceRequest<'a> {
    pub async fn prepare<T: Serialize + DeserializeOwned>(
        mut self,
        selected: Option<SelectedProviderRoute>,
        requirements: CoreRequestRequirements,
        request: &mut T,
    ) -> Result<InferenceExecution<'a>, AppError> {
        let (route, provider) = match selected {
            Some(selection) => selection,
            None => {
                let error = no_compatible_route_error(requirements);
                let labels = ChatMetricLabels {
                    requested_model: &self.resolved.selection.requested_model.model_key,
                    resolved_model: &self.resolved.selection.execution_model.model_key,
                    provider_key: "unavailable",
                    stream: self.stream,
                };
                self.state.metrics.record_chat_request(&ChatRequestMetric {
                    labels: labels.clone(),
                    status_code: i64::from(error.http_status_code()),
                    outcome: error.error_type(),
                    latency_seconds: latency_seconds_since(self.request_started_at),
                });
                self.state.metrics.record_tool_cardinality(
                    &labels,
                    self.request_log_context.operation,
                    &self.request_log_context.tool_cardinality,
                );
                return Err(AppError(error));
            }
        };
        let icon_metadata = request_log_icon_metadata(
            &route,
            self.resolved.provider_connections.get(&route.provider_key),
            &self.resolved.selection.execution_model.model_key,
            &self.resolved.selection.requested_model.model_key,
        );
        best_effort_record_mcp_request_telemetry(
            self.state,
            self.auth,
            &mut self.request_log_context,
            &route,
            self.resolved.provider_connections.get(&route.provider_key),
        )
        .await;
        let labels = ChatMetricLabels {
            requested_model: &self.resolved.selection.requested_model.model_key,
            resolved_model: &self.resolved.selection.execution_model.model_key,
            provider_key: &route.provider_key,
            stream: self.stream,
        };
        record_provider_execution_span_fields(
            &Span::current(),
            &route.provider_key,
            provider.provider_type(),
        );

        let route_key = model_route_key(
            &self.resolved.selection.execution_model.model_key,
            &route.provider_key,
            &route.upstream_model,
        );
        let guard_context =
            match guard_typed_request(self.state, self.request_id, route_key, request).await {
                Ok(context) => context,
                Err(AppError(error)) => {
                    record_guarded_pre_provider_failure(
                        self.state,
                        self.auth,
                        &self.request_log_context,
                        &route,
                        icon_metadata,
                        self.request_started_at,
                        &labels,
                        &error,
                    )
                    .await;
                    return Err(AppError(error));
                }
            };

        if let Err(error) = self
            .state
            .service
            .enforce_pre_provider_budget(
                self.auth,
                self.request_id,
                Some(self.resolved.selection.execution_model.id),
                Some(route.upstream_model.as_str()),
                OffsetDateTime::now_utc(),
            )
            .await
        {
            self.state.metrics.record_chat_request(&ChatRequestMetric {
                labels: labels.clone(),
                status_code: i64::from(error.http_status_code()),
                outcome: error.error_type(),
                latency_seconds: latency_seconds_since(self.request_started_at),
            });
            return Err(AppError(error));
        }

        Ok(InferenceExecution {
            request: self,
            route,
            provider,
            guard_context,
            icon_metadata,
        })
    }
}

impl InferenceExecution<'_> {
    fn labels(&self) -> ChatMetricLabels<'_> {
        ChatMetricLabels {
            requested_model: &self.request.resolved.selection.requested_model.model_key,
            resolved_model: &self.request.resolved.selection.execution_model.model_key,
            provider_key: &self.route.provider_key,
            stream: self.request.stream,
        }
    }

    pub async fn non_stream_provider_failure(
        &self,
        error: ProviderError,
        requirements: CoreRequestRequirements,
        attempt_started_at: OffsetDateTime,
    ) -> AppError {
        let &InferenceRequest {
            state,
            auth,
            request_started_at,
            ref request_log_context,
            ..
        } = &self.request;
        let route = &self.route;
        let guard_context = &self.guard_context;
        let icon_metadata = &self.icon_metadata;
        let labels = self.labels();
        let (error, attempt) = guarded_provider_error_attempt(
            state,
            guard_context,
            request_log_context,
            route,
            RequestAttemptStatus::ProviderError,
            false,
            attempt_started_at,
            error,
            requirements,
        )
        .await;
        best_effort_log_non_stream_failure(
            &state.service,
            auth,
            request_log_context,
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
        AppError(error)
    }

    pub async fn record_stream_start_failure(
        &self,
        gateway_error: &GatewayError,
        attempt: RequestAttemptRecord,
        tool_metric_stream: bool,
    ) {
        let &InferenceRequest {
            state,
            auth,
            request_started_at,
            ref request_log_context,
            ..
        } = &self.request;
        let route = &self.route;
        let icon_metadata = &self.icon_metadata;
        let labels = self.labels();
        best_effort_log_stream_result(
            &state.service,
            auth,
            request_log_context,
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
            &ChatMetricLabels {
                stream: tool_metric_stream,
                ..labels
            },
            request_log_context.operation,
            &request_log_context.tool_cardinality,
        );
    }

    pub async fn stream_response(
        &self,
        stream: ProviderStream,
        attempt_started_at: OffsetDateTime,
        stream_trace: StreamTrace,
        error_context: &str,
    ) -> Result<Response, AppError> {
        let &InferenceRequest {
            state,
            auth,
            resolved,
            request_started_at,
            ref request_log_context,
            ..
        } = &self.request;
        let route = &self.route;
        let guard_context = &self.guard_context;
        let icon_metadata = &self.icon_metadata;
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
                AppError(GatewayError::Internal(format!("{error_context}: {error}")))
            })?;

        Ok(response)
    }

    pub async fn complete_response(
        &self,
        mut value: Value,
        attempt_started_at: OffsetDateTime,
    ) -> Result<Response, AppError> {
        let &InferenceRequest {
            state,
            auth,
            resolved,
            request_id,
            request_started_at,
            ref request_log_context,
            ..
        } = &self.request;
        let route = &self.route;
        let guard_context = &self.guard_context;
        let icon_metadata = &self.icon_metadata;
        let labels = self.labels();
        finalize_successful_usage_accounting(
            state,
            UsageAccountingContext {
                auth,
                model: &resolved.selection.execution_model,
                route,
                request_id,
                labels: labels.clone(),
                operation: request_log_context.operation,
            },
            usage_value_from_response(&value),
        )
        .await;
        if let Err(error) = guard_model_response(state, guard_context, &mut value).await {
            record_guarded_non_stream_failure(
                state,
                auth,
                request_log_context,
                route,
                icon_metadata.clone(),
                request_started_at,
                attempt_started_at,
                &labels,
                &error,
            )
            .await;
            return Err(AppError(error));
        }
        let attempt = success_attempt(request_log_context, route, false, attempt_started_at);
        let tool_cardinality = tool_cardinality_with_invoked(request_log_context, &value);
        best_effort_log_non_stream_success(
            &state.service,
            auth,
            request_log_context,
            &route.provider_key,
            icon_metadata.clone(),
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
        if let Ok(request_id_header) = HeaderValue::from_str(request_id) {
            response
                .headers_mut()
                .insert("x-request-id", request_id_header);
        }
        Ok(response)
    }
}
