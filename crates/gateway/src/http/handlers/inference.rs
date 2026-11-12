//! Shared request preparation, completion and accounting for inference endpoints.
use super::service_tier::effective_service_tier;
use super::*;
use crate::http::anthropic_stream::insert_anthropic_service_tier;

pub(super) enum InferenceResponseFormat {
    OpenAi,
    NativeMessages(std::collections::BTreeMap<String, String>),
    TranslatedMessages,
}

pub(super) struct InferenceRequest<'a> {
    pub state: &'a AppState,
    pub auth: &'a AuthenticatedApiKey,
    /// The team the request's rate-limit check counted.
    pub rate_limit_team: RateLimitMemberTeam,
    pub resolved: &'a gateway_service::ResolvedGatewayRequest,
    pub request_id: &'a str,
    pub request_started_at: Instant,
    pub stream: bool,
    pub endpoint: RoutingEndpoint,
    /// The tier the caller asked for, already checked against the model's allow-list.
    pub requested_service_tier: Option<gateway_core::ServiceTier>,
    pub request_log_context: RequestLogContext,
}

pub(super) struct InferenceExecution<'a> {
    pub request: InferenceRequest<'a>,
    pub route: gateway_core::ModelRoute,
    pub provider: Arc<dyn ProviderClient>,
    pub expected_provider_credential_id: Option<uuid::Uuid>,
    pub guard_context: InferenceGuardContext,
    routing_receipt: Option<RoutingReceipt>,
    icon_metadata: RequestLogIconMetadata,
}

impl<'a> InferenceRequest<'a> {
    fn accounting_operation(&self) -> &'static str {
        match self.endpoint {
            RoutingEndpoint::Messages => "anthropic_messages",
            _ => self.request_log_context.operation,
        }
    }

    pub async fn prepare<T: Serialize + DeserializeOwned>(
        mut self,
        selected: Result<SelectedProviderRoute, GatewayError>,
        request: &mut T,
    ) -> Result<InferenceExecution<'a>, AppError> {
        let SelectedProviderRoute {
            route,
            provider,
            receipt,
            expected_provider_credential_id,
        } = match selected {
            Ok(selection) => selection,
            Err(error) => {
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
                if !matches!(self.endpoint, RoutingEndpoint::Messages) {
                    self.state.metrics.record_tool_cardinality(
                        &labels,
                        self.request_log_context.operation,
                        &self.request_log_context.tool_cardinality,
                    );
                }
                return Err(AppError(error));
            }
        };
        self.request_log_context.service_tier =
            effective_service_tier(&route, self.requested_service_tier);
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

        Ok(InferenceExecution {
            request: self,
            route,
            provider,
            expected_provider_credential_id,
            guard_context,
            routing_receipt: receipt,
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

    /// The provider context for the chosen route, with its credential generation and
    /// the request's service tier.
    pub fn provider_context(
        &self,
        request_headers: BTreeMap<String, String>,
    ) -> ProviderRequestContext {
        let request = &self.request;
        let mut context = build_provider_context(
            request.request_id,
            &request.resolved.selection.requested_model.model_key,
            &self.route,
            request.auth,
            request_headers,
        );
        context.expected_provider_credential_id = self.expected_provider_credential_id;
        context.service_tier = request.request_log_context.service_tier;
        context
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
            rate_limit_team,
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
        // Usage the provider reported before failing still counts, as on the
        // decisions and embeddings paths.
        let (error, partial_provider_usage) = split_partial_provider_error(error);
        if let Some(provider_usage) = partial_provider_usage {
            finalize_successful_usage_accounting(
                state,
                UsageAccountingContext {
                    auth,
                    rate_limit_team,
                    model: &resolved.selection.execution_model,
                    route,
                    request_id,
                    labels: labels.clone(),
                    operation: self.request.accounting_operation(),
                    service_tiers: service_tier_outcome(request_log_context, None),
                    usage_estimate: gateway_service::UsageEstimateInput::default(),
                },
                provider_usage,
            )
            .await;
        }
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
        if !matches!(self.request.endpoint, RoutingEndpoint::Messages) {
            state.metrics.record_tool_cardinality(
                &labels,
                request_log_context.operation,
                &request_log_context.tool_cardinality,
            );
        }
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
        if matches!(self.request.endpoint, RoutingEndpoint::Messages) {
            return;
        }
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
        format: InferenceResponseFormat,
    ) -> Result<Response, AppError> {
        let &InferenceRequest {
            state,
            auth,
            rate_limit_team,
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
            rate_limit_team,
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
        let requires_message_stop = matches!(format, InferenceResponseFormat::NativeMessages(_));
        let (stream, upstream_headers) = match format {
            InferenceResponseFormat::OpenAi => (stream, None),
            InferenceResponseFormat::NativeMessages(headers) => (
                native_messages::stream_with_model(
                    stream,
                    resolved.selection.requested_model.model_key.clone(),
                ),
                Some(headers),
            ),
            InferenceResponseFormat::TranslatedMessages => (
                anthropic_messages_stream_from_openai(
                    stream,
                    resolved.selection.requested_model.model_key.clone(),
                ),
                None,
            ),
        };
        let body_stream = wrap_stream_with_request_logging(LoggingBodyStreamState {
            upstream: stream,
            service: state.service.clone(),
            metrics: state.metrics.clone(),
            auth: auth.clone(),
            rate_limit_team,
            request_log_context: request_log_context.clone(),
            requested_model_key: resolved.selection.requested_model.model_key.clone(),
            resolved_model_key: resolved.selection.execution_model.model_key.clone(),
            execution_model: resolved.selection.execution_model.clone(),
            route: route.clone(),
            routing_receipt: self.routing_receipt.clone(),
            provider_key: route.provider_key.clone(),
            icon_metadata: icon_metadata.clone(),
            started_at: request_started_at,
            attempt_started_at,
            finished: false,
            saw_terminal_event: false,
            requires_message_stop,
            collector: state.service.new_stream_response_collector(),
            stream_trace,
        });

        let mut response = Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(CONTENT_TYPE, "text/event-stream; charset=utf-8")
            .header(CACHE_CONTROL, "no-cache")
            .body(Body::from_stream(body_stream))
            .map_err(|error| {
                AppError(GatewayError::Internal(format!("{error_context}: {error}")))
            })?;

        if let Some(headers) = upstream_headers {
            native_messages::apply_headers(&mut response, &headers);
        }
        if matches!(self.request.endpoint, RoutingEndpoint::Messages)
            && let Ok(request_id) = HeaderValue::from_str(self.request.request_id)
        {
            response.headers_mut().insert("x-request-id", request_id);
        }
        Ok(response)
    }

    pub async fn complete_response(
        &self,
        mut value: Value,
        attempt_started_at: OffsetDateTime,
        format: InferenceResponseFormat,
    ) -> Result<Response, AppError> {
        let &InferenceRequest {
            state,
            auth,
            rate_limit_team,
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
        // Read before the response guard and any Messages translation rewrite the body.
        let served_service_tier = gateway_core::served_service_tier(&value);
        finalize_successful_usage_accounting(
            state,
            UsageAccountingContext {
                auth,
                rate_limit_team,
                model: &resolved.selection.execution_model,
                route,
                request_id,
                labels: labels.clone(),
                operation: self.request.accounting_operation(),
                service_tiers: service_tier_outcome(request_log_context, served_service_tier),
                usage_estimate: request_log_context
                    .usage_estimate(Some(gateway_service::response_text_bytes(&value))),
            },
            usage_value_from_response(&value),
        )
        .await;
        let completion = async {
            guard_model_response(state, guard_context, &mut value).await?;
            if let Some(receipt) = &self.routing_receipt {
                let response_id = match self.request.endpoint {
                    RoutingEndpoint::Messages => None,
                    _ => value.get("id").and_then(Value::as_str),
                };
                receipt.complete(state.store.as_ref(), response_id).await?;
            }
            Ok::<(), GatewayError>(())
        }
        .await;
        if let Err(error) = completion {
            record_post_provider_failure(
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
        if !matches!(self.request.endpoint, RoutingEndpoint::Messages) {
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
        }
        let (value, upstream_headers) = match format {
            InferenceResponseFormat::OpenAi => (value, None),
            InferenceResponseFormat::NativeMessages(headers) => (value, Some(headers)),
            InferenceResponseFormat::TranslatedMessages => {
                let mut message = anthropic_message_from_openai_chat(
                    &value,
                    &resolved.selection.requested_model.model_key,
                );
                if let Some(usage) = message.get_mut("usage") {
                    insert_anthropic_service_tier(usage, served_service_tier);
                }
                (message, None)
            }
        };
        let mut response = Json(value).into_response();
        if let Some(headers) = upstream_headers {
            native_messages::apply_headers(&mut response, &headers);
        }
        if let Ok(request_id_header) = HeaderValue::from_str(request_id) {
            response
                .headers_mut()
                .insert("x-request-id", request_id_header);
        }
        Ok(response)
    }
}
