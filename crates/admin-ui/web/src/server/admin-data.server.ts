import type {
  AddTeamMembersInput,
  AuthSessionView,
  ChangePasswordInput,
  CreateUserInput,
  CreateUserResult,
  IdentityUsersPayload,
  IdentityTeamsPayload,
  InvitationStateView,
  ApiEnvelope,
  ApiKeyView,
  TeamManagementView,
  ModelView,
  Paginated,
  PasswordInviteResult,
  RequestLogView,
  SpendBudgetsView,
  SpendOwnerKind,
  SpendReportView,
  UpsertBudgetInput,
  UpsertBudgetResultView,
  DeactivateBudgetResultView,
  CreateTeamInput,
  PasswordLoginInput,
  UpdateTeamInput,
} from '@/types/api'
import { fetchGatewayJson } from '@/server/gateway-client.server'

function envelope<T>(data: T): ApiEnvelope<T> {
  return {
    data,
    meta: {
      generatedAt: new Date().toISOString(),
    },
  }
}

export async function listApiKeys(): Promise<ApiEnvelope<Paginated<ApiKeyView>>> {
  const items: ApiKeyView[] = [
    {
      id: 'k_01',
      name: 'Production Gateway',
      prefix: 'gwk_prod',
      createdAt: '2026-02-21',
      status: 'active',
    },
    {
      id: 'k_02',
      name: 'Staging CI',
      prefix: 'gwk_stg',
      createdAt: '2026-02-20',
      status: 'active',
    },
    {
      id: 'k_03',
      name: 'Legacy Mobile',
      prefix: 'gwk_old',
      createdAt: '2026-01-30',
      status: 'revoked',
    },
  ]

  return envelope({
    items,
    page: 1,
    pageSize: items.length,
    total: items.length,
  })
}

export async function listModels(): Promise<ApiEnvelope<ModelView[]>> {
  return envelope([
    {
      id: 'fast',
      provider: 'openrouter',
      upstreamModel: 'google/gemini-2.0-flash',
      tags: ['fast', 'cheap'],
      status: 'healthy',
    },
    {
      id: 'reasoning',
      provider: 'openai-prod',
      upstreamModel: 'o3-mini',
      tags: ['reasoning'],
      status: 'healthy',
    },
    {
      id: 'backup-fast',
      provider: 'vertex-gemini',
      upstreamModel: 'gemini-2.0-flash',
      tags: ['fast', 'fallback'],
      status: 'degraded',
    },
  ])
}

export async function getSpendReport(params?: {
  days?: number
  owner_kind?: SpendOwnerKind
}): Promise<ApiEnvelope<SpendReportView>> {
  const query = new URLSearchParams()
  if (params?.days) {
    query.set('days', String(params.days))
  }
  if (params?.owner_kind && params.owner_kind !== 'all') {
    query.set('owner_kind', params.owner_kind)
  } else if (params?.owner_kind === 'all') {
    query.set('owner_kind', 'all')
  }
  const suffix = query.toString() ? `?${query.toString()}` : ''
  return fetchGatewayJson<ApiEnvelope<SpendReportView>>(`/api/v1/admin/spend/report${suffix}`)
}

export async function listSpendBudgets(): Promise<ApiEnvelope<SpendBudgetsView>> {
  return fetchGatewayJson<ApiEnvelope<SpendBudgetsView>>('/api/v1/admin/spend/budgets')
}

export async function upsertUserBudget(
  userId: string,
  input: UpsertBudgetInput,
): Promise<ApiEnvelope<UpsertBudgetResultView>> {
  return fetchGatewayJson<ApiEnvelope<UpsertBudgetResultView>>(
    `/api/v1/admin/spend/budgets/users/${userId}`,
    {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    },
  )
}

export async function deactivateUserBudget(
  userId: string,
): Promise<ApiEnvelope<DeactivateBudgetResultView>> {
  return fetchGatewayJson<ApiEnvelope<DeactivateBudgetResultView>>(
    `/api/v1/admin/spend/budgets/users/${userId}`,
    {
      method: 'DELETE',
    },
  )
}

export async function upsertTeamBudget(
  teamId: string,
  input: UpsertBudgetInput,
): Promise<ApiEnvelope<UpsertBudgetResultView>> {
  return fetchGatewayJson<ApiEnvelope<UpsertBudgetResultView>>(
    `/api/v1/admin/spend/budgets/teams/${teamId}`,
    {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    },
  )
}

export async function deactivateTeamBudget(
  teamId: string,
): Promise<ApiEnvelope<DeactivateBudgetResultView>> {
  return fetchGatewayJson<ApiEnvelope<DeactivateBudgetResultView>>(
    `/api/v1/admin/spend/budgets/teams/${teamId}`,
    {
      method: 'DELETE',
    },
  )
}

export async function listRequestLogs(): Promise<ApiEnvelope<Paginated<RequestLogView>>> {
  const items: RequestLogView[] = Array.from({ length: 500 }, (_, index) => ({
    id: `req_${index + 1}`,
    model: index % 3 === 0 ? 'fast' : index % 3 === 1 ? 'reasoning' : 'backup-fast',
    provider: index % 2 === 0 ? 'openrouter' : 'openai-prod',
    statusCode: index % 11 === 0 ? 500 : 200,
    latencyMs: 120 + (index % 9) * 35,
    tokens: 256 + (index % 7) * 64,
    timestamp: `2026-02-26T${String(index % 24).padStart(2, '0')}:${String(index % 60).padStart(2, '0')}:00Z`,
  }))

  return envelope({
    items,
    page: 1,
    pageSize: items.length,
    total: items.length,
  })
}

export async function listTeams(): Promise<ApiEnvelope<IdentityTeamsPayload>> {
  return fetchGatewayJson<ApiEnvelope<IdentityTeamsPayload>>('/api/v1/admin/identity/teams')
}

export async function createTeam(input: CreateTeamInput): Promise<ApiEnvelope<TeamManagementView>> {
  return fetchGatewayJson<ApiEnvelope<TeamManagementView>>('/api/v1/admin/identity/teams', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(input),
  })
}

export async function updateTeam(
  teamId: string,
  input: UpdateTeamInput,
): Promise<ApiEnvelope<TeamManagementView>> {
  return fetchGatewayJson<ApiEnvelope<TeamManagementView>>(
    `/api/v1/admin/identity/teams/${teamId}`,
    {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    },
  )
}

export async function addTeamMembers(
  teamId: string,
  input: AddTeamMembersInput,
): Promise<ApiEnvelope<TeamManagementView>> {
  return fetchGatewayJson<ApiEnvelope<TeamManagementView>>(
    `/api/v1/admin/identity/teams/${teamId}/members`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    },
  )
}

export async function getSession(): Promise<ApiEnvelope<AuthSessionView | null>> {
  return fetchGatewayJson<ApiEnvelope<AuthSessionView | null>>('/api/v1/auth/session')
}

export async function loginWithPassword(
  input: PasswordLoginInput,
): Promise<ApiEnvelope<AuthSessionView>> {
  return fetchGatewayJson<ApiEnvelope<AuthSessionView>>('/api/v1/auth/login/password', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(input),
  })
}

export async function changePassword(
  input: ChangePasswordInput,
): Promise<ApiEnvelope<AuthSessionView>> {
  return fetchGatewayJson<ApiEnvelope<AuthSessionView>>('/api/v1/auth/password/change', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(input),
  })
}

export async function listUsers(): Promise<ApiEnvelope<IdentityUsersPayload>> {
  return fetchGatewayJson<ApiEnvelope<IdentityUsersPayload>>('/api/v1/admin/identity/users')
}

export async function createUser(input: CreateUserInput): Promise<ApiEnvelope<CreateUserResult>> {
  return fetchGatewayJson<ApiEnvelope<CreateUserResult>>('/api/v1/admin/identity/users', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(input),
  })
}

export async function resendPasswordInvite(
  userId: string,
): Promise<ApiEnvelope<PasswordInviteResult>> {
  return fetchGatewayJson<ApiEnvelope<PasswordInviteResult>>(
    `/api/v1/admin/identity/users/${userId}/password-invite`,
    {
      method: 'POST',
    },
  )
}

export async function getInvitation(token: string): Promise<ApiEnvelope<InvitationStateView>> {
  return fetchGatewayJson<ApiEnvelope<InvitationStateView>>(
    `/api/v1/auth/invitations/${encodeURIComponent(token)}`,
  )
}

export async function completeInvitation(
  token: string,
  password: string,
): Promise<ApiEnvelope<{ status: string }>> {
  return fetchGatewayJson<ApiEnvelope<{ status: string }>>(
    `/api/v1/auth/invitations/${encodeURIComponent(token)}/password`,
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ password }),
    },
  )
}
