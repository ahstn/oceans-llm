import type { ChartConfig } from '@/components/ui/chart'
import type { SpendReportView, SpendTokenPointView } from '@/types/api'

type CacheBuckets = Pick<
  SpendTokenPointView,
  'uncached_input_tokens' | 'cache_read_tokens' | 'cache_write_tokens'
>

export const COMPACT_FORMATTER = new Intl.NumberFormat('en-US', {
  notation: 'compact',
  maximumFractionDigits: 1,
})

/** Neutral tone for the folded "Other" model series, kept apart from the ranked palette. */
const OTHER_SERIES_COLOR = 'var(--color-text-soft)'

const percent = (fraction: number) => `${Math.round(fraction * 100)}%`

/** Hit rates at or above this read as healthy and switch from the warm scale to primary blue. */
const HEALTHY_HIT_RATE = 0.5

/**
 * Bar colour for a cache hit rate. Below 50% it warms from danger red to warning amber; from 50%
 * it is primary blue, deepening from a softer tint to full strength. Blending amber straight into
 * blue passes through green, so the scale switches families at the threshold instead.
 */
export function hitRateColor(rate: number): string {
  const clamped = Math.min(1, Math.max(0, rate))
  if (clamped < HEALTHY_HIT_RATE) {
    const warmth = clamped / HEALTHY_HIT_RATE
    return `color-mix(in oklch, var(--color-warning) ${percent(warmth)}, var(--color-danger))`
  }
  const strength = (clamped - HEALTHY_HIT_RATE) / (1 - HEALTHY_HIT_RATE)
  return `color-mix(in oklab, var(--color-primary) ${percent(0.55 + 0.45 * strength)}, transparent)`
}

/** Input tokens whose provider reported a cache split; the hit-rate denominator. */
function cacheableInputTokens(buckets: CacheBuckets) {
  return buckets.uncached_input_tokens + buckets.cache_read_tokens + buckets.cache_write_tokens
}

/** Share of cache-split input served from cache, or null when nothing was cacheable. */
export function cacheHitRate(buckets: CacheBuckets): number | null {
  const cacheable = cacheableInputTokens(buckets)
  return cacheable > 0 ? buckets.cache_read_tokens / cacheable : null
}

function sumPoints(points: SpendTokenPointView[]) {
  return points.reduce(
    (total, point) => ({
      request_count: total.request_count + point.request_count,
      input_tokens: total.input_tokens + point.input_tokens,
      output_tokens: total.output_tokens + point.output_tokens,
      uncached_input_tokens: total.uncached_input_tokens + point.uncached_input_tokens,
      cache_read_tokens: total.cache_read_tokens + point.cache_read_tokens,
      cache_write_tokens: total.cache_write_tokens + point.cache_write_tokens,
    }),
    {
      request_count: 0,
      input_tokens: 0,
      output_tokens: 0,
      uncached_input_tokens: 0,
      cache_read_tokens: 0,
      cache_write_tokens: 0,
    },
  )
}

export type SeriesChart = {
  config: ChartConfig
  keys: { key: string; label: string; color: string }[]
  rows: Record<string, number | string | null>[]
}

/** One stacked series per top model (plus "Other"): daily input + output tokens. */
export function modelTokenChart(report: SpendReportView): SeriesChart {
  let rankedIndex = 0
  const keys = report.model_token_series.map((model, index) => ({
    key: `model_${index + 1}`,
    label: model.model_key,
    color: model.is_other ? OTHER_SERIES_COLOR : `var(--chart-${++rankedIndex})`,
  }))
  const days = report.model_token_series[0]?.points.map((point) => point.day_start) ?? []
  const rows = days.map((day, dayIndex) => {
    const row: Record<string, number | string | null> = { day }
    report.model_token_series.forEach((model, index) => {
      const point = model.points[dayIndex]
      row[`model_${index + 1}`] = point ? point.input_tokens + point.output_tokens : 0
    })
    return row
  })
  return { config: toConfig(keys), keys, rows }
}

function toConfig(keys: SeriesChart['keys']): ChartConfig {
  return Object.fromEntries(keys.map(({ key, label, color }) => [key, { label, color }]))
}

export type OwnerCacheRow = {
  key: string
  name: string
  inputTokens: number
  cacheReadTokens: number
  hitRate: number | null
}

export function ownerCacheRows(report: SpendReportView): OwnerCacheRow[] {
  return report.owner_token_series.map((owner) => {
    const total = sumPoints(owner.points)
    return {
      key: `${owner.owner_kind}:${owner.owner_id}`,
      name: owner.owner_name,
      inputTokens: total.input_tokens,
      cacheReadTokens: total.cache_read_tokens,
      hitRate: cacheHitRate(total),
    }
  })
}
