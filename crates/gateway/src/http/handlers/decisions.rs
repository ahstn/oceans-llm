//! Decisions endpoints. `/v1/decisions` accepts the OpenAI and System One
//! formats, detected from the body; `/v1/systemone` accepts System One only.
//! Both translate into one core request, and answer in the caller's format
//! whatever format the selected upstream speaks.
use gateway_core::{
    CoreDecisionsRequest, DecisionsWireFormat, ModelRoute, OpenAiDecisionsRequest,
    SystemOneDecisionsRequest, core_decisions_request_to_openai,
    core_decisions_request_to_system_one, openai_decisions_request_to_core,
    system_one_decisions_request_to_core,
};
use serde::Deserialize;

use super::*;
use crate::observability::DecisionsRequestMetric;

pub async fn v1_decisions(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    policy: RequestPolicyContext,
    InferenceAuth(auth): InferenceAuth,
    Json(body): Json<Value>,
) -> Result<Response, AppError> {
    let format = detect_decisions_format(&body).map_err(AppError)?;
    execute_decisions(
        state,
        request_id,
        headers,
        policy,
        auth,
        DecisionsCall {
            body,
            format,
            route_path: "/v1/decisions",
        },
    )
    .await
}

pub async fn v1_systemone(
    State(state): State<AppState>,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    policy: RequestPolicyContext,
    InferenceAuth(auth): InferenceAuth,
    Json(body): Json<Value>,
) -> Result<Response, AppError> {
    if body.get("input").is_some() {
        return Err(AppError(GatewayError::InvalidRequest(
            "/v1/systemone accepts the System One format (`state`) only; send OpenAI-format \
             decisions requests (`input`) to /v1/decisions"
                .to_string(),
        )));
    }
    execute_decisions(
        state,
        request_id,
        headers,
        policy,
        auth,
        DecisionsCall {
            body,
            format: DecisionsWireFormat::SystemOne,
            route_path: "/v1/systemone",
        },
    )
    .await
}

struct DecisionsCall {
    /// The caller's original body; request logs record it verbatim.
    body: Value,
    format: DecisionsWireFormat,
    route_path: &'static str,
}

/// `input` marks the OpenAI format and `state` marks System One. A body with
/// both or neither is ambiguous and rejected.
fn detect_decisions_format(body: &Value) -> Result<DecisionsWireFormat, GatewayError> {
    let Some(object) = body.as_object() else {
        return Err(GatewayError::InvalidRequest(
            "decisions request must be a JSON object".to_string(),
        ));
    };
    match (object.contains_key("input"), object.contains_key("state")) {
        (true, false) => Ok(DecisionsWireFormat::OpenAi),
        (false, true) => Ok(DecisionsWireFormat::SystemOne),
        (true, true) => Err(GatewayError::InvalidRequest(
            "decisions request cannot contain both `input` (OpenAI format) and `state` \
             (System One format)"
                .to_string(),
        )),
        (false, false) => Err(GatewayError::InvalidRequest(
            "decisions request must contain `input` (OpenAI format) or `state` (System One format)"
                .to_string(),
        )),
    }
}

fn parse_decisions_request(
    body: &Value,
    format: DecisionsWireFormat,
) -> Result<CoreDecisionsRequest, GatewayError> {
    let invalid = |error: serde_json::Error| {
        GatewayError::InvalidRequest(format!(
            "invalid {} decisions request: {error}",
            format.as_str()
        ))
    };
    let request = match format {
        DecisionsWireFormat::SystemOne => SystemOneDecisionsRequest::deserialize(body)
            .map(|request| system_one_decisions_request_to_core(&request))
            .map_err(invalid)?,
        DecisionsWireFormat::OpenAi => OpenAiDecisionsRequest::deserialize(body)
            .map_err(invalid)
            .and_then(|request| {
            openai_decisions_request_to_core(&request).map_err(GatewayError::InvalidRequest)
        })?,
    };
    if let Some(message) = request.validation_error() {
        return Err(GatewayError::InvalidRequest(message));
    }
    Ok(request)
}

/// Drops routes whose upstream format cannot carry this request (images or
/// `safety_identifier` for System One; single-option choices or too many
/// questions for OpenAI), so weighted selection only picks routes that can
/// serve it. When no route can, the first translation error explains why.
fn retain_representable_routes(
    routes: &mut Vec<ModelRoute>,
    request: &CoreDecisionsRequest,
) -> Result<(), GatewayError> {
    // Translate only for formats some candidate route speaks.
    let uses = |format| {
        routes
            .iter()
            .any(|route| route.compatibility.decisions_wire_format() == format)
    };
    let system_one_error = uses(DecisionsWireFormat::SystemOne)
        .then(|| core_decisions_request_to_system_one(request).err())
        .flatten();
    let openai_error = uses(DecisionsWireFormat::OpenAi)
        .then(|| core_decisions_request_to_openai(request).err())
        .flatten();
    let mut first_error = None;
    routes.retain(|route| {
        let error = match route.compatibility.decisions_wire_format() {
            DecisionsWireFormat::SystemOne => &system_one_error,
            DecisionsWireFormat::OpenAi => &openai_error,
        };
        match error {
            Some(message) => {
                first_error.get_or_insert_with(|| message.clone());
                false
            }
            None => true,
        }
    });
    match first_error {
        Some(message) if routes.is_empty() => Err(GatewayError::InvalidRequest(message)),
        _ => Ok(()),
    }
}

/// Same-format responses pass through with only `model` rewritten, so System
/// One callers of System One upstreams see the upstream body unchanged.
/// Cross-format responses are rendered from the validated core answers.
fn render_caller_response(
    caller_format: DecisionsWireFormat,
    provider_response: &gateway_core::ProviderDecisionsResponse,
    request: &CoreDecisionsRequest,
    requested_model_key: &str,
) -> Value {
    let body = if provider_response.format == caller_format {
        provider_response.body.clone()
    } else {
        caller_format.render_response(&provider_response.parsed, request)
    };
    normalize_response_model(body, requested_model_key)
}

/// Every exit after model resolution records one `gateway.decisions.requests`
/// sample, including rejections before any route is chosen.
async fn execute_decisions(
    state: AppState,
    request_id: Option<Extension<RequestId>>,
    headers: HeaderMap,
    policy: RequestPolicyContext,
    auth: AuthenticatedApiKey,
    call: DecisionsCall,
) -> Result<Response, AppError> {
    let request_started_at = Instant::now();
    let request_id = canonical_request_id(request_id)?;
    let core_request = parse_decisions_request(&call.body, call.format).map_err(AppError)?;
    let resolved = state
        .service
        .resolve_request(&auth, &core_request.model)
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
        Ok(()) => {
            dispatch_decisions(
                &state,
                &headers,
                &auth,
                &call,
                ResolvedDecisions {
                    request_id,
                    request_started_at,
                    core_request,
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
    state
        .metrics
        .record_decisions_request(&DecisionsRequestMetric {
            http_route: call.route_path,
            request_format: metric_request_format(call.format),
            requested_model: &requested_model,
            resolved_model: &resolved_model,
            provider_key: selected_provider.as_deref().unwrap_or("unavailable"),
            status_code,
            outcome,
            latency_seconds: latency_seconds_since(request_started_at),
        });
    result
}

const fn metric_request_format(format: DecisionsWireFormat) -> &'static str {
    match format {
        DecisionsWireFormat::OpenAi => "openai",
        DecisionsWireFormat::SystemOne => "system_one",
    }
}

struct ResolvedDecisions {
    request_id: String,
    request_started_at: Instant,
    core_request: CoreDecisionsRequest,
    resolved: gateway_service::ResolvedGatewayRequest,
}

/// Routes and executes a resolved request. `selected_provider` is set once a
/// route is chosen, so the caller can label the request metric on any exit.
async fn dispatch_decisions(
    state: &AppState,
    headers: &HeaderMap,
    auth: &AuthenticatedApiKey,
    call: &DecisionsCall,
    request: ResolvedDecisions,
    selected_provider: &mut Option<String>,
) -> Result<Response, AppError> {
    let ResolvedDecisions {
        request_id,
        request_started_at,
        mut core_request,
        mut resolved,
    } = request;
    let request_span = Span::current();
    let requirements = core_request.requirements();
    let route_count = resolved.routes.len();
    retain_representable_routes(&mut resolved.routes, &core_request).map_err(AppError)?;
    record_request_span_fields(&request_span, auth, &resolved, false, call.route_path);
    let request_headers = extract_request_headers(headers);
    let request_tags = extract_request_tags(headers)?;
    let mut request_log_context = state.service.begin_decisions_request_log(
        &request_id,
        &resolved.selection.requested_model.model_key,
        &resolved.selection.execution_model.model_key,
        &call.body,
        &request_headers,
        request_tags,
    );
    let (eligible_route_count, selected) = select_provider_route(
        state,
        &resolved,
        requirements,
        headers,
        &core_request.extra,
        &request_headers,
        RoutingEndpoint::Decisions,
    )
    .await?;

    tracing::info!(
        request_model = %core_request.model,
        resolved_model = %resolved.selection.execution_model.model_key,
        request_format = call.format.as_str(),
        route_count,
        eligible_route_count,
        required_capabilities = ?requirements.required_capability_names(),
        "decisions request resolved"
    );

    let SelectedProviderRoute {
        route,
        provider,
        receipt: _,
        expected_provider_credential_id,
    } = match selected {
        Some(selection) => selection,
        None => {
            return Err(AppError(no_compatible_route_error(requirements)));
        }
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
    let labels = ChatMetricLabels {
        requested_model: &resolved.selection.requested_model.model_key,
        resolved_model: &resolved.selection.execution_model.model_key,
        provider_key: &route.provider_key,
        stream: false,
    };

    // Decisions routes skip the prompt and response evaluators: the input and
    // questions carry no chat-shaped text for them to inspect. Secrets are
    // still redacted before the request leaves the gateway.
    let route_key = model_route_key(
        &resolved.selection.execution_model.model_key,
        &route.provider_key,
        &route.upstream_model,
    );
    redact_typed_request(state, &request_id, &route_key, &mut core_request).await?;

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
        // A summary row with no attempts. The request metric is recorded by
        // `execute_decisions`, so skip the chat metrics the shared
        // pre-provider failure helper would add.
        best_effort_log_non_stream_failure(
            &state.service,
            auth,
            &request_log_context,
            &route.provider_key,
            icon_metadata,
            latency_ms_since(request_started_at),
            &error,
            Vec::new(),
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
        "decisions",
        auth,
        &resolved,
        &route,
        provider.as_ref(),
        false,
    );
    let provider_response = match trace_provider_operation(
        provider_execution_span,
        provider.decisions(&core_request, &context),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            let (provider_error, partial_provider_usage) = split_partial_provider_error(error);
            if let Some(provider_usage) = partial_provider_usage {
                finalize_successful_usage_accounting(
                    state,
                    UsageAccountingContext {
                        auth,
                        model: &resolved.selection.execution_model,
                        route: &route,
                        request_id: &request_id,
                        labels: labels.clone(),
                        operation: "decisions",
                    },
                    provider_usage,
                )
                .await;
            }

            let (error, attempt) = provider_error_attempt(
                &request_log_context,
                &route,
                RequestAttemptStatus::ProviderError,
                false,
                attempt_started_at,
                provider_error,
                requirements,
            );
            best_effort_log_non_stream_failure(
                &state.service,
                auth,
                &request_log_context,
                &route.provider_key,
                icon_metadata.clone(),
                latency_ms_since(request_started_at),
                &error,
                vec![attempt],
            )
            .await;
            return Err(AppError(error));
        }
    };
    let attempt = success_attempt(&request_log_context, &route, false, attempt_started_at);
    let value = render_caller_response(
        call.format,
        &provider_response,
        &core_request,
        &resolved.selection.requested_model.model_key,
    );

    // Price the upstream's own usage report: it carries the cached and
    // reasoning token details, whatever shape the caller receives.
    finalize_successful_usage_accounting(
        state,
        UsageAccountingContext {
            auth,
            model: &resolved.selection.execution_model,
            route: &route,
            request_id: &request_id,
            labels: labels.clone(),
            operation: "decisions",
        },
        usage_value_from_response(&provider_response.body),
    )
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

#[cfg(test)]
mod tests {
    use gateway_core::{DecisionsWireFormat, ProviderDecisionsResponse};
    use serde_json::json;

    use super::{
        detect_decisions_format, parse_decisions_request, render_caller_response,
        retain_representable_routes,
    };

    fn system_one_body() -> serde_json::Value {
        json!({
            "model": "jev",
            "state": "Help! My payouts have been failing for 3 days.",
            "questions": {
                "is_urgent": {"type": "noul", "instructions": "Does this convey urgency?"}
            }
        })
    }

    fn openai_body() -> serde_json::Value {
        json!({
            "model": "jev",
            "input": "Help! My payouts have been failing for 3 days.",
            "questions": [{"type": "predicate", "name": "is_urgent", "instructions": "Urgent?"}]
        })
    }

    #[test]
    fn detects_format_from_input_or_state() {
        assert_eq!(
            detect_decisions_format(&system_one_body()).expect("System One"),
            DecisionsWireFormat::SystemOne
        );
        assert_eq!(
            detect_decisions_format(&openai_body()).expect("OpenAI"),
            DecisionsWireFormat::OpenAi
        );

        let mut both = openai_body();
        both["state"] = json!("state");
        let error = detect_decisions_format(&both).expect_err("both");
        assert!(error.to_string().contains("both `input`"), "{error}");

        let error = detect_decisions_format(&json!({"model": "jev"})).expect_err("neither");
        assert!(
            error.to_string().contains("must contain `input`"),
            "{error}"
        );

        assert!(detect_decisions_format(&json!([])).is_err());
    }

    #[test]
    fn parse_errors_name_the_format() {
        let error = parse_decisions_request(
            &json!({"model": "jev", "state": "x"}),
            DecisionsWireFormat::SystemOne,
        )
        .expect_err("no questions");
        assert!(
            error.to_string().contains("`questions` cannot be empty"),
            "{error}"
        );

        let error = parse_decisions_request(
            &json!({"model": "jev", "input": "x", "questions": [{"type": "bogus"}]}),
            DecisionsWireFormat::OpenAi,
        )
        .expect_err("bad question type");
        assert!(
            error
                .to_string()
                .contains("invalid OpenAI decisions request"),
            "{error}"
        );
    }

    #[test]
    fn same_format_passes_through_and_cross_format_renders() {
        let upstream = json!({
            "model": "jev-1.13.0",
            "answers": {"is_urgent": {"type": "noul", "noul": 0.9, "rationale": "blocked"}},
            "usage": {"input_tokens": 10, "output_tokens": 1},
            "id": "dec_123"
        });
        let request = parse_decisions_request(&system_one_body(), DecisionsWireFormat::SystemOne)
            .expect("request");
        let provider_response = ProviderDecisionsResponse {
            format: DecisionsWireFormat::SystemOne,
            parsed: DecisionsWireFormat::SystemOne
                .parse_response(&upstream, &request)
                .expect("parsed"),
            body: upstream.clone(),
        };

        let mut expected = upstream;
        expected["model"] = json!("jev");
        assert_eq!(
            render_caller_response(
                DecisionsWireFormat::SystemOne,
                &provider_response,
                &request,
                "jev"
            ),
            expected
        );

        let rendered = render_caller_response(
            DecisionsWireFormat::OpenAi,
            &provider_response,
            &request,
            "jev",
        );
        assert_eq!(
            rendered["answers"],
            json!([{"type": "predicate", "name": "is_urgent", "probability": 0.9}])
        );
        assert_eq!(rendered["model"], "jev");
        assert_eq!(rendered["usage"]["total_tokens"], 11);
    }

    fn route(provider_key: &str, openai: bool) -> gateway_core::ModelRoute {
        let mut route = gateway_core::ModelRoute {
            id: uuid::Uuid::new_v4(),
            model_id: uuid::Uuid::new_v4(),
            provider_key: provider_key.to_string(),
            upstream_model: "upstream".to_string(),
            priority: 0,
            weight: 1.0,
            enabled: true,
            extra_headers: Default::default(),
            extra_body: Default::default(),
            context_window_tokens: None,
            pricing_override: None,
            capabilities: gateway_core::ProviderCapabilities::all_enabled(),
            compatibility: Default::default(),
        };
        if openai {
            route.compatibility.openai_compat =
                Some(gateway_core::OpenAiCompatRouteCompatibility {
                    decisions_api: Some(gateway_core::OpenAiCompatDecisionsApi::OpenAi),
                    ..Default::default()
                });
        }
        route
    }

    #[test]
    fn route_filter_keeps_only_routes_that_can_carry_the_request() {
        let routes = vec![route("system-one", false), route("openai", true)];

        let mut image_request = openai_body();
        image_request["input"] = json!([{"role": "user", "content": [
            {"type": "input_image", "image_url": "https://example.test/a.png"}
        ]}]);
        let request =
            parse_decisions_request(&image_request, DecisionsWireFormat::OpenAi).expect("request");
        let mut kept = routes.clone();
        retain_representable_routes(&mut kept, &request).expect("OpenAI route remains");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].provider_key, "openai");

        let mut only_system_one = vec![route("system-one", false)];
        let error = retain_representable_routes(&mut only_system_one, &request)
            .expect_err("no route can carry images");
        assert!(error.to_string().contains("image inputs"), "{error}");

        let single_choice = json!({
            "model": "jev",
            "state": "x",
            "questions": {"pick": {"type": "choice", "instructions": "Pick", "criteria": {"only": null}}}
        });
        let request = parse_decisions_request(&single_choice, DecisionsWireFormat::SystemOne)
            .expect("request");
        let mut kept = routes;
        retain_representable_routes(&mut kept, &request).expect("System One route remains");
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].provider_key, "system-one");
    }
}
