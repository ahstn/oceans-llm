import type { ApiKeyView } from '@/types/api'

// Shared by the API keys page and the profile page's key table.

export function formatModelGrantSummary(item: ApiKeyView) {
  if (item.model_grant_mode === 'all') {
    return 'All models'
  }

  return item.model_keys.length > 0 ? item.model_keys.join(', ') : 'No models'
}

export function maskApiKeyPrefix(prefix: string) {
  return `${prefix.slice(0, 12)}****`
}

export function formatCreatedAt(value: string) {
  return formatUtcDate(value)
}

export function formatLastUsedAt(value: string | null | undefined) {
  return value ? formatUtcDateTime(value) : 'Never'
}

function formatUtcDate(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }

  return `${date.getUTCFullYear()}-${padDatePart(date.getUTCMonth() + 1)}-${padDatePart(date.getUTCDate())}`
}

function formatUtcDateTime(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }

  return `${formatUtcDate(value)} ${padDatePart(date.getUTCHours())}:${padDatePart(date.getUTCMinutes())}`
}

function padDatePart(value: number) {
  return value.toString().padStart(2, '0')
}
