import { formatTimestamp } from '@/lib/format'
import type { ApiKeyView } from '@/types/api'

// Shared by the API keys page and the profile page's key table.

export function formatModelGrantSummary(item: Pick<ApiKeyView, 'model_grant_mode' | 'model_keys'>) {
  if (item.model_grant_mode === 'all') {
    return 'All models'
  }

  return item.model_keys.length > 0 ? item.model_keys.join(', ') : 'No models'
}

export function maskApiKeyPrefix(prefix: string) {
  return `${prefix.slice(0, 12)}****`
}

export function formatCreatedAt(value: string) {
  return formatTimestamp(value)
}

export function formatLastUsedAt(value: string | null | undefined) {
  return value ? formatTimestamp(value) : 'Never'
}
