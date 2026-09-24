//! Vendored Artificial Analysis benchmark scores, sourced through OpenRouter's model list.
//!
//! The snapshot lives in the repository and is only changed by `sync_model_benchmarks`.
//! Syncs add or update entries and never delete them, so models that leave the fetched
//! window keep their last known scores.

use std::{
    collections::{BTreeMap, HashMap},
    sync::LazyLock,
    time::Duration,
};

use anyhow::Context;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

pub const BENCHMARK_ATTRIBUTION: &str = "Benchmark scores by Artificial Analysis (artificialanalysis.ai), retrieved via OpenRouter (openrouter.ai).";
pub const DEFAULT_BENCHMARK_SOURCE_URL: &str =
    "https://openrouter.ai/api/v1/models?sort=intelligence-high-to-low&limit=200";
pub const ARTIFICIAL_ANALYSIS_SOURCE: &str = "artificial_analysis";
const ARTIFICIAL_ANALYSIS_URL: &str = "https://artificialanalysis.ai";
const OPENROUTER_SOURCE: &str = "openrouter";
const OPENROUTER_MODEL_URL_PREFIX: &str = "https://openrouter.ai/";
/// OpenRouter marks routing variants such as `:batch`, `:free`, or `:thinking` with this separator.
/// Config bindings cannot reference variants, so they are never stored.
const VARIANT_SEPARATOR: char = ':';
const VENDORED_BENCHMARKS_JSON: &str = include_str!("../data/model_benchmarks.json");

static VENDORED_SNAPSHOT: LazyLock<BenchmarkSnapshot> = LazyLock::new(|| {
    serde_json::from_str(VENDORED_BENCHMARKS_JSON)
        .expect("vendored model benchmark snapshot should deserialize")
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkSnapshot {
    #[serde(rename = "_metadata")]
    pub metadata: BenchmarkSnapshotMetadata,
    pub models: BTreeMap<String, BenchmarkModelEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkSnapshotMetadata {
    pub attribution: String,
    pub benchmark_source: String,
    pub benchmark_source_url: String,
    pub retrieved_via: String,
    pub source_url: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BenchmarkModelEntry {
    pub name: String,
    pub canonical_slug: String,
    pub artificial_analysis: ArtificialAnalysisIndices,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct ArtificialAnalysisIndices {
    pub intelligence_index: Option<f64>,
    pub coding_index: Option<f64>,
    pub agentic_index: Option<f64>,
}

impl ArtificialAnalysisIndices {
    fn is_empty(&self) -> bool {
        self.intelligence_index.is_none()
            && self.coding_index.is_none()
            && self.agentic_index.is_none()
    }

    /// Keep a previously stored value when the new fetch omits it.
    fn merged_over(self, existing: Self) -> Self {
        Self {
            intelligence_index: self.intelligence_index.or(existing.intelligence_index),
            coding_index: self.coding_index.or(existing.coding_index),
            agentic_index: self.agentic_index.or(existing.agentic_index),
        }
    }

    fn validate(&self, model_id: &str) -> anyhow::Result<()> {
        for value in [
            self.intelligence_index,
            self.coding_index,
            self.agentic_index,
        ]
        .into_iter()
        .flatten()
        {
            if !value.is_finite() || !(0.0..=100.0).contains(&value) {
                anyhow::bail!(
                    "OpenRouter returned an out-of-range index `{value}` for `{model_id}`"
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkMatchKind {
    Explicit,
    Derived,
}

impl BenchmarkMatchKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Derived => "derived",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelBenchmarkScore {
    pub metric_key: &'static str,
    pub label: &'static str,
    pub value: f64,
    pub source: &'static str,
    pub source_model_id: String,
    pub source_url: String,
    pub match_kind: BenchmarkMatchKind,
    pub updated_at: OffsetDateTime,
}

/// Look up scores for an OpenRouter model ID in the vendored snapshot.
#[must_use]
pub fn benchmark_scores_for(
    source_model_id: &str,
    match_kind: BenchmarkMatchKind,
) -> Vec<ModelBenchmarkScore> {
    scores_from_snapshot(&VENDORED_SNAPSHOT, source_model_id, match_kind)
}

/// Return the first derived candidate present in the vendored snapshot.
#[must_use]
pub fn derive_benchmark_model_id(upstream_model: &str) -> Option<String> {
    derive_from_snapshot(&VENDORED_SNAPSHOT, upstream_model)
}

fn scores_from_snapshot(
    snapshot: &BenchmarkSnapshot,
    source_model_id: &str,
    match_kind: BenchmarkMatchKind,
) -> Vec<ModelBenchmarkScore> {
    let Some(entry) = snapshot.models.get(source_model_id) else {
        return Vec::new();
    };
    let indices = entry.artificial_analysis;
    [
        (
            "artificial_analysis_intelligence_index",
            "Artificial Analysis Intelligence Index",
            indices.intelligence_index,
        ),
        (
            "artificial_analysis_coding_index",
            "Artificial Analysis Coding Index",
            indices.coding_index,
        ),
        (
            "artificial_analysis_agentic_index",
            "Artificial Analysis Agentic Index",
            indices.agentic_index,
        ),
    ]
    .into_iter()
    .filter_map(|(metric_key, label, value)| {
        value.map(|value| ModelBenchmarkScore {
            metric_key,
            label,
            value,
            source: ARTIFICIAL_ANALYSIS_SOURCE,
            source_model_id: source_model_id.to_string(),
            source_url: format!("{OPENROUTER_MODEL_URL_PREFIX}{source_model_id}"),
            match_kind,
            updated_at: entry.updated_at,
        })
    })
    .collect()
}

fn derive_from_snapshot(snapshot: &BenchmarkSnapshot, upstream_model: &str) -> Option<String> {
    benchmark_model_id_candidates(upstream_model)
        .into_iter()
        .find(|candidate| snapshot.models.contains_key(candidate))
}

/// Normalize an upstream model ID into exact OpenRouter ID candidates.
///
/// This strips provider decorations (Bedrock ARNs and region prefixes, Vertex `@version`,
/// OpenRouter `:variant`), adds a publisher prefix for bare IDs, and tries a dotted version
/// form (`claude-sonnet-4-6` -> `claude-sonnet-4.6`). The unmodified name is tried before one
/// with a Bedrock `-vN` suffix removed, so IDs that genuinely end in `-v1` still match.
/// Candidates are only ever compared for equality, never by prefix.
pub(crate) fn benchmark_model_id_candidates(upstream_model: &str) -> Vec<String> {
    let mut model = upstream_model.trim();
    if model.starts_with("arn:") {
        model = model.rsplit('/').next().unwrap_or(model);
    }
    let model = model.split(['@', ':']).next().unwrap_or(model);
    let model = ["global.", "us.", "eu.", "apac.", "jp.", "au."]
        .iter()
        .find_map(|prefix| model.strip_prefix(prefix))
        .unwrap_or(model);

    let mut candidates = Vec::new();
    for model in [model, strip_bedrock_version_suffix(model)] {
        let qualified = qualify_model_id(model);
        let dotted = dot_version_separators(&qualified);
        for candidate in [qualified, dotted] {
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn qualify_model_id(model: &str) -> String {
    if model.contains('/') {
        model.to_string()
    } else if let Some((publisher, rest)) = model.split_once('.')
        && BEDROCK_PUBLISHERS.contains(&publisher)
    {
        format!("{}/{rest}", openrouter_publisher(publisher))
    } else if let Some(publisher) = inferred_publisher(model) {
        format!("{publisher}/{model}")
    } else {
        model.to_string()
    }
}

const BEDROCK_PUBLISHERS: [&str; 8] = [
    "anthropic",
    "openai",
    "meta",
    "mistral",
    "deepseek",
    "qwen",
    "moonshotai",
    "google",
];

fn openrouter_publisher(bedrock_publisher: &str) -> &str {
    match bedrock_publisher {
        "meta" => "meta-llama",
        "mistral" => "mistralai",
        other => other,
    }
}

fn inferred_publisher(model: &str) -> Option<&'static str> {
    const PREFIXES: [(&str, &str); 7] = [
        ("gpt-", "openai"),
        ("o1", "openai"),
        ("o3", "openai"),
        ("o4", "openai"),
        ("claude-", "anthropic"),
        ("gemini-", "google"),
        ("gemma-", "google"),
    ];
    PREFIXES
        .iter()
        .find(|(prefix, _)| model.starts_with(prefix))
        .map(|(_, publisher)| *publisher)
}

fn strip_bedrock_version_suffix(model: &str) -> &str {
    let Some((base, version)) = model.rsplit_once("-v") else {
        return model;
    };
    let digits = version.split(':').next().unwrap_or(version);
    if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
        base
    } else {
        model
    }
}

/// Replace `N-M` with `N.M` when both sides are short numeric version parts.
fn dot_version_separators(model: &str) -> String {
    let bytes = model.as_bytes();
    let mut output = String::with_capacity(model.len());
    for (index, character) in model.char_indices() {
        let is_version_dash = character == '-'
            && index > 0
            && bytes[index - 1].is_ascii_digit()
            && numeric_run_len(&bytes[index + 1..]).is_some_and(|len| {
                (1..=2).contains(&len)
                    && bytes
                        .get(index + 1 + len)
                        .is_none_or(|next| matches!(next, b'-' | b'/'))
            });
        output.push(if is_version_dash { '.' } else { character });
    }
    output
}

fn numeric_run_len(bytes: &[u8]) -> Option<usize> {
    let len = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    (len > 0).then_some(len)
}

/// Upsert fetched models into an existing snapshot. Entries are never removed.
///
/// Returns whether any entry changed. `_metadata.updated_at` only moves when data changes,
/// so a no-op sync leaves the file byte-for-byte identical.
pub fn merge_benchmark_models(
    snapshot: &mut BenchmarkSnapshot,
    fetched: Vec<OpenRouterBenchmarkModel>,
    source_url: &str,
    now: OffsetDateTime,
) -> bool {
    let mut changed = false;
    for model in fetched {
        let next = BenchmarkModelEntry {
            name: model.name,
            canonical_slug: model.canonical_slug,
            artificial_analysis: model.indices,
            updated_at: now,
        };
        match snapshot.models.get_mut(&model.id) {
            Some(existing) => {
                let merged = BenchmarkModelEntry {
                    artificial_analysis: next
                        .artificial_analysis
                        .merged_over(existing.artificial_analysis),
                    updated_at: existing.updated_at,
                    ..next
                };
                if &merged != existing {
                    *existing = BenchmarkModelEntry {
                        updated_at: now,
                        ..merged
                    };
                    changed = true;
                }
            }
            None => {
                snapshot.models.insert(model.id, next);
                changed = true;
            }
        }
    }
    let metadata = default_metadata(source_url, snapshot.metadata.updated_at);
    if changed || snapshot.metadata != metadata {
        snapshot.metadata = BenchmarkSnapshotMetadata {
            updated_at: now,
            ..metadata
        };
        changed = true;
    }
    changed
}

#[must_use]
pub fn empty_benchmark_snapshot(source_url: &str, now: OffsetDateTime) -> BenchmarkSnapshot {
    BenchmarkSnapshot {
        metadata: default_metadata(source_url, now),
        models: BTreeMap::new(),
    }
}

fn default_metadata(source_url: &str, updated_at: OffsetDateTime) -> BenchmarkSnapshotMetadata {
    BenchmarkSnapshotMetadata {
        attribution: BENCHMARK_ATTRIBUTION.to_string(),
        benchmark_source: ARTIFICIAL_ANALYSIS_SOURCE.to_string(),
        benchmark_source_url: ARTIFICIAL_ANALYSIS_URL.to_string(),
        retrieved_via: OPENROUTER_SOURCE.to_string(),
        source_url: source_url.to_string(),
        updated_at,
    }
}

pub fn benchmark_snapshot_to_pretty_json(snapshot: &BenchmarkSnapshot) -> anyhow::Result<String> {
    let mut json = serde_json::to_string_pretty(snapshot)
        .context("failed serializing model benchmark snapshot")?;
    json.push('\n');
    Ok(json)
}

/// One OpenRouter model with at least one Artificial Analysis index.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRouterBenchmarkModel {
    pub id: String,
    pub name: String,
    pub canonical_slug: String,
    pub indices: ArtificialAnalysisIndices,
}

/// The 200-model listing is around 1 MiB; anything far larger is not the expected payload.
const MAX_BENCHMARK_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

pub async fn fetch_openrouter_benchmark_models(
    source_url: &str,
) -> anyhow::Result<Vec<OpenRouterBenchmarkModel>> {
    let response = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed building benchmark HTTP client")?
        .get(source_url)
        .send()
        .await
        .with_context(|| format!("failed fetching benchmarks from `{source_url}`"))?;
    let status = response.status();
    if status != StatusCode::OK {
        anyhow::bail!("benchmark fetch returned HTTP {}", status.as_u16());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BENCHMARK_RESPONSE_BYTES as u64)
    {
        anyhow::bail!("benchmark response exceeds {MAX_BENCHMARK_RESPONSE_BYTES} bytes");
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("failed reading benchmark response")?
    {
        if body.len() + chunk.len() > MAX_BENCHMARK_RESPONSE_BYTES {
            anyhow::bail!("benchmark response exceeds {MAX_BENCHMARK_RESPONSE_BYTES} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    let body = String::from_utf8(body).context("benchmark response is not UTF-8")?;
    parse_openrouter_models(&body)
}

fn parse_openrouter_models(body: &str) -> anyhow::Result<Vec<OpenRouterBenchmarkModel>> {
    let response: OpenRouterModelsResponse =
        serde_json::from_str(body).context("OpenRouter model list was invalid")?;
    if response.data.is_empty() {
        anyhow::bail!("OpenRouter model list was empty");
    }

    let mut models = HashMap::new();
    for model in response.data {
        if model.id.contains(VARIANT_SEPARATOR) {
            continue;
        }
        if model.id.trim().is_empty() || model.id.trim() != model.id {
            anyhow::bail!("OpenRouter returned an invalid model id `{}`", model.id);
        }
        let indices = model
            .benchmarks
            .and_then(|benchmarks| benchmarks.artificial_analysis)
            .unwrap_or_default();
        if indices.is_empty() {
            continue;
        }
        indices.validate(&model.id)?;
        let id = model.id.clone();
        let parsed = OpenRouterBenchmarkModel {
            id: model.id,
            name: model.name,
            canonical_slug: model.canonical_slug,
            indices,
        };
        if models.insert(id.clone(), parsed).is_some() {
            anyhow::bail!("OpenRouter returned duplicate model id `{id}`");
        }
    }
    let mut models = models.into_values().collect::<Vec<_>>();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(models)
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    data: Vec<OpenRouterModel>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModel {
    id: String,
    name: String,
    canonical_slug: String,
    #[serde(default)]
    benchmarks: Option<OpenRouterBenchmarks>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterBenchmarks {
    #[serde(default)]
    artificial_analysis: Option<ArtificialAnalysisIndices>,
}

#[cfg(test)]
mod tests;
