import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  cacheHitRate,
  modelTokenChart,
  ownerCacheRows,
} from '@/routes/observability/-usage-costs/token-series'
import type { SpendReportView } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
}
const getSpendUsageReportMock = vi.fn()
const toastErrorMock = vi.fn()

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
}))

vi.mock('@/server/admin-data.functions', () => ({
  getUsageCosts: vi.fn(),
  getSpendUsageReport: (...args: unknown[]) => getSpendUsageReportMock(...args),
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}))

function tokenPoint(inputTokens: number, cacheReadTokens: number) {
  return {
    day_start: '2026-03-01T00:00:00Z',
    request_count: 1,
    input_tokens: inputTokens,
    output_tokens: inputTokens / 4,
    uncached_input_tokens: inputTokens - cacheReadTokens,
    cache_read_tokens: cacheReadTokens,
    cache_write_tokens: 0,
  }
}

function emptyReport(windowDays: 7 | 30, ownerKind: string) {
  return {
    window_days: windowDays,
    owner_kind: ownerKind,
    window_start: '2026-03-01T00:00:00Z',
    window_end: '2026-03-08T00:00:00Z',
    totals: {
      priced_cost_usd_10000: 0,
      priced_request_count: 0,
      unpriced_request_count: 0,
      usage_missing_request_count: 0,
      uncached_input_tokens: null,
      cache_read_tokens: null,
      cache_write_tokens: null,
    },
    daily: Array.from({ length: windowDays }, (_, index) => ({
      day_start: `2026-03-${String(index + 1).padStart(2, '0')}T00:00:00Z`,
      priced_cost_usd_10000: 0,
      priced_request_count: 0,
      unpriced_request_count: 0,
      usage_missing_request_count: 0,
    })),
    owners: [],
    models: [],
    owner_token_series: [],
    model_token_series: [],
  }
}

beforeEach(() => {
  routeMock.useLoaderData.mockReset()
  routeMock.useRouteContext.mockReset()
  getSpendUsageReportMock.mockReset()
  toastErrorMock.mockReset()
  routeMock.useRouteContext.mockReturnValue({
    session: {
      must_change_password: false,
      user: {
        id: 'user_1',
        name: 'Admin User',
        email: 'admin@example.com',
        global_role: 'platform_admin',
      },
    },
  })
})

afterEach(() => {
  cleanup()
})

describe('UsageCostsPage reports', () => {
  it('renders live ledger totals and owner/model breakdowns', async () => {
    routeMock.useLoaderData.mockReturnValue({
      exportOrigin: '',
      data: {
        window_days: 7,
        owner_kind: 'all',
        window_start: '2026-03-01T00:00:00Z',
        window_end: '2026-03-08T00:00:00Z',
        totals: {
          priced_cost_usd_10000: 123_450,
          priced_request_count: 42,
          unpriced_request_count: 3,
          usage_missing_request_count: 1,
          uncached_input_tokens: 12_345,
          cache_read_tokens: 234_567,
          cache_write_tokens: 6_789,
        },
        daily: [
          {
            day_start: '2026-03-01T00:00:00Z',
            priced_cost_usd_10000: 40_000,
            priced_request_count: 10,
            unpriced_request_count: 1,
            usage_missing_request_count: 0,
          },
        ],
        owners: [
          {
            owner_kind: 'service_account',
            owner_id: 'service_account_1',
            owner_name: 'CI Indexer',
            priced_cost_usd_10000: 80_000,
            priced_request_count: 20,
            unpriced_request_count: 2,
            usage_missing_request_count: 1,
          },
        ],
        models: [
          {
            model_key: 'fast',
            priced_cost_usd_10000: 100_000,
            priced_request_count: 24,
            unpriced_request_count: 2,
            usage_missing_request_count: 1,
          },
          ...Array.from({ length: 11 }, (_, index) => ({
            model_key: `long-tail-${index}`,
            priced_cost_usd_10000: 1_000 - index,
            priced_request_count: 1,
            unpriced_request_count: 0,
            usage_missing_request_count: 0,
          })),
        ],
        owner_token_series: [
          {
            owner_kind: 'service_account',
            owner_id: 'service_account_1',
            owner_name: 'CI Indexer',
            points: [tokenPoint(4_000, 3_000)],
          },
        ],
        model_token_series: [
          { model_key: 'fast', is_other: false, points: [tokenPoint(4_000, 3_000)] },
        ],
      },
    })

    const { UsageCostsPage } = await import('@/routes/observability/usage-costs')
    render(<UsageCostsPage />)

    expect(screen.getByRole('heading', { level: 1, name: 'Usage costs' })).toBeInTheDocument()
    expect(screen.getAllByText('CI Indexer')).toHaveLength(2)
    expect(screen.queryByText('service account')).not.toBeInTheDocument()
    expect(screen.getByText('fast')).toBeInTheDocument()
    expect(screen.getByText('Pricing coverage')).toBeInTheDocument()
    expect(screen.getByText('234,567')).toBeInTheDocument()
    // Breakdowns are capped at the top 10 spenders.
    expect(screen.getByText('long-tail-8')).toBeInTheDocument()
    expect(screen.queryByText('long-tail-9')).not.toBeInTheDocument()

    // Cache efficiency uses the same ranked-row style as the owner breakdown.
    const cacheList = screen.getByTestId('owner-cache-list')
    expect(cacheList.tagName).toBe('OL')
    expect(within(cacheList).getByText('CI Indexer')).toBeInTheDocument()
    expect(within(cacheList).getByText('3,000 of 4,000 cached')).toBeInTheDocument()
    expect(within(cacheList).getByText('75%')).toBeInTheDocument()
    expect(within(cacheList).getByLabelText('CI Indexer cache hit rate')).toBeInTheDocument()

    // Owner breakdown sits beside cache efficiency (35/65); model mix beside model breakdown (60/40).
    const ownerRow = screen.getByText('Owner breakdown').closest('.grid:not([data-slot])')
    expect(ownerRow).toHaveClass('xl:grid-cols-[minmax(0,7fr)_minmax(0,13fr)]')
    expect(ownerRow).toContainElement(cacheList)
    const modelRow = screen.getByText('Model breakdown').closest('.grid:not([data-slot])')
    expect(modelRow).toHaveClass('xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]')
    expect(within(modelRow as HTMLElement).getByText('Model mix')).toBeInTheDocument()
  })

  it('treats a zero-filled window as no spend', async () => {
    routeMock.useLoaderData.mockReturnValue({ exportOrigin: '', data: emptyReport(7, 'all') })

    const { UsageCostsPage } = await import('@/routes/observability/usage-costs')
    render(<UsageCostsPage />)

    expect(screen.getByText('No priced spend yet')).toBeInTheDocument()
    expect(screen.getByText('No priced spend in window')).toBeInTheDocument()
    expect(screen.getByText('No token usage in this window.')).toBeInTheDocument()
    expect(screen.getByText('No token usage yet')).toBeInTheDocument()
  })
})

describe('UsageCostsPage filters', () => {
  it('requests the selected window and owner kind, committing filters on success', async () => {
    routeMock.useLoaderData.mockReturnValue({ exportOrigin: '', data: emptyReport(7, 'all') })
    getSpendUsageReportMock.mockResolvedValue({ data: emptyReport(30, 'service_account') })

    const { UsageCostsPage } = await import('@/routes/observability/usage-costs')
    render(<UsageCostsPage />)

    fireEvent.click(screen.getByRole('radio', { name: 'Last 30 days' }))
    await waitFor(() => {
      expect(getSpendUsageReportMock).toHaveBeenCalledWith({
        data: { days: 30, owner_kind: 'all' },
      })
    })
    await waitFor(() => {
      expect(screen.getByRole('radio', { name: 'Last 30 days' })).toHaveAttribute(
        'aria-checked',
        'true',
      )
    })
  })

  it('keeps the previous filters when the report request fails', async () => {
    routeMock.useLoaderData.mockReturnValue({ exportOrigin: '', data: emptyReport(7, 'all') })
    getSpendUsageReportMock.mockRejectedValue(new Error('gateway unavailable'))

    const { UsageCostsPage } = await import('@/routes/observability/usage-costs')
    render(<UsageCostsPage />)

    fireEvent.click(screen.getByRole('radio', { name: 'Last 30 days' }))
    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith('gateway unavailable')
    })
    expect(screen.getByRole('radio', { name: 'Last 7 days' })).toHaveAttribute(
      'aria-checked',
      'true',
    )
    expect(screen.getByRole('radio', { name: 'Last 30 days' })).toHaveAttribute(
      'aria-checked',
      'false',
    )
  })

  it('shows a self-service view without cross-owner controls to regular users', async () => {
    routeMock.useRouteContext.mockReturnValue({
      session: {
        must_change_password: false,
        user: {
          id: 'user_2',
          name: 'Regular User',
          email: 'user@example.com',
          global_role: 'user',
        },
      },
    })
    routeMock.useLoaderData.mockReturnValue({
      exportOrigin: '',
      data: {
        window_days: 7,
        owner_kind: 'user',
        window_start: '2026-03-01T00:00:00Z',
        window_end: '2026-03-08T00:00:00Z',
        totals: {
          priced_cost_usd_10000: 10_000,
          priced_request_count: 2,
          unpriced_request_count: 0,
          usage_missing_request_count: 0,
          uncached_input_tokens: 10,
          cache_read_tokens: 20,
          cache_write_tokens: 30,
        },
        daily: [],
        owners: [],
        models: [],
        owner_token_series: [],
        model_token_series: [],
      },
    })

    const { UsageCostsPage } = await import('@/routes/observability/usage-costs')
    render(<UsageCostsPage />)

    expect(
      screen.getByText('Review your costs over time and see how each model affects the total.'),
    ).toBeVisible()
    expect(screen.getByText('Spend attributed to your user account.')).toBeVisible()
    expect(screen.queryByText('All owners')).not.toBeInTheDocument()
    expect(screen.queryByText('Service accounts')).not.toBeInTheDocument()
  })
})

describe('usage costs token series helpers', () => {
  const series = {
    ...emptyReport(7, 'all'),
    owner_token_series: [
      {
        owner_kind: 'user',
        owner_id: 'user_1',
        owner_name: 'Ada',
        // Day two has 400 input tokens but no provider cache split.
        points: [tokenPoint(600, 50), { ...tokenPoint(400, 0), uncached_input_tokens: 0 }],
      },
    ],
    model_token_series: [
      { model_key: 'fast', is_other: false, points: [tokenPoint(1_200, 700)] },
      { model_key: 'Other', is_other: true, points: [tokenPoint(400, 100)] },
    ],
  } as SpendReportView

  it('treats input without a cache split as outside the hit rate', () => {
    expect(
      cacheHitRate({ uncached_input_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0 }),
    ).toBeNull()
    const [ada] = ownerCacheRows(series)
    expect(ada.inputTokens).toBe(1_000)
    expect(ada.hitRate).toBeCloseTo(50 / 600)
  })

  it('stacks model tokens and gives the Other series a neutral colour', () => {
    const chart = modelTokenChart(series)
    expect(chart.rows[0]).toMatchObject({ model_1: 1_500, model_2: 500 })
    expect(chart.keys[0].color).toBe('var(--chart-1)')
    expect(chart.keys[1].color).not.toMatch(/--chart-/)
  })
})
