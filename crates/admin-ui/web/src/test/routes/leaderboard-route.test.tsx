import { cleanup, fireEvent, render, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const refreshObservabilityLeaderboardMock = vi.fn()

const routeMock = {
  useLoaderData: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
}))

vi.mock('recharts', () => ({
  ResponsiveContainer: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  AreaChart: ({ children }: { children: ReactNode }) => <svg>{children}</svg>,
  Area: () => <path />,
  CartesianGrid: () => <g />,
  XAxis: () => <g />,
  Tooltip: () => null,
  Legend: () => null,
}))

vi.mock('@/server/admin-data.functions', () => ({
  getObservabilityLeaderboard: vi.fn(),
  refreshObservabilityLeaderboard: (...args: unknown[]) =>
    refreshObservabilityLeaderboardMock(...args),
}))

vi.mock('sonner', () => ({
  toast: {
    error: vi.fn(),
  },
}))

const leaderboardData = {
  range: '7d' as const,
  window_start: '2026-03-01T00:00:00Z',
  window_end: '2026-03-08T00:00:00Z',
  bucket_hours: 12,
  chart_users: [
    {
      user_id: 'user_1',
      user_name: 'Ada',
      total_spend_usd_10000: 71000,
    },
    {
      user_id: 'user_2',
      user_name: 'Ben',
      total_spend_usd_10000: 62000,
    },
  ],
  series: [
    {
      bucket_start: '2026-03-01T00:00:00Z',
      values: [
        { user_id: 'user_1', spend_usd_10000: 35000 },
        { user_id: 'user_2', spend_usd_10000: 25000 },
      ],
    },
  ],
  leaders: [
    {
      user_id: 'user_1',
      user_name: 'Ada',
      total_spend_usd_10000: 71000,
      most_used_model: 'fast',
      total_requests: 11,
      tool_cardinality_averages: {
        referenced_mcp_server_count: null,
        exposed_tool_count: 2,
        invoked_tool_count: 0,
        filtered_tool_count: null,
      },
    },
    {
      user_id: 'user_2',
      user_name: 'Ben',
      total_spend_usd_10000: 62000,
      most_used_model: 'reasoning',
      total_requests: 8,
      tool_cardinality_averages: {
        referenced_mcp_server_count: null,
        exposed_tool_count: 3.5,
        invoked_tool_count: 1.25,
        filtered_tool_count: null,
      },
    },
  ],
}

describe('ObservabilityLeaderboardPage', () => {
  afterEach(() => {
    cleanup()
  })

  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    refreshObservabilityLeaderboardMock.mockReset()
  })

  it('renders the chart card, range selector, and ranked leaderboard table', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: leaderboardData })

    const { ObservabilityLeaderboardPage } = await import('@/routes/observability/leaderboard')

    const view = render(<ObservabilityLeaderboardPage />)
    const scope = within(view.container)

    expect(scope.getByText('Leaderboard')).toBeInTheDocument()
    expect(scope.getByRole('radio', { name: 'Last 7 days' })).toBeInTheDocument()
    expect(scope.getByRole('radio', { name: 'Last 31 days' })).toBeInTheDocument()
    expect(scope.getByTestId('leaderboard-table')).toBeInTheDocument()
    expect(scope.getByText('Ada')).toBeInTheDocument()
    expect(scope.getByText('Ben')).toBeInTheDocument()
    expect(scope.getByText('fast')).toBeInTheDocument()
    expect(scope.getByText('Avg Tools')).toBeInTheDocument()
    expect(scope.getByText('exposed 2')).toBeInTheDocument()
    expect(scope.getByText('called 0')).toBeInTheDocument()
    expect(scope.getByText('exposed 3.5')).toBeInTheDocument()
  })

  it('refetches leaderboard data when the date range changes', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: leaderboardData })
    refreshObservabilityLeaderboardMock.mockResolvedValue({
      data: {
        ...leaderboardData,
        range: '31d',
      },
    })

    const { ObservabilityLeaderboardPage } = await import('@/routes/observability/leaderboard')

    const view = render(<ObservabilityLeaderboardPage />)
    fireEvent.click(within(view.container).getByRole('radio', { name: 'Last 31 days' }))

    await waitFor(() => {
      expect(refreshObservabilityLeaderboardMock).toHaveBeenCalledWith({
        data: {
          range: '31d',
        },
      })
    })
  })

  it('renders an explicit empty state when no leaderboard data exists', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...leaderboardData,
        chart_users: [],
        series: [],
        leaders: [],
      },
    })

    const { ObservabilityLeaderboardPage } = await import('@/routes/observability/leaderboard')

    const view = render(<ObservabilityLeaderboardPage />)

    expect(within(view.container).getAllByText('No leaderboard data yet')).toHaveLength(2)
  })

  it('shows loading skeletons while a range refresh is in flight', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: leaderboardData })
    refreshObservabilityLeaderboardMock.mockImplementation(() => new Promise(() => undefined))

    const { ObservabilityLeaderboardPage } = await import('@/routes/observability/leaderboard')

    const view = render(<ObservabilityLeaderboardPage />)
    const scope = within(view.container)
    fireEvent.click(scope.getByRole('radio', { name: 'Last 31 days' }))

    await waitFor(() => {
      expect(scope.getByTestId('leaderboard-chart-skeleton')).toBeInTheDocument()
      expect(scope.getByTestId('leaderboard-table-skeleton')).toBeInTheDocument()
    })
  })
})
