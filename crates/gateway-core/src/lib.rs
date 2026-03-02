pub mod auth;
pub mod domain;
pub mod error;
pub mod protocol;
pub mod traits;

pub use auth::{
    AuthenticatedApiKey, ParsedGatewayApiKey, extract_bearer_token, parse_gateway_api_key,
};
pub use domain::{
    ApiKeyRecord, GatewayModel, ModelRoute, ProviderConnection, ProviderRequestContext, SeedApiKey,
    SeedModel, SeedModelRoute, SeedProvider,
};
pub use error::{AuthError, GatewayError, ProviderError, RouteError, StoreError};
pub use protocol::openai::{
    ChatCompletionsRequest, EmbeddingsRequest, ModelsListResponse, OpenAiErrorBody,
    OpenAiErrorEnvelope,
};
pub use traits::{
    ApiKeyRepository, ModelRepository, ProviderClient, ProviderRegistry, ProviderRepository,
    ProviderStream, RoutePlanner, StoreHealth,
};
