import type { ChartConfig } from '@/components/ui/chart'
import type {
  ApiKeyView,
  MyProfileDayView,
  MyProfileHarnessDayView,
  MyProfileModelDayView,
  MyProfileView,
} from '@/types/api'

export const COMPACT_FORMATTER = new Intl.NumberFormat('en-US', {
  notation: 'compact',
  maximumFractionDigits: 1,
})

export const NUMBER_FORMATTER = new Intl.NumberFormat('en-US')

export const PERCENT_FORMATTER = new Intl.NumberFormat('en-US', {
  style: 'percent',
  maximumFractionDigits: 1,
})

const DAY_MS = 86_400_000

/** Chart windows offered on the profile; `365` is the full history the endpoint returns. */
export type ProfileRange = 30 | 90 | 365

export const PROFILE_RANGES: { value: ProfileRange; label: string }[] = [
  { value: 30, label: '30 days' },
  { value: 90, label: '90 days' },
  { value: 365, label: '1 year' },
]

export function toProfileRange(value: string): ProfileRange {
  return value === '30' ? 30 : value === '90' ? 90 : 365
}

// ── Dates ─────────────────────────────────────────────────────────────────────

export function parseUtcDay(day: string) {
  return new Date(`${day}T00:00:00Z`)
}

export function formatUtcDay(date: Date) {
  return date.toISOString().slice(0, 10)
}

export function addUtcDays(date: Date, days: number) {
  return new Date(date.getTime() + days * DAY_MS)
}

/** Last day covered by the profile window (the window end is exclusive). */
export function lastProfileDay(profile: Pick<MyProfileView, 'window_end'>) {
  return addUtcDays(parseUtcDay(profile.window_end.slice(0, 10)), -1)
}

export function formatShortDay(day: string) {
  return parseUtcDay(day).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
    timeZone: 'UTC',
  })
}

export function formatLongDay(day: string) {
  return parseUtcDay(day).toLocaleDateString('en-US', {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    timeZone: 'UTC',
  })
}

/** ISO days from `range - 1` days before `end` through `end`, inclusive. */
function rangeDays(end: Date, range: number) {
  return Array.from({ length: range }, (_, index) =>
    formatUtcDay(addUtcDays(end, index - range + 1)),
  )
}

function inRange(day: string, end: Date, range: number) {
  const time = parseUtcDay(day).getTime()
  return time <= end.getTime() && time > end.getTime() - range * DAY_MS
}

// ── Headlines ─────────────────────────────────────────────────────────────────

export type UsageTotals = {
  requests: number
  totalTokens: number
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  costUsd10000: number
  activeDays: number
  /** Cache reads over input with a provider cache split; null when nothing was cacheable. */
  cacheHitRate: number | null
}

export function summarizeDays(days: MyProfileDayView[]): UsageTotals {
  let cacheable = 0
  const totals = days.reduce(
    (sum, day) => {
      cacheable += day.uncached_input_tokens + day.cache_read_tokens + day.cache_write_tokens
      return {
        ...sum,
        requests: sum.requests + day.request_count,
        totalTokens: sum.totalTokens + day.total_tokens,
        inputTokens: sum.inputTokens + day.input_tokens,
        outputTokens: sum.outputTokens + day.output_tokens,
        cacheReadTokens: sum.cacheReadTokens + day.cache_read_tokens,
        costUsd10000: sum.costUsd10000 + day.cost_usd_10000,
        activeDays: sum.activeDays + (day.request_count > 0 ? 1 : 0),
      }
    },
    {
      requests: 0,
      totalTokens: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      costUsd10000: 0,
      activeDays: 0,
    },
  )
  return { ...totals, cacheHitRate: cacheable > 0 ? totals.cacheReadTokens / cacheable : null }
}

export type Preference = {
  key: string
  label: string
  requests: number
  /** Share of all requests in the window, 0–1. */
  share: number
}

type KeyedDay = { day: string; key: string; label: string; request_count: number }

function modelRows(rows: MyProfileModelDayView[]): KeyedDay[] {
  return rows.map((row) => ({ ...row, key: row.model_key, label: row.model_key }))
}

function harnessRows(rows: MyProfileHarnessDayView[]): KeyedDay[] {
  return rows.map((row) => ({ ...row, key: row.harness_key, label: row.harness_label }))
}

/** Keys ranked by request count, most used first. */
function rankKeys(rows: KeyedDay[]): Preference[] {
  const totals = new Map<string, Preference>()
  let all = 0
  for (const row of rows) {
    all += row.request_count
    const current = totals.get(row.key) ?? { key: row.key, label: row.label, requests: 0, share: 0 }
    current.requests += row.request_count
    totals.set(row.key, current)
  }
  return [...totals.values()]
    .map((entry) => ({ ...entry, share: all > 0 ? entry.requests / all : 0 }))
    .sort((a, b) => b.requests - a.requests || a.label.localeCompare(b.label))
}

export function rankModels(rows: MyProfileModelDayView[]) {
  return rankKeys(modelRows(rows))
}

export function rankHarnesses(rows: MyProfileHarnessDayView[]) {
  return rankKeys(harnessRows(rows))
}

// ── Range filtering ───────────────────────────────────────────────────────────

/** The profile narrowed to the last `range` days, so every card agrees on the window. */
export function profileInRange(profile: MyProfileView, range: ProfileRange): MyProfileView {
  const end = lastProfileDay(profile)
  return {
    ...profile,
    days: profile.days.filter((row) => inRange(row.day, end, range)),
    model_days: profile.model_days.filter((row) => inRange(row.day, end, range)),
    harness_days: profile.harness_days.filter((row) => inRange(row.day, end, range)),
  }
}

// ── Chart series ──────────────────────────────────────────────────────────────

export type SeriesChart = {
  config: ChartConfig
  keys: { key: string; label: string; color: string }[]
  rows: Record<string, number | string>[]
}

/** Buckets wider than a day keep year-long charts readable. */
function bucketSizeFor(range: number) {
  return range > 90 ? 7 : 1
}

/** Bucket start days, oldest first; every bucket appears even when it had no traffic. */
function buckets(end: Date, range: number) {
  const size = bucketSizeFor(range)
  const days = rangeDays(end, range)
  const starts = days.filter((_, index) => index % size === 0)
  const bucketOf = new Map(days.map((day, index) => [day, starts[Math.floor(index / size)]]))
  return { starts, bucketOf }
}

const TOKEN_KEYS = [
  { key: 'output', label: 'Output', color: 'var(--chart-2)' },
  { key: 'uncached', label: 'Input', color: 'var(--chart-4)' },
  { key: 'cached', label: 'Cached input', color: 'var(--chart-1)' },
]

/** Stacked input, cached input, and output tokens per bucket. */
export function tokenVolumeChart(profile: MyProfileView, range: ProfileRange): SeriesChart {
  const end = lastProfileDay(profile)
  const { starts, bucketOf } = buckets(end, range)
  const rows = new Map(
    starts.map((day) => [
      day,
      { day, output: 0, uncached: 0, cached: 0 } as Record<string, number | string>,
    ]),
  )
  for (const day of profile.days) {
    const row = rows.get(bucketOf.get(day.day) ?? '')
    if (!row) continue
    row.output = Number(row.output) + day.output_tokens
    row.uncached = Number(row.uncached) + Math.max(0, day.input_tokens - day.cache_read_tokens)
    row.cached = Number(row.cached) + day.cache_read_tokens
  }
  return { config: toConfig(TOKEN_KEYS), keys: TOKEN_KEYS, rows: [...rows.values()] }
}

const OTHER_COLOR = 'var(--color-text-soft)'
const TOP_SERIES = 5

/** Requests per bucket for the top keys, with the long tail folded into "Other". */
function requestSeriesChart(rows: KeyedDay[], end: Date, range: ProfileRange): SeriesChart {
  const ranked = rankKeys(rows)
  const top = ranked.slice(0, TOP_SERIES)
  const hasOther = ranked.length > TOP_SERIES
  const keys = top.map((entry, index) => ({
    key: `series_${index + 1}`,
    label: entry.label,
    color: `var(--chart-${5 - index})`,
  }))
  if (hasOther) keys.push({ key: 'series_other', label: 'Other', color: OTHER_COLOR })
  const seriesFor = new Map(top.map((entry, index) => [entry.key, `series_${index + 1}`]))

  const { starts, bucketOf } = buckets(end, range)
  const chartRows = new Map(
    starts.map((day) => {
      const row: Record<string, number | string> = { day }
      for (const { key } of keys) row[key] = 0
      return [day, row]
    }),
  )
  for (const row of rows) {
    const target = chartRows.get(bucketOf.get(row.day) ?? '')
    if (!target) continue
    const series = seriesFor.get(row.key) ?? (hasOther ? 'series_other' : null)
    if (series) target[series] = Number(target[series]) + row.request_count
  }
  return { config: toConfig(keys), keys, rows: [...chartRows.values()] }
}

export function modelRequestsChart(profile: MyProfileView, range: ProfileRange) {
  return requestSeriesChart(modelRows(profile.model_days), lastProfileDay(profile), range)
}

export function harnessRequestsChart(profile: MyProfileView, range: ProfileRange) {
  return requestSeriesChart(harnessRows(profile.harness_days), lastProfileDay(profile), range)
}

function toConfig(keys: SeriesChart['keys']): ChartConfig {
  return Object.fromEntries(keys.map(({ key, label, color }) => [key, { label, color }]))
}

// ── Heatmap ───────────────────────────────────────────────────────────────────

export type HeatmapCell = {
  day: string
  /** 0 = no usage, 1–4 = token-volume quartile among active days. */
  level: 0 | 1 | 2 | 3 | 4
  usage: MyProfileDayView | null
}

export type Heatmap = {
  /** Columns of seven days (Sunday first); null pads the first and last weeks. */
  weeks: (HeatmapCell | null)[][]
  months: { label: string; week: number }[]
}

/** A GitHub-style calendar of the last `weeks` weeks, coloured by total tokens. */
export function buildHeatmap(profile: MyProfileView, weeks = 53): Heatmap {
  const end = lastProfileDay(profile)
  const usageByDay = new Map(profile.days.map((day) => [day.day, day]))
  const thresholds = quartiles(profile.days.map((day) => day.total_tokens).filter((v) => v > 0))
  const firstSunday = addUtcDays(end, -end.getUTCDay() - (weeks - 1) * 7)

  const columns: (HeatmapCell | null)[][] = []
  const months: Heatmap['months'] = []
  for (let week = 0; week < weeks; week += 1) {
    const column: (HeatmapCell | null)[] = []
    for (let weekday = 0; weekday < 7; weekday += 1) {
      const date = addUtcDays(firstSunday, week * 7 + weekday)
      if (date.getTime() > end.getTime()) {
        column.push(null)
        continue
      }
      const day = formatUtcDay(date)
      const usage = usageByDay.get(day) ?? null
      column.push({ day, usage, level: levelFor(usage?.total_tokens ?? 0, thresholds) })
      if (date.getUTCDate() === 1 || (week === 0 && weekday === 0)) {
        months.push({
          week,
          label: date.toLocaleDateString('en-US', { month: 'short', timeZone: 'UTC' }),
        })
      }
    }
    columns.push(column)
  }
  // A label for a partial first month would collide with the next one.
  if (months.length > 1 && months[1].week - months[0].week < 3) months.shift()
  return { weeks: columns, months }
}

function quartiles(values: number[]) {
  if (values.length === 0) return [0, 0, 0]
  const sorted = [...values].sort((a, b) => a - b)
  const at = (fraction: number) => sorted[Math.floor((sorted.length - 1) * fraction)]
  return [at(0.25), at(0.5), at(0.75)]
}

function levelFor(tokens: number, [q1, q2, q3]: number[]): HeatmapCell['level'] {
  if (tokens <= 0) return 0
  if (tokens <= q1) return 1
  if (tokens <= q2) return 2
  if (tokens <= q3) return 3
  return 4
}

// ── API keys ──────────────────────────────────────────────────────────────────

/** Keys the viewer owns personally, active first, then most recently used. */
export function personalApiKeys(items: ApiKeyView[], userId: string) {
  return items
    .filter((item) => item.owner_kind === 'user' && item.owner_id === userId)
    .sort(
      (a, b) =>
        Number(b.status === 'active') - Number(a.status === 'active') ||
        (b.last_used_at ?? '').localeCompare(a.last_used_at ?? '') ||
        b.created_at.localeCompare(a.created_at),
    )
}
