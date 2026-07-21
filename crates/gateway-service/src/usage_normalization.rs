use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const TOKEN_USAGE_SEMANTICS_VERSION: &str = "usage-v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenFieldAvailability {
    Reported,
    Derived,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageCoverage {
    pub fresh_input: TokenFieldAvailability,
    pub cache_read: TokenFieldAvailability,
    pub cache_creation: TokenFieldAvailability,
    pub output: TokenFieldAvailability,
    pub reasoning: TokenFieldAvailability,
    pub provider_total: TokenFieldAvailability,
}

impl Default for UsageCoverage {
    fn default() -> Self {
        Self {
            fresh_input: TokenFieldAvailability::Unavailable,
            cache_read: TokenFieldAvailability::Unavailable,
            cache_creation: TokenFieldAvailability::Unavailable,
            output: TokenFieldAvailability::Unavailable,
            reasoning: TokenFieldAvailability::Unavailable,
            provider_total: TokenFieldAvailability::Unavailable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsageSemantics {
    pub version: String,
    pub source_family: String,
    pub input_includes_cache_read: Option<bool>,
    pub input_includes_cache_creation: Option<bool>,
    pub input_buckets_non_overlapping: Option<bool>,
    pub output_includes_reasoning: Option<bool>,
    pub totals_reconcilable_by_addition: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedTokenUsage {
    pub fresh_input_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub provider_total_tokens: Option<i64>,
    pub semantics: TokenUsageSemantics,
    pub coverage: UsageCoverage,
}

impl NormalizedTokenUsage {
    #[must_use]
    pub fn has_usage(&self) -> bool {
        self.fresh_input_tokens.is_some()
            || self.cache_read_tokens.is_some()
            || self.cache_creation_tokens.is_some()
            || self.output_tokens.is_some()
            || self.reasoning_tokens.is_some()
            || self.provider_total_tokens.is_some()
    }

    #[must_use]
    pub fn legacy_prompt_tokens(&self) -> Option<i64> {
        let mut total = self.fresh_input_tokens?;
        for tokens in [self.cache_read_tokens, self.cache_creation_tokens]
            .into_iter()
            .flatten()
        {
            total = total.checked_add(tokens)?;
        }
        Some(total)
    }

    #[must_use]
    pub fn legacy_completion_tokens(&self) -> Option<i64> {
        self.output_tokens
    }

    #[must_use]
    pub fn legacy_total_tokens(&self) -> Option<i64> {
        self.provider_total_tokens.or_else(|| {
            self.legacy_prompt_tokens()?
                .checked_add(self.legacy_completion_tokens()?)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageNormalizationError {
    InvalidContainer,
    InvalidTokenField {
        field: String,
    },
    NegativeTokenField {
        field: String,
        value: i64,
    },
    TokenArithmeticOverflow,
    InconsistentInputBuckets {
        input_tokens: i64,
        cache_read_tokens: i64,
        cache_creation_tokens: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageNormalizationOutcome {
    pub usage: NormalizedTokenUsage,
    pub error: Option<UsageNormalizationError>,
}

impl fmt::Display for UsageNormalizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidContainer => formatter.write_str("usage must be a JSON object"),
            Self::InvalidTokenField { field } => {
                write!(formatter, "usage field `{field}` must be an integer")
            }
            Self::NegativeTokenField { field, value } => {
                write!(
                    formatter,
                    "usage field `{field}` cannot be negative: {value}"
                )
            }
            Self::TokenArithmeticOverflow => formatter.write_str("token total overflow"),
            Self::InconsistentInputBuckets {
                input_tokens,
                cache_read_tokens,
                cache_creation_tokens,
            } => write!(
                formatter,
                "input token buckets exceed reported input: input={input_tokens}, cache_read={cache_read_tokens}, cache_creation={cache_creation_tokens}"
            ),
        }
    }
}

impl Error for UsageNormalizationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageFamily {
    OpenAiChat,
    OpenAiResponses,
    Anthropic,
    Bedrock,
    VertexGoogle,
    VertexAnthropic,
    VertexGoogleEmbeddings,
    Generic,
}

impl UsageFamily {
    const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiChat => "openai_chat",
            Self::OpenAiResponses => "openai_responses",
            Self::Anthropic => "anthropic_messages",
            Self::Bedrock => "bedrock",
            Self::VertexGoogle => "vertex_google",
            Self::VertexAnthropic => "vertex_anthropic",
            Self::VertexGoogleEmbeddings => "vertex_google_embeddings",
            Self::Generic => "generic",
        }
    }

    const fn input_excludes_cache(self) -> bool {
        matches!(
            self,
            Self::Anthropic | Self::Bedrock | Self::VertexAnthropic
        )
    }

    const fn output_excludes_reasoning(self) -> bool {
        matches!(self, Self::VertexGoogle)
    }
}

#[derive(Debug, Clone, Copy)]
struct ReportedInput {
    total: Option<i64>,
    cache_read: Option<i64>,
    cache_creation: Option<i64>,
    total_includes_cache_read: bool,
    total_includes_cache_creation: bool,
}

pub fn normalize_token_usage(
    value: Option<&Value>,
) -> Result<NormalizedTokenUsage, UsageNormalizationError> {
    let Some(value) = value else {
        return Ok(empty_usage(UsageFamily::Generic));
    };
    let usage = value
        .as_object()
        .ok_or(UsageNormalizationError::InvalidContainer)?;
    let provider_usage = usage.get("provider_usage").and_then(Value::as_object);
    let family = classify_family(usage, provider_usage);

    let input = parse_input(usage, provider_usage, family)?;
    let output_tokens = token_at_any(
        usage,
        provider_usage,
        &[
            "completion_tokens",
            "output_tokens",
            "candidatesTokenCount",
            "outputTokens",
        ],
    )?;
    let reasoning_tokens = parse_reasoning(usage, provider_usage)?;
    let reported_total_tokens = token_at_any(
        usage,
        provider_usage,
        &["total_tokens", "totalTokenCount", "totalTokens"],
    )?;
    let fresh_input_tokens = subtract_included_cache(input)?;
    let derived_total = derive_total(
        fresh_input_tokens,
        input.cache_read,
        input.cache_creation,
        output_tokens,
        reasoning_tokens,
        family.output_excludes_reasoning(),
    )?;

    Ok(build_normalized_usage(
        family,
        input,
        fresh_input_tokens,
        output_tokens,
        reasoning_tokens,
        reported_total_tokens,
        derived_total,
    ))
}

#[must_use]
pub fn normalize_token_usage_best_effort(value: Option<&Value>) -> UsageNormalizationOutcome {
    match normalize_token_usage(value) {
        Ok(usage) => UsageNormalizationOutcome { usage, error: None },
        Err(error) => UsageNormalizationOutcome {
            usage: recover_partial_usage(value),
            error: Some(error),
        },
    }
}

fn recover_partial_usage(value: Option<&Value>) -> NormalizedTokenUsage {
    let Some(usage) = value.and_then(Value::as_object) else {
        return empty_usage(UsageFamily::Generic);
    };
    let provider_usage = usage.get("provider_usage").and_then(Value::as_object);
    let family = classify_family(usage, provider_usage);
    let input = parse_input_lossy(usage, provider_usage, family);
    let output_tokens = token_at_any(
        usage,
        provider_usage,
        &[
            "completion_tokens",
            "output_tokens",
            "candidatesTokenCount",
            "outputTokens",
        ],
    )
    .ok()
    .flatten();
    let reasoning_tokens = parse_reasoning(usage, provider_usage).ok().flatten();
    let reported_total_tokens = token_at_any(
        usage,
        provider_usage,
        &["total_tokens", "totalTokenCount", "totalTokens"],
    )
    .ok()
    .flatten();
    let fresh_input_tokens = subtract_included_cache(input).ok().flatten();
    let derived_total = derive_total(
        fresh_input_tokens,
        input.cache_read,
        input.cache_creation,
        output_tokens,
        reasoning_tokens,
        family.output_excludes_reasoning(),
    )
    .ok()
    .flatten();

    build_normalized_usage(
        family,
        input,
        fresh_input_tokens,
        output_tokens,
        reasoning_tokens,
        reported_total_tokens,
        derived_total,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_normalized_usage(
    family: UsageFamily,
    input: ReportedInput,
    fresh_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    reported_total_tokens: Option<i64>,
    derived_total: Option<i64>,
) -> NormalizedTokenUsage {
    let provider_total_tokens = reported_total_tokens.or(derived_total);
    NormalizedTokenUsage {
        fresh_input_tokens,
        cache_read_tokens: input.cache_read,
        cache_creation_tokens: input.cache_creation,
        output_tokens,
        reasoning_tokens,
        provider_total_tokens,
        semantics: TokenUsageSemantics {
            version: TOKEN_USAGE_SEMANTICS_VERSION.to_string(),
            source_family: family.as_str().to_string(),
            input_includes_cache_read: input.cache_read.map(|_| input.total_includes_cache_read),
            input_includes_cache_creation: input
                .cache_creation
                .map(|_| input.total_includes_cache_creation),
            input_buckets_non_overlapping: (family != UsageFamily::Generic).then_some(true),
            output_includes_reasoning: reasoning_tokens
                .map(|_| !family.output_excludes_reasoning()),
            totals_reconcilable_by_addition: derived_total.is_some()
                && reported_total_tokens.is_none_or(|reported| Some(reported) == derived_total),
        },
        coverage: UsageCoverage {
            fresh_input: availability(input.total, fresh_input_tokens),
            cache_read: reported_availability(input.cache_read),
            cache_creation: reported_availability(input.cache_creation),
            output: reported_availability(output_tokens),
            reasoning: reported_availability(reasoning_tokens),
            provider_total: availability(reported_total_tokens, derived_total),
        },
    }
}

fn empty_usage(family: UsageFamily) -> NormalizedTokenUsage {
    NormalizedTokenUsage {
        fresh_input_tokens: None,
        cache_read_tokens: None,
        cache_creation_tokens: None,
        output_tokens: None,
        reasoning_tokens: None,
        provider_total_tokens: None,
        semantics: TokenUsageSemantics {
            version: TOKEN_USAGE_SEMANTICS_VERSION.to_string(),
            source_family: family.as_str().to_string(),
            input_includes_cache_read: None,
            input_includes_cache_creation: None,
            input_buckets_non_overlapping: None,
            output_includes_reasoning: None,
            totals_reconcilable_by_addition: false,
        },
        coverage: UsageCoverage::default(),
    }
}

fn classify_family(
    usage: &Map<String, Value>,
    provider_usage: Option<&Map<String, Value>>,
) -> UsageFamily {
    match usage.get("usage_source").and_then(Value::as_str) {
        Some("vertex_google") => return UsageFamily::VertexGoogle,
        Some("vertex_anthropic") => return UsageFamily::VertexAnthropic,
        Some("vertex_google_embeddings") => return UsageFamily::VertexGoogleEmbeddings,
        Some("bedrock") => return UsageFamily::Bedrock,
        _ => {}
    }
    if contains_any(usage, &["cacheReadInputTokens", "cacheWriteInputTokens"])
        || provider_usage.is_some_and(|value| {
            contains_any(value, &["cacheReadInputTokens", "cacheWriteInputTokens"])
        })
    {
        return UsageFamily::Bedrock;
    }
    if contains_any(
        usage,
        &["cache_read_input_tokens", "cache_creation_input_tokens"],
    ) || provider_usage.is_some_and(|value| {
        contains_any(
            value,
            &["cache_read_input_tokens", "cache_creation_input_tokens"],
        )
    }) {
        return UsageFamily::Anthropic;
    }
    if contains_any(
        usage,
        &[
            "promptTokenCount",
            "candidatesTokenCount",
            "cachedContentTokenCount",
            "totalTokenCount",
        ],
    ) || provider_usage.is_some_and(|value| {
        contains_any(
            value,
            &[
                "promptTokenCount",
                "candidatesTokenCount",
                "cachedContentTokenCount",
                "totalTokenCount",
            ],
        )
    }) {
        return UsageFamily::VertexGoogle;
    }
    if contains_any(usage, &["input_tokens", "input_tokens_details"]) {
        return UsageFamily::OpenAiResponses;
    }
    if contains_any(usage, &["prompt_tokens", "prompt_tokens_details"]) {
        return UsageFamily::OpenAiChat;
    }
    UsageFamily::Generic
}

fn parse_input(
    usage: &Map<String, Value>,
    provider_usage: Option<&Map<String, Value>>,
    family: UsageFamily,
) -> Result<ReportedInput, UsageNormalizationError> {
    let total = token_at_any(
        usage,
        provider_usage,
        &[
            "prompt_tokens",
            "input_tokens",
            "promptTokenCount",
            "inputTokens",
        ],
    )?;
    let cache_read = token_at_paths_any(
        usage,
        provider_usage,
        &[
            &["prompt_tokens_details", "cached_tokens"],
            &["input_tokens_details", "cached_tokens"],
            &["cache_read_input_tokens"],
            &["cacheReadInputTokens"],
            &["cachedContentTokenCount"],
        ],
    )?;
    let cache_creation = token_at_paths_any(
        usage,
        provider_usage,
        &[
            &["prompt_tokens_details", "cache_write_tokens"],
            &["input_tokens_details", "cache_write_tokens"],
            &["cache_write_tokens"],
            &["cache_creation_input_tokens"],
            &["cacheWriteInputTokens"],
        ],
    )?;

    Ok(reported_input(family, total, cache_read, cache_creation))
}

fn parse_input_lossy(
    usage: &Map<String, Value>,
    provider_usage: Option<&Map<String, Value>>,
    family: UsageFamily,
) -> ReportedInput {
    let total = token_at_any(
        usage,
        provider_usage,
        &[
            "prompt_tokens",
            "input_tokens",
            "promptTokenCount",
            "inputTokens",
        ],
    )
    .ok()
    .flatten();
    let cache_read = token_at_paths_any(
        usage,
        provider_usage,
        &[
            &["prompt_tokens_details", "cached_tokens"],
            &["input_tokens_details", "cached_tokens"],
            &["cache_read_input_tokens"],
            &["cacheReadInputTokens"],
            &["cachedContentTokenCount"],
        ],
    )
    .ok()
    .flatten();
    let cache_creation = token_at_paths_any(
        usage,
        provider_usage,
        &[
            &["prompt_tokens_details", "cache_write_tokens"],
            &["input_tokens_details", "cache_write_tokens"],
            &["cache_write_tokens"],
            &["cache_creation_input_tokens"],
            &["cacheWriteInputTokens"],
        ],
    )
    .ok()
    .flatten();
    reported_input(family, total, cache_read, cache_creation)
}

fn reported_input(
    family: UsageFamily,
    total: Option<i64>,
    cache_read: Option<i64>,
    cache_creation: Option<i64>,
) -> ReportedInput {
    let input_excludes_cache = family.input_excludes_cache();
    ReportedInput {
        total,
        cache_read,
        cache_creation,
        total_includes_cache_read: !input_excludes_cache,
        total_includes_cache_creation: !input_excludes_cache,
    }
}

fn subtract_included_cache(input: ReportedInput) -> Result<Option<i64>, UsageNormalizationError> {
    let Some(mut fresh) = input.total else {
        return Ok(None);
    };
    if input.total_includes_cache_read {
        fresh = fresh
            .checked_sub(input.cache_read.unwrap_or_default())
            .ok_or(UsageNormalizationError::TokenArithmeticOverflow)?;
    }
    if input.total_includes_cache_creation {
        fresh = fresh
            .checked_sub(input.cache_creation.unwrap_or_default())
            .ok_or(UsageNormalizationError::TokenArithmeticOverflow)?;
    }
    if fresh < 0 {
        return Err(UsageNormalizationError::InconsistentInputBuckets {
            input_tokens: input.total.unwrap_or_default(),
            cache_read_tokens: input.cache_read.unwrap_or_default(),
            cache_creation_tokens: input.cache_creation.unwrap_or_default(),
        });
    }
    Ok(Some(fresh))
}

fn derive_total(
    fresh_input: Option<i64>,
    cache_read: Option<i64>,
    cache_creation: Option<i64>,
    output: Option<i64>,
    reasoning: Option<i64>,
    output_excludes_reasoning: bool,
) -> Result<Option<i64>, UsageNormalizationError> {
    let Some(mut total) = fresh_input else {
        return Ok(None);
    };
    let Some(output) = output else {
        return Ok(None);
    };
    for tokens in [cache_read, cache_creation, Some(output)]
        .into_iter()
        .flatten()
    {
        total = total
            .checked_add(tokens)
            .ok_or(UsageNormalizationError::TokenArithmeticOverflow)?;
    }
    if output_excludes_reasoning {
        total = total
            .checked_add(reasoning.unwrap_or_default())
            .ok_or(UsageNormalizationError::TokenArithmeticOverflow)?;
    }
    Ok(Some(total))
}

fn parse_reasoning(
    usage: &Map<String, Value>,
    provider_usage: Option<&Map<String, Value>>,
) -> Result<Option<i64>, UsageNormalizationError> {
    token_at_paths_any(
        usage,
        provider_usage,
        &[
            &["completion_tokens_details", "reasoning_tokens"],
            &["output_tokens_details", "reasoning_tokens"],
            &["reasoning_tokens"],
            &["thoughtsTokenCount"],
        ],
    )
}

fn token_at_any(
    primary: &Map<String, Value>,
    secondary: Option<&Map<String, Value>>,
    keys: &[&str],
) -> Result<Option<i64>, UsageNormalizationError> {
    for key in keys {
        if primary.contains_key(*key) {
            return parse_token(primary.get(*key), key);
        }
    }
    if let Some(secondary) = secondary {
        for key in keys {
            if secondary.contains_key(*key) {
                return parse_token(secondary.get(*key), &format!("provider_usage.{key}"));
            }
        }
    }
    Ok(None)
}

fn token_at_paths(
    root: &Map<String, Value>,
    paths: &[&[&str]],
) -> Result<Option<i64>, UsageNormalizationError> {
    for path in paths {
        let mut current = root.get(path[0]);
        for segment in &path[1..] {
            current = current
                .and_then(Value::as_object)
                .and_then(|object| object.get(*segment));
        }
        if current.is_some() {
            return parse_token(current, &path.join("."));
        }
    }
    Ok(None)
}

fn token_at_paths_any(
    primary: &Map<String, Value>,
    secondary: Option<&Map<String, Value>>,
    paths: &[&[&str]],
) -> Result<Option<i64>, UsageNormalizationError> {
    let value = token_at_paths(primary, paths)?;
    if value.is_some() {
        return Ok(value);
    }
    secondary.map_or(Ok(None), |secondary| token_at_paths(secondary, paths))
}

fn parse_token(value: Option<&Value>, field: &str) -> Result<Option<i64>, UsageNormalizationError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let tokens = value
        .as_i64()
        .ok_or_else(|| UsageNormalizationError::InvalidTokenField {
            field: field.to_string(),
        })?;
    if tokens < 0 {
        return Err(UsageNormalizationError::NegativeTokenField {
            field: field.to_string(),
            value: tokens,
        });
    }
    Ok(Some(tokens))
}

fn contains_any(object: &Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().any(|key| object.contains_key(*key))
}

fn reported_availability(value: Option<i64>) -> TokenFieldAvailability {
    if value.is_some() {
        TokenFieldAvailability::Reported
    } else {
        TokenFieldAvailability::Unavailable
    }
}

fn availability(reported: Option<i64>, normalized: Option<i64>) -> TokenFieldAvailability {
    if reported.is_some() {
        TokenFieldAvailability::Reported
    } else if normalized.is_some() {
        TokenFieldAvailability::Derived
    } else {
        TokenFieldAvailability::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn normalizes_openai_cache_and_reasoning_buckets() {
        let usage = json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "prompt_tokens_details": {"cached_tokens": 40, "cache_write_tokens": 10},
            "completion_tokens_details": {"reasoning_tokens": 8}
        });

        let normalized = normalize_token_usage(Some(&usage)).expect("normalize OpenAI usage");

        assert_eq!(normalized.fresh_input_tokens, Some(50));
        assert_eq!(normalized.cache_read_tokens, Some(40));
        assert_eq!(normalized.cache_creation_tokens, Some(10));
        assert_eq!(normalized.output_tokens, Some(20));
        assert_eq!(normalized.reasoning_tokens, Some(8));
        assert_eq!(normalized.provider_total_tokens, Some(120));
        assert_eq!(normalized.legacy_prompt_tokens(), Some(100));
        assert!(normalized.semantics.totals_reconcilable_by_addition);
    }

    #[test]
    fn normalizes_anthropic_additive_input_buckets() {
        let usage = json!({
            "input_tokens": 50,
            "cache_read_input_tokens": 40,
            "cache_creation_input_tokens": 10,
            "output_tokens": 20
        });

        let normalized = normalize_token_usage(Some(&usage)).expect("normalize Anthropic usage");

        assert_eq!(normalized.fresh_input_tokens, Some(50));
        assert_eq!(normalized.cache_read_tokens, Some(40));
        assert_eq!(normalized.cache_creation_tokens, Some(10));
        assert_eq!(normalized.provider_total_tokens, Some(120));
        assert_eq!(normalized.legacy_prompt_tokens(), Some(100));
    }

    #[test]
    fn normalizes_bedrock_nested_provider_usage() {
        let usage = json!({
            "prompt_tokens": 30,
            "completion_tokens": 5,
            "provider_usage": {
                "cacheReadInputTokens": 20,
                "cacheWriteInputTokens": 10
            }
        });

        let normalized = normalize_token_usage(Some(&usage)).expect("normalize Bedrock usage");

        assert_eq!(normalized.fresh_input_tokens, Some(30));
        assert_eq!(normalized.cache_read_tokens, Some(20));
        assert_eq!(normalized.cache_creation_tokens, Some(10));
        assert_eq!(normalized.provider_total_tokens, Some(65));
    }

    #[test]
    fn normalizes_vertex_usage_metadata_with_distinct_reasoning() {
        let usage = json!({
            "usage_source": "vertex_google",
            "prompt_tokens": 100,
            "completion_tokens": 12,
            "total_tokens": 116,
            "provider_usage": {
                "promptTokenCount": 100,
                "cachedContentTokenCount": 25,
                "candidatesTokenCount": 12,
                "thoughtsTokenCount": 4,
                "totalTokenCount": 116
            }
        });

        let normalized = normalize_token_usage(Some(&usage)).expect("normalize Vertex usage");

        assert_eq!(normalized.fresh_input_tokens, Some(75));
        assert_eq!(normalized.cache_read_tokens, Some(25));
        assert_eq!(normalized.output_tokens, Some(12));
        assert_eq!(normalized.reasoning_tokens, Some(4));
        assert_eq!(normalized.provider_total_tokens, Some(116));
        assert_eq!(normalized.semantics.output_includes_reasoning, Some(false));
        assert!(normalized.semantics.totals_reconcilable_by_addition);
    }

    #[test]
    fn missing_cache_buckets_remain_unavailable() {
        let usage = json!({"prompt_tokens": 4, "completion_tokens": 3});

        let normalized = normalize_token_usage(Some(&usage)).expect("normalize plain usage");

        assert_eq!(normalized.cache_read_tokens, None);
        assert_eq!(normalized.cache_creation_tokens, None);
        assert_eq!(
            normalized.coverage.cache_creation,
            TokenFieldAvailability::Unavailable
        );
        assert_eq!(normalized.provider_total_tokens, Some(7));
        assert_eq!(
            normalized.coverage.provider_total,
            TokenFieldAvailability::Derived
        );
    }

    #[test]
    fn best_effort_preserves_billable_tokens_when_optional_details_are_malformed() {
        let usage = json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "completion_tokens_details": {"reasoning_tokens": "unknown"}
        });

        let outcome = normalize_token_usage_best_effort(Some(&usage));

        assert!(matches!(
            outcome.error,
            Some(UsageNormalizationError::InvalidTokenField { .. })
        ));
        assert_eq!(outcome.usage.legacy_prompt_tokens(), Some(100));
        assert_eq!(outcome.usage.legacy_completion_tokens(), Some(20));
        assert_eq!(outcome.usage.legacy_total_tokens(), Some(120));
        assert_eq!(outcome.usage.cache_read_tokens, None);
        assert_eq!(outcome.usage.reasoning_tokens, None);
    }

    #[test]
    fn recognizes_responses_vertex_anthropic_and_embedding_provenance() {
        let responses = normalize_token_usage(Some(&json!({
            "input_tokens": 80,
            "input_tokens_details": {"cached_tokens": 30, "cache_write_tokens": 10},
            "output_tokens": 15,
            "output_tokens_details": {"reasoning_tokens": 5},
            "total_tokens": 95
        })))
        .expect("OpenAI Responses usage");
        assert_eq!(responses.semantics.source_family, "openai_responses");
        assert_eq!(responses.fresh_input_tokens, Some(40));
        assert_eq!(responses.reasoning_tokens, Some(5));
        assert_eq!(responses.semantics.output_includes_reasoning, Some(true));

        let vertex_anthropic = normalize_token_usage(Some(&json!({
            "usage_source": "vertex_anthropic",
            "prompt_tokens": 20,
            "completion_tokens": 4,
            "provider_usage": {
                "input_tokens": 20,
                "output_tokens": 4,
                "cache_read_input_tokens": 6,
                "cache_creation_input_tokens": 2
            }
        })))
        .expect("Vertex Anthropic usage");
        assert_eq!(vertex_anthropic.semantics.source_family, "vertex_anthropic");
        assert_eq!(vertex_anthropic.fresh_input_tokens, Some(20));
        assert_eq!(vertex_anthropic.cache_read_tokens, Some(6));
        assert_eq!(vertex_anthropic.cache_creation_tokens, Some(2));
        assert_eq!(vertex_anthropic.provider_total_tokens, Some(32));

        let embeddings = normalize_token_usage(Some(&json!({
            "usage_source": "vertex_google_embeddings",
            "prompt_tokens": 9,
            "total_tokens": 9,
            "provider_usage": {
                "input_token_count_provenance": "provider_reported_aggregate"
            }
        })))
        .expect("Vertex embedding usage");
        assert_eq!(
            embeddings.semantics.source_family,
            "vertex_google_embeddings"
        );
        assert_eq!(embeddings.fresh_input_tokens, Some(9));
        assert_eq!(embeddings.output_tokens, None);
        assert_eq!(embeddings.provider_total_tokens, Some(9));
    }

    #[test]
    fn best_effort_preserves_valid_buckets_across_malformed_optional_fields() {
        let outcome = normalize_token_usage_best_effort(Some(&json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "prompt_tokens_details": {
                "cached_tokens": 40,
                "cache_write_tokens": "malformed"
            },
            "completion_tokens_details": {"reasoning_tokens": 8}
        })));

        assert!(matches!(
            outcome.error,
            Some(UsageNormalizationError::InvalidTokenField { .. })
        ));
        assert_eq!(outcome.usage.fresh_input_tokens, Some(60));
        assert_eq!(outcome.usage.cache_read_tokens, Some(40));
        assert_eq!(outcome.usage.cache_creation_tokens, None);
        assert_eq!(outcome.usage.output_tokens, Some(20));
        assert_eq!(outcome.usage.reasoning_tokens, Some(8));
        assert_eq!(outcome.usage.provider_total_tokens, Some(120));
        assert_eq!(
            outcome.usage.coverage.cache_creation,
            TokenFieldAvailability::Unavailable
        );
    }

    #[test]
    fn inconsistent_reported_total_is_preserved_but_not_marked_additive() {
        let normalized = normalize_token_usage(Some(&json!({
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 99
        })))
        .expect("inconsistent provider total remains auditable");

        assert_eq!(normalized.provider_total_tokens, Some(99));
        assert!(!normalized.semantics.totals_reconcilable_by_addition);
        assert_eq!(normalized.legacy_total_tokens(), Some(99));
    }

    #[test]
    fn rejects_negative_malformed_overflow_and_inconsistent_usage() {
        assert!(matches!(
            normalize_token_usage(Some(&json!({"prompt_tokens": -1}))),
            Err(UsageNormalizationError::NegativeTokenField { .. })
        ));
        assert!(matches!(
            normalize_token_usage(Some(&json!({"prompt_tokens": "1"}))),
            Err(UsageNormalizationError::InvalidTokenField { .. })
        ));
        assert!(matches!(
            normalize_token_usage(Some(&json!({
                "prompt_tokens": i64::MAX,
                "completion_tokens": 1
            }))),
            Err(UsageNormalizationError::TokenArithmeticOverflow)
        ));
        assert!(matches!(
            normalize_token_usage(Some(&json!({
                "prompt_tokens": 5,
                "prompt_tokens_details": {"cached_tokens": 6}
            }))),
            Err(UsageNormalizationError::InconsistentInputBuckets { .. })
        ));
    }
}
