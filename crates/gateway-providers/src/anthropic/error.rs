use gateway_core::ProviderError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnthropicAdapterError {
    #[error("unsupported message role `{role}` for Anthropic mapping")]
    UnsupportedMessageRole { role: String },
    #[error("anthropic request requires at least one user or assistant message")]
    EmptyMessages,
    #[error("content array entries must be objects")]
    InvalidContentEntry,
    #[error("content array entries must include `type`")]
    MissingContentType,
    #[error("text content entries must include a string `text`")]
    MissingContentText,
    #[error("message content must be a string or typed content array")]
    InvalidMessageContent,
    #[error(
        "forced `tool_choice` is not supported for `{model}`; Fable 5.1 rejects forced tool choice, use `auto` or omit the field"
    )]
    ForcedToolChoiceRejected { model: String },
    #[error("unsupported tool_choice for Anthropic mapping")]
    UnsupportedToolChoice,
    #[error("tools must be an array for Anthropic mapping")]
    InvalidToolsArray,
    #[error("tools entries must be objects for Anthropic mapping")]
    InvalidToolEntry,
    #[error("function tools must include `function`")]
    MissingToolFunction,
    #[error("function tools must include function.name")]
    MissingFunctionName,
    #[error("function tools must include an object `function`")]
    InvalidToolFunction,
    #[error("function tool_choice must include function.name")]
    MissingToolChoiceFunctionName,
    #[error("assistant tool_calls must be an array")]
    InvalidToolCalls,
    #[error("only function tool_calls are supported for Anthropic mapping")]
    UnsupportedToolCallType,
    #[error("assistant tool_calls entries must include `id`")]
    MissingToolCallId,
    #[error("assistant tool_calls entries must be objects")]
    InvalidToolCallEntry,
    #[error("assistant function tool_calls must include `function`")]
    MissingToolCallFunction,
    #[error("assistant function tool_calls must include function.name")]
    MissingToolCallFunctionName,
    #[error("assistant function tool_calls arguments must be a JSON string: {reason}")]
    InvalidToolArguments { reason: String },
    #[error("tool messages must include `tool_call_id`")]
    MissingToolCallIdInToolMessage,
    #[error("`output_config` must be an object for Anthropic mapping")]
    InvalidOutputConfig,
    #[error("`reasoning` must be an object for Anthropic mapping")]
    InvalidReasoningConfig,
    #[error("`reasoning_effort` conflicts with `reasoning.effort` for Anthropic mapping")]
    ConflictingReasoningEffort,
    #[error(
        "`reasoning.budget_tokens` conflicts with `reasoning_budget_tokens` for Anthropic mapping"
    )]
    ConflictingReasoningBudgetTokens,
    #[error(
        "`thinking_budget_tokens` conflicts with `reasoning_budget_tokens` for Anthropic mapping"
    )]
    ConflictingThinkingBudgetTokens,
    #[error("`reasoning_effort` conflicts with `output_config.effort` for `{model}`")]
    ConflictingEffort { model: String },
    #[error("`output_config.effort` is not supported for `{model}`")]
    EffortNotSupported { model: String },
    #[error(
        "`reasoning_effort` requires an explicit manual thinking budget for `{model}` because this Claude model does not support adaptive thinking"
    )]
    ManualBudgetRequiredForEffort { model: String },
    #[error(
        "`reasoning_effort` requires an explicit manual thinking budget for `{model}` because this Claude model does not support adaptive thinking or effort"
    )]
    ManualBudgetRequired { model: String },
    #[error(
        "`reasoning.budget_tokens` is not supported for `{model}`; use adaptive thinking with `reasoning_effort` or `output_config.effort`"
    )]
    AdaptiveOnlyBudgetNotSupported { model: String },
    #[error(
        "`thinking.type: enabled` with manual `budget_tokens` is not supported for `{model}`; use `thinking.type: adaptive` and `output_config.effort`"
    )]
    AdaptiveOnlyManualThinkingNotSupported { model: String },
    #[error(
        "`thinking.type: disabled` is not supported for `{model}`; adaptive thinking is always enabled"
    )]
    AdaptiveOnlyDisabledNotSupported { model: String },
    #[error("`thinking.type: disabled` is not supported for Claude Mythos Preview")]
    MythosDisabledNotSupported,
    #[error("`thinking.type: enabled` for `{model}` must include `budget_tokens`")]
    MissingBudgetTokens { model: String },
    #[error(
        "`thinking.type: adaptive` is not supported for `{model}`; use `thinking.type: enabled` with `budget_tokens`"
    )]
    AdaptiveNotSupported { model: String },
    #[error(
        "`reasoning_effort` requires `thinking.type: adaptive` for `{model}` and conflicts with caller-supplied `thinking`"
    )]
    ConflictingAdaptiveThinking { model: String },
    #[error(
        "manual Anthropic thinking budget for `{model}` conflicts with caller-supplied `thinking.budget_tokens`"
    )]
    ConflictingManualBudget { model: String },
    #[error(
        "manual Anthropic thinking budget for `{model}` conflicts with caller-supplied `thinking`"
    )]
    ConflictingCallerThinking { model: String },
    #[error("`thinking` must be an object for Anthropic mapping")]
    InvalidThinkingObject,
    #[error(
        "`{field}` is not supported with non-default values for `{model}`; omit the field for adaptive-only Claude models"
    )]
    UnsupportedSamplingField { field: &'static str, model: String },
    #[error("invalid url: {0}")]
    InvalidUrl(#[from] url::ParseError),
}

impl From<AnthropicAdapterError> for ProviderError {
    fn from(err: AnthropicAdapterError) -> Self {
        match err {
            AnthropicAdapterError::InvalidUrl(err) => Self::Transport(err.to_string()),
            other => Self::InvalidRequest(other.to_string()),
        }
    }
}
