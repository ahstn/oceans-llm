import {
  EMPTY_VALUE,
  formatCount,
  formatTimestamp,
  formatUsd10000Precise,
  formatUsdNanos,
} from '@/lib/format'
import { callerPrimary, callerSecondary } from '@/lib/caller'
import type { RequestLogView } from '@/types/api'
import type { EntityTag } from '@/lib/entity-tags'

export { callerPrimary, callerSecondary } from '@/lib/caller'

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

/** An attempt status in sentence case: `upstream_error` reads `Upstream error`. */
export function attemptStatusLabel(status: string) {
  const words = status.replaceAll('_', ' ')
  return words.charAt(0).toUpperCase() + words.slice(1)
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

export function formatRequestCost(item: Pick<RequestLogView, 'cost_usd_10000' | 'cost_usd_nanos'>) {
  if (item.cost_usd_nanos != null) return formatUsdNanos(item.cost_usd_nanos)
  return item.cost_usd_10000 == null ? EMPTY_VALUE : formatUsd10000Precise(item.cost_usd_10000)
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
    case 'messages':
      return 'Messages'
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
export function requestTags(item: RequestLogView): EntityTag[] {
  const { service, component, env, bespoke } = item.request_tags
  return [
    service ? { key: 'service', value: service } : null,
    component ? { key: 'component', value: component } : null,
    env ? { key: 'env', value: env } : null,
    ...bespoke.map((tag) => ({ key: tag.key, value: tag.value })),
  ].filter((tag): tag is EntityTag => tag !== null)
}
