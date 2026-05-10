import { cleanup, fireEvent, render, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const refreshObservabilityHarnessUsageMock = vi.fn()

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
  getObservabilityHarnessUsage: vi.fn(),
  refreshObservabilityHarnessUsage: (...args: unknown[]) =>
    refreshObservabilityHarnessUsageMock(...args),
}))

vi.mock('sonner', () => ({
  toast: {
    error: vi.fn(),
  },
}))

const harnessUsageData = {
  range: '7d',
  window_start: '2026-03-01T00:00:00Z',
  window_end: '2026-03-08T00:00:00Z',
  bucket_hours: 12,
  chart_harnesses: [
    {
      agent_harness_key: 'opencode',
      agent_harness_label: 'Opencode',
      total_requests: 17,
    },
    {
      agent_harness_key: 'claude_code',
      agent_harness_label: 'Claude Code',
      total_requests: 9,
    },
  ],
  series: [
    {
      bucket_start: '2026-03-01T00:00:00Z',
      values: [
        { agent_harness_key: 'opencode', request_count: 12 },
        { agent_harness_key: 'claude_code', request_count: 7 },
      ],
    },
  ],
  leaders: [
    {
      agent_harness_key: 'opencode',
      agent_harness_label: 'Opencode',
      total_requests: 17,
    },
    {
      agent_harness_key: 'claude_code',
      agent_harness_label: 'Claude Code',
      total_requests: 9,
    },
  ],
}

describe('AgentHarnessesPage', () => {
  afterEach(() => {
    cleanup()
  })

  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    refreshObservabilityHarnessUsageMock.mockReset()
  })

  it('renders the chart card, range selector, and ranked harness table', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: harnessUsageData })

    const { AgentHarnessesPage } = await import('@/routes/observability/agent-harnesses')

    const view = render(<AgentHarnessesPage />)
    const scope = within(view.container)

    expect(scope.getByText('Agent Harnesses')).toBeInTheDocument()
    expect(scope.getByRole('radio', { name: 'Last 7 days' })).toBeInTheDocument()
    expect(scope.getByRole('radio', { name: 'Last 31 days' })).toBeInTheDocument()
    expect(scope.getByTestId('harness-usage-table')).toBeInTheDocument()
    expect(scope.getAllByText('Opencode').length).toBeGreaterThan(0)
    expect(scope.getAllByText('Claude Code').length).toBeGreaterThan(0)
    expect(scope.getByText('opencode')).toBeInTheDocument()
  })

  it('refetches harness data when the date range changes', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: harnessUsageData })
    refreshObservabilityHarnessUsageMock.mockResolvedValue({
      data: {
        ...harnessUsageData,
        range: '31d',
      },
    })

    const { AgentHarnessesPage } = await import('@/routes/observability/agent-harnesses')

    const view = render(<AgentHarnessesPage />)
    fireEvent.click(within(view.container).getByRole('radio', { name: 'Last 31 days' }))

    await waitFor(() => {
      expect(refreshObservabilityHarnessUsageMock).toHaveBeenCalledWith({
        data: {
          range: '31d',
        },
      })
    })
  })

  it('renders an explicit empty state when no harness data exists', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...harnessUsageData,
        chart_harnesses: [],
        series: [],
        leaders: [],
      },
    })

    const { AgentHarnessesPage } = await import('@/routes/observability/agent-harnesses')

    const view = render(<AgentHarnessesPage />)

    expect(within(view.container).getAllByText('No harness data yet')).toHaveLength(2)
  })

  it('shows loading skeletons while a range refresh is in flight', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: harnessUsageData })
    refreshObservabilityHarnessUsageMock.mockImplementation(() => new Promise(() => undefined))

    const { AgentHarnessesPage } = await import('@/routes/observability/agent-harnesses')

    const view = render(<AgentHarnessesPage />)
    const scope = within(view.container)
    fireEvent.click(scope.getByRole('radio', { name: 'Last 31 days' }))

    await waitFor(() => {
      expect(scope.getByTestId('harness-chart-skeleton')).toBeInTheDocument()
      expect(scope.getByTestId('harness-table-skeleton')).toBeInTheDocument()
    })
  })
})
