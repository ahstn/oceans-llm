// Pure derivations over the attribution report for the tabs: buckets, pull request rows with lead
// time and AI share, repository rollups and the punch card. The report carries at most 1,000
// commits and pull requests, so these run in the browser.

import { canonicalHarness } from '@/components/attribution/format'
import type { AttributionReportView, AuthSessionView } from '@/types/api'
import { AI_BANDS, aiBand, type AiBandKey } from '@/components/attribution/ai-bands'

/** A tab of the Attribution page. `concepts` is the separate mockup route, not a tab here. */
export type AttributionTab = Exclude<
  AuthSessionView['capabilities']['attribution_tabs'][number],
  'concepts'
>

export const TAB_LABELS: Record<AttributionTab, string> = {
  impact: 'Impact',
  delivery: 'Delivery',
  pull_requests: 'Pull requests',
  repositories: 'Repositories',
  durability: 'Durability',
  my_work: 'My work',
}

export const TAB_QUESTIONS: Record<AttributionTab, string> = {
  impact: 'What AI delivered, what it cost, and where agent spend ended up.',
  delivery: 'How AI-written work moves from first commit to merge, by DORA measures.',
  pull_requests: 'What went into each merged pull request: AI share, commits, harnesses and cost.',
  repositories: 'Which repositories agents work well in, and which need harness investment.',
  durability: 'Whether AI-written code lasts. Most of this needs data Oceans does not collect yet.',
  my_work: 'Your own commits, pull requests and agent spend, against your previous period.',
}

export const ATTRIBUTION_TABS = Object.keys(TAB_LABELS) as AttributionTab[]

const KNOWN_TABS = new Set<string>(ATTRIBUTION_TABS)

/** The report tabs the gateway config turns on, in the gateway's display order. */
export function enabledAttributionTabs(session: AuthSessionView | null | undefined) {
  return (session?.capabilities.attribution_tabs ?? []).filter((tab): tab is AttributionTab =>
    KNOWN_TABS.has(tab),
  )
}

/** The requested tab when it is on, else the first one that is. */
export function resolveAttributionTab(
  requested: AttributionTab | undefined,
  session: AuthSessionView | null | undefined,
) {
  const enabled = enabledAttributionTabs(session)
  return requested && enabled.includes(requested) ? requested : enabled[0]
}

/**
 * The report request for a tab: the route search without the tab, and on My work always the
 * signed-in user's own id, whatever user filter the URL carries.
 */
/**
 * The search to redirect to when the URL names no tab, or a tab that is turned off: the same
 * search on the tab the page renders, the first enabled one. `null` when the URL already names an
 * enabled tab, or every report tab is off. The loader then only sees an explicit enabled tab, so
 * the report it reads fits the tab on screen.
 */
export function attributionTabRedirect<Search extends { tab?: AttributionTab }>(
  search: Search,
  session: AuthSessionView | null | undefined,
): Search | null {
  const tab = resolveAttributionTab(search.tab, session)
  return tab === search.tab ? null : { ...search, tab }
}

/** Tabs that show changes against the previous window, so their report builds that window too. */
const COMPARING_TABS = new Set<AttributionTab>(['impact', 'my_work'])

/** The three reports the tabs read: My work's own, a compared one, and one without comparison. */
type ReportShape = 'my_work' | 'compare' | 'plain'

/**
 * The search fields the report depends on. Tabs that read the same report share a shape, so
 * switching between them reuses the loaded report rather than reading it again.
 */
export function attributionReportDeps<Search extends { tab?: AttributionTab }>({
  tab,
  ...filters
}: Search) {
  const report: ReportShape | undefined =
    tab === undefined
      ? undefined
      : tab === 'my_work'
        ? 'my_work'
        : COMPARING_TABS.has(tab)
          ? 'compare'
          : 'plain'
  return { ...filters, report }
}

/** An enabled tab that reads the report a shape names, for the loader to build its input from. */
export function reportShapeTab(
  report: ReportShape | undefined,
  session: AuthSessionView | null | undefined,
): AttributionTab | undefined {
  if (report === 'my_work') return 'my_work'
  if (report === 'compare') return 'impact'
  if (report === 'plain') {
    return enabledAttributionTabs(session).find((tab) => !COMPARING_TABS.has(tab))
  }
  return undefined
}

export function attributionReportInput<Search extends { tab?: AttributionTab; user_id?: string }>(
  search: Search,
  session: AuthSessionView | null | undefined,
) {
  const { tab: requested, ...filters } = search
  const tab = resolveAttributionTab(requested, session)
  // With every report tab off there is nothing to read, and the page shows how to turn them on.
  if (!tab) return null
  return {
    ...filters,
    user_id: tab === 'my_work' ? session?.user.id : filters.user_id,
    compare: COMPARING_TABS.has(tab),
  }
}

export type PullRequestView = AttributionReportView['pull_requests'][number]
export type ChangeView = AttributionReportView['changes'][number]
export type DayView = AttributionReportView['daily'][number]
export type HarnessModelView = AttributionReportView['harness_models'][number]
export type SummaryView = AttributionReportView['summary']

export function ratio(part: number, whole: number) {
  return whole > 0 ? part / whole : null
}

/** Change from `previous` to `current`, as a fraction; `null` without a previous value. */
export function delta(current: number | null | undefined, previous: number | null | undefined) {
  if (current == null || previous == null || previous === 0) return null
  return (current - previous) / Math.abs(previous)
}

export function percentile(values: number[], p: number) {
  if (values.length === 0) return null
  const sorted = [...values].sort((a, b) => a - b)
  const index = (sorted.length - 1) * p
  const lower = Math.floor(index)
  const upper = Math.ceil(index)
  return sorted[lower] + (sorted[upper] - sorted[lower]) * (index - lower)
}

export function median(values: number[]) {
  return percentile(values, 0.5)
}

/** AI share of merged code: lines of commits in merged pull requests. */
export function mergedAiShare(summary: SummaryView) {
  return ratio(summary.merged_ai_lines, summary.merged_ai_lines + summary.merged_human_lines)
}

export function shippedShare(summary: SummaryView) {
  return ratio(summary.shipped_cost_usd_10000, summary.priced_cost_usd_10000)
}

export type Bucket = {
  day: string
  aiLines: number
  humanLines: number
  aiShare: number | null
  commits: number
  mergedPrs: number
  committedCost: number
  uncommittedCost: number
}

/**
 * Buckets of `size` days counted back from the window's last day. The oldest bucket holds the
 * remainder, so the buckets add up to the report's totals.
 */
export function bucketDays(daily: DayView[], size: number): Bucket[] {
  const buckets: Bucket[] = []
  const first = daily.length % size || size
  for (let start = 0, end = first; start < daily.length; start = end, end += size) {
    const days = daily.slice(start, end)
    const aiLines = days.reduce((total, day) => total + day.ai_lines, 0)
    const humanLines = days.reduce((total, day) => total + day.human_lines, 0)
    buckets.push({
      day: days[0].day,
      aiLines,
      humanLines,
      aiShare: ratio(aiLines, aiLines + humanLines),
      commits: days.reduce((total, day) => total + day.commit_count, 0),
      mergedPrs: days.reduce((total, day) => total + day.merged_pull_request_count, 0),
      committedCost:
        days.reduce((total, day) => total + day.committed.priced_cost_usd_10000, 0) / 10_000,
      uncommittedCost:
        days.reduce((total, day) => total + day.uncommitted.priced_cost_usd_10000, 0) / 10_000,
    })
  }
  return buckets
}

/** Chart buckets for a window: daily for 7 days, two days for 30, weekly for 90. */
export function chartBucketSize(windowDays: number) {
  return windowDays <= 7 ? 1 : windowDays <= 30 ? 2 : 7
}

export { AI_BANDS, aiBand, type AiBandKey }

export type PullRequestRow = {
  pr: PullRequestView
  added: number
  /** `null` when no commit of the PR has a note: attribution unknown, not 0% AI. */
  aiShare: number | null
  leadHours: number | null
  /** `null` with `aiShare`; such PRs stay out of every AI-share statistic. */
  band: AiBandKey | null
}

export function pullRequestRows(pullRequests: PullRequestView[]): PullRequestRow[] {
  return pullRequests.map((pr) => {
    const added = pr.ai_lines + pr.human_lines
    const aiShare = added > 0 ? pr.ai_lines / added : null
    return {
      pr,
      added,
      aiShare,
      leadHours: pr.lead_time_seconds == null ? null : pr.lead_time_seconds / 3600,
      band: aiShare == null ? null : aiBand(aiShare),
    }
  })
}

export type BandSummary = {
  key: AiBandKey
  label: string
  count: number
  medianSize: number | null
  medianLeadHours: number | null
  revertRate: number | null
  medianCost: number | null
}

export function bandSummaries(rows: PullRequestRow[]): BandSummary[] {
  return AI_BANDS.map((band) => {
    const inBand = rows.filter((row) => row.band === band.key)
    const priced = inBand.filter((row) => row.pr.priced_request_count > 0)
    return {
      key: band.key,
      label: band.label,
      count: inBand.length,
      medianSize: median(inBand.map((row) => row.added)),
      medianLeadHours: median(
        inBand.flatMap((row) => (row.leadHours == null ? [] : [row.leadHours])),
      ),
      revertRate: ratio(inBand.filter((row) => row.pr.is_revert).length, inBand.length),
      medianCost: median(priced.map((row) => row.pr.priced_cost_usd_10000)),
    }
  })
}

/** Lead time percentiles for each week of merges, oldest first. */
export function weeklyLeadTimes(rows: PullRequestRow[], windowStart: string, windowEnd: string) {
  const start = Date.parse(windowStart)
  const week = 7 * 86_400_000
  const buckets = []
  // Weeks count back from the window's end; the oldest holds the remaining days, so every merge
  // in the window lands in a bucket.
  for (let to = Date.parse(windowEnd); to > start; to -= week) {
    const from = Math.max(start, to - week)
    const hours = rows.flatMap((row) => {
      const merged = Date.parse(row.pr.merged_at)
      return merged >= from && merged < to && row.leadHours != null ? [row.leadHours] : []
    })
    buckets.push({
      day: new Date(from).toISOString().slice(0, 10),
      p50: percentile(hours, 0.5),
      p85: percentile(hours, 0.85),
    })
  }
  return buckets.reverse()
}

const SIZE_EDGES = [0, 25, 50, 100, 200, 400, 800, 1600, Infinity]

/** Attested lines per PR. PRs without notes have no known size and are left out. */
export function sizeHistogram(allRows: PullRequestRow[]) {
  const rows = allRows.filter((row) => row.aiShare != null)
  const med = median(rows.map((row) => row.added)) ?? 0
  return SIZE_EDGES.slice(0, -1).map((low, index) => {
    const high = SIZE_EDGES[index + 1]
    return {
      label: high === Infinity ? `${low}+` : `${low}–${high}`,
      count: rows.filter((row) => row.added >= low && row.added < high).length,
      median: rows.length > 0 && med >= low && med < high,
    }
  })
}

export type RepositoryRow = {
  repository: string
  commits: number
  mergedPrs: number
  aiLines: number
  humanLines: number
  aiShare: number | null
  /** AI lines in commits with no human edit beyond 5% of their AI lines, over all AI lines. */
  autonomy: number | null
  medianPrCost: number | null
  medianLeadHours: number | null
  costUsd10000: number
  /** AI share per bucket, oldest first, for a sparkline. */
  trend: (number | null)[]
}

export function repositoryRows(
  changes: ChangeView[],
  pullRequests: PullRequestView[],
  windowStart: string,
  windowEnd: string,
): RepositoryRow[] {
  const start = new Date(windowStart).getTime()
  const span = Math.max(1, new Date(windowEnd).getTime() - start)
  const slots = 8
  // One pass groups commits and pull requests by repository, placing each commit in its trend
  // slot, so the per-repository figures below never rescan the whole report.
  const groups = new Map<
    string,
    { commits: ChangeView[]; prs: PullRequestView[]; ai: number[]; human: number[] }
  >()
  const group = (repository: string) => {
    let entry = groups.get(repository)
    if (!entry) {
      entry = {
        commits: [],
        prs: [],
        ai: Array.from({ length: slots }, () => 0),
        human: Array.from({ length: slots }, () => 0),
      }
      groups.set(repository, entry)
    }
    return entry
  }
  for (const change of changes) {
    const entry = group(change.repository)
    entry.commits.push(change)
    const at = (new Date(change.committed_at).getTime() - start) / span
    const slot = Math.min(slots - 1, Math.max(0, Math.floor(at * slots)))
    entry.ai[slot] += change.ai_lines
    entry.human[slot] += change.human_lines
  }
  for (const pr of pullRequests) group(pr.repository).prs.push(pr)
  return [...groups.entries()]
    .map(([repository, entry]) => {
      const { commits } = entry
      const prs = pullRequestRows(entry.prs)
      const aiLines = commits.reduce((total, change) => total + change.ai_lines, 0)
      const humanLines = commits.reduce((total, change) => total + change.human_lines, 0)
      const autonomous = commits
        .filter((change) => change.ai_lines > 0 && change.human_lines <= change.ai_lines * 0.05)
        .reduce((total, change) => total + change.ai_lines, 0)
      const trend = entry.ai.map((ai, slot) => ratio(ai, ai + entry.human[slot]))
      return {
        repository,
        commits: commits.length,
        mergedPrs: prs.length,
        aiLines,
        humanLines,
        aiShare: ratio(aiLines, aiLines + humanLines),
        autonomy: ratio(autonomous, aiLines),
        medianPrCost: median(
          prs.flatMap((row) =>
            row.pr.priced_request_count > 0 ? [row.pr.priced_cost_usd_10000] : [],
          ),
        ),
        medianLeadHours: median(
          prs.flatMap((row) => (row.leadHours == null ? [] : [row.leadHours])),
        ),
        costUsd10000: commits.reduce((total, change) => total + change.priced_cost_usd_10000, 0),
        trend,
      }
    })
    .sort((a, b) => b.commits - a.commits || a.repository.localeCompare(b.repository))
}

/** Commits by UTC weekday and hour; `ai` counts commits where AI wrote most attested lines. */
export function punchCard(changes: ChangeView[]) {
  const grid = Array.from({ length: 7 }, () =>
    Array.from({ length: 24 }, () => ({ ai: 0, all: 0 })),
  )
  for (const change of changes) {
    const date = new Date(change.committed_at)
    const cell = grid[date.getUTCDay()][date.getUTCHours()]
    cell.all += 1
    if (change.ai_lines > change.human_lines) cell.ai += 1
  }
  return grid
}

/** One cell per day of the window, for the contribution calendar. */
export function calendarCells(daily: DayView[]) {
  return daily.map((day) => ({
    day: day.day,
    commits: day.commit_count,
    aiShare: ratio(day.ai_lines, day.ai_lines + day.human_lines),
    weekday: new Date(`${day.day}T12:00:00Z`).getUTCDay(),
  }))
}

/**
 * AI lines per harness, from each session's own line count in the window's commits. A commit two
 * harnesses wrote is split by what each session wrote, not evenly.
 */
export function harnessShares(harnessModels: HarnessModelView[]) {
  const lines = new Map<string, number>()
  for (const row of harnessModels) {
    if (row.ai_lines === 0) continue
    const harness = canonicalHarness(row.harness)
    lines.set(harness, (lines.get(harness) ?? 0) + row.ai_lines)
  }
  return [...lines.entries()]
    .map(([harness, aiLines]) => ({ harness, aiLines }))
    .sort((a, b) => b.aiLines - a.aiLines)
}

/**
 * The AI share of the lines in pull requests merged in each bucket, so the trend measures what the
 * "AI share of merged code" figure does. `null` for a bucket with no merged lines.
 */
export function mergedShareByBucket(pullRequests: PullRequestView[], buckets: Bucket[]) {
  const ai = buckets.map(() => 0)
  const human = buckets.map(() => 0)
  for (const pr of pullRequests) {
    const day = pr.merged_at.slice(0, 10)
    // Buckets run oldest first, so the merge belongs to the last one starting on or before it.
    let index = -1
    for (const [at, bucket] of buckets.entries()) if (bucket.day <= day) index = at
    if (index < 0) continue
    ai[index] += pr.ai_lines
    human[index] += pr.human_lines
  }
  return buckets.map((_, index) => ratio(ai[index], ai[index] + human[index]))
}
