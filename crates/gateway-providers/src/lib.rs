pub mod anthropic;
mod anthropic_compat;
mod bedrock;
mod copilot;
mod http;
mod media;
mod openai_compat;
mod replay_id;
mod streaming;
mod token;
mod vertex;

pub use anthropic_compat::{
    AnthropicCompatAuth, AnthropicCompatAuthKind, AnthropicCompatConfig, AnthropicCompatProvider,
};
pub use bedrock::{BedrockAuthConfig, BedrockEndpointKind, BedrockProvider, BedrockProviderConfig};
pub use copilot::{
    CopilotAuthConfig, CopilotProvider, CopilotProviderConfig, DEFAULT_COPILOT_API_URL,
    DEFAULT_COPILOT_EDITOR_VERSION, DEFAULT_COPILOT_INTEGRATION_ID, DEFAULT_COPILOT_PLUGIN_VERSION,
};
pub use openai_compat::{
    BearerAuthHeader, CloudRunOpenAiCompatAuth, OpenAiBatchConfig, OpenAiBatchDialect,
    OpenAiCompatConfig, OpenAiCompatProvider,
};
pub use vertex::{VertexAuthConfig, VertexBatchConfig, VertexProvider, VertexProviderConfig};
