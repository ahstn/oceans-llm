import type { ApiEnvelope, ModelPageView } from '@/types/api'

function envelope<T>(data: T): ApiEnvelope<T> {
  return {
    data,
    meta: {
      generated_at: new Date().toISOString(),
    },
  }
}

export async function listModels(): Promise<ApiEnvelope<ModelPageView>> {
  return envelope({
    items: [
      {
        id: 'fast',
        resolved_model_key: 'fast',
        alias_of: null,
        description: 'Gemini via OpenRouter',
        provider_key: 'openrouter',
        provider_label: 'OpenRouter',
        provider_icon_key: 'openrouter',
        upstream_model: 'google/gemini-2.0-flash',
        model_icon_key: 'gemini',
        tags: ['fast', 'cheap'],
        status: 'healthy',
      },
      {
        id: 'reasoning',
        resolved_model_key: 'reasoning',
        alias_of: null,
        description: 'OpenAI reasoning tier',
        provider_key: 'openai-prod',
        provider_label: 'OpenAI',
        provider_icon_key: 'openai',
        upstream_model: 'o3-mini',
        model_icon_key: 'openai',
        tags: ['reasoning'],
        status: 'healthy',
      },
      {
        id: 'backup-fast',
        resolved_model_key: 'backup-fast',
        alias_of: 'fast',
        description: 'Gemini fallback on Vertex',
        provider_key: 'vertex-gemini',
        provider_label: 'Google Vertex AI',
        provider_icon_key: 'vertexai',
        upstream_model: 'google/gemini-2.0-flash',
        model_icon_key: 'gemini',
        tags: ['fast', 'fallback'],
        status: 'degraded',
      },
    ],
    page: 1,
    page_size: 3,
    total: 3,
  })
}
