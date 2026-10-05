//! Provider dispatch, bounded retries, and route failover for online inference.
use std::collections::BTreeSet;

use gateway_core::{CoreDecisionsRequest, CoreResponsesRequest, ProviderFailureKind};

use super::{
    inference::{InferenceExecution, InferenceRequest},
    *,
};

/// Keep the original input so each destination applies its own prompt policy.
#[derive(Clone)]
pub(super) enum InferenceInput {
    Chat(CoreChatRequest),
    Responses(CoreResponsesRequest),
    Embeddings(EmbeddingsRequest),
    Decisions(CoreDecisionsRequest),
}

impl InferenceInput {
    fn operation(&self) -> &'static str {
        match self {
            Self::Chat(_) => "chat",
            Self::Responses(_) => "responses",
            Self::Embeddings(_) => "embeddings",
            Self::Decisions(_) => "decisions",
        }
    }

    async fn guard(
        &mut self,
        state: &AppState,
        request_id: &str,
        route_key: String,
    ) -> Result<Option<InferenceGuardContext>, AppError> {
        let context = match self {
            Self::Chat(request) => {
                guard_typed_request(state, request_id, route_key, request).await?
            }
            Self::Responses(request) => {
                guard_typed_request(state, request_id, route_key, request).await?
            }
            Self::Embeddings(request) => {
                guard_typed_request(state, request_id, route_key, request).await?
            }
            // Decisions have no chat-shaped text for these evaluators.
            Self::Decisions(_) => return Ok(None),
        };
        Ok(Some(context))
    }

    async fn invoke(
        &self,
        provider: &dyn ProviderClient,
        context: &ProviderRequestContext,
        stream: bool,
    ) -> Result<ProviderOutput, ProviderError> {
        match self {
            Self::Chat(request) if stream => provider
                .chat_completions_stream(request, context)
                .await
                .map(ProviderOutput::Stream),
            Self::Responses(request) if stream => provider
                .responses_stream(request, context)
                .await
                .map(ProviderOutput::Stream),
            Self::Chat(request) => provider
                .chat_completions(request, context)
                .await
                .map(ProviderOutput::Value),
            Self::Responses(request) => provider
                .responses(request, context)
                .await
                .map(ProviderOutput::Value),
            Self::Embeddings(request) => provider
                .embeddings(&openai_embeddings_request_to_core(request), context)
                .await
                .map(ProviderOutput::Value),
            Self::Decisions(request) => provider
                .decisions(request, context)
                .await
                .map(ProviderOutput::Value),
        }
    }
}

pub(super) struct RouteRequest<'a> {
    pub requirements: CoreRequestRequirements,
    pub headers: &'a HeaderMap,
    pub extra: &'a BTreeMap<String, Value>,
    pub request_headers: BTreeMap<String, String>,
    pub endpoint: RoutingEndpoint,
}

impl RouteRequest<'_> {
    async fn select(
        &self,
        request: &InferenceRequest<'_>,
        excluded: &BTreeSet<uuid::Uuid>,
    ) -> Result<Option<SelectedProviderRoute>, GatewayError> {
        if excluded.is_empty() {
            let (_, selected) = routing::select_provider_route(
                request.state,
                request.resolved,
                self.requirements,
                self.headers,
                self.extra,
                &self.request_headers,
                self.endpoint,
            )
            .await?;
            return Ok(selected);
        }
        let (_, selected) = routing::select_provider_route_excluding(
            request.state,
            request.resolved,
            self.requirements,
            self.headers,
            self.extra,
            &self.request_headers,
            self.endpoint,
            excluded,
        )
        .await?;
        Ok(selected)
    }

    fn strict_continuation(&self) -> bool {
        self.endpoint == RoutingEndpoint::Responses
            && self
                .extra
                .get("previous_response_id")
                .is_some_and(|value| !value.is_null())
    }
}

pub(super) enum ProviderOutput {
    Value(Value),
    Stream(ProviderStream),
}

pub(super) struct AttemptContext {
    pub number: i64,
    pub started_at: OffsetDateTime,
    pub prior_attempts: Vec<RequestAttemptRecord>,
}

pub(super) struct ExecutedInference<'a> {
    execution: InferenceExecution<'a>,
    output: ProviderOutput,
    attempt: AttemptContext,
    stream_trace: Option<StreamTrace>,
    endpoint: RoutingEndpoint,
}

impl ExecutedInference<'_> {
    pub async fn into_response(self) -> Result<Response, AppError> {
        match self.output {
            ProviderOutput::Value(value) => {
                self.execution
                    .complete_response(value, self.attempt, self.endpoint)
                    .await
            }
            ProviderOutput::Stream(stream) => {
                self.execution
                    .stream_response(
                        stream,
                        self.attempt,
                        self.stream_trace.expect("stream dispatch creates a trace"),
                        self.endpoint,
                    )
                    .await
            }
        }
    }
}

struct FailedAttempt {
    error: GatewayError,
    kind: ProviderFailureKind,
    retry_after: Option<std::time::Duration>,
    record: RequestAttemptRecord,
}

/// The loop ends as soon as a provider returns a value or a stream. Response
/// guards, usage accounting, and stream polling are never inside the retry loop.
pub(super) async fn execute<'a>(
    mut request: InferenceRequest<'a>,
    original: InferenceInput,
    routing: RouteRequest<'_>,
) -> Result<ExecutedInference<'a>, AppError> {
    let policy = request
        .resolved
        .selection
        .execution_model
        .routing
        .as_ref()
        .and_then(|routing| routing.failover.as_ref());
    let mut excluded = BTreeSet::new();
    let selected = match routing.select(&request, &excluded).await {
        Ok(Some(selected)) => selected,
        Ok(None) => {
            return Err(request
                .unavailable(no_compatible_route_error(routing.requirements))
                .await);
        }
        Err(error) => return Err(request.unavailable(error).await),
    };
    best_effort_record_mcp_request_telemetry(
        request.state,
        request.auth,
        &mut request.request_log_context,
        &selected.route,
        request
            .resolved
            .provider_connections
            .get(&selected.route.provider_key),
    )
    .await;
    let mut attempts = Vec::new();
    let (mut execution, mut prepared) =
        prepare_route(&request, selected, &original, &attempts).await?;
    let mut retries = 0;
    loop {
        if let Err(error) = execution.check_budget().await {
            execution.record_failure(&error, attempts).await;
            return Err(AppError(error));
        }
        let attempt = AttemptContext {
            number: attempts.len() as i64 + 1,
            started_at: gateway_service::offset_now(),
            prior_attempts: Vec::new(),
        };
        let mut stream_trace = request.stream.then(|| {
            StreamTrace::new(
                original.operation(),
                request.request_id,
                &execution.route,
                execution.provider.as_ref(),
                Instant::now(),
            )
        });
        let context = execution.provider_context(routing.request_headers.clone());
        let span = provider_operation_span(
            request.request_id,
            original.operation(),
            request.auth,
            request.resolved,
            &execution.route,
            execution.provider.as_ref(),
            request.stream,
        );
        match trace_provider_operation(
            span,
            prepared.invoke(execution.provider.as_ref(), &context, request.stream),
        )
        .await
        {
            Ok(output) => {
                return Ok(ExecutedInference {
                    execution,
                    output,
                    attempt: AttemptContext {
                        prior_attempts: attempts,
                        ..attempt
                    },
                    stream_trace,
                    endpoint: routing.endpoint,
                });
            }
            Err(error) => {
                if let Some(trace) = stream_trace.as_mut() {
                    trace.finish("stream_start_error", Some("stream_start_error"));
                }
                let failure =
                    failed_attempt(&execution, error, routing.requirements, &attempt).await;
                append_attempt(&mut attempts, failure.record);
                let Some(policy) = policy.filter(|_| failure.kind != ProviderFailureKind::Terminal)
                else {
                    execution.record_failure(&failure.error, attempts).await;
                    return Err(AppError(failure.error));
                };
                let exhausted = attempts.len() >= policy.max_attempts as usize;
                if !exhausted
                    && failure.kind == ProviderFailureKind::Transient
                    && let Some(delay) = policy.retry_delay(retries, failure.retry_after)
                {
                    retries += 1;
                    tokio::time::sleep(delay).await;
                    continue;
                }
                if let Some(receipt) = &execution.routing_receipt
                    && let Err(error) = receipt
                        .record_failure(
                            request.state.store.as_ref(),
                            policy.cooldown(failure.kind, failure.retry_after),
                        )
                        .await
                {
                    execution.record_failure(&error, attempts).await;
                    return Err(AppError(error));
                }
                if exhausted || routing.strict_continuation() {
                    execution.record_failure(&failure.error, attempts).await;
                    return Err(AppError(failure.error));
                }
                excluded.insert(execution.route.id);
                let selected = match routing.select(&request, &excluded).await {
                    Ok(Some(selected)) => selected,
                    Ok(None) => {
                        execution.record_failure(&failure.error, attempts).await;
                        return Err(AppError(failure.error));
                    }
                    Err(GatewayError::Route(gateway_core::RouteError::TemporarilyUnavailable(
                        _,
                    ))) => {
                        execution.record_failure(&failure.error, attempts).await;
                        return Err(AppError(failure.error));
                    }
                    Err(error) => {
                        execution.record_failure(&error, attempts).await;
                        return Err(AppError(error));
                    }
                };
                (execution, prepared) =
                    prepare_route(&request, selected, &original, &attempts).await?;
                retries = 0;
            }
        }
    }
}

async fn prepare_route<'a>(
    request: &InferenceRequest<'a>,
    selected: SelectedProviderRoute,
    original: &InferenceInput,
    attempts: &[RequestAttemptRecord],
) -> Result<(InferenceExecution<'a>, InferenceInput), AppError> {
    let mut execution = InferenceExecution::new(request.clone(), selected);
    let mut prepared = original.clone();
    let route_key = model_route_key(
        &request.resolved.selection.execution_model.model_key,
        &execution.route.provider_key,
        &execution.route.upstream_model,
    );
    execution.guard_context = match prepared
        .guard(request.state, request.request_id, route_key)
        .await
    {
        Ok(context) => context,
        Err(error) => {
            execution.record_failure(&error.0, attempts.to_vec()).await;
            return Err(error);
        }
    };
    Ok((execution, prepared))
}

async fn failed_attempt(
    execution: &InferenceExecution<'_>,
    error: ProviderError,
    requirements: CoreRequestRequirements,
    attempt: &AttemptContext,
) -> FailedAttempt {
    let mut kind = error.failure_kind(execution.provider.provider_type());
    let retry_after = error.retry_after();
    let (error, partial_usage) = split_partial_provider_error(error);
    if let Some(usage) = partial_usage {
        execution.account_usage(usage).await;
    }
    let error = match guard_provider_error(
        execution.request.state,
        execution.guard_context.as_ref(),
        error,
    )
    .await
    {
        Ok(error) => map_operation_provider_error(error, requirements),
        Err(error) => {
            kind = ProviderFailureKind::Terminal;
            error
        }
    };
    let record = gateway_service::build_request_attempt(
        &execution.request.request_log_context,
        &execution.route,
        attempt.number,
        execution.request.stream,
        attempt.started_at,
        gateway_service::offset_now(),
        gateway_service::failed_attempt_outcome(
            if execution.request.stream {
                RequestAttemptStatus::StreamStartError
            } else {
                RequestAttemptStatus::ProviderError
            },
            &error,
            kind != ProviderFailureKind::Terminal,
            error.to_string(),
        ),
    );
    FailedAttempt {
        error,
        kind,
        retry_after,
        record,
    }
}

/// Terminal marks the last provider execution, including when later preparation
/// fails before another provider is called.
pub(super) fn append_attempt(
    attempts: &mut Vec<RequestAttemptRecord>,
    attempt: RequestAttemptRecord,
) {
    if let Some(previous) = attempts.last_mut() {
        previous.terminal = false;
    }
    attempts.push(attempt);
}

pub(super) fn completed_attempts(
    prior: &[RequestAttemptRecord],
    final_attempt: RequestAttemptRecord,
) -> Vec<RequestAttemptRecord> {
    let mut attempts = prior.to_vec();
    append_attempt(&mut attempts, final_attempt);
    attempts
}
