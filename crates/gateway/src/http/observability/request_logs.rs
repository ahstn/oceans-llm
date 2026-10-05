//! Request log list and detail handlers, plus the views that join each log with its
//! caller names, provider display, and usage ledger figures.

use super::*;

#[utoipa::path(
    get,
    path = "/api/v1/admin/observability/request-logs",
    params(RequestLogListQuery),
    responses((status = 200, body = Envelope<RequestLogPageView>)),
    security(("session_cookie" = []))
)]
pub async fn list_request_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RequestLogListQuery>,
) -> Result<Json<Envelope<RequestLogPageView>>, AppError> {
    let current_user = require_authenticated_session(&state, &headers).await?;

    let request_log_query = RequestLogQuery {
        page: query.page.unwrap_or(DEFAULT_PAGE).max(1),
        page_size: query
            .page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_REQUEST_LOG_PAGE_SIZE),
        request_id: empty_to_none(query.request_id),
        model_key: empty_to_none(query.model_key),
        provider_key: empty_to_none(query.provider_key),
        status_code: query.status_code,
        user_id: scoped_user_id(
            current_user.user_id,
            current_user.global_role,
            parse_optional_uuid(query.user_id.as_deref(), "user_id")?,
        ),
        team_id: parse_optional_uuid(query.team_id.as_deref(), "team_id")?,
        service_account_id: parse_optional_uuid(
            query.service_account_id.as_deref(),
            "service_account_id",
        )?,
        service: empty_to_none(query.service),
        component: empty_to_none(query.component),
        env: empty_to_none(query.env),
        tag_key: None,
        tag_value: None,
        q: empty_to_none(query.q),
    };
    let (tag_key, tag_value) = parse_optional_tag_filter(query.tag_key, query.tag_value)?;
    let query = RequestLogQuery {
        tag_key,
        tag_value,
        ..request_log_query
    };

    let page = state.service.list_request_logs(&query).await?;
    let providers = provider_connections_by_key(&state, &page.items).await?;
    let callers = request_caller_directory(&state, &page.items).await?;
    let usage = request_usage_directory(&state, &page.items).await?;
    let items = page
        .items
        .iter()
        .map(|log| {
            summary_view(
                log,
                providers.get(log.provider_key.as_str()),
                &callers,
                &usage,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(envelope(RequestLogPageView {
        items,
        page: page.page,
        page_size: page.page_size,
        total: page.total,
    })))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/observability/request-logs/{request_log_id}",
    params(("request_log_id" = String, Path, description = "Request log identifier")),
    responses(
        (status = 200, body = Envelope<RequestLogDetailView>),
        (status = 404, body = OpenAiErrorEnvelopeView, description = "Request log not found")
    ),
    security(("session_cookie" = []))
)]
pub async fn get_request_log_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(request_log_id): Path<Uuid>,
) -> Result<Json<Envelope<RequestLogDetailView>>, AppError> {
    let current_user = require_authenticated_session(&state, &headers).await?;

    let detail = state.service.get_request_log_detail(request_log_id).await?;
    require_owned_record(
        current_user.user_id,
        current_user.global_role,
        detail.log.user_id,
    )?;
    let provider = provider_connection(&state, detail.log.provider_key.as_str()).await?;
    let callers = request_caller_directory(&state, std::slice::from_ref(&detail.log)).await?;
    let usage = request_usage_directory(&state, std::slice::from_ref(&detail.log)).await?;
    let mcp_token_overhead = state
        .store
        .get_request_mcp_token_overhead(&detail.log.request_id)
        .await?;
    Ok(Json(envelope(detail_view(
        detail,
        provider.as_ref(),
        &callers,
        &usage,
        mcp_token_overhead,
    )?)))
}

async fn provider_connections_by_key(
    state: &AppState,
    logs: &[RequestLogRecord],
) -> Result<HashMap<String, ProviderConnection>, AppError> {
    let provider_keys: HashSet<_> = logs
        .iter()
        .filter(|log| provider_icon_key_from_metadata(&log.metadata).is_none())
        .map(|log| log.provider_key.clone())
        .collect();

    let mut providers = HashMap::new();
    for provider_key in provider_keys {
        if let Some(provider) = provider_connection(state, provider_key.as_str()).await? {
            providers.insert(provider_key, provider);
        }
    }

    Ok(providers)
}

async fn provider_connection(
    state: &AppState,
    provider_key: &str,
) -> Result<Option<ProviderConnection>, AppError> {
    state
        .store
        .get_provider_by_key(provider_key)
        .await
        .map_err(|error| AppError(error.into()))
}

/// Display names for the api keys, users, and service accounts referenced by
/// a page of request logs, resolved once per distinct id. Missing entries are
/// expected: callers may have been deleted after the log was recorded.
#[derive(Debug, Default)]
struct RequestCallerDirectory {
    api_key_names: HashMap<Uuid, String>,
    users: HashMap<Uuid, (String, String)>,
    service_account_names: HashMap<Uuid, String>,
}

async fn request_caller_directory(
    state: &AppState,
    logs: &[RequestLogRecord],
) -> Result<RequestCallerDirectory, AppError> {
    let mut directory = RequestCallerDirectory::default();

    let api_key_ids: HashSet<Uuid> = logs.iter().map(|log| log.api_key_id).collect();
    for api_key_id in api_key_ids {
        if let Some(api_key) = state.store.get_api_key_by_id(api_key_id).await? {
            directory.api_key_names.insert(api_key_id, api_key.name);
        }
    }

    let user_ids: HashSet<Uuid> = logs.iter().filter_map(|log| log.user_id).collect();
    for user_id in user_ids {
        if let Some(identity_user) = state.store.get_identity_user(user_id).await? {
            directory
                .users
                .insert(user_id, (identity_user.user.name, identity_user.user.email));
        }
    }

    let service_account_ids: HashSet<Uuid> = logs
        .iter()
        .filter_map(|log| log.service_account_id)
        .collect();
    for service_account_id in service_account_ids {
        if let Some(service_account) = state
            .store
            .get_service_account_by_id(service_account_id)
            .await?
        {
            directory
                .service_account_names
                .insert(service_account_id, service_account.service_account_name);
        }
    }

    Ok(directory)
}

/// Usage ledger cost and cache figures for a page of request logs. Usage events carry no
/// request log id, so they are matched on `(request_id, api_key_id)`. The store only returns
/// usage whose pair identifies a single request log, so a reused client request id shows no
/// cost instead of another request's cost.
#[derive(Debug, Default)]
struct RequestUsageDirectory {
    by_request_id: HashMap<String, Vec<UsageLedgerRecord>>,
}

impl RequestUsageDirectory {
    fn usage_for(&self, log: &RequestLogRecord) -> Option<&UsageLedgerRecord> {
        self.by_request_id
            .get(&log.request_id)?
            .iter()
            .find(|record| record.api_key_id == log.api_key_id)
    }
}

async fn request_usage_directory(
    state: &AppState,
    logs: &[RequestLogRecord],
) -> Result<RequestUsageDirectory, AppError> {
    let request_ids: Vec<String> = logs
        .iter()
        .map(|log| log.request_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut directory = RequestUsageDirectory::default();
    for record in state
        .store
        .get_request_log_usage_by_request_ids(&request_ids)
        .await?
    {
        directory
            .by_request_id
            .entry(record.request_id.clone())
            .or_default()
            .push(record);
    }
    Ok(directory)
}

fn summary_view(
    log: &RequestLogRecord,
    provider: Option<&ProviderConnection>,
    callers: &RequestCallerDirectory,
    usage: &RequestUsageDirectory,
) -> Result<RequestLogSummaryView, AppError> {
    let provider_icon_key = provider_icon_key_from_metadata(&log.metadata)
        .or_else(|| Some(resolve_provider_display(log.provider_key.as_str(), provider).icon_key))
        .map(Into::into);
    let model_icon_key = model_icon_key_from_metadata(&log.metadata)
        .or_else(|| {
            resolve_model_icon_key([log.resolved_model_key.as_str(), log.model_key.as_str()])
        })
        .map(Into::into);

    let user = log.user_id.and_then(|user_id| callers.users.get(&user_id));
    let usage = usage.usage_for(log);

    Ok(RequestLogSummaryView {
        request_log_id: log.request_log_id.to_string(),
        request_id: log.request_id.clone(),
        api_key_id: log.api_key_id.to_string(),
        api_key_name: callers.api_key_names.get(&log.api_key_id).cloned(),
        user_id: log.user_id.map(|value| value.to_string()),
        user_name: user.map(|(name, _)| name.clone()),
        user_email: user.map(|(_, email)| email.clone()),
        team_id: log.team_id.map(|value| value.to_string()),
        service_account_id: log.service_account_id.map(|value| value.to_string()),
        service_account_name: log
            .service_account_id
            .and_then(|id| callers.service_account_names.get(&id).cloned()),
        model_key: log.model_key.clone(),
        resolved_model_key: log.resolved_model_key.clone(),
        model_icon_key,
        provider_key: log.provider_key.clone(),
        provider_icon_key,
        status_code: log.status_code,
        latency_ms: log.latency_ms,
        prompt_tokens: log.prompt_tokens,
        completion_tokens: log.completion_tokens,
        total_tokens: log.total_tokens,
        cache_read_tokens: usage.and_then(|record| record.cache_read_tokens),
        // Unpriced and usage-missing rows store a zero cost, which would read as "free".
        cost_usd_10000: usage
            .filter(|record| record.pricing_status.counts_toward_spend())
            .map(|record| record.computed_cost_usd.as_scaled_i64()),
        error_code: log.error_code.clone(),
        has_payload: log.has_payload,
        request_payload_truncated: log.request_payload_truncated,
        response_payload_truncated: log.response_payload_truncated,
        payload_policy: payload_policy_view(&log.metadata)?,
        request_tags: request_tags_view(&log.request_tags),
        tool_cardinality: RequestToolCardinalityView {
            referenced_mcp_server_count: log.tool_cardinality.referenced_mcp_server_count,
            exposed_tool_count: log.tool_cardinality.exposed_tool_count,
            invoked_tool_count: log.tool_cardinality.invoked_tool_count,
            filtered_tool_count: log.tool_cardinality.filtered_tool_count,
            request_tool_count: log.tool_cardinality.request_tool_count,
            invoked_distinct_tool_count: log.tool_cardinality.invoked_distinct_tool_count,
        },
        agent_harness_key: log.agent_harness_key.clone(),
        agent_harness_label: log.agent_harness_label.clone(),
        metadata: log.metadata.clone(),
        occurred_at: format_timestamp(log.occurred_at),
    })
}

fn payload_policy_view(
    metadata: &Map<String, Value>,
) -> Result<RequestLogPayloadPolicyView, AppError> {
    let policy = metadata
        .get("payload_policy")
        .and_then(Value::as_object)
        .ok_or_else(|| payload_policy_contract_error("missing payload_policy object"))?;

    Ok(RequestLogPayloadPolicyView {
        capture_mode: match required_payload_policy_string(policy, "capture_mode")? {
            "disabled" => RequestLogPayloadCaptureModeView::Disabled,
            "summary_only" => RequestLogPayloadCaptureModeView::SummaryOnly,
            "redacted_payloads" => RequestLogPayloadCaptureModeView::RedactedPayloads,
            other => {
                return Err(payload_policy_contract_error(format!(
                    "unknown capture_mode `{other}`"
                )));
            }
        },
        request_max_bytes: required_positive_payload_policy_u64(policy, "request_max_bytes")?,
        response_max_bytes: required_positive_payload_policy_u64(policy, "response_max_bytes")?,
        stream_max_events: required_positive_payload_policy_u64(policy, "stream_max_events")?,
        version: required_payload_policy_string(policy, "version")?.to_string(),
    })
}

fn required_payload_policy_string<'a>(
    policy: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, AppError> {
    policy
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| payload_policy_contract_error(format!("missing string field `{field}`")))
}

fn required_positive_payload_policy_u64(
    policy: &Map<String, Value>,
    field: &str,
) -> Result<u64, AppError> {
    let value = policy
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| payload_policy_contract_error(format!("missing u64 field `{field}`")))?;
    if value == 0 {
        return Err(payload_policy_contract_error(format!(
            "field `{field}` must be greater than zero"
        )));
    }
    Ok(value)
}

fn payload_policy_contract_error(message: impl Into<String>) -> AppError {
    AppError(GatewayError::Internal(format!(
        "invalid request log payload_policy metadata: {}",
        message.into()
    )))
}

fn detail_view(
    detail: RequestLogDetail,
    provider: Option<&ProviderConnection>,
    callers: &RequestCallerDirectory,
    usage: &RequestUsageDirectory,
    mcp_token_overhead: Option<RequestMcpTokenOverheadRecord>,
) -> Result<RequestLogDetailView, AppError> {
    Ok(RequestLogDetailView {
        log: summary_view(&detail.log, provider, callers, usage)?,
        user_agent_raw: detail.log.user_agent_raw,
        payload: detail.payload.map(payload_view),
        attempts: detail.attempts.into_iter().map(attempt_view).collect(),
        mcp_token_overhead: mcp_token_overhead.map(mcp_token_overhead_view),
    })
}

fn mcp_token_overhead_view(overhead: RequestMcpTokenOverheadRecord) -> RequestMcpTokenOverheadView {
    RequestMcpTokenOverheadView {
        provider_family: overhead.provider_family,
        model_or_encoding: overhead.model_or_encoding,
        exposed_tool_count: overhead.exposed_tool_count,
        estimated_definition_tokens: overhead.estimated_definition_tokens,
        estimated_result_tokens: overhead.estimated_result_tokens,
        estimator_source: overhead.estimator_source.as_str().to_string(),
        confidence: overhead.confidence.as_str().to_string(),
        cache_hit_count: overhead.cache_hit_count,
        cache_miss_count: overhead.cache_miss_count,
        context_window_tokens: overhead.context_window_tokens,
        context_window_percent_bps: overhead.context_window_percent_bps,
        metadata: overhead.metadata,
    }
}

fn attempt_view(attempt: RequestAttemptRecord) -> RequestAttemptView {
    RequestAttemptView {
        request_attempt_id: attempt.request_attempt_id.to_string(),
        request_log_id: attempt.request_log_id.to_string(),
        request_id: attempt.request_id,
        attempt_number: attempt.attempt_number,
        route_id: attempt.route_id.to_string(),
        provider_key: attempt.provider_key,
        upstream_model: attempt.upstream_model,
        status: attempt.status.as_str().to_string(),
        status_code: attempt.status_code,
        error_code: attempt.error_code,
        error_detail: attempt.error_detail,
        error_detail_truncated: attempt.error_detail_truncated,
        retryable: attempt.retryable,
        terminal: attempt.terminal,
        produced_final_response: attempt.produced_final_response,
        stream: attempt.stream,
        started_at: format_timestamp(attempt.started_at),
        completed_at: attempt.completed_at.map(format_timestamp),
        latency_ms: attempt.latency_ms,
        metadata: attempt.metadata,
    }
}

fn payload_view(payload: RequestLogPayloadRecord) -> RequestLogPayloadView {
    RequestLogPayloadView {
        request_json: payload.request_json,
        response_json: payload.response_json,
    }
}

fn parse_optional_tag_filter(
    key: Option<String>,
    value: Option<String>,
) -> Result<(Option<String>, Option<String>), AppError> {
    let key = empty_to_none(key);
    let value = empty_to_none(value);

    match (key, value) {
        (None, None) => Ok((None, None)),
        (Some(_), None) => Err(AppError(GatewayError::InvalidRequest(
            "request log tag filters require both `tag_key` and `tag_value`".to_string(),
        ))),
        (None, Some(_)) => Err(AppError(GatewayError::InvalidRequest(
            "request log tag filters require both `tag_key` and `tag_value`".to_string(),
        ))),
        (Some(key), Some(value)) => {
            let tag = build_bespoke_tag_filter(&key, &value).map_err(AppError)?;
            Ok((Some(tag.key), Some(tag.value)))
        }
    }
}

fn request_tags_view(tags: &RequestTags) -> RequestTagsView {
    RequestTagsView {
        service: tags.service.clone(),
        component: tags.component.clone(),
        env: tags.env.clone(),
        bespoke: tags.bespoke.iter().map(request_tag_view).collect(),
    }
}

fn request_tag_view(tag: &RequestTag) -> RequestTagView {
    RequestTagView {
        key: tag.key.clone(),
        value: tag.value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use gateway_core::UsagePricingStatus;
    use gateway_service::REQUEST_LOG_PROVIDER_ICON_KEY;
    use serde_json::{Map, Value, json};
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn summary_view_uses_provider_display_config_when_metadata_is_missing() {
        let log = request_log_record(payload_policy_metadata());
        let provider = ProviderConnection {
            provider_key: "router".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://openrouter.ai/api/v1",
                "display": {
                    "label": "OpenRouter",
                    "icon_key": "openrouter"
                }
            }),
            secrets: None,
        };

        let summary = summary_view(
            &log,
            Some(&provider),
            &RequestCallerDirectory::default(),
            &RequestUsageDirectory::default(),
        )
        .unwrap_or_else(|error| panic!("summary should succeed: {}", error.0));

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::OpenRouter)
        ));
    }

    #[test]
    fn summary_view_falls_back_to_provider_key_when_provider_config_is_unavailable() {
        let log = request_log_record(payload_policy_metadata());

        let summary = summary_view(
            &log,
            None,
            &RequestCallerDirectory::default(),
            &RequestUsageDirectory::default(),
        )
        .unwrap_or_else(|error| panic!("summary should succeed: {}", error.0));

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::OpenAI)
        ));
    }

    #[test]
    fn summary_view_prefers_stored_metadata_over_provider_fallbacks() {
        let mut metadata = payload_policy_metadata();
        metadata.insert(
            REQUEST_LOG_PROVIDER_ICON_KEY.to_string(),
            Value::String("anthropic".to_string()),
        );
        let log = request_log_record(metadata);
        let provider = ProviderConnection {
            provider_key: "router".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://openrouter.ai/api/v1",
                "display": {
                    "label": "OpenRouter",
                    "icon_key": "openrouter"
                }
            }),
            secrets: None,
        };

        let summary = summary_view(
            &log,
            Some(&provider),
            &RequestCallerDirectory::default(),
            &RequestUsageDirectory::default(),
        )
        .unwrap_or_else(|error| panic!("summary should succeed: {}", error.0));

        assert!(matches!(
            summary.provider_icon_key,
            Some(crate::http::admin_contract::ProviderIconKeyView::Anthropic)
        ));
    }

    #[test]
    fn summary_view_requires_payload_policy_metadata() {
        let log = request_log_record(Map::new());

        let error = summary_view(
            &log,
            None,
            &RequestCallerDirectory::default(),
            &RequestUsageDirectory::default(),
        )
        .expect_err("summary should fail");

        assert!(
            error
                .0
                .to_string()
                .contains("missing payload_policy object")
        );
    }

    #[test]
    fn summary_view_rejects_unknown_payload_policy_capture_mode() {
        let mut metadata = payload_policy_metadata();
        metadata["payload_policy"]
            .as_object_mut()
            .expect("policy")
            .insert("capture_mode".to_string(), json!("legacy"));
        let log = request_log_record(metadata);

        let error = summary_view(
            &log,
            None,
            &RequestCallerDirectory::default(),
            &RequestUsageDirectory::default(),
        )
        .expect_err("summary should fail");

        assert!(
            error
                .0
                .to_string()
                .contains("unknown capture_mode `legacy`")
        );
    }

    #[test]
    fn summary_view_rejects_malformed_payload_policy_metadata() {
        let mut metadata = payload_policy_metadata();
        metadata["payload_policy"]
            .as_object_mut()
            .expect("policy")
            .insert("request_max_bytes".to_string(), json!("65536"));
        let log = request_log_record(metadata);

        let error = summary_view(
            &log,
            None,
            &RequestCallerDirectory::default(),
            &RequestUsageDirectory::default(),
        )
        .expect_err("summary should fail");

        assert!(
            error
                .0
                .to_string()
                .contains("missing u64 field `request_max_bytes`")
        );
    }

    #[test]
    fn summary_view_rejects_zero_payload_policy_limits() {
        let mut metadata = payload_policy_metadata();
        metadata["payload_policy"]
            .as_object_mut()
            .expect("policy")
            .insert("stream_max_events".to_string(), json!(0));
        let log = request_log_record(metadata);

        let error = summary_view(
            &log,
            None,
            &RequestCallerDirectory::default(),
            &RequestUsageDirectory::default(),
        )
        .expect_err("summary should fail");

        assert!(
            error
                .0
                .to_string()
                .contains("field `stream_max_events` must be greater than zero")
        );
    }

    #[test]
    fn summary_view_reports_usage_for_matching_api_key_and_hides_unpriced_cost() {
        let mut log = request_log_record(payload_policy_metadata());
        log.tool_cardinality.request_tool_count = Some(5);
        let other_key_usage = UsageLedgerRecord {
            api_key_id: Uuid::new_v4(),
            computed_cost_usd: gateway_core::Money4::from_scaled(999),
            cache_read_tokens: Some(1),
            ..usage_ledger_record(&log)
        };
        let summary_with = |pricing_status| {
            let mut usage = RequestUsageDirectory::default();
            usage.by_request_id.insert(
                log.request_id.clone(),
                vec![
                    other_key_usage.clone(),
                    UsageLedgerRecord {
                        pricing_status,
                        ..usage_ledger_record(&log)
                    },
                ],
            );
            summary_view(&log, None, &RequestCallerDirectory::default(), &usage)
                .unwrap_or_else(|error| panic!("summary should succeed: {}", error.0))
        };

        let priced = summary_with(UsagePricingStatus::Priced);
        assert_eq!(priced.cost_usd_10000, Some(1_234));
        assert_eq!(priced.cache_read_tokens, Some(80));
        assert_eq!(priced.tool_cardinality.request_tool_count, Some(5));
        let legacy = summary_with(UsagePricingStatus::LegacyEstimated);
        assert_eq!(legacy.cost_usd_10000, Some(1_234));
        let unpriced = summary_with(UsagePricingStatus::Unpriced);
        assert_eq!(unpriced.cost_usd_10000, None);
        assert_eq!(unpriced.cache_read_tokens, Some(80));

        let without_usage = summary_view(
            &log,
            None,
            &RequestCallerDirectory::default(),
            &RequestUsageDirectory::default(),
        )
        .unwrap_or_else(|error| panic!("summary should succeed: {}", error.0));
        assert_eq!(without_usage.cost_usd_10000, None);
        assert_eq!(without_usage.cache_read_tokens, None);
    }

    fn usage_ledger_record(log: &RequestLogRecord) -> UsageLedgerRecord {
        UsageLedgerRecord {
            usage_event_id: Uuid::new_v4(),
            request_id: log.request_id.clone(),
            ownership_scope_key: "user:test".to_string(),
            api_key_id: log.api_key_id,
            user_id: None,
            team_id: None,
            service_account_id: None,
            actor_user_id: None,
            model_id: None,
            model_route_id: None,
            provider_key: log.provider_key.clone(),
            upstream_model: "gpt-4o-mini".to_string(),
            prompt_tokens: Some(100),
            uncached_input_tokens: Some(20),
            cache_read_tokens: Some(80),
            cache_write_tokens: Some(0),
            completion_tokens: Some(50),
            total_tokens: Some(150),
            provider_usage: json!({}),
            pricing_status: UsagePricingStatus::Priced,
            unpriced_reason: None,
            pricing_row_id: None,
            pricing_provider_id: None,
            pricing_model_id: None,
            pricing_source: None,
            pricing_source_etag: None,
            pricing_source_fetched_at: None,
            pricing_last_updated: None,
            input_cost_per_million_tokens: None,
            output_cost_per_million_tokens: None,
            cache_read_cost_per_million_tokens: None,
            cache_write_cost_per_million_tokens: None,
            computed_cost_usd: gateway_core::Money4::from_scaled(1_234),
            occurred_at: log.occurred_at,
        }
    }

    fn request_log_record(metadata: Map<String, Value>) -> RequestLogRecord {
        RequestLogRecord {
            request_log_id: Uuid::new_v4(),
            request_id: "req_123".to_string(),
            api_key_id: Uuid::new_v4(),
            user_id: None,
            team_id: None,
            service_account_id: None,
            model_key: "router-model".to_string(),
            resolved_model_key: "router-model".to_string(),
            provider_key: "router".to_string(),
            status_code: Some(200),
            latency_ms: Some(42),
            prompt_tokens: Some(1),
            completion_tokens: Some(2),
            total_tokens: Some(3),
            error_code: None,
            has_payload: false,
            request_payload_truncated: false,
            response_payload_truncated: false,
            request_tags: RequestTags::default(),
            tool_cardinality: gateway_core::RequestToolCardinality::default(),
            user_agent_raw: None,
            agent_harness_key: "unknown".to_string(),
            agent_harness_label: "Unknown".to_string(),
            metadata,
            occurred_at: OffsetDateTime::now_utc(),
        }
    }

    fn payload_policy_metadata() -> Map<String, Value> {
        let mut metadata = Map::new();
        metadata.insert(
            "payload_policy".to_string(),
            json!({
                "capture_mode": "redacted_payloads",
                "request_max_bytes": 131072,
                "response_max_bytes": 65536,
                "stream_max_events": 128,
                "version": "builtin:v1"
            }),
        );
        metadata
    }
}
