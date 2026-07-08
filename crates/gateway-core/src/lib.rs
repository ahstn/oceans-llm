pub mod auth;
pub mod budgets;
pub mod domain;
pub mod error;
pub mod gateway_keys;
pub mod protocol;
pub mod streaming;
pub mod traits;

pub use auth::{
    AuthenticatedApiKey, ParsedGatewayApiKey, extract_bearer_token, parse_gateway_api_key,
};
pub use budgets::{
    BudgetModelSelector, BudgetRecord, BudgetScope, BudgetScopeKind, BudgetSettings, BudgetSource,
    BudgetSourceKind,
};
pub use domain::{
    ApiKeyModelGrantMode, ApiKeyOwnerKind, ApiKeyRecord, ApiKeySecretMaterialRecord,
    ApiKeySecretStorageKind, ApiKeyStatus, AuthMode, AwsBedrockApiStyle,
    AwsBedrockRouteCompatibility, BudgetAlertChannel, BudgetAlertDeliveryRecord,
    BudgetAlertDeliveryStatus, BudgetAlertDispatchTask, BudgetAlertHistoryPage,
    BudgetAlertHistoryQuery, BudgetAlertHistoryRecord, BudgetAlertRecord, BudgetCadence,
    BudgetWindow, ExternalMcpAuthMode, ExternalMcpDiscoveryRunRecord, ExternalMcpDiscoveryStatus,
    ExternalMcpServerRecord, ExternalMcpServerStatus, ExternalMcpToolRecord, ExternalMcpTransport,
    FocusExportAggregateRecord, FocusExportDiagnosticsRecord, GatewayModel, GlobalRole,
    HarnessUsageBucketRecord, HarnessUsageLeaderRecord, IdentityUserRecord, MAX_ENTITY_TAGS,
    MAX_MCP_TOOL_INVOCATION_PAGE_SIZE, MAX_TAG_KEY_LEN, MAX_TAG_VALUE_LEN, ManagedApiKeySource,
    McpAccessResolution, McpAggregateSessionRecord, McpCatalogAccessResolution,
    McpCatalogToolRecord, McpGrantSubject, McpTokenEstimateConfidence, McpTokenEstimateSource,
    McpToolGrantRecord, McpToolGrantSubjectKind, McpToolGrantTargetKind, McpToolInvocationDetail,
    McpToolInvocationPage, McpToolInvocationPayloadRecord, McpToolInvocationQuery,
    McpToolInvocationRecord, McpToolInvocationStatus, McpToolPolicyResult,
    McpToolTokenEstimateRecord, McpToolsetRecord, McpToolsetStatus, McpToolsetToolRecord,
    McpUpstreamCredentialBindingRecord, McpUpstreamCredentialMaterialKind,
    McpUpstreamCredentialOwnerScopeKind, McpUpstreamSecretStorageKind, MembershipRole,
    ModelAccessMode, ModelAllowlistPolicy, ModelPricingRecord, ModelRoute, Money4, NewApiKeyRecord,
    NewExternalMcpServerRecord, NewMcpAggregateSessionRecord, NewMcpToolsetRecord,
    NewReviewAgentRepositoryRecord, NewReviewAgentRunRecord, OauthJitMembership, OauthJitPolicy,
    OauthLoginStateRecord, OauthProviderRecord, OidcJitMembership, OidcJitPolicy,
    OidcLoginStateRecord, OidcProviderRecord, OpenAiCompatDeveloperRole,
    OpenAiCompatMaxTokensField, OpenAiCompatReasoningEffort, OpenAiCompatRouteCompatibility,
    OpenRouterMaxPrice, OpenRouterPercentileCutoffs, OpenRouterPercentilePreference,
    OpenRouterProviderRouting, OpenRouterRouteCompatibility, PasswordInvitationRecord,
    PricingCatalogCacheRecord, PricingLimits, PricingModalities, PricingProvenance,
    PricingResolution, PricingUnpricedReason, ProviderCapabilities, ProviderConnection,
    ProviderRequestContext, RequestAttemptRecord, RequestAttemptStatus, RequestLogDetail,
    RequestLogPage, RequestLogPayloadRecord, RequestLogPurgeResult, RequestLogQuery,
    RequestLogRecord, RequestLogRetentionWindow, RequestMcpTokenOverheadRecord, RequestTag,
    RequestTags, RequestToolCardinality, RequestToolCardinalityAverages, ResolvedModelPricing,
    ReviewAgentProvider, ReviewAgentPullRequestRecord, ReviewAgentPullRequestState,
    ReviewAgentRepositoryRecord, ReviewAgentRepositoryStatus, ReviewAgentRunRecord,
    ReviewAgentRunStatus, ReviewAgentSettings, RouteCompatibility, SYSTEM_BOOTSTRAP_ADMIN_EMAIL,
    SYSTEM_BOOTSTRAP_ADMIN_USER_ID, SeedApiKey, SeedApiKeySecretMaterial, SeedBudget,
    SeedHumanBudgetDefaults, SeedManagedServiceAccountApiKey, SeedModel, SeedModelRoute,
    SeedOauthProvider, SeedOidcProvider, SeedProvider, SeedServiceAccount, SeedTeam, SeedUser,
    SeedUserMembership, SeedUserModelBudgetDefault, ServiceAccountRecord, ServiceAccountStatus,
    SpendDailyAggregateRecord, SpendModelAggregateRecord, SpendOwnerAggregateRecord,
    TeamMembershipRecord, TeamRecord, UpdateExternalMcpServerRecord, UpdateMcpToolsetRecord,
    UpdateReviewAgentRepositoryRecord, UpdateReviewAgentRunRecord, UpsertExternalMcpToolRecord,
    UpsertMcpToolGrantRecord, UpsertMcpUpstreamCredentialBindingRecord,
    UpsertReviewAgentPullRequestRecord, UsageLeaderboardBucketRecord, UsageLeaderboardUserRecord,
    UsageLedgerRecord, UsagePricingStatus, UserOauthAuthRecord, UserOidcAuthRecord,
    UserPasswordAuthRecord, UserRecord, UserSessionRecord, UserStatus,
    VERTEX_TEXT_EMBEDDING_MODEL_IDS, budget_window_utc,
    is_supported_vertex_text_embedding_model_id, is_supported_vertex_text_embedding_upstream_model,
    validate_entity_tags, validate_tag_key, validate_tag_value,
    vertex_route_capabilities_for_upstream_model, vertex_text_embedding_capabilities,
};
pub use error::{AuthError, GatewayError, ProviderError, RouteError, StoreError};
pub use gateway_keys::{
    EncryptedSecret, GATEWAY_API_KEY_SECRET_KEY_ENV, GATEWAY_API_KEY_SECRET_KEY_ID,
    decrypt_gateway_api_key_secret, decrypt_secret_with_key, encrypt_gateway_api_key_secret,
    encrypt_secret_with_key, generate_gateway_api_key_value, hash_gateway_key_secret,
    validate_secret_key_env,
};
pub use protocol::anthropic::{AnthropicMessage, AnthropicMessagesRequest};
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
    anthropic_messages_request_to_core, core_chat_request_to_openai,
    core_embeddings_request_to_openai, core_responses_request_to_openai,
    openai_chat_request_to_core, openai_embeddings_request_to_core,
    openai_responses_request_to_core,
};
pub use streaming::{ParsedSseEvent, SseEventParser, Utf8ChunkDecoder};
pub use traits::{
    AdminApiKeyRepository, AdminIdentityRepository, ApiKeyRepository, BudgetAlertRepository,
    BudgetRepository, IdentityRepository, McpAccessRepository, McpAggregateSessionRepository,
    McpRegistryRepository, McpTokenOverheadRepository, McpToolInvocationRepository,
    McpUpstreamCredentialRepository, ModelRepository, PricingCatalogRepository, ProviderClient,
    ProviderRegistry, ProviderRepository, ProviderStream, RequestAttemptRepository,
    RequestLogRepository, ReviewAgentRepository, RoutePlanner, StoreHealth,
};
