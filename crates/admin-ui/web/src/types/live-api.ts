import type { components, operations, paths } from '@/generated/admin-api'

export type GatewayPaths = paths
export type ResponseMeta = components['schemas']['ResponseMeta']

export interface ApiEnvelope<T> {
  data: T
  meta: ResponseMeta
}

export type SpendOwnerKind = 'all' | 'user' | 'team'

export type SpendTotalsView = components['schemas']['SpendTotalsView']
export type SpendDailyPointView = components['schemas']['SpendDailyPointView']
export type SpendOwnerBreakdownView = components['schemas']['SpendOwnerBreakdownView']
export type SpendModelBreakdownView = components['schemas']['SpendModelBreakdownView']
export type SpendReportView = components['schemas']['SpendReportView']
export type BudgetSettingsView = components['schemas']['BudgetSettingsView']
export type SpendBudgetUserView = components['schemas']['SpendBudgetUserView']
export type SpendBudgetTeamView = components['schemas']['SpendBudgetTeamView']
export type SpendBudgetsView = components['schemas']['SpendBudgetsView']
export type BudgetAlertHistoryItemView = components['schemas']['BudgetAlertHistoryItemView']
export type BudgetAlertHistoryView = components['schemas']['BudgetAlertHistoryView']
export type UpsertBudgetInput = components['schemas']['UpsertBudgetRequest']
export type UpsertBudgetResultView = components['schemas']['UpsertBudgetResultView']
export type DeactivateBudgetResultView = components['schemas']['DeactivateBudgetResultView']

export type RequestTagView = components['schemas']['RequestTagView']
export type RequestLogTagsView = components['schemas']['RequestTagsView']
export type RequestLogView = components['schemas']['RequestLogSummaryView']
export type RequestLogPayloadView = components['schemas']['RequestLogPayloadView']
export type RequestLogDetailView = components['schemas']['RequestLogDetailView']
export type RequestLogPageView = components['schemas']['RequestLogPageView']
export type RequestLogFiltersInput = NonNullable<
  operations['list_request_logs']['parameters']['query']
>

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
