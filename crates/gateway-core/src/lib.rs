pub mod auth;
pub mod domain;
pub mod error;
pub mod protocol;
pub mod streaming;
pub mod traits;

pub use auth::{
    AuthenticatedApiKey, ParsedGatewayApiKey, extract_bearer_token, parse_gateway_api_key,
};
pub use domain::{
    ApiKeyOwnerKind, ApiKeyRecord, ApiKeyStatus, AuthMode, BudgetAlertChannel,
    BudgetAlertDeliveryRecord, BudgetAlertDeliveryStatus, BudgetAlertDispatchTask,
    BudgetAlertHistoryPage, BudgetAlertHistoryQuery, BudgetAlertHistoryRecord, BudgetAlertRecord,
    BudgetCadence, BudgetWindow, GatewayModel, GlobalRole, IdentityUserRecord, MembershipRole,
    ModelAccessMode, ModelPricingRecord, ModelRoute, Money4, NewApiKeyRecord, OidcProviderRecord,
    PasswordInvitationRecord, PricingCatalogCacheRecord, PricingLimits, PricingModalities,
    PricingProvenance, PricingResolution, PricingUnpricedReason, ProviderCapabilities,
    ProviderConnection, ProviderRequestContext, RequestLogDetail, RequestLogPage,
    RequestLogPayloadRecord, RequestLogQuery, RequestLogRecord, RequestTag, RequestTags,
    ResolvedModelPricing, SYSTEM_BOOTSTRAP_ADMIN_EMAIL, SYSTEM_BOOTSTRAP_ADMIN_USER_ID,
    SYSTEM_LEGACY_TEAM_ID, SYSTEM_LEGACY_TEAM_KEY, SeedApiKey, SeedBudget, SeedModel,
    SeedModelRoute, SeedProvider, SeedTeam, SeedUser, SeedUserMembership,
    SpendDailyAggregateRecord, SpendModelAggregateRecord, SpendOwnerAggregateRecord,
    TeamBudgetRecord, TeamMembershipRecord, TeamRecord, UsageLedgerRecord, UsagePricingStatus,
    UserBudgetRecord, UserOidcAuthRecord, UserPasswordAuthRecord, UserRecord, UserSessionRecord,
    UserStatus, budget_window_utc,
};
pub use error::{AuthError, GatewayError, ProviderError, RouteError, StoreError};
pub use protocol::core::{
    ChatMessage as CoreChatMessage, ChatRequest as CoreChatRequest,
    EmbeddingsRequest as CoreEmbeddingsRequest, RequestRequirements as CoreRequestRequirements,
};
pub use protocol::openai::{
    ChatCompletionsRequest, EmbeddingsRequest, ModelsListResponse, OpenAiErrorBody,
    OpenAiErrorEnvelope,
};
pub use protocol::translate::{
    core_chat_request_to_openai, core_embeddings_request_to_openai, openai_chat_request_to_core,
    openai_embeddings_request_to_core,
};
pub use streaming::{ParsedSseEvent, SseEventParser, Utf8ChunkDecoder};
pub use traits::{
    AdminApiKeyRepository, AdminIdentityRepository, ApiKeyRepository, BudgetAlertRepository,
    BudgetRepository, IdentityRepository, ModelRepository, PricingCatalogRepository,
    ProviderClient, ProviderRegistry, ProviderRepository, ProviderStream, RequestLogRepository,
    RoutePlanner, StoreHealth,
};
