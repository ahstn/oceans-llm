//! Long-tail usage history for the profile page: months of daily activity on a few personal
//! keys, spread across models and agent harnesses. Rows are deterministic so reseeding replaces
//! them in place.
use std::collections::HashMap;

use anyhow::Context;
use gateway_core::{
    ApiKeyRecord, BudgetRepository, Money4, RequestLogRecord, RequestLogRepository, RequestTags,
    UsageLedgerRecord, UsagePricingStatus,
};
use gateway_store::AnyStore;
use serde_json::{Map, Value, json};
use time::{Duration, OffsetDateTime, Time};
use uuid::Uuid;

use super::usage::demo_seed_hash;
use super::{demo_request_log_uuid, demo_usage_event_uuid, pricing_provider_id_for_demo_provider};

/// Days of history before today. Today is left to the hand-written fixtures.
const HISTORY_DAYS: i64 = 330;

pub(super) struct HistoryProfile {
    pub api_key_public_id: &'static str,
    /// Busiest-day request count; quieter days scale down from it.
    pub peak_requests: u64,
    /// `(model_key, provider_key, upstream_model, weight)`.
    pub models: &'static [(&'static str, &'static str, &'static str, u64)],
    /// `(harness_key, harness_label, user_agent, weight)`.
    pub harnesses: &'static [(&'static str, &'static str, &'static str, u64)],
}

pub(super) const HISTORY_PROFILES: &[HistoryProfile] = &[
    HistoryProfile {
        api_key_public_id: "locdemoalice1",
        peak_requests: 9,
        models: &[
            (
                "claude-sonnet",
                "vertex-claude",
                "anthropic/claude-sonnet-4-6",
                6,
            ),
            ("openai-fast-v2", "openai-prod", "gpt-5", 5),
            (
                "claude-sonnet-4.6",
                "bedrock-us-east-1",
                "claude-sonnet-4-6",
                3,
            ),
            (
                "gemini-pro-preview",
                "vertex-adc",
                "google/gemini-3.1-pro-preview",
                2,
            ),
            (
                "claude-opus",
                "vertex-claude",
                "anthropic/claude-opus-4-7",
                1,
            ),
            ("gpt-oss-120b", "bedrock-us-east-1", "gpt-oss-120b", 1),
        ],
        harnesses: &[
            (
                "claude_code",
                "Claude Code",
                "claude-code/2.1.0 (local demo)",
                6,
            ),
            ("codex", "Codex", "codex/1.0.0 (local demo)", 4),
            ("opencode", "Opencode", "opencode/1.0.0 (local demo)", 2),
            ("mastra", "Mastra", "mastra/1.0.0 (local demo)", 1),
        ],
    },
    HistoryProfile {
        api_key_public_id: "locdemoben1",
        peak_requests: 5,
        models: &[
            (
                "gemini-pro-preview",
                "vertex-adc",
                "google/gemini-3.1-pro-preview",
                4,
            ),
            ("openai-fast-v2", "openai-prod", "gpt-5", 3),
        ],
        harnesses: &[
            ("codex", "Codex", "codex/1.0.0 (local demo)", 3),
            ("oh_my_pi", "Oh My Pi", "omp/1.0.0 (local demo)", 1),
        ],
    },
];

/// One synthetic request in the history.
struct HistoryRequest {
    request_id: String,
    occurred_at: OffsetDateTime,
    model: (&'static str, &'static str, &'static str),
    harness: (&'static str, &'static str, &'static str),
    prompt_tokens: i64,
    completion_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    cost_scaled: i64,
}

/// Every request id the history can produce, for deleting prior runs.
pub(super) fn history_request_ids() -> Vec<String> {
    HISTORY_PROFILES
        .iter()
        .flat_map(|profile| {
            (1..=HISTORY_DAYS).flat_map(move |days_ago| {
                (0..profile.peak_requests)
                    .map(move |index| request_id(profile.api_key_public_id, days_ago, index))
            })
        })
        .collect()
}

fn request_id(public_id: &str, days_ago: i64, index: u64) -> String {
    format!("demo-hist-{public_id}-{days_ago:03}-{index}")
}

/// Requests for one day: weekdays are busier, some days are skipped, and activity ramps up
/// towards the present so the heatmap shows a trend.
fn requests_for_day(
    profile: &HistoryProfile,
    days_ago: i64,
    today: OffsetDateTime,
) -> Vec<HistoryRequest> {
    let day = today - Duration::days(days_ago);
    let seed = demo_seed_hash(&format!("{}:{days_ago}", profile.api_key_public_id));
    let weekend = matches!(
        day.weekday(),
        time::Weekday::Saturday | time::Weekday::Sunday
    );
    // Roughly one weekday in six and most weekends are quiet.
    if seed.is_multiple_of(6) || (weekend && !seed.is_multiple_of(3)) {
        return Vec::new();
    }
    let recency = (HISTORY_DAYS - days_ago) as u64 * 100 / HISTORY_DAYS as u64;
    let scale = 30 + recency * 70 / 100;
    let count = (1 + (seed / 7) % profile.peak_requests) * scale / 100;

    (0..count.max(1))
        .map(|index| {
            let request_seed = demo_seed_hash(&format!("{seed}:{index}"));
            let model = weighted(profile.models, request_seed);
            let harness = weighted(profile.harnesses, request_seed / 13);
            // Agent harness turns carry large, mostly re-sent contexts.
            let prompt_tokens = 10_000 + (request_seed % 110_000) as i64;
            let completion_tokens = 500 + ((request_seed / 97) % 6_000) as i64;
            let read_percent = 20 + ((request_seed / 31) % 60) as i64;
            let cache_read_tokens = prompt_tokens * read_percent / 100;
            let cache_write_tokens =
                (prompt_tokens * 5 / 100).min(prompt_tokens - cache_read_tokens);
            let fresh_tokens = prompt_tokens - cache_read_tokens;
            // $3/M fresh input, $0.30/M cached, $15/M output, in Money4 units.
            let cost_scaled =
                (fresh_tokens * 300 + cache_read_tokens * 30 + completion_tokens * 1_500) / 10_000;
            let minutes = 8 * 60 + ((request_seed / 7) % (10 * 60)) as i64;
            HistoryRequest {
                request_id: request_id(profile.api_key_public_id, days_ago, index),
                occurred_at: day.replace_time(Time::MIDNIGHT) + Duration::minutes(minutes),
                model: (model.0, model.1, model.2),
                harness: (harness.0, harness.1, harness.2),
                prompt_tokens,
                completion_tokens,
                cache_read_tokens,
                cache_write_tokens,
                cost_scaled: cost_scaled.max(10),
            }
        })
        .collect()
}

fn weighted<T: Copy>(options: &[(T, T, T, u64)], seed: u64) -> (T, T, T, u64) {
    let total: u64 = options.iter().map(|option| option.3).sum();
    let mut pick = seed % total;
    for option in options {
        if pick < option.3 {
            return *option;
        }
        pick -= option.3;
    }
    options[0]
}

pub(super) async fn seed_demo_usage_history(
    store: &AnyStore,
    api_keys: &HashMap<&'static str, ApiKeyRecord>,
    team_for_key: &HashMap<&'static str, Option<Uuid>>,
    model_ids: &HashMap<&'static str, Uuid>,
    now: OffsetDateTime,
) -> anyhow::Result<()> {
    for profile in HISTORY_PROFILES {
        let api_key = api_keys
            .get(profile.api_key_public_id)
            .ok_or_else(|| anyhow::anyhow!("missing demo key `{}`", profile.api_key_public_id))?;
        let team_id = team_for_key
            .get(profile.api_key_public_id)
            .copied()
            .flatten();
        for days_ago in 1..=HISTORY_DAYS {
            for request in requests_for_day(profile, days_ago, now) {
                insert_history_request(store, api_key, team_id, model_ids, &request)
                    .await
                    .with_context(|| format!("failed inserting `{}`", request.request_id))?;
            }
        }
    }
    Ok(())
}

async fn insert_history_request(
    store: &AnyStore,
    api_key: &ApiKeyRecord,
    team_id: Option<Uuid>,
    model_ids: &HashMap<&'static str, Uuid>,
    request: &HistoryRequest,
) -> anyhow::Result<()> {
    let (model_key, provider_key, upstream_model) = request.model;
    let (harness_key, harness_label, user_agent) = request.harness;
    let total_tokens = request.prompt_tokens + request.completion_tokens;
    let user_id = api_key.owner_user_id;
    let log = RequestLogRecord {
        request_log_id: demo_request_log_uuid(&request.request_id),
        request_id: request.request_id.clone(),
        api_key_id: api_key.id,
        user_id,
        team_id,
        service_account_id: None,
        model_key: model_key.to_string(),
        resolved_model_key: model_key.to_string(),
        provider_key: provider_key.to_string(),
        status_code: Some(200),
        latency_ms: Some(800 + request.completion_tokens / 2),
        prompt_tokens: Some(request.prompt_tokens),
        completion_tokens: Some(request.completion_tokens),
        total_tokens: Some(total_tokens),
        error_code: None,
        has_payload: false,
        request_payload_truncated: false,
        response_payload_truncated: false,
        request_tags: RequestTags::default(),
        tool_cardinality: Default::default(),
        user_agent_raw: Some(user_agent.to_string()),
        agent_harness_key: harness_key.to_string(),
        agent_harness_label: harness_label.to_string(),
        metadata: Map::from_iter([(
            "seed_source".to_string(),
            Value::String("local_demo_seed_history".to_string()),
        )]),
        occurred_at: request.occurred_at,
    };
    store
        .insert_request_log_with_attempts(&log, None, &[])
        .await?;

    let uncached = request.prompt_tokens - request.cache_read_tokens - request.cache_write_tokens;
    let ledger = UsageLedgerRecord {
        usage_event_id: demo_usage_event_uuid(&request.request_id),
        request_id: request.request_id.clone(),
        ownership_scope_key: format!(
            "user:{}",
            user_id.ok_or_else(|| anyhow::anyhow!("history key must be user-owned"))?
        ),
        api_key_id: api_key.id,
        user_id,
        team_id,
        service_account_id: None,
        actor_user_id: None,
        model_id: model_ids.get(model_key).copied(),
        model_route_id: None,
        provider_key: provider_key.to_string(),
        upstream_model: upstream_model.to_string(),
        prompt_tokens: Some(request.prompt_tokens),
        uncached_input_tokens: Some(uncached),
        cache_read_tokens: Some(request.cache_read_tokens),
        cache_write_tokens: Some(request.cache_write_tokens),
        completion_tokens: Some(request.completion_tokens),
        total_tokens: Some(total_tokens),
        provider_usage: json!({
            "prompt_tokens": request.prompt_tokens,
            "completion_tokens": request.completion_tokens,
            "total_tokens": total_tokens,
            "prompt_tokens_details": {
                "cached_tokens": request.cache_read_tokens,
                "cache_write_tokens": request.cache_write_tokens,
            },
        }),
        pricing_status: UsagePricingStatus::Priced,
        unpriced_reason: None,
        pricing_row_id: None,
        pricing_provider_id: pricing_provider_id_for_demo_provider(provider_key)
            .map(str::to_string),
        pricing_model_id: Some(upstream_model.to_string()),
        pricing_source: Some("local_demo_seed".to_string()),
        pricing_source_etag: None,
        pricing_source_fetched_at: None,
        pricing_last_updated: Some(request.occurred_at.date().to_string()),
        input_cost_per_million_tokens: Some(Money4::from_scaled(30_000)),
        output_cost_per_million_tokens: Some(Money4::from_scaled(150_000)),
        cache_read_cost_per_million_tokens: Some(Money4::from_scaled(3_000)),
        cache_write_cost_per_million_tokens: None,
        computed_cost_usd: Money4::from_scaled(request.cost_scaled),
        occurred_at: request.occurred_at,
    };
    store.insert_usage_ledger_if_absent(&ledger).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn history_is_deterministic_and_covers_every_generated_id() {
        let today = datetime!(2026-09-25 12:00 UTC);
        let ids = history_request_ids();
        let profile = &HISTORY_PROFILES[0];
        let mut generated = 0;
        for days_ago in 1..=HISTORY_DAYS {
            let first = requests_for_day(profile, days_ago, today);
            let second = requests_for_day(profile, days_ago, today);
            assert_eq!(first.len(), second.len());
            for request in &first {
                assert!(ids.contains(&request.request_id), "{}", request.request_id);
                assert!(request.occurred_at < today);
                assert!(
                    request.cache_read_tokens + request.cache_write_tokens <= request.prompt_tokens
                );
            }
            generated += first.len();
        }
        // Enough active days for a readable heatmap, with some quiet ones.
        assert!(generated > 600, "generated {generated}");
        assert!(generated < 330 * 9);
    }
}
