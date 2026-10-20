mod controls;
pub mod count_tokens;
pub mod error;
mod model;
pub mod request;
#[cfg(test)]
mod request_tests;
pub mod response;
pub mod streaming;
#[cfg(test)]
mod tests;
pub mod thinking;

pub use count_tokens::{
    COUNT_TOKENS_FIELDS, normalize_count_tokens_response, retain_count_tokens_fields,
};
pub use error::AnthropicAdapterError;
pub use request::{AnthropicRequestOptions, map_anthropic_request};
pub use response::{
    anthropic_usage_total, extract_anthropic_thinking_blocks, extract_anthropic_tool_calls,
    map_anthropic_finish_reason, map_anthropic_stream_usage, map_anthropic_usage,
    normalize_anthropic_response, normalize_anthropic_thinking_delta,
    normalize_anthropic_thinking_start, provider_reasoning_metadata,
};
pub use streaming::normalize_anthropic_stream;
pub use thinking::{
    ClaudeThinkingPolicy, apply_anthropic_thinking_compatibility,
    claude_rejects_forced_tool_choice, claude_thinking_policy, contains_exact_claude_model_marker,
    is_adaptive_only_claude, validate_anthropic_sampling_fields, validate_anthropic_tool_choice,
};
