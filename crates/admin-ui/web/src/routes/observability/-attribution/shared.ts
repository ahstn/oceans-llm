import { EMPTY_VALUE, formatUsd10000 } from '@/lib/format'

/** The setup guide for `oceans hook install`, linked from the empty state. */
export const ATTRIBUTION_DOCS_URL = 'https://oceans-llm.com/harnesses/git-ai-attribution/'

export const ATTRIBUTION_WINDOWS = [7, 30, 90] as const
export type AttributionWindowDays = (typeof ATTRIBUTION_WINDOWS)[number]
const DEFAULT_ATTRIBUTION_WINDOW: AttributionWindowDays = 30

/** Route search: the report window plus the optional user and repository filters. */
export type AttributionSearch = {
  days: AttributionWindowDays
  user_id?: string
  repository?: string
}

export function normalizeAttributionSearch(search: Record<string, unknown>): AttributionSearch {
  // Search params usually arrive JSON-parsed, but `?days="30"` stays a string. Any other type,
  // such as a parsed object, falls back: `Number()` on one can throw.
  const rawDays = search.days
  const days = typeof rawDays === 'number' || typeof rawDays === 'string' ? Number(rawDays) : NaN
  const result: AttributionSearch = {
    days: ATTRIBUTION_WINDOWS.find((window) => window === days) ?? DEFAULT_ATTRIBUTION_WINDOW,
  }
  for (const key of ['user_id', 'repository'] as const) {
    const value = search[key]
    if (typeof value === 'string' && value.trim()) result[key] = value.trim()
  }
  return result
}

export function toAttributionWindow(value: string): AttributionWindowDays | undefined {
  return ATTRIBUTION_WINDOWS.find((window) => String(window) === value)
}

/**
 * Attributed cost sums priced requests only; unpriced ones are counted, not costed. A row with no
 * priced request therefore has an unknown cost and reads `—`, never `$0.00`.
 */
export function formatAttributedCost(costUsd10000: number, pricedRequestCount: number) {
  return pricedRequestCount > 0 ? formatUsd10000(costUsd10000) : EMPTY_VALUE
}

/** AI share of attested lines. Unattested lines are unknown, not human, so they stay out. */
export function aiLineShare(aiLines: number, humanLines: number) {
  const attested = aiLines + humanLines
  return attested > 0 ? aiLines / attested : null
}

/** The abbreviated commit SHA git shows by default. */
export function shortSha(sha: string) {
  return sha.slice(0, 7)
}

export function pullRequestLabel(repository: string, number: number) {
  return `${repository}#${number}`
}
