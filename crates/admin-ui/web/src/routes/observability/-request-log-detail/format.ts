import { EMPTY_VALUE, formatCount, formatTimestamp, formatUsd10000Precise } from '@/lib/format'
import type { RequestLogView } from '@/types/api'

export function callerPrimary(item: RequestLogView): string | null {
  return item.user_name ?? item.service_account_name ?? null
}

export function callerSecondary(item: RequestLogView): string | null {
  if (item.user_name) {
    return item.user_email ?? null
  }
  return item.service_account_name ? 'service account' : null
}

export function callerLabel(item: RequestLogView): string {
  const primary = callerPrimary(item)
  if (!primary) {
    return 'Unknown'
  }
  const secondary = callerSecondary(item)
  return secondary ? `${primary} · ${secondary}` : primary
}

export function formatOccurredAt(occurredAt: string) {
  // Seconds matter here: several requests often land within the same minute.
  return formatTimestamp(occurredAt, { seconds: true })
}

export function attemptStatusBadgeVariant(status: string): 'success' | 'warning' | 'outline' {
  return status === 'success' ? 'success' : status.endsWith('error') ? 'warning' : 'outline'
}

/**
 * Status badge variant for an HTTP status: success below 400, warning from 400. The list and the
 * detail sheet share it so one request never reads two ways.
 */
export function statusCodeBadgeVariant(
  statusCode: number | null | undefined,
): 'success' | 'warning' | 'outline' {
  if (statusCode == null) {
    return 'outline'
  }
  return statusCode >= 400 ? 'warning' : 'success'
}

export function formatRequestCost(costUsd10000: number | null | undefined) {
  return costUsd10000 == null ? EMPTY_VALUE : formatUsd10000Precise(costUsd10000)
}

export function cacheHitLabel(cachedTokens: number, promptTokens: number | null | undefined) {
  const cached = formatCount(cachedTokens)
  if (!promptTokens) {
    return `${cached} prompt tokens served from cache`
  }
  const percent = Math.round((cachedTokens / promptTokens) * 100)
  return `${cached} of ${formatCount(promptTokens)} prompt tokens cached (${percent}%)`
}

/** The cached share of prompt tokens as a whole percentage, or null without a cache read. */
export function cacheHitPercent(item: RequestLogView): number | null {
  const cached = item.cache_read_tokens ?? 0
  if (cached <= 0 || !item.prompt_tokens) {
    return null
  }
  return Math.round((cached / item.prompt_tokens) * 100)
}

export function formatBasisPoints(value: number | null | undefined) {
  return value == null ? EMPTY_VALUE : `${(value / 100).toFixed(2)}%`
}

export function operationLabel(item: RequestLogView) {
  const operation = item.metadata.operation
  return typeof operation === 'string' && operation.trim().length > 0
    ? formatOperation(operation)
    : null
}

function formatOperation(operation: string) {
  switch (operation) {
    case 'chat_completions':
      return 'Chat Completions'
    case 'responses':
      return 'Responses'
    case 'embeddings':
      return 'Embeddings'
    case 'decisions':
      return 'Decisions'
    default: {
      const formatted = operation
        .split(/[_\s-]+/)
        .filter((part) => part.length > 0)
        .map((part) => part[0].toUpperCase() + part.slice(1))
        .join(' ')
      return formatted.length > 0 ? formatted : operation
    }
  }
}

export function metadataBoolean(item: RequestLogView, key: string) {
  return item.metadata[key] === true
}

/** Caller tags in display order: the three standard tags, then bespoke ones. */
export function requestTagLabels(item: RequestLogView): string[] {
  return [
    item.request_tags.service ? `service:${item.request_tags.service}` : null,
    item.request_tags.component ? `component:${item.request_tags.component}` : null,
    item.request_tags.env ? `env:${item.request_tags.env}` : null,
    ...item.request_tags.bespoke.map((tag) => `${tag.key}:${tag.value}`),
  ].filter((value): value is string => value !== null)
}
