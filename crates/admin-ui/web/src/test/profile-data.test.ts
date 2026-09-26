import { describe, expect, it } from 'vitest'

import {
  budgetStatus,
  buildHeatmap,
  formatProfileCost,
  harnessRequestsChart,
  modelRequestsChart,
  profileHeadlines,
  profileInRange,
  rankModels,
  sortProfileApiKeys,
  summarizeDays,
  tokenVolumeChart,
  type ProfileRange,
} from '@/routes/profile/-components/profile-data'
import { apiKey, profileDay, profileView } from '@/test/profile-fixtures'

describe('profile data', () => {
  it('summarizes totals and the cache hit rate over cacheable input', () => {
    const totals = summarizeDays(profileView().days)

    expect(totals.requests).toBe(16)
    expect(totals.totalTokens).toBe(10_700)
    expect(totals.activeDays).toBe(3)
    // 1,200 cache reads over 3,000 cacheable input tokens.
    expect(totals.cacheHitRate).toBeCloseTo(0.4)
  })

  it('reports no cache hit rate when nothing was cacheable', () => {
    const totals = summarizeDays([
      profileDay('2026-09-25', { uncached_input_tokens: 0, cache_read_tokens: 0 }),
    ])
    expect(totals.cacheHitRate).toBeNull()
  })

  it('picks the preferred model and client by request count', () => {
    const headlines = profileHeadlines(profileView())

    expect(headlines.model).toMatchObject({ key: 'reasoning', requests: 13 })
    expect(headlines.model?.share).toBeCloseTo(13 / 16)
    expect(headlines.harness).toMatchObject({ key: 'claude_code', label: 'Claude Code' })
  })

  it('narrows every series to the selected range', () => {
    const scoped = profileInRange(profileView(), 2 as ProfileRange)

    expect(scoped.days.map((day) => day.day)).toEqual(['2026-09-24', '2026-09-25'])
    expect(rankModels(scoped.model_days).map((entry) => entry.key)).toEqual(['reasoning', 'fast'])
  })

  it('zero-fills daily buckets and switches to weekly buckets for a year', () => {
    const daily = tokenVolumeChart(profileView(), 30)
    expect(daily.rows).toHaveLength(30)
    expect(daily.rows.at(-1)).toMatchObject({ day: '2026-09-25', output: 1_000, cached: 400 })

    const weekly = modelRequestsChart(profileView(), 365)
    // 365 days is 52 whole weeks plus one day; the newest bucket is whole and the spare day drops.
    expect(weekly.rows).toHaveLength(52)
    expect(weekly.rows.at(-1)?.day).toBe('2026-09-19')
    const total = weekly.rows.reduce(
      (sum, row) => sum + Number(row.series_1 ?? 0) + Number(row.series_2 ?? 0),
      0,
    )
    expect(total).toBe(16)
  })

  it('flags cost that leaves out unpriced requests', () => {
    const totals = summarizeDays([
      profileDay('2026-09-24', { unpriced_request_count: 2, usage_missing_request_count: 1 }),
      profileDay('2026-09-25'),
    ])
    expect(totals.unpricedRequests).toBe(3)
    expect(formatProfileCost(totals.costUsd10000, totals.unpricedRequests)).toBe(
      '$5.00 + 3 unpriced',
    )
    expect(formatProfileCost(0, 0)).toBe('$0.00')
  })

  it('folds harnesses beyond the top five into other', () => {
    const harness_days = ['a', 'b', 'c', 'd', 'e', 'f'].map((key, index) => ({
      day: '2026-09-25',
      harness_key: key,
      harness_label: key.toUpperCase(),
      request_count: 10 - index,
      total_tokens: 1,
    }))
    const chart = harnessRequestsChart(profileView({ harness_days }), 30)

    expect(chart.keys.map((key) => key.label)).toEqual(['A', 'B', 'C', 'D', 'E', 'Other'])
    expect(chart.rows.at(-1)?.series_other).toBe(5)
  })

  it('lays the heatmap out Sunday-first, ending on the last profile day', () => {
    const heatmap = buildHeatmap(profileView())

    expect(heatmap.weeks).toHaveLength(53)
    const lastWeek = heatmap.weeks.at(-1) ?? []
    // 2026-09-25 is a Friday, so Saturday is padding.
    expect(lastWeek[5]).toMatchObject({ day: '2026-09-25', level: 4 })
    expect(lastWeek[6]).toBeNull()
    // History starts on Friday 2025-09-26; the days before it are unknown, not zero usage.
    expect(heatmap.weeks[0].slice(0, 5)).toEqual([null, null, null, null, null])
    expect(heatmap.weeks[0][5]).toMatchObject({ day: '2025-09-26', level: 0 })
    // The partial September label would crowd October's, so it is dropped.
    expect(heatmap.months[0]).toMatchObject({ week: 1, label: 'Oct' })
    expect(lastWeek[4]?.level).toBeGreaterThan(0)
    expect(lastWeek[0]?.level).toBe(0)
  })

  it('lists active and recently used keys first', () => {
    const keys = sortProfileApiKeys([
      apiKey('old', { last_used_at: '2026-09-01T00:00:00Z' }),
      apiKey('revoked', { status: 'revoked', last_used_at: '2026-09-25T00:00:00Z' }),
      apiKey('recent', { last_used_at: '2026-09-24T00:00:00Z' }),
    ])
    expect(keys.map((key) => key.id)).toEqual(['recent', 'old', 'revoked'])
  })

  it('flags budgets near and over the limit', () => {
    const budget = profileView().budget!
    expect(budgetStatus(budget)).toMatchObject({ ratio: 0.25, tone: 'ok', remaining: 750_000 })
    expect(budgetStatus({ ...budget, spent_usd_10000: 850_000 }).tone).toBe('warning')
    expect(budgetStatus({ ...budget, spent_usd_10000: 1_200_000 })).toMatchObject({
      tone: 'over',
      remaining: 0,
    })
  })
})
