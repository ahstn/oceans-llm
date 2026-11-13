use gateway_core::GatewayError;
use serde_json::Value;
use tracing::warn;

mod estimate;

pub(crate) use estimate::stream_event_text_bytes;
pub use estimate::{UsageEstimateInput, response_text_bytes};

#[derive(Debug, Clone, Default)]
pub(crate) struct UsageSummary {
    pub(crate) prompt_tokens: Option<i64>,
    pub(crate) uncached_input_tokens: Option<i64>,
    pub(crate) cache_read_tokens: Option<i64>,
    /// All cache writes, 5-minute and 1-hour.
    pub(crate) cache_write_tokens: Option<i64>,
    /// The 1-hour-TTL share of `cache_write_tokens`.
    pub(crate) cache_write_1h_tokens: Option<i64>,
    pub(crate) completion_tokens: Option<i64>,
    /// Image tokens within `completion_tokens`; never more than them.
    pub(crate) image_output_tokens: Option<i64>,
    pub(crate) total_tokens: Option<i64>,
    pub(crate) cache_usage: CacheUsageNormalization,
}

impl UsageSummary {
    pub(crate) fn has_usage(&self) -> bool {
        self.prompt_tokens.is_some()
            || self.completion_tokens.is_some()
            || self.total_tokens.is_some()
    }
}

pub(crate) fn usage_summary_from_value(
    value: Option<&Value>,
) -> Result<UsageSummary, GatewayError> {
    let Some(usage) = value.and_then(Value::as_object) else {
        return Ok(UsageSummary::default());
    };

    let raw_prompt_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_i64);
    let completion_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_i64);
    let total_tokens = match usage.get("total_tokens").and_then(Value::as_i64) {
        some @ Some(_) => some,
        None => match (raw_prompt_tokens, completion_tokens) {
            (Some(prompt), Some(completion)) => prompt
                .checked_add(completion)
                .ok_or_else(|| GatewayError::Internal("token total overflow".to_string()))
                .map(Some)?,
            _ => None,
        },
    };

    let cache_usage = normalize_cache_tokens(usage, raw_prompt_tokens);
    let prompt_tokens = cache_usage
        .tokens()
        .map_or(raw_prompt_tokens, |tokens| Some(tokens.total_input_tokens));
    let total_tokens = if prompt_tokens != raw_prompt_tokens {
        match (prompt_tokens, completion_tokens) {
            (Some(prompt), Some(completion)) => prompt
                .checked_add(completion)
                .ok_or_else(|| GatewayError::Internal("token total overflow".to_string()))
                .map(Some)?,
            _ => total_tokens,
        }
    } else {
        total_tokens
    };
    // Preserve an explicit unknown after validating the counters and their checked totals.
    let total_tokens = if usage.get("total_tokens").is_some_and(Value::is_null) {
        None
    } else {
        total_tokens
    };
    let (uncached_input_tokens, cache_read_tokens, cache_write_tokens, cache_write_1h_tokens) =
        cache_usage
            .tokens()
            .map_or((None, None, None, None), |tokens| {
                (
                    Some(tokens.uncached_input_tokens),
                    Some(tokens.cache_read_tokens),
                    Some(tokens.cache_write_tokens),
                    Some(tokens.cache_write_1h_tokens),
                )
            });
    let image_output_tokens = image_output_tokens_within_completion(usage, completion_tokens);

    Ok(UsageSummary {
        prompt_tokens,
        uncached_input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        cache_write_1h_tokens,
        completion_tokens,
        image_output_tokens,
        total_tokens,
        cache_usage,
    })
}

/// Image tokens within the completion tokens, from Chat Completions
/// `completion_tokens_details` or Responses `output_tokens_details`.
///
/// A count that is not an integer from zero to the completion tokens is
/// dropped with a warning rather than failing or unpricing the request. Image
/// tokens are a subset of completion tokens, so dropping the split loses no
/// tokens: every completion token then bills at the output rate, as it did
/// before image rates existed.
fn image_output_tokens_within_completion(
    usage: &serde_json::Map<String, Value>,
    completion_tokens: Option<i64>,
) -> Option<i64> {
    let image_tokens = ["completion_tokens_details", "output_tokens_details"]
        .into_iter()
        .find_map(|details| {
            usage
                .get(details)?
                .get("image_tokens")
                .filter(|tokens| !tokens.is_null())
        })?;
    let valid = image_tokens.as_i64().filter(|tokens| {
        *tokens >= 0 && completion_tokens.is_some_and(|completion| *tokens <= completion)
    });
    if valid.is_none() {
        warn!(
            image_tokens = %image_tokens,
            completion_tokens = ?completion_tokens,
            "ignoring image token count outside the completion tokens; image output bills at the output rate"
        );
    }
    valid
}

#[derive(Debug, Clone, Default)]
pub(crate) enum CacheUsageNormalization {
    #[default]
    Unavailable,
    Valid(CacheTokenSummary),
    Invalid(String),
}

impl CacheUsageNormalization {
    fn tokens(&self) -> Option<&CacheTokenSummary> {
        match self {
            Self::Valid(tokens) => Some(tokens),
            Self::Unavailable | Self::Invalid(_) => None,
        }
    }

    pub(crate) fn reason(&self) -> Option<String> {
        match self {
            Self::Invalid(reason) => Some(format!("invalid_cache_token_usage:{reason}")),
            Self::Unavailable | Self::Valid(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CacheTokenSummary {
    total_input_tokens: i64,
    uncached_input_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    /// The 1-hour share of `cache_write_tokens`; never more than them.
    cache_write_1h_tokens: i64,
}

fn normalize_cache_tokens(
    usage: &serde_json::Map<String, Value>,
    prompt_tokens: Option<i64>,
) -> CacheUsageNormalization {
    // Native Messages usage counts uncached input separately from cache reads/writes.
    // Its complete usage object reaches accounting without a Chat Completions wrapper.
    let provider_usage = usage
        .get("provider_usage")
        .and_then(Value::as_object)
        .or_else(|| usage.contains_key("input_tokens").then_some(usage));
    // Chat-compatible root aliases use inclusive prompt_tokens. Do not apply them
    // to native exclusive input counters or change the Responses usage contract.
    let root_aliases = usage
        .contains_key("prompt_tokens")
        .then_some(usage)
        .filter(|fields| {
            fields.contains_key("cached_tokens")
                || fields.contains_key("cache_write_tokens")
                || fields.contains_key("prompt_cache_hit_tokens")
        });
    let details = usage
        .get("input_tokens_details")
        .or_else(|| usage.get("prompt_tokens_details"))
        .or_else(|| provider_usage.and_then(|raw| raw.get("input_tokens_details")))
        .or_else(|| provider_usage.and_then(|raw| raw.get("prompt_tokens_details")));
    if let Some(details) = details {
        let Some(details) = details.as_object() else {
            return CacheUsageNormalization::Invalid(
                "cache token details must be an object".to_string(),
            );
        };
        return normalize_inclusive_cache_tokens(details, prompt_tokens, root_aliases);
    }

    if let Some(provider_usage) = provider_usage {
        if provider_usage.contains_key("cached_tokens")
            || provider_usage.contains_key("cache_write_tokens")
        {
            return normalize_inclusive_cache_tokens(provider_usage, prompt_tokens, None);
        }
        if provider_usage.contains_key("cacheReadInputTokens")
            || provider_usage.contains_key("cacheWriteInputTokens")
        {
            return normalize_exclusive_cache_tokens(
                provider_usage,
                prompt_tokens,
                "cacheReadInputTokens",
                "cacheWriteInputTokens",
                bedrock_1h_cache_write_tokens(provider_usage),
            );
        }
        if provider_usage.contains_key("cache_read_input_tokens")
            || provider_usage.contains_key("cache_creation_input_tokens")
        {
            return normalize_exclusive_cache_tokens(
                provider_usage,
                prompt_tokens,
                "cache_read_input_tokens",
                "cache_creation_input_tokens",
                anthropic_1h_cache_write_tokens(provider_usage),
            );
        }
    }

    if let Some(root_aliases) = root_aliases {
        return normalize_inclusive_cache_tokens(root_aliases, prompt_tokens, Some(root_aliases));
    }

    CacheUsageNormalization::Unavailable
}

fn normalize_inclusive_cache_tokens(
    fields: &serde_json::Map<String, Value>,
    prompt_tokens: Option<i64>,
    root_aliases: Option<&serde_json::Map<String, Value>>,
) -> CacheUsageNormalization {
    // Presence, not truthiness or successful parsing, decides precedence. A
    // canonical zero wins; an invalid canonical count must not fall back.
    let (read_fields, read_field) = if fields.contains_key("cached_tokens") {
        (fields, "cached_tokens")
    } else if let Some(aliases) = root_aliases {
        if aliases.contains_key("cached_tokens") {
            (aliases, "cached_tokens")
        } else {
            (aliases, "prompt_cache_hit_tokens")
        }
    } else {
        (fields, "cached_tokens")
    };
    let write_fields = if fields.contains_key("cache_write_tokens") {
        fields
    } else {
        root_aliases.unwrap_or(fields)
    };
    let cache_read_tokens = match cache_token_field(read_fields, read_field) {
        Ok(tokens) => tokens,
        Err(reason) => return CacheUsageNormalization::Invalid(reason),
    };
    let cache_write_tokens = match cache_token_field(write_fields, "cache_write_tokens") {
        Ok(tokens) => tokens,
        Err(reason) => return CacheUsageNormalization::Invalid(reason),
    };
    let Some(prompt_tokens) = prompt_tokens else {
        return CacheUsageNormalization::Invalid(
            "cache token details require total input tokens".to_string(),
        );
    };
    if prompt_tokens < 0 {
        return CacheUsageNormalization::Invalid(
            "input token count cannot be negative".to_string(),
        );
    }
    if read_field == "prompt_cache_hit_tokens"
        && read_fields.contains_key(read_field)
        && read_fields.contains_key("prompt_cache_miss_tokens")
    {
        let miss_tokens = match cache_token_field(read_fields, "prompt_cache_miss_tokens") {
            Ok(tokens) => tokens,
            Err(reason) => return CacheUsageNormalization::Invalid(reason),
        };
        if cache_read_tokens.checked_add(miss_tokens) != Some(prompt_tokens) {
            return CacheUsageNormalization::Invalid(
                "cache hits and misses must sum to total input tokens".to_string(),
            );
        }
    }
    let Some(cached_input_tokens) = cache_read_tokens.checked_add(cache_write_tokens) else {
        return CacheUsageNormalization::Invalid("cache token total overflow".to_string());
    };
    if cached_input_tokens > prompt_tokens {
        return CacheUsageNormalization::Invalid(
            "cache token buckets exceed total input tokens".to_string(),
        );
    }
    CacheUsageNormalization::Valid(CacheTokenSummary {
        total_input_tokens: prompt_tokens,
        uncached_input_tokens: prompt_tokens - cached_input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        cache_write_1h_tokens: 0,
    })
}

/// Normalizes native counters that report uncached input apart from cache reads and
/// writes. `cache_write_1h_tokens` is the 1-hour share of the writes, or why it is invalid.
fn normalize_exclusive_cache_tokens(
    fields: &serde_json::Map<String, Value>,
    uncached_input_tokens: Option<i64>,
    read_field: &str,
    write_field: &str,
    cache_write_1h_tokens: Result<i64, String>,
) -> CacheUsageNormalization {
    let Some(uncached_input_tokens) = uncached_input_tokens else {
        return CacheUsageNormalization::Invalid(
            "provider cache counters require uncached input tokens".to_string(),
        );
    };
    if uncached_input_tokens < 0 {
        return CacheUsageNormalization::Invalid(
            "input token count cannot be negative".to_string(),
        );
    }
    let cache_read_tokens = match cache_token_field(fields, read_field) {
        Ok(tokens) => tokens,
        Err(reason) => return CacheUsageNormalization::Invalid(reason),
    };
    let cache_write_tokens = match cache_token_field(fields, write_field) {
        Ok(tokens) => tokens,
        Err(reason) => return CacheUsageNormalization::Invalid(reason),
    };
    // The 1-hour writes are a share of all writes; a count above them bills every write at
    // the 1-hour rate rather than discard the valid input total.
    let cache_write_1h_tokens = match cache_write_1h_tokens {
        Ok(tokens) => tokens.min(cache_write_tokens),
        Err(reason) => return CacheUsageNormalization::Invalid(reason),
    };
    let Some(total_input_tokens) = uncached_input_tokens
        .checked_add(cache_read_tokens)
        .and_then(|total| total.checked_add(cache_write_tokens))
    else {
        return CacheUsageNormalization::Invalid("cache token total overflow".to_string());
    };
    CacheUsageNormalization::Valid(CacheTokenSummary {
        total_input_tokens,
        uncached_input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        cache_write_1h_tokens,
    })
}

/// Anthropic's 1-hour cache writes, `cache_creation.ephemeral_1h_input_tokens`, a share of
/// `cache_creation_input_tokens`. An absent or null count is none.
fn anthropic_1h_cache_write_tokens(fields: &serde_json::Map<String, Value>) -> Result<i64, String> {
    let Some(classes) = fields.get("cache_creation").and_then(Value::as_object) else {
        return Ok(0);
    };
    match classes.get("ephemeral_1h_input_tokens") {
        None | Some(Value::Null) => Ok(0),
        Some(_) => cache_token_field(classes, "ephemeral_1h_input_tokens"),
    }
}

/// Bedrock's 1-hour cache writes: the `cacheDetails` entries whose `ttl` is `1h`, a share
/// of `cacheWriteInputTokens`.
fn bedrock_1h_cache_write_tokens(fields: &serde_json::Map<String, Value>) -> Result<i64, String> {
    let Some(details) = fields.get("cacheDetails").and_then(Value::as_array) else {
        return Ok(0);
    };
    details
        .iter()
        .filter_map(Value::as_object)
        .filter(|detail| detail.get("ttl").and_then(Value::as_str) == Some("1h"))
        .try_fold(0_i64, |total, detail| {
            let tokens = cache_token_field(detail, "inputTokens")?;
            total
                .checked_add(tokens)
                .ok_or_else(|| "cache token total overflow".to_string())
        })
}

fn cache_token_field(details: &serde_json::Map<String, Value>, field: &str) -> Result<i64, String> {
    let Some(value) = details.get(field) else {
        return Ok(0);
    };
    let Some(tokens) = value.as_i64() else {
        return Err(format!("cache token field `{field}` must be an integer"));
    };
    if tokens < 0 {
        return Err(format!("cache token field `{field}` cannot be negative"));
    }
    Ok(tokens)
}
