// Pure derivations over the attribution report for the tabs: buckets, pull request rows with lead
// time and AI share, repository rollups and the punch card. The report carries at most 1,000
// commits and pull requests, so these run in the browser.

import type { AttributionReportView, AuthSessionView } from '@/types/api'

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

/** The tabs the gateway config turns on, in display order. */
export function enabledAttributionTabs(session: AuthSessionView | null | undefined) {
  const enabled = new Set(session?.capabilities.attribution_tabs ?? [])
  return ATTRIBUTION_TABS.filter((tab) => enabled.has(tab))
}

/** The requested tab when it is on, else the first one that is. */
export function resolveAttributionTab(
  requested: AttributionTab | undefined,
  session: AuthSessionView | null | undefined,
) {
  const enabled = enabledAttributionTabs(session)
  return requested && enabled.includes(requested) ? requested : enabled[0]
}

export type PullRequestView = AttributionReportView['pull_requests'][number]
export type ChangeView = AttributionReportView['changes'][number]
export type DayView = AttributionReportView['daily'][number]
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

/** Whole buckets of `size` days counted back from the window's last day. */
export function bucketDays(daily: DayView[], size: number): Bucket[] {
  const buckets: Bucket[] = []
  const start = daily.length % size
  for (let index = start; index < daily.length; index += size) {
    const days = daily.slice(index, index + size)
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

/** AI share bands: more honest than an AI-assisted yes or no. */
export const AI_BANDS = [
  { key: 'none', label: 'No AI', min: 0, max: 0.01 },
  { key: 'light', label: '1–49% AI', min: 0.01, max: 0.5 },
  { key: 'heavy', label: '50–89% AI', min: 0.5, max: 0.9 },
  { key: 'agentic', label: '90%+ AI', min: 0.9, max: 1.01 },
] as const
export type AiBandKey = (typeof AI_BANDS)[number]['key']

export function aiBand(share: number): AiBandKey {
  return (AI_BANDS.find((band) => share >= band.min && share < band.max) ?? AI_BANDS[3]).key
}

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
export function weeklyLeadTimes(rows: PullRequestRow[], windowEnd: string, weeks: number) {
  const end = new Date(windowEnd).getTime()
  const week = 7 * 86_400_000
  return Array.from({ length: weeks }, (_, index) => {
    const to = end - (weeks - 1 - index) * week
    const hours = rows.flatMap((row) => {
      const merged = new Date(row.pr.merged_at).getTime()
      return merged > to - week && merged <= to && row.leadHours != null ? [row.leadHours] : []
    })
    return {
      day: new Date(to - week + 86_400_000).toISOString().slice(0, 10),
      p50: percentile(hours, 0.5),
      p85: percentile(hours, 0.85),
    }
  })
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
  const names = new Set([
    ...changes.map((c) => c.repository),
    ...pullRequests.map((p) => p.repository),
  ])
  const start = new Date(windowStart).getTime()
  const span = Math.max(1, new Date(windowEnd).getTime() - start)
  const slots = 8
  return [...names]
    .map((repository) => {
      const commits = changes.filter((change) => change.repository === repository)
      const prs = pullRequestRows(pullRequests.filter((pr) => pr.repository === repository))
      const aiLines = commits.reduce((total, change) => total + change.ai_lines, 0)
      const humanLines = commits.reduce((total, change) => total + change.human_lines, 0)
      const autonomous = commits
        .filter((change) => change.ai_lines > 0 && change.human_lines <= change.ai_lines * 0.05)
        .reduce((total, change) => total + change.ai_lines, 0)
      const trend = Array.from({ length: slots }, (_, slot) => {
        const inSlot = commits.filter((change) => {
          const at = (new Date(change.committed_at).getTime() - start) / span
          return Math.min(slots - 1, Math.max(0, Math.floor(at * slots))) === slot
        })
        const ai = inSlot.reduce((total, change) => total + change.ai_lines, 0)
        return ratio(ai, ai + inSlot.reduce((total, change) => total + change.human_lines, 0))
      })
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

/** AI lines per harness across the window's commits; a commit's lines split across its tools. */
export function harnessShares(changes: ChangeView[]) {
  const lines = new Map<string, number>()
  for (const change of changes) {
    if (change.agent_tools.length === 0 || change.ai_lines === 0) continue
    const share = change.ai_lines / change.agent_tools.length
    for (const tool of change.agent_tools) lines.set(tool, (lines.get(tool) ?? 0) + share)
  }
  return [...lines.entries()]
    .map(([harness, aiLines]) => ({ harness, aiLines: Math.round(aiLines) }))
    .sort((a, b) => b.aiLines - a.aiLines)
}
