use anyhow::Context;
use gateway_core::{
    AdminApiKeyRepository, ApiKeyModelGrantMode, ApiKeyOwnerKind, ApiKeyRepository, ApiKeyStatus,
    BudgetRepository, IdentityRepository, McpTokenEstimateConfidence, McpTokenEstimateSource,
    McpTokenOverheadRepository, ModelRepository, Money4, NewApiKeyRecord, RequestAttemptRecord,
    RequestAttemptStatus, RequestLogPayloadRecord, RequestLogRecord, RequestLogRepository,
    RequestMcpTokenOverheadRecord, RequestTag, RequestTags, UsageLedgerRecord, UsagePricingStatus,
    UserStatus,
};
use gateway_service::{
    RequestLogPayloadCaptureMode, RequestLogPayloadPolicy, hash_gateway_key_secret,
};
use gateway_store::{AnyStore, GatewayStore};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

mod api_keys;
mod models;
mod teams;
mod usage;
mod users;

#[derive(Debug, Clone, Copy)]
struct LocalDemoUserFixture {
    email: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum LocalDemoOwnerFixture {
    User(&'static str),
}

#[derive(Debug, Clone, Copy)]
struct LocalDemoApiKeyFixture {
    name: &'static str,
    public_id: &'static str,
    secret: &'static str,
    owner: LocalDemoOwnerFixture,
    model_keys: &'static [&'static str],
}

/// Shape of the sanitized payload seeded for a demo request, so the request-log
/// detail drawer has fixtures for every payload state it can render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoPayloadProfile {
    /// Short single-turn request and response payloads.
    Standard,
    /// Standard payloads recorded for a streaming request.
    Streamed,
    /// Multi-turn request with a long completion, for payload-viewer stress.
    Longform,
    /// Payloads stored but flagged as truncated by the capture budget.
    Truncated,
    /// Summary-only capture: no payload bodies are persisted.
    SummaryOnly,
}

#[derive(Debug, Clone, Copy)]
struct LocalDemoRequestFixture {
    request_id: &'static str,
    api_key_public_id: &'static str,
    days_ago: i64,
    hours_ago: i64,
    minutes_ago: i64,
    model_key: &'static str,
    resolved_model_key: &'static str,
    provider_key: &'static str,
    upstream_model: &'static str,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    cost_scaled: i64,
    status_code: i64,
    latency_ms: i64,
    service: &'static str,
    component: &'static str,
    env: &'static str,
    bespoke_key: &'static str,
    bespoke_value: &'static str,
    prompt: &'static str,
    completion: &'static str,
    error_code: Option<&'static str>,
    payload_profile: DemoPayloadProfile,
}

pub const LOCAL_DEMO_USER_PASSWORD: &str = "localdemo123";

pub async fn seed_local_demo_data(store: &AnyStore) -> anyhow::Result<Vec<(&'static str, String)>> {
    let password_hash = hash_gateway_key_secret(LOCAL_DEMO_USER_PASSWORD)
        .context("failed hashing local demo user password")?;
    let now = OffsetDateTime::now_utc();

    let mut user_ids = std::collections::HashMap::new();
    let mut user_team_ids = std::collections::HashMap::new();
    for fixture in users::LOCAL_DEMO_USERS {
        let user = store
            .get_user_by_email_normalized(&normalize_demo_email(fixture.email))
            .await
            .with_context(|| format!("failed loading demo user `{}`", fixture.email))?
            .ok_or_else(|| {
                anyhow::anyhow!("demo user `{}` is missing from config seed", fixture.email)
            })?;
        store
            .store_user_password(user.user_id, &password_hash, now)
            .await
            .with_context(|| format!("failed storing password for `{}`", fixture.email))?;
        store
            .update_user_status(user.user_id, UserStatus::Active, now)
            .await
            .with_context(|| format!("failed activating `{}`", fixture.email))?;
        store
            .update_user_must_change_password(user.user_id, false, now)
            .await
            .with_context(|| {
                format!("failed clearing password rotation for `{}`", fixture.email)
            })?;

        let team_id = store
            .get_team_membership_for_user(user.user_id)
            .await
            .with_context(|| format!("failed loading team membership for `{}`", fixture.email))?
            .map(|membership| membership.team_id);
        user_ids.insert(fixture.email, user.user_id);
        user_team_ids.insert(fixture.email, team_id);
    }

    let mut team_ids = std::collections::HashMap::new();
    for &team_key in teams::LOCAL_DEMO_TEAM_KEYS {
        let team = store
            .get_team_by_key(team_key)
            .await
            .with_context(|| format!("failed loading demo team `{team_key}`"))?
            .ok_or_else(|| anyhow::anyhow!("demo team `{team_key}` is missing from config seed"))?;
        team_ids.insert(team_key, team.team_id);
    }

    let mut model_ids = std::collections::HashMap::new();
    for &model_key in models::LOCAL_DEMO_MODEL_KEYS {
        let model = store
            .get_model_by_key(model_key)
            .await
            .with_context(|| format!("failed loading demo model `{model_key}`"))?
            .ok_or_else(|| {
                anyhow::anyhow!("demo model `{model_key}` is missing from config seed")
            })?;
        model_ids.insert(model_key, model.id);
    }

    let mut api_keys = std::collections::HashMap::new();
    let mut raw_keys = Vec::new();
    for fixture in api_keys::LOCAL_DEMO_API_KEYS {
        let owner = match fixture.owner {
            LocalDemoOwnerFixture::User(email) => (
                ApiKeyOwnerKind::User,
                Some(
                    *user_ids
                        .get(email)
                        .ok_or_else(|| anyhow::anyhow!("missing demo user `{email}`"))?,
                ),
                None,
                None,
            ),
        };
        let model_grants = fixture
            .model_keys
            .iter()
            .map(|model_key| {
                model_ids
                    .get(model_key)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("missing demo model `{model_key}`"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let raw_key = format!("gwk_{}.{}", fixture.public_id, fixture.secret);
        let api_key = match store
            .get_api_key_by_public_id(fixture.public_id)
            .await
            .with_context(|| format!("failed loading demo api key `{}`", fixture.public_id))?
        {
            Some(existing) => {
                if existing.status != ApiKeyStatus::Active {
                    anyhow::bail!(
                        "demo api key `{}` already exists but is not active; reset the local database and reseed",
                        fixture.public_id
                    );
                }
                if existing.owner_kind != owner.0
                    || existing.owner_user_id != owner.1
                    || existing.owner_team_id != owner.2
                    || existing.owner_service_account_id != owner.3
                {
                    anyhow::bail!(
                        "demo api key `{}` already exists with a different owner; reset the local database and reseed",
                        fixture.public_id
                    );
                }
                store
                    .replace_api_key_model_access(
                        existing.id,
                        ApiKeyModelGrantMode::Explicit,
                        &model_grants,
                    )
                    .await
                    .with_context(|| {
                        format!("failed refreshing grants for `{}`", fixture.public_id)
                    })?;
                existing
            }
            None => {
                let secret_hash = hash_gateway_key_secret(fixture.secret)
                    .with_context(|| format!("failed hashing api key `{}`", fixture.public_id))?;
                let created = store
                    .create_api_key(&NewApiKeyRecord {
                        name: fixture.name.to_string(),
                        public_id: fixture.public_id.to_string(),
                        secret_hash,
                        model_grant_mode: ApiKeyModelGrantMode::Explicit,
                        owner_kind: owner.0,
                        owner_user_id: owner.1,
                        owner_team_id: owner.2,
                        owner_service_account_id: owner.3,
                        created_at: now,
                    })
                    .await
                    .with_context(|| {
                        format!("failed creating demo api key `{}`", fixture.public_id)
                    })?;
                store
                    .replace_api_key_model_access(
                        created.id,
                        ApiKeyModelGrantMode::Explicit,
                        &model_grants,
                    )
                    .await
                    .with_context(|| {
                        format!("failed storing grants for `{}`", fixture.public_id)
                    })?;
                created
            }
        };
        api_keys.insert(fixture.public_id, api_key);
        raw_keys.push((fixture.name, raw_key));
    }

    // Demo usage rows are re-anchored to the current clock on every run: any
    // previously seeded rows are removed by their fixed request ids, then
    // reinserted with deterministic ids and fresh relative timestamps.
    let demo_request_ids = usage::LOCAL_DEMO_REQUESTS
        .iter()
        .map(|fixture| fixture.request_id.to_string())
        .collect::<Vec<_>>();
    store
        .delete_request_logs_by_request_ids(&demo_request_ids)
        .await
        .context("failed deleting previously seeded demo request logs")?;
    store
        .delete_usage_ledger_events_by_request_ids(&demo_request_ids)
        .await
        .context("failed deleting previously seeded demo usage events")?;

    for fixture in usage::LOCAL_DEMO_REQUESTS {
        let api_key = api_keys.get(fixture.api_key_public_id).ok_or_else(|| {
            anyhow::anyhow!("missing demo api key `{}`", fixture.api_key_public_id)
        })?;
        let occurred_at = now
            - time::Duration::days(fixture.days_ago)
            - time::Duration::hours(fixture.hours_ago)
            - time::Duration::minutes(fixture.minutes_ago);
        let ownership_scope_key = match api_key.owner_kind {
            ApiKeyOwnerKind::User => format!(
                "user:{}",
                api_key
                    .owner_user_id
                    .ok_or_else(|| anyhow::anyhow!("user-owned demo key missing owner_user_id"))?
            ),
            ApiKeyOwnerKind::ServiceAccount => format!(
                "service_account:{}",
                api_key.owner_service_account_id.ok_or_else(|| {
                    anyhow::anyhow!(
                        "service-account-owned demo key missing owner_service_account_id"
                    )
                })?
            ),
        };

        let user_id = api_key.owner_user_id;
        let team_id = match api_key.owner_kind {
            ApiKeyOwnerKind::User => {
                let owner_email = api_keys::LOCAL_DEMO_API_KEYS
                    .iter()
                    .find(|candidate| candidate.public_id == fixture.api_key_public_id)
                    .map(|candidate| match candidate.owner {
                        LocalDemoOwnerFixture::User(email) => email,
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "demo key `{}` is missing a user owner fixture",
                            fixture.api_key_public_id
                        )
                    })?;
                user_team_ids.get(owner_email).copied().flatten()
            }
            ApiKeyOwnerKind::ServiceAccount => api_key.owner_team_id,
        };

        let total_tokens = fixture
            .prompt_tokens
            .zip(fixture.completion_tokens)
            .map(|(prompt, completion)| prompt + completion);
        let priced = fixture.error_code.is_none();
        let request_log_id = demo_request_log_uuid(fixture.request_id);
        let stream = fixture.payload_profile == DemoPayloadProfile::Streamed;
        let payload_policy = match fixture.payload_profile {
            DemoPayloadProfile::SummaryOnly => RequestLogPayloadPolicy::new(
                RequestLogPayloadCaptureMode::SummaryOnly,
                65_536,
                65_536,
                128,
                Vec::new(),
            ),
            _ => RequestLogPayloadPolicy::default(),
        };
        let (request_payload_truncated, response_payload_truncated) = match fixture.payload_profile
        {
            DemoPayloadProfile::Truncated => (true, true),
            _ => (false, false),
        };
        let request_tags = RequestTags {
            service: Some(fixture.service.to_string()),
            component: Some(fixture.component.to_string()),
            env: Some(fixture.env.to_string()),
            bespoke: vec![RequestTag {
                key: fixture.bespoke_key.to_string(),
                value: fixture.bespoke_value.to_string(),
            }],
        };
        let metadata = Map::from_iter([
            (
                "operation".to_string(),
                Value::String("chat_completions".to_string()),
            ),
            ("stream".to_string(), Value::Bool(stream)),
            (
                "payload_policy".to_string(),
                payload_policy.metadata_value(),
            ),
            (
                "seed_source".to_string(),
                Value::String("local_demo_seed".to_string()),
            ),
            (
                "api_key_public_id".to_string(),
                Value::String(fixture.api_key_public_id.to_string()),
            ),
        ]);
        let payload = demo_payload_record(fixture, request_log_id, total_tokens);
        let log = RequestLogRecord {
            request_log_id,
            request_id: fixture.request_id.to_string(),
            api_key_id: api_key.id,
            user_id,
            team_id,
            service_account_id: api_key.owner_service_account_id,
            model_key: fixture.model_key.to_string(),
            resolved_model_key: fixture.resolved_model_key.to_string(),
            provider_key: fixture.provider_key.to_string(),
            status_code: Some(fixture.status_code),
            latency_ms: Some(fixture.latency_ms),
            prompt_tokens: fixture.prompt_tokens,
            completion_tokens: fixture.completion_tokens,
            total_tokens,
            error_code: fixture.error_code.map(str::to_string),
            has_payload: payload.is_some(),
            request_payload_truncated,
            response_payload_truncated,
            request_tags,
            tool_cardinality: usage::demo_tool_cardinality(fixture),
            user_agent_raw: Some("opencode/1.0.0 (local demo)".to_string()),
            agent_harness_key: "opencode".to_string(),
            agent_harness_label: "Opencode".to_string(),
            metadata,
            occurred_at,
        };
        let attempts = demo_attempts(fixture, request_log_id, occurred_at, stream);
        store
            .insert_request_log_with_attempts(&log, payload.as_ref(), &attempts)
            .await
            .with_context(|| format!("failed inserting request log `{}`", fixture.request_id))?;

        if let Some(overhead) = demo_mcp_token_overhead(fixture, request_log_id, occurred_at) {
            store
                .upsert_request_mcp_token_overhead(&overhead)
                .await
                .with_context(|| {
                    format!(
                        "failed inserting mcp token overhead `{}`",
                        fixture.request_id
                    )
                })?;
        }

        let model_id = model_ids
            .get(fixture.resolved_model_key)
            .copied()
            .ok_or_else(|| {
                anyhow::anyhow!("missing demo model `{}`", fixture.resolved_model_key)
            })?;
        let ledger = UsageLedgerRecord {
            usage_event_id: demo_usage_event_uuid(fixture.request_id),
            request_id: fixture.request_id.to_string(),
            ownership_scope_key,
            api_key_id: api_key.id,
            user_id,
            team_id,
            service_account_id: api_key.owner_service_account_id,
            actor_user_id: None,
            model_id: Some(model_id),
            provider_key: fixture.provider_key.to_string(),
            upstream_model: fixture.upstream_model.to_string(),
            prompt_tokens: fixture.prompt_tokens,
            completion_tokens: fixture.completion_tokens,
            total_tokens,
            provider_usage: if priced {
                json!({
                    "prompt_tokens": fixture.prompt_tokens,
                    "completion_tokens": fixture.completion_tokens,
                    "total_tokens": total_tokens,
                })
            } else {
                json!({"status_code": fixture.status_code, "error_code": fixture.error_code})
            },
            pricing_status: if priced {
                UsagePricingStatus::Priced
            } else {
                UsagePricingStatus::UsageMissing
            },
            unpriced_reason: if priced {
                None
            } else {
                Some("upstream_error".to_string())
            },
            pricing_row_id: None,
            pricing_provider_id: pricing_provider_id_for_demo_provider(fixture.provider_key)
                .map(str::to_string),
            pricing_model_id: Some(fixture.upstream_model.to_string()),
            pricing_source: if priced {
                Some("local_demo_seed".to_string())
            } else {
                None
            },
            pricing_source_etag: None,
            pricing_source_fetched_at: None,
            pricing_last_updated: if priced {
                Some(occurred_at.date().to_string())
            } else {
                None
            },
            input_cost_per_million_tokens: if priced {
                Some(Money4::from_scaled(1_000))
            } else {
                None
            },
            output_cost_per_million_tokens: if priced {
                Some(Money4::from_scaled(2_000))
            } else {
                None
            },
            computed_cost_usd: Money4::from_scaled(fixture.cost_scaled),
            occurred_at,
        };
        store
            .insert_usage_ledger_if_absent(&ledger)
            .await
            .with_context(|| format!("failed inserting usage ledger `{}`", fixture.request_id))?;
        store
            .touch_api_key_last_used(api_key.id)
            .await
            .with_context(|| {
                format!(
                    "failed updating last-used for `{}`",
                    fixture.api_key_public_id
                )
            })?;
    }

    Ok(raw_keys)
}

fn normalize_demo_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// Stable UUIDs keyed on the fixture request id, so reseeding replaces demo
/// rows in place instead of accumulating duplicates.
fn local_demo_uuid(kind: &str, key: &str) -> Uuid {
    Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("local_demo:{kind}:{key}").as_bytes(),
    )
}

fn demo_request_log_uuid(request_id: &str) -> Uuid {
    local_demo_uuid("request_log", request_id)
}

fn demo_usage_event_uuid(request_id: &str) -> Uuid {
    local_demo_uuid("usage_event", request_id)
}

fn demo_attempt_uuid(request_id: &str, attempt_number: i64) -> Uuid {
    local_demo_uuid("request_attempt", &format!("{request_id}:{attempt_number}"))
}

fn demo_route_uuid(model_key: &str, provider_key: &str) -> Uuid {
    local_demo_uuid("route", &format!("{model_key}:{provider_key}"))
}

fn demo_seed_metadata() -> Map<String, Value> {
    Map::from_iter([(
        "seed_source".to_string(),
        Value::String("local_demo_seed".to_string()),
    )])
}

fn demo_payload_record(
    fixture: &LocalDemoRequestFixture,
    request_log_id: Uuid,
    total_tokens: Option<i64>,
) -> Option<RequestLogPayloadRecord> {
    if fixture.payload_profile == DemoPayloadProfile::SummaryOnly {
        return None;
    }

    let priced = fixture.error_code.is_none();
    let (messages, completion) = match fixture.payload_profile {
        DemoPayloadProfile::Longform => (
            demo_longform_messages(fixture),
            demo_longform_completion(fixture),
        ),
        _ => (
            vec![
                json!({"role": "system", "content": "You are a local demo assistant."}),
                json!({"role": "user", "content": fixture.prompt}),
            ],
            fixture.completion.to_string(),
        ),
    };

    Some(RequestLogPayloadRecord {
        request_log_id,
        request_json: json!({
            "model": fixture.model_key,
            "messages": messages,
            "stream": fixture.payload_profile == DemoPayloadProfile::Streamed,
            "temperature": 0.2,
        }),
        response_json: if priced {
            json!({
                "id": format!("chatcmpl_{}", fixture.request_id),
                "object": "chat.completion",
                "model": fixture.resolved_model_key,
                "choices": [
                    {
                        "index": 0,
                        "finish_reason": "stop",
                        "message": {"role": "assistant", "content": completion}
                    }
                ],
                "usage": {
                    "prompt_tokens": fixture.prompt_tokens,
                    "completion_tokens": fixture.completion_tokens,
                    "total_tokens": total_tokens,
                }
            })
        } else {
            json!({
                "error": {
                    "code": fixture.error_code,
                    "message": fixture.completion,
                    "type": "upstream_error",
                }
            })
        },
    })
}

fn demo_longform_messages(fixture: &LocalDemoRequestFixture) -> Vec<Value> {
    let mut messages = vec![json!({
        "role": "system",
        "content": "You are a local demo assistant reviewing longform evaluation transcripts. \
                    Preserve the full context from every prior turn when weighing evidence.",
    })];
    for turn in 1..=4 {
        messages.push(json!({
            "role": "user",
            "content": format!(
                "Turn {turn}: {} Include routing metadata, token accounting, provider fallback \
                 notes, and cache behaviour for every experiment in the batch.",
                fixture.prompt
            ),
        }));
        messages.push(json!({
            "role": "assistant",
            "content": format!(
                "Turn {turn} review: the batch held steady across providers. {} Longer \
                 transcripts continue to show stable refusal behaviour, consistent grounding, \
                 and no token-accounting drift across retries.",
                fixture.completion
            ),
        }));
    }
    messages.push(json!({
        "role": "user",
        "content": format!("{} Produce the final consolidated synthesis.", fixture.prompt),
    }));
    messages
}

fn demo_longform_completion(fixture: &LocalDemoRequestFixture) -> String {
    (1..=6)
        .map(|section| {
            format!(
                "Section {section}: {} The consolidated synthesis tracks grounding quality, \
                 refusal stability, retry behaviour, provider fallback frequency, and cost \
                 drift for every experiment in the batch, with per-turn citations back to the \
                 transcript evidence gathered above.",
                fixture.completion
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Provider routing attempts for a demo request log: failed requests record a
/// retryable first attempt plus a terminal retry, secondary-provider requests
/// record the failed primary attempt before the fallback success, and
/// everything else records a single successful attempt.
fn demo_attempts(
    fixture: &LocalDemoRequestFixture,
    request_log_id: Uuid,
    occurred_at: OffsetDateTime,
    stream: bool,
) -> Vec<RequestAttemptRecord> {
    let final_latency = fixture.latency_ms;
    let final_started_at = occurred_at - time::Duration::milliseconds(final_latency);
    let final_attempt = RequestAttemptRecord {
        request_attempt_id: demo_attempt_uuid(fixture.request_id, 1),
        request_log_id,
        request_id: fixture.request_id.to_string(),
        attempt_number: 1,
        route_id: demo_route_uuid(fixture.model_key, fixture.provider_key),
        provider_key: fixture.provider_key.to_string(),
        upstream_model: fixture.upstream_model.to_string(),
        status: RequestAttemptStatus::Success,
        status_code: Some(fixture.status_code),
        error_code: None,
        error_detail: None,
        error_detail_truncated: false,
        retryable: false,
        terminal: true,
        produced_final_response: true,
        stream,
        started_at: final_started_at,
        completed_at: Some(occurred_at),
        latency_ms: Some(final_latency),
        metadata: demo_seed_metadata(),
    };

    let first_latency = (final_latency / 2).max(1);
    let first_completed_at = final_started_at - time::Duration::milliseconds(250);
    let first_started_at = first_completed_at - time::Duration::milliseconds(first_latency);

    if let Some(error_code) = fixture.error_code {
        let first = RequestAttemptRecord {
            status: RequestAttemptStatus::ProviderError,
            error_code: Some(error_code.to_string()),
            error_detail: Some(format!("attempt 1: {}", fixture.completion)),
            retryable: true,
            terminal: false,
            produced_final_response: false,
            started_at: first_started_at,
            completed_at: Some(first_completed_at),
            latency_ms: Some(first_latency),
            ..final_attempt.clone()
        };
        let second = RequestAttemptRecord {
            request_attempt_id: demo_attempt_uuid(fixture.request_id, 2),
            attempt_number: 2,
            status: RequestAttemptStatus::ProviderError,
            error_code: Some(error_code.to_string()),
            error_detail: Some(format!("attempt 2: {}", fixture.completion)),
            ..final_attempt
        };
        return vec![first, second];
    }

    if fixture.provider_key == "openai-secondary" {
        let first = RequestAttemptRecord {
            route_id: demo_route_uuid(fixture.model_key, "openai-prod"),
            provider_key: "openai-prod".to_string(),
            status: RequestAttemptStatus::ProviderError,
            status_code: Some(503),
            error_code: Some("upstream_http_503".to_string()),
            error_detail: Some(
                "primary openai route unavailable; retrying on the secondary provider".to_string(),
            ),
            retryable: true,
            terminal: false,
            produced_final_response: false,
            started_at: first_started_at,
            completed_at: Some(first_completed_at),
            latency_ms: Some(first_latency),
            ..final_attempt.clone()
        };
        let second = RequestAttemptRecord {
            request_attempt_id: demo_attempt_uuid(fixture.request_id, 2),
            attempt_number: 2,
            ..final_attempt
        };
        return vec![first, second];
    }

    vec![final_attempt]
}

fn demo_mcp_token_overhead(
    fixture: &LocalDemoRequestFixture,
    request_log_id: Uuid,
    occurred_at: OffsetDateTime,
) -> Option<RequestMcpTokenOverheadRecord> {
    let cardinality = usage::demo_tool_cardinality(fixture);
    let exposed_tool_count = cardinality.exposed_tool_count.filter(|count| *count > 0)?;
    let estimated_definition_tokens = exposed_tool_count * 115;
    let context_window_tokens = demo_context_window_tokens(fixture.upstream_model);
    Some(RequestMcpTokenOverheadRecord {
        request_id: fixture.request_id.to_string(),
        request_log_id: Some(request_log_id),
        model_key: Some(fixture.model_key.to_string()),
        provider_family: demo_provider_family(fixture.upstream_model).to_string(),
        model_or_encoding: fixture.upstream_model.to_string(),
        exposed_tool_count,
        estimated_definition_tokens,
        estimated_result_tokens: cardinality
            .invoked_tool_count
            .filter(|count| *count > 0)
            .map(|count| count * 180),
        estimator_source: McpTokenEstimateSource::LocalTokenizer,
        confidence: McpTokenEstimateConfidence::High,
        cache_hit_count: (exposed_tool_count - 2).max(0),
        cache_miss_count: exposed_tool_count.min(2),
        context_window_tokens: Some(context_window_tokens),
        context_window_percent_bps: Some(
            (estimated_definition_tokens * 10_000) / context_window_tokens,
        ),
        metadata: demo_seed_metadata(),
        created_at: occurred_at,
        updated_at: occurred_at,
    })
}

fn demo_provider_family(upstream_model: &str) -> &'static str {
    if upstream_model.contains("claude") {
        "anthropic"
    } else if upstream_model.contains("gemini") {
        "google"
    } else {
        "openai"
    }
}

fn demo_context_window_tokens(upstream_model: &str) -> i64 {
    if upstream_model.contains("claude") {
        200_000
    } else if upstream_model.contains("gemini") {
        1_048_576
    } else if upstream_model.contains("gpt-oss") {
        131_072
    } else {
        400_000
    }
}

fn pricing_provider_id_for_demo_provider(provider_key: &str) -> Option<&'static str> {
    match provider_key {
        "openai-prod" | "openai-secondary" => Some("openai"),
        "vertex-adc" => Some("google-vertex"),
        "vertex-claude" => Some("google-vertex-anthropic"),
        _ => None,
    }
}
