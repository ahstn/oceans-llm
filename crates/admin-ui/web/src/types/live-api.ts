import type { components, operations, paths } from '@/generated/admin-api'

export type GatewayPaths = paths
export type ResponseMeta = components['schemas']['ResponseMeta']

export interface ApiEnvelope<T> {
  data: T
  meta: ResponseMeta
}

export type ApiKeyOwnerUserView = components['schemas']['AdminApiKeyUserOwnerView']
export type ApiKeyOwnerServiceAccountView =
  components['schemas']['AdminApiKeyServiceAccountOwnerView']
export type ApiKeyModelOptionView = components['schemas']['AdminApiKeyModelView']
export type ApiKeyView = components['schemas']['AdminApiKeyView']
export type ApiKeysPayload = components['schemas']['AdminApiKeysPayload']
export type CreateApiKeyInput = components['schemas']['CreateApiKeyRequest']
export type CreateApiKeyResult = components['schemas']['CreateApiKeyResponse']
export type UpdateApiKeyInput = components['schemas']['UpdateApiKeyRequest']
export type UpdateApiKeyResult = components['schemas']['UpdateApiKeyResponse']
export type RevokeApiKeyResult = components['schemas']['RevokeApiKeyResponse']

export type SpendOwnerKind = 'all' | 'user' | 'service_account'

export type SpendTotalsView = components['schemas']['SpendTotalsView']
export type SpendDailyPointView = components['schemas']['SpendDailyPointView']
export type SpendOwnerBreakdownView = components['schemas']['SpendOwnerBreakdownView']
export type SpendModelBreakdownView = components['schemas']['SpendModelBreakdownView']
export type SpendReportView = components['schemas']['SpendReportView']
export type LeaderboardRange = '7d' | '31d'
export type LeaderboardChartUserView = components['schemas']['LeaderboardChartUserView']
export type LeaderboardSeriesValueView = components['schemas']['LeaderboardSeriesValueView']
export type LeaderboardSeriesPointView = components['schemas']['LeaderboardSeriesPointView']
export type LeaderboardLeaderView = components['schemas']['LeaderboardLeaderView']
export type LeaderboardView = components['schemas']['LeaderboardView']
export type HarnessUsageRange = '7d' | '31d'
export type HarnessUsageChartHarnessView = components['schemas']['HarnessUsageChartHarnessView']
export type HarnessUsageSeriesValueView = components['schemas']['HarnessUsageSeriesValueView']
export type HarnessUsageSeriesPointView = components['schemas']['HarnessUsageSeriesPointView']
export type HarnessUsageLeaderView = components['schemas']['HarnessUsageLeaderView']
export type HarnessUsageView = components['schemas']['HarnessUsageView']
export type BudgetSettingsView = components['schemas']['BudgetSettingsView']
export type BudgetScopeRequest = components['schemas']['BudgetScopeRequest']
export type BudgetScopeView = components['schemas']['BudgetScopeView']
export type SpendBudgetUserView = components['schemas']['SpendBudgetUserView']
export type SpendBudgetServiceAccountView = components['schemas']['SpendBudgetServiceAccountView']
export type SpendBudgetUserModelView = components['schemas']['SpendBudgetUserModelView']
export type SpendBudgetsView = components['schemas']['SpendBudgetsView']
export type BudgetAlertHistoryItemView = components['schemas']['BudgetAlertHistoryItemView']
export type BudgetAlertHistoryView = components['schemas']['BudgetAlertHistoryView']
export type UpsertBudgetInput = components['schemas']['UpsertBudgetRequest']
export type DeactivateBudgetInput = components['schemas']['DeactivateBudgetRequest']
export type UpsertBudgetResultView = components['schemas']['UpsertBudgetResultView']
export type DeactivateBudgetResultView = components['schemas']['DeactivateBudgetResultView']

export type ModelView = components['schemas']['AdminModelView']
export type ModelPageView = components['schemas']['AdminModelPageView']
export type ModelListQuery = NonNullable<operations['list_models']['parameters']['query']>
export type RequestTagView = components['schemas']['RequestTagView']
export type RequestLogTagsView = components['schemas']['RequestTagsView']
export type RequestLogView = components['schemas']['RequestLogSummaryView']
export type RequestLogPayloadView = components['schemas']['RequestLogPayloadView']
export type RequestLogDetailView = components['schemas']['RequestLogDetailView']
export type RequestLogPageView = components['schemas']['RequestLogPageView']
export type RequestLogFiltersInput = NonNullable<
  operations['list_request_logs']['parameters']['query']
>
export type McpInvocationView = components['schemas']['McpToolInvocationSummaryView']
export type McpInvocationPayloadView = components['schemas']['McpToolInvocationPayloadView']
export type McpInvocationDetailView = components['schemas']['McpToolInvocationDetailView']
export type McpInvocationPageView = components['schemas']['McpToolInvocationPageView']
export type McpInvocationFiltersInput = NonNullable<
  operations['list_mcp_tool_invocations']['parameters']['query']
>
export type McpInvocationStatus = NonNullable<McpInvocationFiltersInput['status']>
export type RecommendedMcpServerView = components['schemas']['RecommendedMcpServerView']
export type RecommendedMcpServersPayload = components['schemas']['RecommendedMcpServersPayload']
export type McpServerView = components['schemas']['McpServerView']
export type McpServersPayload = components['schemas']['McpServersPayload']
export type McpServerPayload = components['schemas']['McpServerPayload']
export type McpToolView = components['schemas']['McpToolView']
export type McpToolsPayload = components['schemas']['McpToolsPayload']
export type McpDiscoveryRefreshPayload = components['schemas']['McpDiscoveryRefreshPayload']
export type CreateMcpServerInput = components['schemas']['CreateMcpServerRequest']
export type UpdateMcpServerInput = components['schemas']['UpdateMcpServerRequest']
export type McpToolsetView = components['schemas']['McpToolsetView']
export type McpToolsetsPayload = components['schemas']['McpToolsetsPayload']
export type McpToolsetPayload = components['schemas']['McpToolsetPayload']
export type McpToolsetToolsPayload = components['schemas']['McpToolsetToolsPayload']
export type CreateMcpToolsetInput = components['schemas']['CreateMcpToolsetRequest']
export type UpdateMcpToolsetInput = components['schemas']['UpdateMcpToolsetRequest']
export type ReplaceMcpToolsetToolsInput = components['schemas']['ReplaceMcpToolsetToolsRequest']
export type McpGrantView = components['schemas']['McpGrantView']
export type McpGrantPayload = components['schemas']['McpGrantPayload']
export type McpGrantsPayload = components['schemas']['McpGrantsPayload']
export type UpsertMcpGrantInput = components['schemas']['UpsertMcpGrantRequest']
export type McpCredentialBindingView = components['schemas']['McpCredentialBindingView']
export type McpCredentialBindingPayload = components['schemas']['McpCredentialBindingPayload']
export type McpCredentialBindingsPayload = components['schemas']['McpCredentialBindingsPayload']
export type UpsertMcpCredentialBindingInput =
  components['schemas']['UpsertMcpCredentialBindingRequest']
export type McpCredentialBindingsQuery = NonNullable<
  operations['list_mcp_credential_bindings']['parameters']['query']
>
export type McpEffectiveAccessPayload = components['schemas']['McpEffectiveAccessPayload']
export type McpEffectiveAccessQuery = NonNullable<
  operations['preview_mcp_effective_access']['parameters']['query']
>
export type McpGrantsQuery = NonNullable<operations['list_mcp_grants']['parameters']['query']>

export type TeamAdminView = components['schemas']['AdminTeamAdminView']
export type TeamMemberView = components['schemas']['AdminTeamMemberView']
export type TeamManagementView = components['schemas']['AdminTeamManagementView']
export type TeamAssignableUserView = components['schemas']['AdminTeamAssignableUserView']
export type IdentityTeamsPayload = components['schemas']['AdminTeamsPayload']
export type CreateTeamInput = components['schemas']['CreateTeamRequest']
export type UpdateTeamInput = components['schemas']['UpdateTeamRequest']
export type AddTeamMembersInput = components['schemas']['AddTeamMembersRequest']
export type TransferTeamMemberInput = components['schemas']['TransferTeamMemberRequest']
export type AdminTeamOption = components['schemas']['AdminTeamView']
export type OidcProviderView = components['schemas']['AdminOidcProviderView']
export type UserOnboardingAction = components['schemas']['AdminOnboardingActionView']
export type UserView = components['schemas']['AdminIdentityUserView']
export type IdentityUsersPayload = components['schemas']['AdminIdentityPayload']
export type CreateUserInput = components['schemas']['CreateUserRequest']
export type UpdateUserInput = components['schemas']['UpdateUserRequest']
export type IdentityActionResult = components['schemas']['IdentityActionStatus']
export type CreateUserResult = components['schemas']['CreateUserResponse']
export type PasswordInviteResult = components['schemas']['PasswordInviteResponse']
export type InvitationStateView = components['schemas']['InvitationView']
export type AuthSessionUserView = components['schemas']['AuthSessionUserView']
export type AuthSessionView = components['schemas']['AuthSessionView']
export type PasswordLoginInput = components['schemas']['PasswordLoginRequest']
export type ChangePasswordInput = components['schemas']['ChangePasswordRequest']
export type LogoutResult = IdentityActionResult
