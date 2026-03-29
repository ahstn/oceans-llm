import type {
  AddTeamMembersInput,
  ApiEnvelope,
  ApiKeysPayload,
  AuthSessionView,
  BudgetAlertHistoryView,
  ChangePasswordInput,
  CreateApiKeyInput,
  CreateApiKeyResult,
  CreateTeamInput,
  CreateUserInput,
  CreateUserResult,
  DeactivateBudgetResultView,
  IdentityActionResult,
  IdentityTeamsPayload,
  IdentityUsersPayload,
  InvitationStateView,
  PasswordInviteResult,
  PasswordLoginInput,
  RequestLogDetailView,
  RequestLogFiltersInput,
  RequestLogPageView,
  RevokeApiKeyResult,
  SpendBudgetsView,
  SpendOwnerKind,
  SpendReportView,
  TeamManagementView,
  TransferTeamMemberInput,
  UpdateTeamInput,
  UpdateUserInput,
  UpsertBudgetInput,
  UpsertBudgetResultView,
} from '@/types/api'
import {
  createGatewayApiClient,
  fetchGatewayJson,
  unwrapGatewayResponse,
} from '@/server/gateway-client.server'

export async function listApiKeys(): Promise<ApiEnvelope<ApiKeysPayload>> {
  return fetchGatewayJson<ApiEnvelope<ApiKeysPayload>>('/api/v1/admin/api-keys')
}

export async function createApiKey(
  input: CreateApiKeyInput,
): Promise<ApiEnvelope<CreateApiKeyResult>> {
  return fetchGatewayJson<ApiEnvelope<CreateApiKeyResult>>('/api/v1/admin/api-keys', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(input),
  })
}

export async function revokeApiKey(
  apiKeyId: string,
): Promise<ApiEnvelope<RevokeApiKeyResult>> {
  return fetchGatewayJson<ApiEnvelope<RevokeApiKeyResult>>(
    `/api/v1/admin/api-keys/${apiKeyId}/revoke`,
    {
      method: 'POST',
    },
  )
}

export async function getSpendReport(params?: {
  days?: number
  owner_kind?: SpendOwnerKind
}): Promise<ApiEnvelope<SpendReportView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.GET('/api/v1/admin/spend/report', {
      params: {
        query: {
          days: params?.days,
          owner_kind: params?.owner_kind,
        },
      },
    }),
  )
}

export async function listSpendBudgets(): Promise<ApiEnvelope<SpendBudgetsView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(await client.GET('/api/v1/admin/spend/budgets'))
}

export async function listBudgetAlertHistory(params?: {
  page?: number
  page_size?: number
  owner_kind?: SpendOwnerKind
  status?: 'all' | 'pending' | 'sent' | 'failed'
  channel?: 'all' | 'email'
}): Promise<ApiEnvelope<BudgetAlertHistoryView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.GET('/api/v1/admin/spend/budget-alerts', {
      params: {
        query: {
          page: params?.page,
          page_size: params?.page_size,
          owner_kind: params?.owner_kind,
          status: params?.status,
          channel: params?.channel,
        },
      },
    }),
  )
}

export async function upsertUserBudget(
  userId: string,
  input: UpsertBudgetInput,
): Promise<ApiEnvelope<UpsertBudgetResultView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.PUT('/api/v1/admin/spend/budgets/users/{user_id}', {
      params: { path: { user_id: userId } },
      body: input,
    }),
  )
}

export async function deactivateUserBudget(
  userId: string,
): Promise<ApiEnvelope<DeactivateBudgetResultView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.DELETE('/api/v1/admin/spend/budgets/users/{user_id}', {
      params: { path: { user_id: userId } },
    }),
  )
}

export async function upsertTeamBudget(
  teamId: string,
  input: UpsertBudgetInput,
): Promise<ApiEnvelope<UpsertBudgetResultView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.PUT('/api/v1/admin/spend/budgets/teams/{team_id}', {
      params: { path: { team_id: teamId } },
      body: input,
    }),
  )
}

export async function deactivateTeamBudget(
  teamId: string,
): Promise<ApiEnvelope<DeactivateBudgetResultView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.DELETE('/api/v1/admin/spend/budgets/teams/{team_id}', {
      params: { path: { team_id: teamId } },
    }),
  )
}

export async function listRequestLogs(
  filters: RequestLogFiltersInput = {},
): Promise<ApiEnvelope<RequestLogPageView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.GET('/api/v1/admin/observability/request-logs', {
      params: {
        query: filters,
      },
    }),
  )
}

export async function getRequestLogDetail(
  requestLogId: string,
): Promise<ApiEnvelope<RequestLogDetailView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.GET('/api/v1/admin/observability/request-logs/{request_log_id}', {
      params: { path: { request_log_id: requestLogId } },
    }),
  )
}

export async function listTeams(): Promise<ApiEnvelope<IdentityTeamsPayload>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(await client.GET('/api/v1/admin/identity/teams'))
}

export async function createTeam(input: CreateTeamInput): Promise<ApiEnvelope<TeamManagementView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(await client.POST('/api/v1/admin/identity/teams', { body: input }))
}

export async function updateTeam(
  teamId: string,
  input: UpdateTeamInput,
): Promise<ApiEnvelope<TeamManagementView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.PATCH('/api/v1/admin/identity/teams/{team_id}', {
      params: { path: { team_id: teamId } },
      body: input,
    }),
  )
}

export async function addTeamMembers(
  teamId: string,
  input: AddTeamMembersInput,
): Promise<ApiEnvelope<TeamManagementView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.POST('/api/v1/admin/identity/teams/{team_id}/members', {
      params: { path: { team_id: teamId } },
      body: input,
    }),
  )
}

export async function removeTeamMember(
  teamId: string,
  userId: string,
): Promise<ApiEnvelope<IdentityActionResult>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.DELETE('/api/v1/admin/identity/teams/{team_id}/members/{user_id}', {
      params: { path: { team_id: teamId, user_id: userId } },
    }),
  )
}

export async function transferTeamMember(
  teamId: string,
  userId: string,
  input: TransferTeamMemberInput,
): Promise<ApiEnvelope<IdentityActionResult>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.POST('/api/v1/admin/identity/teams/{team_id}/members/{user_id}/transfer', {
      params: { path: { team_id: teamId, user_id: userId } },
      body: input,
    }),
  )
}

export async function getSession(): Promise<ApiEnvelope<AuthSessionView | null>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(await client.GET('/api/v1/auth/session'))
}

export async function loginWithPassword(
  input: PasswordLoginInput,
): Promise<ApiEnvelope<AuthSessionView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(await client.POST('/api/v1/auth/login/password', { body: input }))
}

export async function changePassword(
  input: ChangePasswordInput,
): Promise<ApiEnvelope<AuthSessionView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(await client.POST('/api/v1/auth/password/change', { body: input }))
}

export async function listUsers(): Promise<ApiEnvelope<IdentityUsersPayload>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(await client.GET('/api/v1/admin/identity/users'))
}

export async function createUser(input: CreateUserInput): Promise<ApiEnvelope<CreateUserResult>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(await client.POST('/api/v1/admin/identity/users', { body: input }))
}

export async function updateUser(
  userId: string,
  input: UpdateUserInput,
): Promise<ApiEnvelope<IdentityActionResult>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.PATCH('/api/v1/admin/identity/users/{user_id}', {
      params: { path: { user_id: userId } },
      body: input,
    }),
  )
}

export async function deactivateUser(
  userId: string,
): Promise<ApiEnvelope<IdentityActionResult>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.POST('/api/v1/admin/identity/users/{user_id}/deactivate', {
      params: { path: { user_id: userId } },
    }),
  )
}

export async function reactivateUser(
  userId: string,
): Promise<ApiEnvelope<IdentityActionResult>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.POST('/api/v1/admin/identity/users/{user_id}/reactivate', {
      params: { path: { user_id: userId } },
    }),
  )
}

export async function resetUserOnboarding(
  userId: string,
): Promise<ApiEnvelope<CreateUserResult>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.POST('/api/v1/admin/identity/users/{user_id}/reset-onboarding', {
      params: { path: { user_id: userId } },
    }),
  )
}

export async function resendPasswordInvite(
  userId: string,
): Promise<ApiEnvelope<PasswordInviteResult>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.POST('/api/v1/admin/identity/users/{user_id}/password-invite', {
      params: { path: { user_id: userId } },
    }),
  )
}

export async function getInvitation(
  token: string,
): Promise<ApiEnvelope<InvitationStateView>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.GET('/api/v1/auth/invitations/{token}', {
      params: { path: { token } },
    }),
  )
}

export async function completeInvitation(
  token: string,
  password: string,
): Promise<ApiEnvelope<{ status: string }>> {
  const client = createGatewayApiClient()
  return unwrapGatewayResponse(
    await client.POST('/api/v1/auth/invitations/{token}/password', {
      params: { path: { token } },
      body: { password },
    }),
  )
}
