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
    IdentityUserRecord, MembershipRole, ModelAccessMode, ModelRoute, Money4, OidcProviderRecord,
    PasswordInvitationRecord, PricingCatalogCacheRecord, PricingLimits, PricingModalities,
    PricingProvenance, PricingResolution, PricingUnpricedReason, ProviderCapabilities,
    ProviderConnection, ProviderRequestContext, RequestLogBundle, RequestLogPayloadRecord,
    RequestLogRecord, ResolvedModelPricing, SYSTEM_BOOTSTRAP_ADMIN_EMAIL,
    SYSTEM_BOOTSTRAP_ADMIN_USER_ID, SYSTEM_LEGACY_TEAM_ID, SYSTEM_LEGACY_TEAM_KEY, SeedApiKey,
    SeedModel, SeedModelRoute, SeedProvider, TeamMembershipRecord, TeamRecord,
    UsageCostEventRecord, UserBudgetRecord, UserOidcAuthRecord, UserPasswordAuthRecord,
    UserRecord, UserSessionRecord,
};
pub use error::{AuthError, GatewayError, ProviderError, RouteError, StoreError};
pub use protocol::openai::{
    ChatCompletionsRequest, EmbeddingsRequest, ModelsListResponse, OpenAiErrorBody,
    OpenAiErrorEnvelope,
};
pub use traits::{
    ApiKeyRepository, BudgetRepository, IdentityRepository, ModelRepository,
    PricingCatalogRepository, ProviderClient, ProviderRegistry, ProviderRepository, ProviderStream,
    RequestLogRepository, RoutePlanner, StoreHealth,
};
