import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const routeMock = {
  useLoaderData: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

describe('UsageCostsPage', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
  })

  it('renders live ledger totals and owner/model breakdowns', async () => {
    routeMock.useLoaderData.mockReturnValue({
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
            owner_kind: 'team',
            owner_id: 'team_1',
            owner_name: 'Core Platform',
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
        ],
      },
    })

    const { UsageCostsPage } = await import('@/routes/observability/usage-costs')
    render(<UsageCostsPage />)

    expect(screen.getByText('Usage Costs')).toBeInTheDocument()
    expect(screen.getByText('Core Platform')).toBeInTheDocument()
    expect(screen.getByText('fast')).toBeInTheDocument()
    expect(screen.getByText('Priced requests')).toBeInTheDocument()
  })
})
