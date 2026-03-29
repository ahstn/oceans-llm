import type { ApiEnvelope, ApiKeyView, ModelView, Paginated } from '@/types/api'

function envelope<T>(data: T): ApiEnvelope<T> {
  return {
    data,
    meta: {
      generated_at: new Date().toISOString(),
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
