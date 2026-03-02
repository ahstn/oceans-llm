import type {
  ApiEnvelope,
  ApiKeyView,
  ModelView,
  Paginated,
  RequestLogView,
  TeamView,
  UsageCostPoint,
  UserView,
} from '@/types/api'

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

export async function listTeams(): Promise<ApiEnvelope<TeamView[]>> {
  return envelope([
    { id: 'team_1', name: 'Core Platform', users: 6, status: 'active' },
    { id: 'team_2', name: 'Customer Success', users: 4, status: 'active' },
    { id: 'team_3', name: 'Integrations', users: 3, status: 'inactive' },
  ])
}

export async function listUsers(): Promise<ApiEnvelope<UserView[]>> {
  return envelope([
    {
      id: 'user_1',
      email: 'sre@acme.local',
      role: 'admin',
      team: 'Core Platform',
      status: 'active',
    },
    {
      id: 'user_2',
      email: 'ops@acme.local',
      role: 'operator',
      team: 'Core Platform',
      status: 'active',
    },
    {
      id: 'user_3',
      email: 'analyst@acme.local',
      role: 'viewer',
      team: 'Customer Success',
      status: 'active',
    },
    {
      id: 'user_4',
      email: 'newhire@acme.local',
      role: 'viewer',
      team: 'Integrations',
      status: 'invited',
    },
  ])
}
