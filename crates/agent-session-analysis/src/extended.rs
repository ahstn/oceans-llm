use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    GatewayOutcomeState, InferredObservation, SessionRequestFact, SessionUsageFact, TraceEvidence,
};
const MAX_DIAGNOSTIC_ITEMS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheTtl {
    FiveMinutes,
    ThirtyMinutes,
    OneHour,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheProfileRule {
    pub provider_key_contains: Option<String>,
    pub upstream_model_contains: Option<String>,
    pub minimum_cacheable_tokens: i64,
    pub default_ttl: CacheTtl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisMetricPolicy {
    pub token_metrics: bool,
    pub cache_metrics: bool,
    pub context_metrics: bool,
    pub tool_metrics: bool,
    pub skill_metrics: bool,
    pub reliability_metrics: bool,
    pub outcome_metrics: bool,
    pub finish_reason_metrics: bool,
}

impl Default for AnalysisMetricPolicy {
    fn default() -> Self {
        Self {
            token_metrics: true,
            cache_metrics: true,
            context_metrics: true,
            tool_metrics: true,
            skill_metrics: true,
            reliability_metrics: true,
            outcome_metrics: true,
            finish_reason_metrics: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedSkillFact {
    pub name: String,
    pub description_token_estimate: Option<u64>,
    pub body_token_estimate: Option<u64>,
    pub resource_token_estimate: Option<u64>,
    pub used: bool,
    pub abandoned: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedFileInteractionFact {
    pub opaque_file_id: String,
    pub operation: String,
    pub tool_name: Option<String>,
    pub succeeded: Option<bool>,
    pub error_signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestAttemptFact {
    pub request_id: String,
    pub attempt_number: i64,
    pub produced_final_response: bool,
    pub retryable: bool,
    pub status: String,
    pub status_code: Option<i64>,
    pub error_code: Option<String>,
    pub latency_ms: Option<i64>,
    pub provider_key: String,
    pub upstream_model: String,
    pub occurred_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInvocationFact {
    pub request_id: String,
    pub server_key: Option<String>,
    pub tool_key: String,
    pub status: String,
    pub error_code: Option<String>,
    pub latency_ms: Option<i64>,
    pub result_payload_truncated: bool,
    pub occurred_at_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolReliabilityItem {
    pub server_key: Option<String>,
    pub tool_key: String,
    pub invocation_count: u32,
    pub failed_count: u32,
    pub truncated_result_count: u32,
    pub latency_ms: i64,
    pub post_error_input_tokens: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolServerDiagnostics {
    pub server_key: String,
    pub exposed_tool_definitions: u32,
    pub invoked_tool_definitions: u32,
    pub invocation_count: u32,
    pub failed_count: u32,
    pub schema_token_estimate_per_request: u64,
    pub estimated_uncached_schema_cost_10000: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReliabilityDiagnostics {
    pub attempt_coverage_percent: u8,
    pub total_attempts: u32,
    pub wasted_attempts: u32,
    pub wasted_attempt_latency_ms: i64,
    pub wasted_attempt_cost_10000: Option<i64>,
    pub tool_invocations: u32,
    pub failed_tool_invocations: u32,
    pub truncated_tool_results: u32,
    pub attempts: Vec<RequestAttemptFact>,
    pub tools: Vec<ToolReliabilityItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SkillDiagnosticItem {
    pub name: String,
    pub available_request_count: u32,
    pub used_request_count: u32,
    pub abandoned_request_count: u32,
    pub description_token_estimate: Option<u64>,
    pub loaded_body_tokens: Option<u64>,
    pub loaded_resource_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SkillDiagnostics {
    pub instrumented_request_count: u32,
    pub available_skill_count: Option<u32>,
    pub used_skill_count: Option<u32>,
    pub unused_skill_count: Option<u32>,
    pub description_tokens_per_request: Option<u64>,
    pub loaded_body_tokens: Option<u64>,
    pub loaded_resource_tokens: Option<u64>,
    pub items: Vec<SkillDiagnosticItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OutcomeDiagnostics {
    pub file_signal_coverage_percent: u8,
    pub cost_per_file_touched_10000: Option<i64>,
    pub cost_per_successful_session_10000: Option<i64>,
    pub rework_ratio_basis_points: Option<i32>,
    pub verification_rate_basis_points: Option<i32>,
    pub zero_outcome: Option<bool>,
    pub repeated_file_interactions_suspected: Option<u32>,
    pub files_with_repeated_interactions_suspected: Option<u32>,
    pub failed_file_interactions: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FinishReasonItem {
    pub reason: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FinishReasonDiagnostics {
    pub instrumented_request_count: u32,
    pub length_limited_requests: u32,
    pub items: Vec<FinishReasonItem>,
}

#[must_use]
pub(crate) fn total_input_tokens(usage: &SessionUsageFact) -> Option<i64> {
    let mut total = usage.fresh_input_tokens?;
    for value in [usage.cache_read_tokens, usage.cache_creation_tokens]
        .into_iter()
        .flatten()
    {
        total = total.checked_add(value)?;
    }
    Some(total)
}

#[must_use]
pub(crate) fn visible_output_tokens(usage: &SessionUsageFact) -> Option<i64> {
    let output = usage.output_tokens?;
    match (usage.output_includes_reasoning, usage.reasoning_tokens) {
        (Some(true), Some(reasoning)) => output.checked_sub(reasoning),
        (Some(false), _) | (_, None) => Some(output),
        (None, Some(_)) => None,
    }
}

#[must_use]
pub(crate) fn cache_profile_for<'a>(
    usage: &SessionUsageFact,
    profiles: &'a [CacheProfileRule],
) -> Option<&'a CacheProfileRule> {
    let provider = usage.provider_key.as_deref()?.to_ascii_lowercase();
    let model = usage.upstream_model.as_deref()?.to_ascii_lowercase();
    profiles.iter().find(|profile| {
        profile
            .provider_key_contains
            .as_ref()
            .is_none_or(|pattern| provider.contains(&pattern.to_ascii_lowercase()))
            && profile
                .upstream_model_contains
                .as_ref()
                .is_none_or(|pattern| model.contains(&pattern.to_ascii_lowercase()))
    })
}
#[must_use]
pub(crate) fn cache_creation_tokens_for_ttl(
    requests: &[SessionRequestFact],
    profiles: &[CacheProfileRule],
    ttl: CacheTtl,
) -> Option<i64> {
    if requests.is_empty() {
        return None;
    }
    requests
        .iter()
        .map(|request| {
            let usage = request.usage.as_ref()?;
            let explicit = match ttl {
                CacheTtl::FiveMinutes => usage.cache_creation_5m_tokens,
                CacheTtl::ThirtyMinutes => usage.cache_creation_30m_tokens,
                CacheTtl::OneHour => usage.cache_creation_1h_tokens,
                CacheTtl::Unknown => None,
            };
            if explicit.is_some() {
                return explicit;
            }
            if usage.cache_creation_5m_tokens.is_some()
                || usage.cache_creation_30m_tokens.is_some()
                || usage.cache_creation_1h_tokens.is_some()
            {
                return None;
            }
            let creation = usage.cache_creation_tokens?;
            let profile = cache_profile_for(usage, profiles)?;
            if profile.default_ttl == CacheTtl::Unknown {
                return None;
            }
            Some(if profile.default_ttl == ttl {
                creation
            } else {
                0
            })
        })
        .try_fold(0_i64, |total, value| {
            value.and_then(|value| total.checked_add(value))
        })
}

#[must_use]
pub(crate) fn provider_model_switches(requests: &[SessionRequestFact]) -> u32 {
    let mut ordered = requests
        .iter()
        .filter_map(|request| {
            let usage = request.usage.as_ref()?;
            Some((
                request.occurred_at,
                request.ordinal,
                request.request_id.as_str(),
                usage.provider_key.as_deref()?,
                usage.upstream_model.as_deref()?,
            ))
        })
        .collect::<Vec<_>>();
    ordered
        .sort_unstable_by(|left, right| (left.0, left.1, left.2).cmp(&(right.0, right.1, right.2)));
    ordered
        .windows(2)
        .filter(|pair| pair[0].3 != pair[1].3 || pair[0].4 != pair[1].4)
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

#[must_use]
pub(crate) fn reasoning_config_switches(observations: &[InferredObservation]) -> Option<u32> {
    let mut values = observations
        .iter()
        .filter_map(|observation| {
            Some((
                observation.occurred_at,
                observation.facts.reasoning_config_hash.as_deref()?,
            ))
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by_key(|(occurred_at, _)| *occurred_at);
    Some(
        values
            .windows(2)
            .filter(|pair| pair[0].1 != pair[1].1)
            .count()
            .try_into()
            .unwrap_or(u32::MAX),
    )
}

#[must_use]
pub(crate) fn silent_cache_threshold_misses(
    requests: &[SessionRequestFact],
    observations: &[InferredObservation],
    profiles: &[CacheProfileRule],
) -> Option<u32> {
    let cache_requests = observations
        .iter()
        .filter_map(|observation| {
            observation
                .facts
                .cache_requested
                .map(|requested| (observation.source_request_id.as_str(), requested))
        })
        .collect::<BTreeMap<_, _>>();
    if cache_requests.is_empty() {
        return None;
    }
    Some(
        requests
            .iter()
            .filter(|request| {
                if !cache_requests
                    .get(request.request_id.as_str())
                    .copied()
                    .unwrap_or(false)
                {
                    return false;
                }
                let Some(usage) = request.usage.as_ref() else {
                    return false;
                };
                let Some(profile) = cache_profile_for(usage, profiles) else {
                    return false;
                };
                let no_cache_activity = usage.cache_read_tokens.unwrap_or_default() == 0
                    && usage.cache_creation_tokens.unwrap_or_default() == 0;
                no_cache_activity
                    && total_input_tokens(usage)
                        .is_some_and(|tokens| tokens < profile.minimum_cacheable_tokens)
            })
            .count()
            .try_into()
            .unwrap_or(u32::MAX),
    )
}

#[must_use]
pub(crate) fn skill_diagnostics(observations: &[InferredObservation]) -> SkillDiagnostics {
    let mut instrumented_requests = BTreeSet::new();
    let mut items = BTreeMap::<String, SkillDiagnosticItem>::new();
    for observation in observations {
        if observation.facts.supplied_skills.is_empty() {
            continue;
        }
        instrumented_requests.insert(observation.source_request_id.as_str());
        for skill in &observation.facts.supplied_skills {
            if !items.contains_key(&skill.name) && items.len() >= MAX_DIAGNOSTIC_ITEMS {
                continue;
            }
            let item = items
                .entry(skill.name.clone())
                .or_insert_with(|| SkillDiagnosticItem {
                    name: skill.name.clone(),
                    ..SkillDiagnosticItem::default()
                });
            item.available_request_count = item.available_request_count.saturating_add(1);
            item.description_token_estimate = max_option(
                item.description_token_estimate,
                skill.description_token_estimate,
            );
            if skill.used {
                item.used_request_count = item.used_request_count.saturating_add(1);
                item.loaded_body_tokens =
                    sum_optional(item.loaded_body_tokens, skill.body_token_estimate);
                item.loaded_resource_tokens =
                    sum_optional(item.loaded_resource_tokens, skill.resource_token_estimate);
            }
            if skill.abandoned == Some(true) {
                item.abandoned_request_count = item.abandoned_request_count.saturating_add(1);
            }
        }
    }
    if instrumented_requests.is_empty() {
        return SkillDiagnostics::default();
    }
    let items = items.into_values().collect::<Vec<_>>();
    let available_skill_count = items.len().try_into().unwrap_or(u32::MAX);
    let used_skill_count = items
        .iter()
        .filter(|item| item.used_request_count > 0)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let description_tokens_per_request = items.iter().try_fold(0_u64, |total, item| {
        item.description_token_estimate
            .and_then(|value| total.checked_add(value))
    });
    SkillDiagnostics {
        instrumented_request_count: instrumented_requests.len().try_into().unwrap_or(u32::MAX),
        available_skill_count: Some(available_skill_count),
        used_skill_count: Some(used_skill_count),
        unused_skill_count: Some(available_skill_count.saturating_sub(used_skill_count)),
        description_tokens_per_request,
        loaded_body_tokens: sum_item_tokens(&items, |item| item.loaded_body_tokens),
        loaded_resource_tokens: sum_item_tokens(&items, |item| item.loaded_resource_tokens),
        items,
    }
}

fn supplied_tool_servers(
    observations: &[InferredObservation],
) -> BTreeMap<String, BTreeSet<String>> {
    let mut servers_by_tool = BTreeMap::<String, BTreeSet<String>>::new();
    for observation in observations {
        for tool in &observation.facts.supplied_tools {
            if let Some(server) = &tool.server_key {
                servers_by_tool
                    .entry(tool.name.clone())
                    .or_default()
                    .insert(server.clone());
            }
        }
    }
    servers_by_tool
}

fn unique_tool_server(
    servers_by_tool: &BTreeMap<String, BTreeSet<String>>,
    tool_name: &str,
) -> Option<String> {
    let servers = servers_by_tool.get(tool_name)?;
    (servers.len() == 1).then(|| servers.first().expect("one server is present").clone())
}

#[must_use]
pub(crate) fn reliability_diagnostics(
    requests: &[SessionRequestFact],
    evidence: &TraceEvidence,
    observations: &[InferredObservation],
) -> ReliabilityDiagnostics {
    let attempts = requests
        .iter()
        .flat_map(|request| request.attempts.iter().cloned())
        .collect::<Vec<_>>();
    let requests_with_attempts = requests
        .iter()
        .filter(|request| !request.attempts.is_empty())
        .count();
    let wasted_attempts = attempts
        .iter()
        .filter(|attempt| !attempt.produced_final_response)
        .collect::<Vec<_>>();
    let servers_by_tool = supplied_tool_servers(observations);
    let direct_calls = evidence
        .tool_invocations
        .iter()
        .map(|invocation| (invocation.request_id.as_str(), invocation.tool_key.as_str()))
        .collect::<BTreeSet<_>>();
    let mut tools = BTreeMap::<(Option<String>, String), ToolReliabilityItem>::new();
    for invocation in &evidence.tool_invocations {
        let key = (invocation.server_key.clone(), invocation.tool_key.clone());
        if !tools.contains_key(&key) && tools.len() >= MAX_DIAGNOSTIC_ITEMS {
            continue;
        }
        let item = tools.entry(key).or_insert_with(|| ToolReliabilityItem {
            server_key: invocation.server_key.clone(),
            tool_key: invocation.tool_key.clone(),
            ..ToolReliabilityItem::default()
        });
        item.invocation_count = item.invocation_count.saturating_add(1);
        item.latency_ms = item
            .latency_ms
            .saturating_add(invocation.latency_ms.unwrap_or_default().max(0));
        if invocation.status != "succeeded" {
            item.failed_count = item.failed_count.saturating_add(1);
            item.post_error_input_tokens = sum_optional_i64(
                item.post_error_input_tokens,
                next_request_input_tokens(requests, invocation.occurred_at_unix_ms),
            );
        }
        if invocation.result_payload_truncated {
            item.truncated_result_count = item.truncated_result_count.saturating_add(1);
        }
    }
    for observation in observations {
        let Some(tool_name) = observation.facts.tool_name.as_ref() else {
            continue;
        };
        if direct_calls.contains(&(observation.source_request_id.as_str(), tool_name.as_str())) {
            continue;
        }
        let server_key = unique_tool_server(&servers_by_tool, tool_name);
        let key = (server_key.clone(), tool_name.clone());
        if !tools.contains_key(&key) && tools.len() >= MAX_DIAGNOSTIC_ITEMS {
            continue;
        }
        let item = tools.entry(key).or_insert_with(|| ToolReliabilityItem {
            server_key,
            tool_key: tool_name.clone(),
            ..ToolReliabilityItem::default()
        });
        item.invocation_count = item.invocation_count.saturating_add(1);
    }
    for observation in observations {
        for file in &observation.facts.file_interactions {
            let Some(tool_name) = file.tool_name.as_ref() else {
                continue;
            };
            let key = (None, tool_name.clone());
            if !tools.contains_key(&key) && tools.len() >= MAX_DIAGNOSTIC_ITEMS {
                continue;
            }
            let item = tools.entry(key).or_insert_with(|| ToolReliabilityItem {
                server_key: None,
                tool_key: tool_name.clone(),
                ..ToolReliabilityItem::default()
            });
            item.invocation_count = item.invocation_count.saturating_add(1);
            if file.succeeded == Some(false) {
                item.failed_count = item.failed_count.saturating_add(1);
            }
        }
    }
    let tools = tools.into_values().collect::<Vec<_>>();
    ReliabilityDiagnostics {
        attempt_coverage_percent: percent(requests_with_attempts, requests.len()),
        total_attempts: attempts.len().try_into().unwrap_or(u32::MAX),
        wasted_attempts: wasted_attempts.len().try_into().unwrap_or(u32::MAX),
        wasted_attempt_latency_ms: wasted_attempts.iter().fold(0_i64, |total, attempt| {
            total.saturating_add(attempt.latency_ms.unwrap_or_default().max(0))
        }),
        // Request attempts do not yet carry per-attempt provider usage. Do not allocate the
        // final request cost across attempts because that would present an estimate as spend.
        wasted_attempt_cost_10000: None,
        tool_invocations: tools.iter().fold(0_u32, |total, item| {
            total.saturating_add(item.invocation_count)
        }),
        failed_tool_invocations: tools
            .iter()
            .fold(0_u32, |total, item| total.saturating_add(item.failed_count)),
        truncated_tool_results: tools.iter().fold(0_u32, |total, item| {
            total.saturating_add(item.truncated_result_count)
        }),
        attempts,
        tools,
    }
}

#[must_use]
pub(crate) fn tool_server_diagnostics(
    observations: &[InferredObservation],
    invocations: &[ToolInvocationFact],
    requests: &[SessionRequestFact],
) -> Vec<ToolServerDiagnostics> {
    #[derive(Default)]
    struct ServerAccumulator {
        exposed: BTreeSet<String>,
        invoked: BTreeSet<String>,
        invocation_count: u32,
        failed_count: u32,
        schema_tokens_by_request: BTreeMap<String, BTreeMap<String, u64>>,
    }

    let servers_by_tool = supplied_tool_servers(observations);
    let direct_calls = invocations
        .iter()
        .map(|invocation| (invocation.request_id.as_str(), invocation.tool_key.as_str()))
        .collect::<BTreeSet<_>>();
    let mut servers = BTreeMap::<String, ServerAccumulator>::new();
    for observation in observations {
        for tool in &observation.facts.supplied_tools {
            let Some(server) = tool.server_key.as_ref() else {
                continue;
            };
            if !servers.contains_key(server) && servers.len() >= MAX_DIAGNOSTIC_ITEMS {
                continue;
            }
            let accumulator = servers.entry(server.clone()).or_default();
            accumulator.exposed.insert(tool.name.clone());
            accumulator
                .schema_tokens_by_request
                .entry(observation.source_request_id.clone())
                .or_default()
                .entry(tool.name.clone())
                .and_modify(|value| *value = (*value).max(tool.token_estimate))
                .or_insert(tool.token_estimate);
        }
    }
    for invocation in invocations {
        let Some(server) = invocation.server_key.as_ref() else {
            continue;
        };
        if !servers.contains_key(server) && servers.len() >= MAX_DIAGNOSTIC_ITEMS {
            continue;
        }
        let accumulator = servers.entry(server.clone()).or_default();
        accumulator.invoked.insert(invocation.tool_key.clone());
        accumulator.invocation_count = accumulator.invocation_count.saturating_add(1);
        if invocation.status != "succeeded" {
            accumulator.failed_count = accumulator.failed_count.saturating_add(1);
        }
    }
    for observation in observations {
        let Some(tool_name) = observation.facts.tool_name.as_ref() else {
            continue;
        };
        if direct_calls.contains(&(observation.source_request_id.as_str(), tool_name.as_str())) {
            continue;
        }
        let Some(server) = unique_tool_server(&servers_by_tool, tool_name) else {
            continue;
        };
        if !servers.contains_key(&server) && servers.len() >= MAX_DIAGNOSTIC_ITEMS {
            continue;
        }
        let accumulator = servers.entry(server).or_default();
        accumulator.invoked.insert(tool_name.clone());
        accumulator.invocation_count = accumulator.invocation_count.saturating_add(1);
    }
    let rate = aggregate_fresh_input_rate(requests);
    servers
        .into_iter()
        .map(|(server_key, value)| {
            let shipped_tokens = value
                .schema_tokens_by_request
                .values()
                .flat_map(|tools| tools.values())
                .copied()
                .fold(0_u64, u64::saturating_add);
            let schema_tokens_per_request = u64::try_from(value.schema_tokens_by_request.len())
                .ok()
                .filter(|request_count| *request_count > 0)
                .map_or(0, |request_count| shipped_tokens / request_count);
            ToolServerDiagnostics {
                server_key,
                exposed_tool_definitions: value.exposed.len().try_into().unwrap_or(u32::MAX),
                invoked_tool_definitions: value.invoked.len().try_into().unwrap_or(u32::MAX),
                invocation_count: value.invocation_count,
                failed_count: value.failed_count,
                schema_token_estimate_per_request: schema_tokens_per_request,
                estimated_uncached_schema_cost_10000: rate.and_then(|(cost, tokens)| {
                    i64::try_from(shipped_tokens)
                        .ok()?
                        .checked_mul(cost)?
                        .checked_div(tokens)
                }),
            }
        })
        .collect()
}

pub(crate) struct OutcomeMetricInputs {
    pub gateway_outcome: GatewayOutcomeState,
    pub actual_cost_10000: Option<i64>,
    pub file_writes: u32,
    pub unique_files: u32,
    pub rework: u32,
    pub verification: u32,
}

#[must_use]
pub(crate) fn outcome_diagnostics(
    requests: &[SessionRequestFact],
    observations: &[InferredObservation],
    evidence: &TraceEvidence,
    inputs: OutcomeMetricInputs,
) -> OutcomeDiagnostics {
    let instrumented_file_requests = observations
        .iter()
        .filter(|observation| !observation.facts.file_interactions.is_empty())
        .map(|observation| observation.source_request_id.as_str())
        .collect::<BTreeSet<_>>();
    let untruncated_payload_count = evidence
        .response_payload_count
        .saturating_sub(evidence.truncated_payload_count);
    let file_signal_count = usize::try_from(untruncated_payload_count)
        .unwrap_or(usize::MAX)
        .max(instrumented_file_requests.len())
        .min(requests.len());
    let file_signal_coverage_percent = percent(file_signal_count, requests.len());
    let fully_measured = !requests.is_empty() && file_signal_coverage_percent == 100;
    let mut per_file = BTreeMap::<&str, (u32, u32)>::new();
    let mut failed_file_interactions = 0_u32;
    for observation in observations {
        for interaction in &observation.facts.file_interactions {
            if !per_file.contains_key(interaction.opaque_file_id.as_str())
                && per_file.len() >= MAX_DIAGNOSTIC_ITEMS
            {
                continue;
            }
            let counts = per_file
                .entry(interaction.opaque_file_id.as_str())
                .or_insert((0, 0));
            counts.0 = counts.0.saturating_add(1);
            if interaction.succeeded == Some(false) {
                counts.1 = counts.1.saturating_add(1);
                failed_file_interactions = failed_file_interactions.saturating_add(1);
            }
        }
    }
    let repeated_file_interactions_suspected = (!per_file.is_empty()).then(|| {
        per_file.values().fold(0_u32, |total, (count, _)| {
            total.saturating_add(count.saturating_sub(1))
        })
    });
    let files_with_repeated_interactions_suspected = (!per_file.is_empty()).then(|| {
        per_file
            .values()
            .filter(|(count, _)| *count > 1)
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    });
    OutcomeDiagnostics {
        file_signal_coverage_percent,
        cost_per_file_touched_10000: inputs.actual_cost_10000.and_then(|cost| {
            (inputs.unique_files > 0)
                .then(|| cost.checked_div(i64::from(inputs.unique_files)))
                .flatten()
        }),
        cost_per_successful_session_10000: (inputs.gateway_outcome
            == GatewayOutcomeState::Succeeded)
            .then_some(inputs.actual_cost_10000)
            .flatten(),
        rework_ratio_basis_points: (fully_measured && inputs.file_writes > 0)
            .then(|| ratio_basis_points(i64::from(inputs.rework), i64::from(inputs.file_writes)))
            .flatten(),
        verification_rate_basis_points: (fully_measured && inputs.file_writes > 0)
            .then(|| {
                ratio_basis_points(
                    i64::from(inputs.verification),
                    i64::from(inputs.file_writes),
                )
            })
            .flatten(),
        zero_outcome: fully_measured.then_some(inputs.file_writes == 0 && inputs.verification == 0),
        repeated_file_interactions_suspected,
        files_with_repeated_interactions_suspected,
        failed_file_interactions: (!per_file.is_empty()).then_some(failed_file_interactions),
    }
}

#[must_use]
pub(crate) fn finish_reason_diagnostics(
    observations: &[InferredObservation],
) -> FinishReasonDiagnostics {
    let mut requests = BTreeSet::new();
    let mut reasons = BTreeMap::<String, u32>::new();
    for observation in observations {
        let reason = observation
            .facts
            .incomplete_reason
            .as_ref()
            .or(observation.facts.finish_reason.as_ref());
        let Some(reason) = reason else {
            continue;
        };
        requests.insert(observation.source_request_id.as_str());
        if !reasons.contains_key(reason) && reasons.len() >= MAX_DIAGNOSTIC_ITEMS {
            continue;
        }
        reasons
            .entry(reason.clone())
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
    }
    FinishReasonDiagnostics {
        instrumented_request_count: requests.len().try_into().unwrap_or(u32::MAX),
        length_limited_requests: reasons
            .iter()
            .filter(|(reason, _)| is_length_reason(reason))
            .fold(0_u32, |total, (_, count)| total.saturating_add(*count)),
        items: reasons
            .into_iter()
            .map(|(reason, count)| FinishReasonItem { reason, count })
            .collect(),
    }
}

fn unix_timestamp_millis(value: time::OffsetDateTime) -> i64 {
    i64::try_from(value.unix_timestamp_nanos().div_euclid(1_000_000)).unwrap_or_else(|_| {
        if value < time::OffsetDateTime::UNIX_EPOCH {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}

fn next_request_input_tokens(
    requests: &[SessionRequestFact],
    occurred_at_unix_ms: i64,
) -> Option<i64> {
    requests
        .iter()
        .filter(|request| unix_timestamp_millis(request.occurred_at) > occurred_at_unix_ms)
        .min_by_key(|request| request.occurred_at)
        .and_then(|request| request.usage.as_ref())
        .and_then(total_input_tokens)
}

fn aggregate_fresh_input_rate(requests: &[SessionRequestFact]) -> Option<(i64, i64)> {
    requests
        .iter()
        .try_fold((0_i64, 0_i64), |(cost, tokens), request| {
            let usage = request.usage.as_ref()?;
            Some((
                cost.checked_add(usage.fresh_input_cost_10000?)?,
                tokens.checked_add(usage.fresh_input_tokens?)?,
            ))
        })
        .filter(|(_, tokens)| *tokens > 0)
}

fn sum_item_tokens(
    items: &[SkillDiagnosticItem],
    field: fn(&SkillDiagnosticItem) -> Option<u64>,
) -> Option<u64> {
    items.iter().try_fold(0_u64, |total, item| {
        field(item).and_then(|value| total.checked_add(value))
    })
}

fn max_option(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn sum_optional(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => left.checked_add(right),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn sum_optional_i64(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => left.checked_add(right),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn ratio_basis_points(numerator: i64, denominator: i64) -> Option<i32> {
    if denominator <= 0 {
        return None;
    }
    let ratio = i128::from(numerator) * 10_000 / i128::from(denominator);
    Some(ratio.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32)
}

fn percent(observed: usize, total: usize) -> u8 {
    if total == 0 {
        return 0;
    }
    u8::try_from((observed.saturating_mul(100) + total / 2) / total)
        .unwrap_or(100)
        .min(100)
}

fn is_length_reason(reason: &str) -> bool {
    matches!(
        reason.to_ascii_lowercase().as_str(),
        "length" | "max_tokens" | "max_output_tokens" | "model_length"
    )
}
