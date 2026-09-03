use serde::{Deserialize, Serialize};

pub const DEFAULT_GATEWAY_BASE_URL: &str = "http://127.0.0.1:3000";
pub const DEFAULT_API_KEY_ENV_VAR: &str = "OCEANS_LLM_API_KEY";
pub const DEFAULT_PROVIDER_ID: &str = "oceans-llm";
pub const MAX_CLIENT_CONTEXT_WINDOW_TOKENS: i64 = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ThinkingPolicy {
    AnthropicSafeEffort,
    AnthropicManualBudget,
    /// Gemini 3.x `thinkingLevel` models.
    GeminiLevel {
        supports_minimal: bool,
        supports_medium: bool,
    },
    /// Gemini 2.5 `thinkingBudget` models.
    GeminiBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodexReasoningEffort {
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

impl CodexReasoningEffort {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ClientModelCapabilities {
    #[serde(default)]
    pub responses: bool,
    pub tool_calling: bool,
    pub attachments: bool,
    pub vision: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfigInput {
    pub model_id: String,
    pub display_name: String,
    pub upstream_model: Option<String>,
    pub provider_id: String,
    pub provider_name: String,
    pub gateway_base_url: String,
    pub api_key_env_var: String,
    pub input_cost_per_million_tokens_usd_10000: Option<i64>,
    pub output_cost_per_million_tokens_usd_10000: Option<i64>,
    pub cache_read_cost_per_million_tokens_usd_10000: Option<i64>,
    pub cache_write_cost_per_million_tokens_usd_10000: Option<i64>,
    pub context_window_tokens: Option<i64>,
    pub input_window_tokens: Option<i64>,
    pub output_window_tokens: Option<i64>,
    pub capabilities: ClientModelCapabilities,
    pub thinking_policy: Option<ThinkingPolicy>,
    /// An explicit Codex default. A gateway maximum is a ceiling and must not populate this field.
    pub codex_reasoning_effort: Option<CodexReasoningEffort>,
}

impl ClientConfigInput {
    #[must_use]
    pub fn context_window(&self) -> i64 {
        self.input_window_tokens
            .or(self.context_window_tokens)
            .unwrap_or_default()
            .min(MAX_CLIENT_CONTEXT_WINDOW_TOKENS)
    }

    #[must_use]
    pub fn context_window_is_capped(&self) -> bool {
        self.input_window_tokens
            .or(self.context_window_tokens)
            .is_some_and(|window| window > MAX_CLIENT_CONTEXT_WINDOW_TOKENS)
    }

    #[must_use]
    pub fn client_base_url(&self) -> String {
        let trimmed = self.gateway_base_url.trim_end_matches('/');
        trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_string()
    }

    #[must_use]
    pub fn openai_compatible_client_base_url(&self) -> String {
        format!("{}/v1", self.client_base_url())
    }

    #[must_use]
    pub fn output_window(&self) -> i64 {
        self.output_window_tokens.unwrap_or_default()
    }

    #[must_use]
    pub fn input_modalities(&self) -> Vec<&'static str> {
        let mut modalities = vec!["text"];
        if self.capabilities.vision || self.capabilities.attachments {
            modalities.push("image");
        }
        modalities
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfigInputSet {
    pub models: Vec<ClientConfigInput>,
}

impl ClientConfigInputSet {
    #[must_use]
    pub fn new(models: Vec<ClientConfigInput>) -> Self {
        Self { models }
    }

    #[must_use]
    pub fn first(&self) -> Option<&ClientConfigInput> {
        self.models.first()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }
}

impl Default for ClientConfigInput {
    fn default() -> Self {
        Self {
            model_id: String::new(),
            display_name: String::new(),
            upstream_model: None,
            provider_id: DEFAULT_PROVIDER_ID.to_string(),
            provider_name: DEFAULT_PROVIDER_ID.to_string(),
            gateway_base_url: DEFAULT_GATEWAY_BASE_URL.to_string(),
            api_key_env_var: DEFAULT_API_KEY_ENV_VAR.to_string(),
            input_cost_per_million_tokens_usd_10000: None,
            output_cost_per_million_tokens_usd_10000: None,
            cache_read_cost_per_million_tokens_usd_10000: None,
            cache_write_cost_per_million_tokens_usd_10000: None,
            context_window_tokens: None,
            input_window_tokens: None,
            output_window_tokens: None,
            capabilities: ClientModelCapabilities::default(),
            thinking_policy: None,
            codex_reasoning_effort: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfig {
    pub key: String,
    pub label: String,
    pub model_ids: Vec<String>,
    pub setup: Vec<ClientConfigSetupItem>,
    pub blocks: Vec<ClientConfigCodeBlock>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfigSetupItem {
    pub label: String,
    pub value: String,
    pub href: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConfigCodeBlock {
    pub label: String,
    pub filename: String,
    pub content: String,
}

pub trait ClientConfigTemplate {
    fn render(&self, input: &ClientConfigInput) -> ClientConfig;
}
