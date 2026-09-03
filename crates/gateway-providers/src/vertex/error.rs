use gateway_core::ProviderError;
use thiserror::Error;

/// Request-mapping and response-normalization failures for the Vertex adapter.
///
/// Every variant maps to `ProviderError::InvalidRequest` except the upstream-content variants,
/// which map to `ProviderError::Transport` so the gateway treats them as retryable.
#[derive(Debug, Error)]
pub enum VertexAdapterError {
    #[error("vertex route upstream_model must be <publisher>/<model_id>, got `{0}`")]
    InvalidUpstreamModel(String),
    #[error("vertex publisher `{0}` is not supported")]
    UnsupportedPublisher(String),
    #[error("unsupported message role `{0}` for google vertex mapping")]
    UnsupportedMessageRole(String),
    #[error("google vertex request requires at least one user/assistant message")]
    EmptyMessages,
    #[error("content array entries must be objects")]
    InvalidContentEntry,
    #[error("content array entries must include `type`")]
    MissingContentType,
    #[error("text content entries must include a string `text`")]
    MissingContentText,
    #[error("unsupported content type `{0}` for google vertex mapping")]
    UnsupportedContentType(String),
    #[error("message content must be a string or typed content array")]
    InvalidMessageContent,
    #[error("`{field}` content entries must include a `{field}` object")]
    MissingMediaObject { field: String },
    #[error("{field}.url must be a string")]
    MissingMediaUrl { field: String },
    #[error("invalid google vertex media URI: {0}")]
    InvalidMediaUri(#[from] url::ParseError),
    #[error("unsupported google vertex media URI scheme `{0}`; expected gs:// or https://")]
    UnsupportedMediaScheme(String),
    #[error("google vertex media URI must include a host; expected gs:// or https://")]
    MissingMediaHost,
    #[error("google vertex media URI must not include user credentials")]
    MediaUriCredentials,
    #[error("could not infer MIME type for {field} URI; set {field}.mime_type")]
    UnknownMediaMimeType { field: String },
    #[error("{field}.{key} must be a non-empty string")]
    InvalidMimeField { field: String, key: &'static str },
    #[error("{field}.{key} must be a valid MIME type")]
    InvalidMimeType { field: String, key: &'static str },
    #[error("{field} MIME type fields conflict")]
    ConflictingMimeFields { field: String },
    #[error("{modality} content requires a {expected_prefix} MIME type, got `{mime_type}`")]
    MediaModalityMismatch {
        modality: &'static str,
        expected_prefix: &'static str,
        mime_type: String,
    },
    #[error("tool_use content must include `{0}`")]
    MissingToolUseField(&'static str),
    #[error("tool_result content must include `{0}`")]
    MissingToolResultField(&'static str),
    #[error("tool_result content is only valid in user messages")]
    ToolResultOutsideUserMessage,
    #[error("tool messages must include `tool_call_id`")]
    MissingToolCallId,
    #[error("tool result references unknown tool call id `{0}`")]
    UnknownToolCallId(String),
    #[error("assistant tool_calls must be an array")]
    InvalidToolCalls,
    #[error("assistant tool_calls entries must be objects")]
    InvalidToolCallEntry,
    #[error("only function tool_calls are supported for google vertex mapping")]
    UnsupportedToolCallType,
    #[error("assistant function tool_calls must include `function`")]
    MissingToolCallFunction,
    #[error("assistant function tool_calls must include function.name")]
    MissingToolCallFunctionName,
    #[error("assistant function tool_calls arguments must be a JSON string")]
    InvalidToolCallArguments,
    #[error("assistant function tool_calls arguments must contain valid JSON: {0}")]
    MalformedToolCallArguments(serde_json::Error),
    #[error("tools must be an array for google vertex mapping")]
    InvalidToolsArray,
    #[error("tools entries must be objects for google vertex mapping")]
    InvalidToolEntry,
    #[error("function tools must include an object `function`")]
    InvalidToolFunction,
    #[error("function tools must include a name")]
    MissingToolName,
    #[error("anthropic function tools must include `input_schema`")]
    MissingToolInputSchema,
    #[error("`parallel_tool_calls: false` is not supported for google vertex chat")]
    ParallelToolCallsDisabled,
    #[error("`parallel_tool_calls` must be a boolean")]
    InvalidParallelToolCalls,
    #[error("tool tool_choice must include `name`")]
    MissingToolChoiceName,
    #[error("function tool_choice must include function.name")]
    MissingToolChoiceFunctionName,
    #[error("unsupported tool_choice for google vertex mapping")]
    UnsupportedToolChoice,
    #[error("`toolConfig.functionCallingConfig` must be an object")]
    InvalidFunctionCallingConfig,
    #[error(
        "`streamFunctionCallArguments` is not supported for google vertex chat until partial argument accumulation is implemented"
    )]
    StreamedFunctionCallArguments,
    #[error(
        "google vertex streaming supports only a single candidate; remove `n`/`candidateCount` or use non-streaming"
    )]
    StreamCandidateCount,
    #[error("`max_completion_tokens` conflicts with `max_tokens` for google vertex mapping")]
    ConflictingMaxTokens,
    #[error("`generationConfig` must be an object for google vertex mapping")]
    InvalidGenerationConfig,
    #[error("`reasoning_effort` conflicts with `reasoning.effort` for google vertex mapping")]
    ConflictingReasoningEffort,
    #[error("`reasoning` must be an object for google vertex mapping")]
    InvalidReasoning,
    #[error("`reasoning_effort` must be a string for google vertex mapping")]
    InvalidReasoningEffortType,
    #[error("unsupported `reasoning_effort` `{0}` for google vertex mapping")]
    UnsupportedReasoningEffort(String),
    #[error(
        "`reasoning_effort` conflicts with caller-supplied `generationConfig.thinkingConfig`; set only one"
    )]
    ConflictingThinkingConfig,
    #[error("`{model}` does not support thinking; omit `reasoning_effort`")]
    ThinkingNotSupported { model: String },
    #[error("`response_format` must be an object for google vertex mapping")]
    InvalidResponseFormat,
    #[error("`response_format.type` must be `text`, `json_object`, or `json_schema`, got `{0}`")]
    UnsupportedResponseFormat(String),
    #[error("`response_format.json_schema.schema` must be an object")]
    MissingResponseSchema,
    #[error("`response_format` conflicts with caller-supplied `generationConfig.responseMimeType`")]
    ConflictingResponseFormat,
    #[error("`{0}` is not supported for google vertex chat")]
    UnsupportedRequestField(&'static str),
    #[error("google vertex returned a malformed function call: {0}")]
    MalformedFunctionCall(String),
    #[error("google vertex stream reported an error: {0}")]
    StreamError(String),
}

impl From<VertexAdapterError> for ProviderError {
    fn from(error: VertexAdapterError) -> Self {
        match error {
            VertexAdapterError::MalformedFunctionCall(_) | VertexAdapterError::StreamError(_) => {
                Self::Transport(error.to_string())
            }
            other => Self::InvalidRequest(other.to_string()),
        }
    }
}
