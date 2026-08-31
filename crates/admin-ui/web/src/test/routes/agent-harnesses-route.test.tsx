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
      rank: 1,
      agent_harness_key: 'mastra',
      agent_harness_label: 'Mastra',
      total_requests: 17,
    },
    {
      agent_harness_key: 'oh_my_pi',
      rank: 2,
      agent_harness_label: 'Oh My Pi',
      total_requests: 9,
    },
  ],
  series: [
    {
      bucket_start: '2026-03-01T00:00:00Z',
      values: [
        { agent_harness_key: 'mastra', request_count: 12 },
        { agent_harness_key: 'oh_my_pi', request_count: 7 },
      ],
    },
  ],
  leaders: [
    {
      rank: 1,
      agent_harness_key: 'mastra',
      agent_harness_label: 'Mastra',
      total_requests: 17,
      prompt_tokens: 1_200,
      completion_tokens: 500,
      total_tokens: 1_700,
    },
    {
      rank: 2,
      agent_harness_key: 'oh_my_pi',
      agent_harness_label: 'Oh My Pi',
      total_requests: 9,
      prompt_tokens: null,
      completion_tokens: null,
      total_tokens: null,
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

    expect(scope.getByRole('heading', { level: 1, name: 'Agent harnesses' })).toBeInTheDocument()
    expect(scope.getByRole('radio', { name: 'Last 7 days' })).toBeInTheDocument()
    expect(scope.getByRole('radio', { name: 'Last 31 days' })).toBeInTheDocument()
    const table = scope.getByTestId('harness-usage-table')
    expect(table).toBeInTheDocument()
    expect(scope.getByTestId('harness-usage-mobile-list')).toBeInTheDocument()
    expect(scope.getAllByText('Mastra').length).toBeGreaterThan(1)
    expect(scope.getAllByText('Oh My Pi').length).toBeGreaterThan(1)
    expect(scope.getAllByText('mastra')).toHaveLength(2)
    expect(within(table).getByText('Input tokens')).toBeInTheDocument()
    expect(within(table).getByText('Output tokens')).toBeInTheDocument()
    expect(within(table).getByText('Total tokens')).toBeInTheDocument()
    expect(within(table).getByText('1,200')).toBeInTheDocument()
    expect(within(table).getByText('500')).toBeInTheDocument()
    expect(within(table).getByText('1,700')).toBeInTheDocument()
    expect(within(table).getAllByText('n/a')).toHaveLength(3)
    const mastraRow = within(table).getByText('Mastra').closest('tr')
    const ohMyPiRow = within(table).getByText('Oh My Pi').closest('tr')
    expect(mastraRow?.querySelector('[data-agent-harness-icon="mastra"]')).toBeInTheDocument()
    expect(ohMyPiRow?.querySelector('[data-agent-harness-icon]')).not.toBeInTheDocument()
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
    const scope = within(view.container)
    fireEvent.click(scope.getByRole('radio', { name: 'Last 31 days' }))

    await waitFor(() => {
      expect(refreshObservabilityHarnessUsageMock).toHaveBeenCalledWith({
        data: {
          range: '31d',
        },
      })
      expect(scope.getByRole('radio', { name: 'Last 31 days' })).toHaveAttribute('data-state', 'on')
    })
  })

  it('keeps the previous range selected when a range refresh fails', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: harnessUsageData })
    refreshObservabilityHarnessUsageMock.mockRejectedValue(new Error('refresh failed'))

    const { AgentHarnessesPage } = await import('@/routes/observability/agent-harnesses')

    const view = render(<AgentHarnessesPage />)
    const scope = within(view.container)
    fireEvent.click(scope.getByRole('radio', { name: 'Last 31 days' }))

    await waitFor(() => {
      expect(refreshObservabilityHarnessUsageMock).toHaveBeenCalledWith({
        data: {
          range: '31d',
        },
      })
      expect(scope.getByRole('radio', { name: 'Last 7 days' })).toHaveAttribute('data-state', 'on')
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
