import { describe, expect, it } from 'vitest'

import { getMockDataset } from '@/routes/observability/-attribution-concepts/fixtures'
import {
  aiBand,
  headline,
  rollupPullRequest,
  series,
  sessionOutcomes,
  sliceWindow,
} from '@/routes/observability/-attribution-concepts/metrics'

// The concept mockups read as a story only if the sample data holds together. These pin the
// properties each concept relies on, so a generator tweak can't quietly break a page.
describe('attribution concept sample data', () => {
  const dataset = getMockDataset()
  const slice = sliceWindow(dataset, 30)

  it('is deterministic across loads', () => {
    expect(getMockDataset()).toBe(dataset)
    expect(dataset.commits.length).toBeGreaterThan(1_000)
  })

  it('accounts for every session and every dollar in the outcome flow', () => {
    const { tally } = sessionOutcomes(dataset, slice)
    const entries = [...tally.values()]
    expect(entries.reduce((total, entry) => total + entry.sessions, 0)).toBe(slice.sessions.length)
    const cost = slice.sessions.reduce((total, session) => total + session.costUsd10000, 0)
    expect(entries.reduce((total, entry) => total + entry.costUsd10000, 0)).toBe(cost)
  })

  it('keeps AI share a fraction of attested lines', () => {
    const totals = headline(slice)
    expect(totals.aiShare).toBeGreaterThan(0)
    expect(totals.aiShare).toBeLessThan(1)
    expect(totals.costUsd10000).toBe(totals.committedCostUsd10000 + totals.uncommittedCostUsd10000)
  })

  it('lists PR commits in commit order, so lead time is never negative', () => {
    for (const pr of slice.merged) {
      const rollup = rollupPullRequest(slice, pr)
      expect(rollup.leadTimeMs).toBeGreaterThanOrEqual(0)
      const times = rollup.commits.map((commit) => commit.at)
      expect(times).toEqual([...times].sort((a, b) => a - b))
    }
  })

  it('fills every AI band, including fully agentic PRs', () => {
    const bands = new Set(slice.merged.map((pr) => aiBand(rollupPullRequest(slice, pr).aiShare)))
    expect(bands).toContain('agentic')
    expect(bands).toContain('light')
  })

  it('buckets whole periods back from the window end', () => {
    const weekly = series(sliceWindow(dataset, 90), 7)
    expect(weekly).toHaveLength(12)
    expect(weekly.at(-1)?.commits).toBeGreaterThan(0)
  })
})
