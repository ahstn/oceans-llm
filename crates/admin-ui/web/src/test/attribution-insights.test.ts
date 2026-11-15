import { describe, expect, it } from 'vitest'

import {
  aiBand,
  attributionReportDeps,
  ATTRIBUTION_TABS,
  attributionReportInput,
  type AttributionTab,
  reportShapeTab,
  attributionTabRedirect,
  bucketDays,
  enabledAttributionTabs,
  harnessShares,
  mergedShareByBucket,
  resolveAttributionTab,
  weeklyLeadTimes,
} from '@/routes/observability/-attribution/insights'

import { deployBand, failBand, leadBand } from '@/components/attribution/delivery-bands'
import { canonicalHarness, harnessLabel } from '@/components/attribution/format'

import { platformAdminSession } from './auth-session'

describe('attribution report input', () => {
  const admin = platformAdminSession()

  it('scopes My work to the signed-in user whatever user filter the URL carries', () => {
    expect(
      attributionReportInput({ tab: 'my_work', days: 30, user_id: 'someone-else' }, admin),
    ).toEqual({ days: 30, user_id: admin.user.id, compare: true })
  })

  it('keeps the user filter on other tabs and drops the tab itself', () => {
    expect(
      attributionReportInput({ tab: 'impact', days: 7, user_id: 'someone-else' }, admin),
    ).toEqual({ days: 7, user_id: 'someone-else', compare: true })
  })

  it('asks for the previous window only on tabs that show changes against it', () => {
    expect(attributionReportInput({ tab: 'delivery', days: 7 }, admin)).toEqual({
      days: 7,
      compare: false,
    })
  })

  it('shares loader deps between tabs that read the same report', () => {
    const deps = (tab: AttributionTab) => attributionReportDeps({ tab, days: 30, user_id: 'u' })
    expect(deps('delivery')).toEqual(deps('pull_requests'))
    expect(deps('durability')).toEqual(deps('repositories'))
    expect(deps('impact')).not.toEqual(deps('delivery'))
    expect(deps('my_work')).not.toEqual(deps('impact'))
    // The loader rebuilds the same input from the deps as from the full search.
    for (const tab of ATTRIBUTION_TABS) {
      const { report, ...filters } = attributionReportDeps({ tab, days: 7 })
      expect(
        attributionReportInput({ ...filters, tab: reportShapeTab(report, admin) }, admin),
      ).toEqual(attributionReportInput({ tab, days: 7 }, admin))
    }
  })

  it("loads the first enabled tab's report for a bare URL with no tab", () => {
    const bare: { tab?: AttributionTab; days: 30 } = { days: 30 }
    const { report, ...filters } = attributionReportDeps(bare)
    expect(report).toBeUndefined()
    // The loader passes no tab, and the input resolves it to the first enabled one (Impact).
    expect(
      attributionReportInput({ ...filters, tab: reportShapeTab(report, admin) }, admin),
    ).toEqual({ days: 30, compare: true })
  })

  it('redirects a turned-off tab to the one the page renders, so the loader reads its report', () => {
    const noDelivery = {
      ...admin,
      capabilities: {
        ...admin.capabilities,
        attribution_tabs: ['impact' as const, 'pull_requests' as const],
      },
    }
    const next = attributionTabRedirect({ tab: 'delivery' as const, days: 30 }, noDelivery)
    expect(next).toEqual({ tab: 'impact', days: 30 })
    const { report, ...filters } = attributionReportDeps(next ?? {})
    expect(
      attributionReportInput({ ...filters, tab: reportShapeTab(report, noDelivery) }, noDelivery),
    ).toEqual({ days: 30, compare: true })
    expect(attributionTabRedirect({ tab: 'impact' as const, days: 30 }, noDelivery)).toBeNull()
    // A URL with no tab names the first enabled one, so the loader never guesses.
    const bare: { tab?: AttributionTab; days: 30 } = { days: 30 }
    expect(attributionTabRedirect(bare, noDelivery)).toEqual({ days: 30, tab: 'impact' })
    const conceptsOnly = {
      ...admin,
      capabilities: { ...admin.capabilities, attribution_tabs: ['concepts' as const] },
    }
    expect(attributionTabRedirect(bare, conceptsOnly)).toBeNull()
  })

  it('reads no report when every report tab is off', () => {
    const conceptsOnly = {
      ...admin,
      capabilities: { ...admin.capabilities, attribution_tabs: ['concepts' as const] },
    }
    expect(attributionReportInput({ tab: 'impact', days: 30 }, conceptsOnly)).toBeNull()
  })

  it('treats a turned-off My work tab like any other request', () => {
    const noMyWork = {
      ...admin,
      capabilities: { ...admin.capabilities, attribution_tabs: ['impact' as const] },
    }
    expect(resolveAttributionTab('my_work', noMyWork)).toBe('impact')
    expect(
      attributionReportInput({ tab: 'my_work', days: 30, user_id: 'someone-else' }, noMyWork),
    ).toEqual({ days: 30, user_id: 'someone-else', compare: true })
  })
})

describe('AI share bands', () => {
  it('keeps No AI for exactly zero AI lines', () => {
    expect(aiBand(0)).toBe('none')
    expect(aiBand(0.005)).toBe('light')
    expect(aiBand(0.5)).toBe('heavy')
    expect(aiBand(0.95)).toBe('agentic')
    expect(aiBand(1)).toBe('agentic')
  })
})

describe('delivery bands', () => {
  it('orders change-fail bands by rate, with 10% High and 20% Low', () => {
    expect(failBand(null)).toBeUndefined()
    expect(failBand(0.05)).toBe('Elite')
    expect(failBand(0.1)).toBe('High')
    expect(failBand(0.15)).toBe('Medium')
    expect(failBand(0.2)).toBe('Low')
  })

  it('bands merges per day and lead time on the DORA 2024 thresholds', () => {
    expect(deployBand(1)).toBe('Elite')
    expect(deployBand(1 / 7)).toBe('High')
    expect(deployBand(1 / 30)).toBe('Medium')
    expect(deployBand(0.01)).toBe('Low')
    expect(leadBand(null)).toBeUndefined()
    expect(leadBand(23)).toBe('Elite')
    expect(leadBand(24)).toBe('High')
    expect(leadBand(24 * 7)).toBe('Medium')
    expect(leadBand(24 * 30)).toBe('Low')
  })
})

describe('day buckets', () => {
  const day = (index: number) => ({
    day: `2026-11-${String(index + 1).padStart(2, '0')}`,
    ai_lines: 1,
    human_lines: 0,
    commit_count: 1,
    merged_pull_request_count: 0,
    committed: { priced_cost_usd_10000: 10_000 },
    uncommitted: { priced_cost_usd_10000: 0 },
  })

  it('keeps the leading partial bucket so the buckets add up to the window', () => {
    const daily = Array.from({ length: 9 }, (_, index) => day(index)) as unknown as Parameters<
      typeof bucketDays
    >[0]
    const buckets = bucketDays(daily, 7)

    expect(buckets.map((bucket) => [bucket.day, bucket.commits])).toEqual([
      ['2026-11-01', 2],
      ['2026-11-03', 7],
    ])
    expect(buckets.reduce((total, bucket) => total + bucket.committedCost, 0)).toBe(9)
  })
})

describe('enabled tabs', () => {
  it("follows the gateway's order and leaves out the concepts link and unknown tabs", () => {
    const session = platformAdminSession()
    session.capabilities.attribution_tabs = ['delivery', 'concepts', 'impact']

    expect(enabledAttributionTabs(session)).toEqual(['delivery', 'impact'])
  })
})

describe('weekly lead times', () => {
  it('keeps merges from the leading partial week of the window', () => {
    const merged = (at: string, leadHours: number) =>
      ({ pr: { merged_at: at }, leadHours }) as unknown as Parameters<typeof weeklyLeadTimes>[0][0]
    const weeks = weeklyLeadTimes(
      [merged('2026-11-02T12:00:00Z', 4), merged('2026-11-09T12:00:00Z', 8)],
      '2026-11-01T00:00:00Z',
      '2026-11-10T00:00:00Z',
    )

    expect(weeks.map((week) => [week.day, week.p50])).toEqual([
      ['2026-11-01', 4],
      ['2026-11-03', 8],
    ])
  })
})

describe('harness shares', () => {
  it("splits a commit's lines by what each session wrote, and joins Claude Code's two names", () => {
    const row = (harness: string, aiLines: number) =>
      ({ harness, ai_lines: aiLines }) as unknown as Parameters<typeof harnessShares>[0][number]

    expect(harnessShares([row('claude', 600), row('codex', 100), row('claude_code', 300)])).toEqual(
      [
        { harness: 'claude', aiLines: 900 },
        { harness: 'codex', aiLines: 100 },
      ],
    )
  })
})

describe('merged share by bucket', () => {
  it('buckets merged pull request lines by merge date', () => {
    const bucket = (day: string) => ({ day }) as Parameters<typeof mergedShareByBucket>[1][number]
    const pr = (mergedAt: string, ai: number, human: number) =>
      ({ merged_at: mergedAt, ai_lines: ai, human_lines: human }) as unknown as Parameters<
        typeof mergedShareByBucket
      >[0][number]

    expect(
      mergedShareByBucket(
        [pr('2026-11-02T10:00:00Z', 30, 10), pr('2026-11-09T10:00:00Z', 0, 50)],
        [bucket('2026-11-01'), bucket('2026-11-08'), bucket('2026-11-15')],
      ),
    ).toEqual([0.75, 0, null])
  })
})

describe('harness names', () => {
  it('never returns inherited properties for a harness name', () => {
    for (const name of ['__proto__', 'toString', 'constructor']) {
      expect(canonicalHarness(name)).toBe(name)
      expect(harnessLabel(name)).toBe(name)
    }
    expect(canonicalHarness('claude_code')).toBe('claude')
    expect(harnessLabel('claude')).toBe('Claude Code')
  })
})
