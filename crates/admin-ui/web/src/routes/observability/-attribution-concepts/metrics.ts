// Derivations behind the concept mockups. Each function reads the sample dataset the way a gateway
// aggregation would read stored attribution rows, so the same names can move server side later.

import {
  AGENT_TOOLS,
  DAY_MS,
  HOUR_MS,
  MOCK_REPOSITORIES,
  MOCK_USERS,
  type AgentTool,
  type MockCommit,
  type MockDataset,
  type MockPullRequest,
  type MockSession,
} from './fixtures'
import { AI_BANDS, aiBand, type AiBandKey } from '@/components/attribution/ai-bands'

export const CONCEPT_WINDOWS = [7, 30, 90] as const
export type ConceptWindow = (typeof CONCEPT_WINDOWS)[number]

export type WindowSlice = {
  days: ConceptWindow
  from: number
  to: number
  commits: MockCommit[]
  sessions: MockSession[]
  /** PRs merged, or still open or closed, inside the window. */
  pullRequests: MockPullRequest[]
  merged: MockPullRequest[]
  commitBySha: Map<string, MockCommit>
}

/** Rows inside `[to - days, to]`. `offset` windows step back, for the previous period. */
export function sliceWindow(dataset: MockDataset, days: ConceptWindow, offset = 0): WindowSlice {
  const to = dataset.now - offset * days * DAY_MS
  const from = to - days * DAY_MS
  const inWindow = (at: number) => at > from && at <= to
  const commits = dataset.commits.filter((commit) => inWindow(commit.at))
  const pullRequests = dataset.pullRequests.filter((pr) => inWindow(pr.mergedAt ?? pr.openedAt))
  const commitBySha = new Map(dataset.commits.map((commit) => [commit.sha, commit]))
  return {
    days,
    from,
    to,
    commits,
    sessions: dataset.sessions.filter((session) => inWindow(session.startedAt)),
    pullRequests,
    merged: pullRequests.filter((pr) => pr.state === 'merged'),
    commitBySha,
  }
}

export function sum<T>(rows: T[], value: (row: T) => number) {
  let total = 0
  for (const row of rows) total += value(row)
  return total
}

export function ratio(part: number, whole: number) {
  return whole > 0 ? part / whole : null
}

export function median(values: number[]) {
  return percentile(values, 0.5)
}

export function percentile(values: number[], p: number) {
  if (values.length === 0) return null
  const sorted = [...values].sort((a, b) => a - b)
  const index = (sorted.length - 1) * p
  const lower = Math.floor(index)
  const upper = Math.ceil(index)
  return sorted[lower] + (sorted[upper] - sorted[lower]) * (index - lower)
}

export function delta(current: number | null, previous: number | null) {
  if (current == null || previous == null || previous === 0) return null
  return (current - previous) / Math.abs(previous)
}

// Pull request roll-ups ----------------------------------------------------------------------------

export type PullRequestRollup = {
  pr: MockPullRequest
  commits: MockCommit[]
  aiLines: number
  humanLines: number
  unknownLines: number
  addedLines: number
  aiShare: number
  requests: number
  tokens: number
  costUsd10000: number
  sessionCount: number
  tools: AgentTool[]
  models: string[]
  /** First commit to merge: the delivery lead time we can compute today. */
  leadTimeMs: number | null
  /** PR opened to merge: needs `opened_at` from `oceans hook prs`. */
  reviewTimeMs: number | null
}

export function rollupPullRequest(slice: WindowSlice, pr: MockPullRequest): PullRequestRollup {
  const commits = pr.commitShas
    .map((sha) => slice.commitBySha.get(sha))
    .filter((commit): commit is MockCommit => Boolean(commit))
  const aiLines = sum(commits, (commit) => commit.aiLines)
  const humanLines = sum(commits, (commit) => commit.humanLines)
  const unknownLines = sum(commits, (commit) => commit.unknownLines)
  const addedLines = aiLines + humanLines + unknownLines
  const tools = [...new Set(commits.flatMap((commit) => (commit.tool ? [commit.tool] : [])))]
  const models = [...new Set(commits.flatMap((commit) => (commit.model ? [commit.model] : [])))]
  return {
    pr,
    commits,
    aiLines,
    humanLines,
    unknownLines,
    addedLines,
    aiShare: addedLines > 0 ? aiLines / addedLines : 0,
    requests: sum(commits, (commit) => commit.requests),
    tokens: sum(commits, (commit) => commit.tokens),
    costUsd10000: sum(commits, (commit) => commit.costUsd10000),
    sessionCount: new Set(commits.flatMap((commit) => commit.sessionIds)).size,
    tools,
    models,
    leadTimeMs: pr.mergedAt ? pr.mergedAt - pr.firstCommitAt : null,
    reviewTimeMs: pr.mergedAt ? pr.mergedAt - pr.openedAt : null,
  }
}

export { AI_BANDS, aiBand, type AiBandKey }

// Headline totals ----------------------------------------------------------------------------------

export type HeadlineTotals = {
  commits: number
  mergedPrs: number
  aiLines: number
  humanLines: number
  unknownLines: number
  aiShare: number | null
  /** Merged AI lines: the "AI merged to main" figure. */
  mergedAiLines: number
  mergedAiShare: number | null
  costUsd10000: number
  committedCostUsd10000: number
  uncommittedCostUsd10000: number
  /** Gateway-attributed cost of merged PRs, per PR. */
  medianCostPerMergedPr: number | null
  aiLinesPerDollar: number | null
  sessions: number
  joinedSessions: number
  activeAiUsers: number
  committingUsers: number
  medianLeadTimeMs: number | null
  tokens: number
}

export function headline(slice: WindowSlice): HeadlineTotals {
  const rollups = slice.merged.map((pr) => rollupPullRequest(slice, pr))
  const aiLines = sum(slice.commits, (commit) => commit.aiLines)
  const humanLines = sum(slice.commits, (commit) => commit.humanLines)
  const mergedAiLines = sum(rollups, (row) => row.aiLines)
  const mergedAdded = sum(rollups, (row) => row.addedLines)
  const committedCost = sum(slice.commits, (commit) => commit.costUsd10000)
  const uncommittedCost = sum(
    slice.sessions.filter((session) => session.commitShas.length === 0),
    (session) => session.costUsd10000,
  )
  return {
    commits: slice.commits.length,
    mergedPrs: slice.merged.length,
    aiLines,
    humanLines,
    unknownLines: sum(slice.commits, (commit) => commit.unknownLines),
    aiShare: ratio(aiLines, aiLines + humanLines),
    mergedAiLines,
    mergedAiShare: ratio(mergedAiLines, mergedAdded),
    costUsd10000: committedCost + uncommittedCost,
    committedCostUsd10000: committedCost,
    uncommittedCostUsd10000: uncommittedCost,
    medianCostPerMergedPr: median(rollups.map((row) => row.costUsd10000)),
    aiLinesPerDollar: ratio(aiLines, committedCost / 10_000),
    sessions: slice.sessions.length,
    joinedSessions: slice.sessions.filter((session) => session.joined).length,
    activeAiUsers: new Set(slice.sessions.map((session) => session.userId)).size,
    committingUsers: new Set(slice.commits.map((commit) => commit.userId)).size,
    medianLeadTimeMs: median(rollups.flatMap((row) => (row.leadTimeMs ? [row.leadTimeMs] : []))),
    tokens: sum(slice.commits, (commit) => commit.tokens),
  }
}

// Time series --------------------------------------------------------------------------------------

export type DayPoint = {
  day: string
  aiLines: number
  humanLines: number
  aiShare: number | null
  commits: number
  mergedPrs: number
  committedCost: number
  uncommittedCost: number
  medianLeadHours: number | null
}

function dayKey(at: number) {
  return new Date(at).toISOString().slice(0, 10)
}

/** Daily buckets, or weekly ones when `bucketDays` is 7. Keys are the bucket's first day. */
export function series(slice: WindowSlice, bucketDays = 1): DayPoint[] {
  const buckets: DayPoint[] = []
  const bucketMs = bucketDays * DAY_MS
  // Whole buckets counted back from the window end, so the latest bucket is never a stub.
  const count = Math.floor((slice.to - slice.from) / bucketMs)
  for (let index = 0; index < count; index += 1) {
    const start = slice.to - (count - index) * bucketMs
    const end = start + bucketMs
    const inBucket = (at: number) => at > start && at <= end
    const commits = slice.commits.filter((commit) => inBucket(commit.at))
    const merged = slice.merged.filter((pr) => pr.mergedAt != null && inBucket(pr.mergedAt))
    const aiLines = sum(commits, (commit) => commit.aiLines)
    const humanLines = sum(commits, (commit) => commit.humanLines)
    const lead = median(merged.map((pr) => ((pr.mergedAt ?? 0) - pr.firstCommitAt) / HOUR_MS))
    buckets.push({
      day: dayKey(start + DAY_MS / 2),
      aiLines,
      humanLines,
      aiShare: ratio(aiLines, aiLines + humanLines),
      commits: commits.length,
      mergedPrs: merged.length,
      committedCost: sum(commits, (commit) => commit.costUsd10000) / 10_000,
      uncommittedCost:
        sum(
          slice.sessions.filter(
            (session) => session.commitShas.length === 0 && inBucket(session.startedAt),
          ),
          (session) => session.costUsd10000,
        ) / 10_000,
      medianLeadHours: lead,
    })
  }
  return buckets
}

// Session outcomes ---------------------------------------------------------------------------------

export type SessionOutcome = 'merged' | 'in_review' | 'closed' | 'direct' | 'branch' | 'no_commit'

export const SESSION_OUTCOME_LABELS: Record<SessionOutcome, string> = {
  merged: 'Merged',
  in_review: 'In review',
  closed: 'Closed unmerged',
  direct: 'Pushed to main',
  branch: 'Branch, no PR yet',
  no_commit: 'No commit',
}

/** Where each session's work ended up: the furthest stage any of its commits reached. */
export function sessionOutcomes(dataset: MockDataset, slice: WindowSlice) {
  const prByKey = new Map(dataset.pullRequests.map((pr) => [`${pr.repository}#${pr.number}`, pr]))
  const rank: Record<SessionOutcome, number> = {
    merged: 5,
    direct: 4,
    in_review: 3,
    branch: 2,
    closed: 1,
    no_commit: 0,
  }
  const tally = new Map<SessionOutcome, { sessions: number; costUsd10000: number }>()
  const rows: { session: MockSession; outcome: SessionOutcome }[] = []
  for (const session of slice.sessions) {
    let outcome: SessionOutcome = 'no_commit'
    for (const sha of session.commitShas) {
      const commit = slice.commitBySha.get(sha)
      if (!commit) continue
      let next: SessionOutcome = 'branch'
      if (commit.isDefaultBranch) next = 'direct'
      else if (commit.pullRequestNumber != null) {
        const pr = prByKey.get(`${commit.repository}#${commit.pullRequestNumber}`)
        next = pr?.state === 'merged' ? 'merged' : pr?.state === 'closed' ? 'closed' : 'in_review'
      }
      if (rank[next] > rank[outcome]) outcome = next
    }
    rows.push({ session, outcome })
    const entry = tally.get(outcome) ?? { sessions: 0, costUsd10000: 0 }
    entry.sessions += 1
    entry.costUsd10000 += session.costUsd10000
    tally.set(outcome, entry)
  }
  return { rows, tally }
}

// Breakdowns ---------------------------------------------------------------------------------------

export type RepositoryRow = {
  repository: string
  commits: number
  mergedPrs: number
  aiShare: number | null
  /** Agent lines written per line committed: Git AI's generated : committed thrash ratio. */
  generatedPerCommitted: number | null
  /** Lines an agent landed with no human edits in the same commit, as a share of AI lines. */
  autonomy: number | null
  medianCostPerMergedPr: number | null
  medianLeadTimeMs: number | null
  durable30: number | null
  revertRate: number | null
  costUsd10000: number
  weeklyAiShare: (number | null)[]
}

export function repositoryRows(slice: WindowSlice): RepositoryRow[] {
  return MOCK_REPOSITORIES.map(({ name }) => {
    const commits = slice.commits.filter((commit) => commit.repository === name)
    const merged: PullRequestRollup[] = []
    for (const pr of slice.merged) {
      if (pr.repository === name) merged.push(rollupPullRequest(slice, pr))
    }
    const sessions = slice.sessions.filter((session) => session.repository === name)
    const aiLines = sum(commits, (commit) => commit.aiLines)
    const humanLines = sum(commits, (commit) => commit.humanLines)
    const aiCommits = commits.filter((commit) => commit.aiLines > 0)
    const pureAi = aiCommits.filter((commit) => commit.humanLines <= commit.aiLines * 0.05)
    const mergedAi = sum(merged, (row) => row.aiLines)
    const weeks = Math.max(1, Math.round(slice.days / 7))
    const weeklyAiShare = Array.from({ length: Math.min(weeks, 13) }, (_, index) => {
      const end = slice.to - (Math.min(weeks, 13) - 1 - index) * 7 * DAY_MS
      const inWeek = commits.filter((commit) => commit.at > end - 7 * DAY_MS && commit.at <= end)
      const ai = sum(inWeek, (commit) => commit.aiLines)
      return ratio(ai, ai + sum(inWeek, (commit) => commit.humanLines))
    })
    return {
      repository: name,
      commits: commits.length,
      mergedPrs: merged.length,
      aiShare: ratio(aiLines, aiLines + humanLines),
      generatedPerCommitted: ratio(
        sum(sessions, (session) => session.generatedLines),
        aiLines,
      ),
      autonomy: ratio(
        sum(pureAi, (commit) => commit.aiLines),
        aiLines,
      ),
      medianCostPerMergedPr: median(merged.map((row) => row.costUsd10000)),
      medianLeadTimeMs: median(merged.flatMap((row) => (row.leadTimeMs ? [row.leadTimeMs] : []))),
      durable30: ratio(
        sum(merged, (row) => row.pr.aiLinesDurable30),
        mergedAi,
      ),
      revertRate: ratio(merged.filter((row) => row.pr.reverted).length, merged.length),
      costUsd10000:
        sum(commits, (commit) => commit.costUsd10000) +
        sum(
          sessions.filter((session) => session.commitShas.length === 0),
          (session) => session.costUsd10000,
        ),
      weeklyAiShare,
    }
  }).sort((a, b) => b.commits - a.commits)
}

export type ToolModelCell = {
  tool: AgentTool
  model: string
  sessions: number
  aiLines: number
  costUsd10000: number
  /** Cost per 100 committed AI lines. */
  costPer100Lines: number | null
  mergedShare: number | null
  steeringPerSession: number | null
}

export function toolModelCells(dataset: MockDataset, slice: WindowSlice): ToolModelCell[] {
  const { rows } = sessionOutcomes(dataset, slice)
  const cells = new Map<string, ToolModelCell & { merged: number; steering: number }>()
  for (const { session, outcome } of rows) {
    const key = `${session.tool}::${session.model}`
    const cell = cells.get(key) ?? {
      tool: session.tool,
      model: session.model,
      sessions: 0,
      aiLines: 0,
      costUsd10000: 0,
      costPer100Lines: null,
      mergedShare: null,
      steeringPerSession: null,
      merged: 0,
      steering: 0,
    }
    cell.sessions += 1
    cell.costUsd10000 += session.costUsd10000
    cell.steering += session.steeringTurns
    for (const sha of session.commitShas) {
      const commit = slice.commitBySha.get(sha)
      if (commit) cell.aiLines += commit.aiLines / Math.max(1, commit.sessionIds.length)
    }
    if (outcome === 'merged' || outcome === 'direct') cell.merged += 1
    cells.set(key, cell)
  }
  return Array.from(cells.values(), ({ merged, steering, ...cell }) => ({
    ...cell,
    aiLines: Math.round(cell.aiLines),
    costPer100Lines: cell.aiLines > 0 ? (cell.costUsd10000 / cell.aiLines) * 100 : null,
    mergedShare: ratio(merged, cell.sessions),
    steeringPerSession: ratio(steering, cell.sessions),
  }))
}

export function toolTotals(dataset: MockDataset, slice: WindowSlice) {
  const cells = toolModelCells(dataset, slice)
  return AGENT_TOOLS.flatMap((tool) => {
    const own = cells.filter((cell) => cell.tool === tool)
    const aiLines = sum(own, (cell) => cell.aiLines)
    const cost = sum(own, (cell) => cell.costUsd10000)
    const sessions = sum(own, (cell) => cell.sessions)
    if (sessions === 0) return []
    return {
      tool,
      sessions,
      aiLines,
      costUsd10000: cost,
      costPer100Lines: aiLines > 0 ? (cost / aiLines) * 100 : null,
      mergedShare: ratio(
        sum(own, (cell) => (cell.mergedShare ?? 0) * cell.sessions),
        sessions,
      ),
      steeringPerSession: ratio(
        sum(own, (cell) => (cell.steeringPerSession ?? 0) * cell.sessions),
        sessions,
      ),
    }
  })
}

/** Weekly cost by model, for the model mix chart. */
export function weeklyModelCost(slice: WindowSlice) {
  const weeks = Math.max(1, Math.floor(slice.days / 7))
  const models = [...new Set(slice.sessions.map((session) => session.model))]
  const totals = new Map(models.map((model) => [model, 0]))
  const rows = Array.from({ length: weeks }, (_, index) => {
    const start = slice.to - (weeks - index) * 7 * DAY_MS
    const row: Record<string, number | string> = { day: dayKey(start + DAY_MS) }
    for (const model of models) row[model] = 0
    for (const session of slice.sessions) {
      if (session.startedAt > start && session.startedAt <= start + 7 * DAY_MS) {
        row[session.model] = Number(row[session.model]) + session.costUsd10000 / 10_000
        totals.set(session.model, (totals.get(session.model) ?? 0) + session.costUsd10000)
      }
    }
    return row
  })
  const ranked = [...totals.entries()].sort((a, b) => b[1] - a[1]).map(([model]) => model)
  return { rows, models: ranked }
}

// People -------------------------------------------------------------------------------------------

/** One user's daily activity for the contribution calendar. */
export function userCalendar(dataset: MockDataset, userId: string, days = 140) {
  const from = dataset.now - days * DAY_MS
  const cells: { day: string; commits: number; aiShare: number | null; weekday: number }[] = []
  for (let index = 0; index < days; index += 1) {
    const start = from + index * DAY_MS
    const commits = dataset.commits.filter(
      (commit) => commit.userId === userId && commit.at > start && commit.at <= start + DAY_MS,
    )
    const ai = sum(commits, (commit) => commit.aiLines)
    cells.push({
      day: dayKey(start + DAY_MS / 2),
      commits: commits.length,
      aiShare: ratio(ai, ai + sum(commits, (commit) => commit.humanLines)),
      weekday: new Date(start + DAY_MS / 2).getUTCDay(),
    })
  }
  return cells
}

/** Commits by weekday and hour, for the punch card. */
export function punchCard(commits: MockCommit[]) {
  const grid = Array.from({ length: 7 }, () =>
    Array.from({ length: 24 }, () => ({ ai: 0, all: 0 })),
  )
  for (const commit of commits) {
    const date = new Date(commit.at)
    const cell = grid[date.getUTCDay()][date.getUTCHours()]
    cell.all += 1
    if (commit.aiLines > commit.humanLines) cell.ai += 1
  }
  return grid
}

export function teamOf(userId: string) {
  return MOCK_USERS.find((user) => user.id === userId)?.team ?? 'Unknown'
}

// Durability (needs new data) ----------------------------------------------------------------------

/**
 * Share of merged lines still unchanged N days after merge. AI and human lines decay at different
 * rates in the sample, which is what a survival view should make visible.
 */
export function survivalCurve(slice: WindowSlice) {
  const rollups = slice.merged.map((pr) => rollupPullRequest(slice, pr))
  const ai30 =
    ratio(
      sum(rollups, (row) => row.pr.aiLinesDurable30),
      sum(rollups, (row) => row.aiLines),
    ) ?? 0.75
  const human30 = Math.min(0.97, ai30 + 0.11)
  const decay = (at30: number, day: number) =>
    1 - ((1 - at30) * (1 - Math.exp(-day / 9))) / (1 - Math.exp(-30 / 9))
  return [0, 1, 2, 3, 5, 7, 10, 14, 21, 30, 45, 60, 90].map((day) => ({
    day,
    ai: Math.max(0, decay(ai30, day)),
    human: Math.max(0, decay(human30, day)),
  }))
}
