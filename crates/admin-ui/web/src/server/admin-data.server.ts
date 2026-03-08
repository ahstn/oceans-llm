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
  RequestLogDetailView,
  UsageCostPoint,
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

export async function listUsageCosts(): Promise<ApiEnvelope<UsageCostPoint[]>> {
  return envelope([
    { day: 'Mon', amountUsd: 312.4 },
    { day: 'Tue', amountUsd: 298.2 },
    { day: 'Wed', amountUsd: 344.1 },
    { day: 'Thu', amountUsd: 367.7 },
    { day: 'Fri', amountUsd: 352.8 },
    { day: 'Sat', amountUsd: 276.6 },
    { day: 'Sun', amountUsd: 241.9 },
  ])
}

export async function listRequestLogs(): Promise<ApiEnvelope<Paginated<RequestLogView>>> {
  const response = await fetchGatewayJson<
    ApiEnvelope<{
      items: Array<{
        id: string
        model: string
        provider: string
        upstream_model: string
        status_code: number
        latency_ms: number
        prompt_tokens: number | null
        completion_tokens: number | null
        total_tokens: number | null
        stream: boolean
        fallback_used: boolean
        attempt_count: number
        payload_available: boolean
        error_code: string | null
        timestamp: string
      }>
      page: number
      page_size: number
      total: number
    }>
  >('/api/v1/admin/observability/request-logs')

  return envelope({
    items: response.data.items.map((item) => ({
      id: item.id,
      model: item.model,
      provider: item.provider,
      upstreamModel: item.upstream_model,
      statusCode: item.status_code,
      latencyMs: item.latency_ms,
      promptTokens: item.prompt_tokens,
      completionTokens: item.completion_tokens,
      totalTokens: item.total_tokens,
      stream: item.stream,
      fallbackUsed: item.fallback_used,
      attemptCount: item.attempt_count,
      payloadAvailable: item.payload_available,
      errorCode: item.error_code,
      timestamp: item.timestamp,
    })),
    page: response.data.page,
    pageSize: response.data.page_size,
    total: response.data.total,
  })
}

export async function getRequestLogDetail(
  requestId: string,
): Promise<ApiEnvelope<RequestLogDetailView>> {
  const response = await fetchGatewayJson<
    ApiEnvelope<{
      request_id: string
      request_json: unknown
      response_json: unknown
      request_bytes: number
      response_bytes: number
      request_truncated: boolean
      response_truncated: boolean
      request_sha256: string
      response_sha256: string
      timestamp: string
    }>
  >(`/api/v1/admin/observability/request-logs/${encodeURIComponent(requestId)}`)

  return envelope({
    requestId: response.data.request_id,
    requestJson: response.data.request_json,
    responseJson: response.data.response_json,
    requestBytes: response.data.request_bytes,
    responseBytes: response.data.response_bytes,
    requestTruncated: response.data.request_truncated,
    responseTruncated: response.data.response_truncated,
    requestSha256: response.data.request_sha256,
    responseSha256: response.data.response_sha256,
    timestamp: response.data.timestamp,
  })
}

export async function listTeams(): Promise<ApiEnvelope<IdentityTeamsPayload>> {
  return fetchGatewayJson<ApiEnvelope<IdentityTeamsPayload>>('/api/v1/admin/identity/teams')
}

export async function createTeam(
  input: CreateTeamInput,
): Promise<ApiEnvelope<TeamManagementView>> {
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
