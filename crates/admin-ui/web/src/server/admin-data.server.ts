import type {
  AddTeamMembersInput,
  AuthSessionView,
  BudgetAlertHistoryView,
  ChangePasswordInput,
  CreateApiKeyInput,
  CreateApiKeyResult,
  CreateUserInput,
  CreateUserResult,
  IdentityActionResult,
  IdentityUsersPayload,
  IdentityTeamsPayload,
  InvitationStateView,
  ApiEnvelope,
  ApiKeysPayload,
  RevokeApiKeyResult,
  TeamManagementView,
  ModelView,
  Paginated,
  PasswordInviteResult,
  RequestLogDetailView,
  RequestLogFiltersInput,
  RequestLogView,
  SpendBudgetsView,
  SpendOwnerKind,
  SpendReportView,
  UpsertBudgetInput,
  UpsertBudgetResultView,
  DeactivateBudgetResultView,
  CreateTeamInput,
  PasswordLoginInput,
  TransferTeamMemberInput,
  UpdateTeamInput,
  UpdateUserInput,
} from '@/types/api'
import { fetchGatewayJson } from '@/server/gateway-client.server'

function envelope<T>(data: T): ApiEnvelope<T> {
  return {
    data,
    meta: {
      generated_at: new Date().toISOString(),
    },
  }
}

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

export async function listBudgetAlertHistory(params?: {
  page?: number
  page_size?: number
  owner_kind?: SpendOwnerKind
  status?: 'all' | 'pending' | 'sent' | 'failed'
  channel?: 'all' | 'email'
}): Promise<ApiEnvelope<BudgetAlertHistoryView>> {
  const query = new URLSearchParams()
  if (params?.page) {
    query.set('page', String(params.page))
  }
  if (params?.page_size) {
    query.set('page_size', String(params.page_size))
  }
  if (params?.owner_kind && params.owner_kind !== 'all') {
    query.set('owner_kind', params.owner_kind)
  }
  if (params?.status && params.status !== 'all') {
    query.set('status', params.status)
  }
  if (params?.channel && params.channel !== 'all') {
    query.set('channel', params.channel)
  }
  const suffix = query.toString() ? `?${query.toString()}` : ''
  return fetchGatewayJson<ApiEnvelope<BudgetAlertHistoryView>>(
    `/api/v1/admin/spend/budget-alerts${suffix}`,
  )
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

export async function listRequestLogs(
  filters: RequestLogFiltersInput = {},
): Promise<ApiEnvelope<Paginated<RequestLogView>>> {
  const query = new URLSearchParams()
  if (filters.requestId) {
    query.set('request_id', filters.requestId)
  }
  if (filters.modelKey) {
    query.set('model_key', filters.modelKey)
  }
  if (filters.providerKey) {
    query.set('provider_key', filters.providerKey)
  }
  if (filters.service) {
    query.set('service', filters.service)
  }
  if (filters.component) {
    query.set('component', filters.component)
  }
  if (filters.env) {
    query.set('env', filters.env)
  }
  if (filters.tagKey) {
    query.set('tag_key', filters.tagKey)
  }
  if (filters.tagValue) {
    query.set('tag_value', filters.tagValue)
  }
  const suffix = query.toString() ? `?${query.toString()}` : ''
  const response = await fetchGatewayJson<
    ApiEnvelope<{
      items: GatewayRequestLogSummary[]
      page: number
      page_size: number
      total: number
    }>
  >(`/api/v1/admin/observability/request-logs${suffix}`)

  return {
    data: {
      items: response.data.items.map(mapRequestLogSummary),
      page: response.data.page,
      pageSize: response.data.page_size,
      total: response.data.total,
    },
    meta: response.meta,
  }
}

export async function getRequestLogDetail(
  requestLogId: string,
): Promise<ApiEnvelope<RequestLogDetailView>> {
  const response = await fetchGatewayJson<ApiEnvelope<GatewayRequestLogDetail>>(
    `/api/v1/admin/observability/request-logs/${encodeURIComponent(requestLogId)}`,
  )

  return {
    data: mapRequestLogDetail(response.data),
    meta: response.meta,
  }
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

export async function removeTeamMember(
  teamId: string,
  userId: string,
): Promise<ApiEnvelope<IdentityActionResult>> {
  return fetchGatewayJson<ApiEnvelope<IdentityActionResult>>(
    `/api/v1/admin/identity/teams/${teamId}/members/${userId}`,
    {
      method: 'DELETE',
    },
  )
}

export async function transferTeamMember(
  teamId: string,
  userId: string,
  input: TransferTeamMemberInput,
): Promise<ApiEnvelope<IdentityActionResult>> {
  return fetchGatewayJson<ApiEnvelope<IdentityActionResult>>(
    `/api/v1/admin/identity/teams/${teamId}/members/${userId}/transfer`,
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

export async function updateUser(
  userId: string,
  input: UpdateUserInput,
): Promise<ApiEnvelope<IdentityActionResult>> {
  return fetchGatewayJson<ApiEnvelope<IdentityActionResult>>(
    `/api/v1/admin/identity/users/${userId}`,
    {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(input),
    },
  )
}

export async function deactivateUser(userId: string): Promise<ApiEnvelope<IdentityActionResult>> {
  return fetchGatewayJson<ApiEnvelope<IdentityActionResult>>(
    `/api/v1/admin/identity/users/${userId}/deactivate`,
    {
      method: 'POST',
    },
  )
}

export async function reactivateUser(userId: string): Promise<ApiEnvelope<IdentityActionResult>> {
  return fetchGatewayJson<ApiEnvelope<IdentityActionResult>>(
    `/api/v1/admin/identity/users/${userId}/reactivate`,
    {
      method: 'POST',
    },
  )
}

export async function resetUserOnboarding(
  userId: string,
): Promise<ApiEnvelope<CreateUserResult>> {
  return fetchGatewayJson<ApiEnvelope<CreateUserResult>>(
    `/api/v1/admin/identity/users/${userId}/reset-onboarding`,
    {
      method: 'POST',
    },
  )
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

interface GatewayRequestLogSummary {
  request_log_id: string
  request_id: string
  api_key_id: string
  user_id: string | null
  team_id: string | null
  model_key: string
  resolved_model_key: string
  provider_key: string
  status_code: number | null
  latency_ms: number | null
  prompt_tokens: number | null
  completion_tokens: number | null
  total_tokens: number | null
  error_code: string | null
  has_payload: boolean
  request_payload_truncated: boolean
  response_payload_truncated: boolean
  request_tags: {
    service: string | null
    component: string | null
    env: string | null
    bespoke: Array<{
      key: string
      value: string
    }>
  }
  metadata: Record<string, unknown>
  occurred_at: string
}

interface GatewayRequestLogDetail {
  log: GatewayRequestLogSummary
  payload: {
    request_json: unknown
    response_json: unknown
  } | null
}

function mapRequestLogSummary(summary: GatewayRequestLogSummary): RequestLogView {
  return {
    requestLogId: summary.request_log_id,
    requestId: summary.request_id,
    apiKeyId: summary.api_key_id,
    userId: summary.user_id,
    teamId: summary.team_id,
    modelKey: summary.model_key,
    resolvedModelKey: summary.resolved_model_key,
    providerKey: summary.provider_key,
    statusCode: summary.status_code,
    latencyMs: summary.latency_ms,
    promptTokens: summary.prompt_tokens,
    completionTokens: summary.completion_tokens,
    totalTokens: summary.total_tokens,
    errorCode: summary.error_code,
    hasPayload: summary.has_payload,
    requestPayloadTruncated: summary.request_payload_truncated,
    responsePayloadTruncated: summary.response_payload_truncated,
    requestTags: {
      service: summary.request_tags.service,
      component: summary.request_tags.component,
      env: summary.request_tags.env,
      bespoke: summary.request_tags.bespoke,
    },
    metadata: summary.metadata,
    occurredAt: summary.occurred_at,
  }
}

function mapRequestLogDetail(detail: GatewayRequestLogDetail): RequestLogDetailView {
  return {
    log: mapRequestLogSummary(detail.log),
    payload: detail.payload
      ? {
          requestJson: detail.payload.request_json,
          responseJson: detail.payload.response_json,
        }
      : null,
  }
}
