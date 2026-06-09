pub mod auth;
pub mod budgets;
pub mod domain;
pub mod error;
pub mod protocol;
pub mod streaming;
pub mod traits;

pub use auth::{
    AuthenticatedApiKey, ParsedGatewayApiKey, extract_bearer_token, parse_gateway_api_key,
};
pub use budgets::{
    BudgetModelSelector, BudgetRecord, BudgetScope, BudgetScopeKind, BudgetSettings,
};
pub use domain::{
    ApiKeyOwnerKind, ApiKeyRecord, ApiKeyStatus, AuthMode, AwsBedrockApiStyle,
    AwsBedrockRouteCompatibility, BudgetAlertChannel, BudgetAlertDeliveryRecord,
    BudgetAlertDeliveryStatus, BudgetAlertDispatchTask, BudgetAlertHistoryPage,
    BudgetAlertHistoryQuery, BudgetAlertHistoryRecord, BudgetAlertRecord, BudgetCadence,
    BudgetWindow, ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpServerRecord, ExternalMcpServerStatus, ExternalMcpToolRecord, ExternalMcpTransport,
    FocusExportAggregateRecord, FocusExportDiagnosticsRecord, GatewayModel, GlobalRole,
    HarnessUsageBucketRecord, HarnessUsageLeaderRecord, IdentityUserRecord,
    MAX_MCP_TOOL_INVOCATION_PAGE_SIZE, McpToolInvocationDetail, McpToolInvocationPage,
    McpToolInvocationPayloadRecord, McpToolInvocationQuery, McpToolInvocationRecord,
    McpToolInvocationStatus, McpToolPolicyResult, MembershipRole, ModelAccessMode,
    ModelPricingRecord, ModelRoute, Money4, NewApiKeyRecord, NewExternalMcpServerRecord,
    OauthJitMembership, OauthJitPolicy, OauthLoginStateRecord, OauthProviderRecord,
    OidcJitMembership, OidcJitPolicy, OidcLoginStateRecord, OidcProviderRecord,
    OpenAiCompatDeveloperRole, OpenAiCompatMaxTokensField, OpenAiCompatReasoningEffort,
    OpenAiCompatRouteCompatibility, PasswordInvitationRecord, PricingCatalogCacheRecord,
    PricingLimits, PricingModalities, PricingProvenance, PricingResolution, PricingUnpricedReason,
    ProviderCapabilities, ProviderConnection, ProviderRequestContext, RequestAttemptRecord,
    RequestAttemptStatus, RequestLogDetail, RequestLogPage, RequestLogPayloadRecord,
    RequestLogPurgeResult, RequestLogQuery, RequestLogRecord, RequestLogRetentionWindow,
    RequestTag, RequestTags, RequestToolCardinality, RequestToolCardinalityAverages,
    ResolvedModelPricing, RouteCompatibility, SYSTEM_BOOTSTRAP_ADMIN_EMAIL,
    SYSTEM_BOOTSTRAP_ADMIN_USER_ID, SeedApiKey, SeedBudget, SeedModel, SeedModelRoute,
    SeedOauthProvider, SeedOidcProvider, SeedProvider, SeedTeam, SeedUser, SeedUserMembership,
    ServiceAccountRecord, ServiceAccountStatus, SpendDailyAggregateRecord,
    SpendModelAggregateRecord, SpendOwnerAggregateRecord, TeamMembershipRecord, TeamRecord,
    UpdateExternalMcpServerRecord, UpsertExternalMcpToolRecord, UsageLeaderboardBucketRecord,
    UsageLeaderboardUserRecord, UsageLedgerRecord, UsagePricingStatus, UserOauthAuthRecord,
    UserOidcAuthRecord, UserPasswordAuthRecord, UserRecord, UserSessionRecord, UserStatus,
    budget_window_utc,
};
pub use error::{AuthError, GatewayError, ProviderError, RouteError, StoreError};
pub use protocol::core::{
    ChatMessage as CoreChatMessage, ChatRequest as CoreChatRequest,
    EmbeddingsRequest as CoreEmbeddingsRequest, RequestRequirements as CoreRequestRequirements,
    ResponsesRequest as CoreResponsesRequest,
};
pub use protocol::openai::{
    ChatCompletionsRequest, EmbeddingsRequest, ModelsListResponse, OpenAiErrorBody,
    OpenAiErrorEnvelope, ResponseOutputItem, ResponseUsage, ResponsesRequest, ResponsesResponse,
    ResponsesStreamEvent,
};
pub use protocol::translate::{
    core_chat_request_to_openai, core_embeddings_request_to_openai,
    core_responses_request_to_openai, openai_chat_request_to_core,
    openai_embeddings_request_to_core, openai_responses_request_to_core,
};
pub use streaming::{ParsedSseEvent, SseEventParser, Utf8ChunkDecoder};
pub use traits::{
    AdminApiKeyRepository, AdminIdentityRepository, ApiKeyRepository, BudgetAlertRepository,
    BudgetRepository, IdentityRepository, McpRegistryRepository, McpToolInvocationRepository,
    ModelRepository, PricingCatalogRepository, ProviderClient, ProviderRegistry,
    ProviderRepository, ProviderStream, RequestAttemptRepository, RequestLogRepository,
    RoutePlanner, StoreHealth,
};
