use gateway_core::ProviderConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const REQUEST_LOG_PROVIDER_ICON_KEY: &str = "provider_icon_key";
pub const REQUEST_LOG_MODEL_ICON_KEY: &str = "model_icon_key";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderIconKey {
    Anthropic,
    OpenAI,
    OpenRouter,
    VertexAI,
}

impl ProviderIconKey {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAI => "openai",
            Self::OpenRouter => "openrouter",
            Self::VertexAI => "vertexai",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAI),
            "openrouter" => Some(Self::OpenRouter),
            "vertexai" => Some(Self::VertexAI),
            _ => None,
        }
    }

    #[must_use]
    pub const fn default_label(self) -> &'static str {
        match self {
            Self::Anthropic => "Anthropic",
            Self::OpenAI => "OpenAI",
            Self::OpenRouter => "OpenRouter",
            Self::VertexAI => "Google Vertex AI",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelIconKey {
    Anthropic,
    Claude,
    Gemini,
    OpenAI,
    OpenRouter,
    VertexAI,
}

impl ModelIconKey {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::OpenAI => "openai",
            Self::OpenRouter => "openrouter",
            Self::VertexAI => "vertexai",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "anthropic" => Some(Self::Anthropic),
            "claude" => Some(Self::Claude),
            "gemini" => Some(Self::Gemini),
            "openai" => Some(Self::OpenAI),
            "openrouter" => Some(Self::OpenRouter),
            "vertexai" => Some(Self::VertexAI),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDisplayIdentity {
    pub label: String,
    pub icon_key: ProviderIconKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLogIconMetadata {
    pub provider_icon_key: ProviderIconKey,
    pub model_icon_key: Option<ModelIconKey>,
}

#[must_use]
pub fn resolve_provider_display(
    provider_key: &str,
    provider: Option<&ProviderConnection>,
) -> ProviderDisplayIdentity {
    let configured_icon_key = provider.and_then(provider_display_icon_key);
    let icon_key =
        configured_icon_key.unwrap_or_else(|| infer_provider_icon_key(provider_key, provider));

    let label = provider
        .and_then(provider_display_label)
        .unwrap_or_else(|| icon_key.default_label().to_string());

    ProviderDisplayIdentity { label, icon_key }
}

#[must_use]
pub fn resolve_model_icon_key<'a>(
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<ModelIconKey> {
    let values = candidates
        .into_iter()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect::<Vec<_>>();

    if values.iter().any(|value| value.contains("claude")) {
        return Some(ModelIconKey::Claude);
    }
    if values.iter().any(|value| value.contains("anthropic")) {
        return Some(ModelIconKey::Anthropic);
    }
    if values.iter().any(|value| value.contains("gemini")) {
        return Some(ModelIconKey::Gemini);
    }
    if values.iter().any(|value| value.contains("openrouter")) {
        return Some(ModelIconKey::OpenRouter);
    }
    if values.iter().any(|value| is_openai_model_candidate(value)) {
        return Some(ModelIconKey::OpenAI);
    }
    if values
        .iter()
        .any(|value| value.contains("vertex") || value.starts_with("veo-"))
    {
        return Some(ModelIconKey::VertexAI);
    }

    None
}

#[must_use]
pub fn provider_icon_key_from_metadata(metadata: &Map<String, Value>) -> Option<ProviderIconKey> {
    metadata
        .get(REQUEST_LOG_PROVIDER_ICON_KEY)
        .and_then(Value::as_str)
        .and_then(ProviderIconKey::parse)
}

#[must_use]
pub fn model_icon_key_from_metadata(metadata: &Map<String, Value>) -> Option<ModelIconKey> {
    metadata
        .get(REQUEST_LOG_MODEL_ICON_KEY)
        .and_then(Value::as_str)
        .and_then(ModelIconKey::parse)
}

fn infer_provider_icon_key(
    provider_key: &str,
    provider: Option<&ProviderConnection>,
) -> ProviderIconKey {
    if let Some(provider) = provider {
        if provider.provider_type == "gcp_vertex" {
            return ProviderIconKey::VertexAI;
        }

        if let Some(base_url) = provider
            .config
            .get("base_url")
            .and_then(Value::as_str)
            .map(|value| value.to_ascii_lowercase())
        {
            if base_url.contains("openrouter") {
                return ProviderIconKey::OpenRouter;
            }
            if base_url.contains("anthropic") {
                return ProviderIconKey::Anthropic;
            }
            if base_url.contains("openai") {
                return ProviderIconKey::OpenAI;
            }
        }
    }

    let provider_key = provider_key.to_ascii_lowercase();
    if provider_key.contains("openrouter") {
        ProviderIconKey::OpenRouter
    } else if provider_key.contains("anthropic") {
        ProviderIconKey::Anthropic
    } else if provider_key.contains("vertex") {
        ProviderIconKey::VertexAI
    } else {
        ProviderIconKey::OpenAI
    }
}

fn provider_display_label(provider: &ProviderConnection) -> Option<String> {
    provider
        .config
        .get("display")
        .and_then(Value::as_object)
        .and_then(|display| display.get("label"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn provider_display_icon_key(provider: &ProviderConnection) -> Option<ProviderIconKey> {
    provider
        .config
        .get("display")
        .and_then(Value::as_object)
        .and_then(|display| display.get("icon_key"))
        .and_then(Value::as_str)
        .and_then(ProviderIconKey::parse)
}

fn is_openai_model_candidate(value: &str) -> bool {
    value.contains("openai")
        || value.starts_with("gpt-")
        || value.contains("/gpt-")
        || value.starts_with("o1")
        || value.starts_with("o3")
        || value.starts_with("o4")
        || value.contains("/o1")
        || value.contains("/o3")
        || value.contains("/o4")
        || value.starts_with("text-embedding-")
        || value.starts_with("tts-")
        || value.starts_with("whisper-")
        || value.contains("codex")
        || value.contains("davinci")
        || value.contains("babbage")
}

#[cfg(test)]
mod tests {
    use gateway_core::ProviderConnection;
    use serde_json::json;

    use super::{ModelIconKey, ProviderIconKey, resolve_model_icon_key, resolve_provider_display};

    #[test]
    fn claude_wins_over_anthropic_for_model_icons() {
        let icon = resolve_model_icon_key(["anthropic/claude-sonnet-4-6", "anthropic"])
            .expect("claude icon");
        assert_eq!(icon, ModelIconKey::Claude);
    }

    #[test]
    fn provider_display_uses_configured_icon_key_when_present() {
        let provider = ProviderConnection {
            provider_key: "router".to_string(),
            provider_type: "openai_compat".to_string(),
            config: json!({
                "base_url": "https://openrouter.ai/api/v1",
                "display": {
                    "label": "OpenRouter",
                    "icon_key": "openrouter"
                }
            }),
            secrets: None,
        };

        let display = resolve_provider_display(&provider.provider_key, Some(&provider));
        assert_eq!(display.label, "OpenRouter");
        assert_eq!(display.icon_key, ProviderIconKey::OpenRouter);
    }

    #[test]
    fn vertex_defaults_to_vertex_ai_provider_icon() {
        let provider = ProviderConnection {
            provider_key: "vertex-claude".to_string(),
            provider_type: "gcp_vertex".to_string(),
            config: json!({"project_id": "demo"}),
            secrets: None,
        };

        let display = resolve_provider_display(&provider.provider_key, Some(&provider));
        assert_eq!(display.label, "Google Vertex AI");
        assert_eq!(display.icon_key, ProviderIconKey::VertexAI);
    }
}
