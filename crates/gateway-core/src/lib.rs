pub mod auth;
pub mod domain;
pub mod error;
pub mod protocol;
pub mod traits;

pub use auth::{
    AuthenticatedApiKey, ParsedGatewayApiKey, extract_bearer_token, parse_gateway_api_key,
};
pub use domain::{
    ApiKeyOwnerKind, ApiKeyRecord, AuthMode, BudgetCadence, GatewayModel, GlobalRole,
    MembershipRole, ModelAccessMode, ModelRoute, Money4, ProviderCapabilities, ProviderConnection,
    ProviderRequestContext, RequestLogRecord, SYSTEM_LEGACY_TEAM_ID, SYSTEM_LEGACY_TEAM_KEY,
    SeedApiKey, SeedModel, SeedModelRoute, SeedProvider, TeamMembershipRecord, TeamRecord,
    UsageCostEventRecord, UserBudgetRecord, UserRecord,
};
pub use error::{AuthError, GatewayError, ProviderError, RouteError, StoreError};
pub use protocol::openai::{
    ChatCompletionsRequest, EmbeddingsRequest, ModelsListResponse, OpenAiErrorBody,
    OpenAiErrorEnvelope,
};
pub use traits::{
    ApiKeyRepository, BudgetRepository, IdentityRepository, ModelRepository, ProviderClient,
    ProviderRegistry, ProviderRepository, ProviderStream, RequestLogRepository, RoutePlanner,
    StoreHealth,
};
