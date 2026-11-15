import { cleanup, fireEvent, render, waitFor, within } from '@testing-library/react'
import { cloneElement, type ReactElement, type ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { TooltipProvider } from '@/components/ui/tooltip'
import { AgentHarnessesPage } from '@/routes/observability/agent-harnesses'

const refreshObservabilityHarnessUsageMock = vi.fn()

// Hoisted with the mocks: the route module reads it as it loads.
const routeMock = vi.hoisted(() => ({
  useLoaderData: vi.fn(),
}))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
}))

vi.mock('recharts', () => ({
  ResponsiveContainer: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  AreaChart: ({ children }: { children: ReactNode }) => <svg>{children}</svg>,
  Area: () => <path />,
  CartesianGrid: () => <g />,
  XAxis: ({
    ticks,
    tickFormatter,
  }: {
    ticks?: string[]
    tickFormatter: (value: string) => string
  }) => (
    <g data-testid="x-axis">
      {ticks?.map((tick) => (
        <text key={tick}>{tickFormatter(tick)}</text>
      ))}
    </g>
  ),
  // Render the tooltip as if the noon bucket on 1 Mar were hovered.
  Tooltip: ({ content }: { content: ReactElement }) =>
    cloneElement(content as ReactElement<Record<string, unknown>>, {
      active: true,
      payload: [
        {
          dataKey: 'harness_1',
          name: 'harness_1',
          value: 4,
          payload: { bucket_start: '2026-03-01T12:00:00Z' },
        },
      ],
    }),
  Legend: () => null,
}))

vi.mock('@/server/admin-data.functions', () => ({
  getAttribution: vi.fn(),
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

  it('renders the chart card, range selector, and ranked harness table', () => {
    routeMock.useLoaderData.mockReturnValue({ data: harnessUsageData })

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
    expect(ohMyPiRow?.querySelector('[data-agent-harness-icon="ohmypi"]')).toBeInTheDocument()
  })

  it('refetches harness data when the date range changes', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: harnessUsageData })
    refreshObservabilityHarnessUsageMock.mockResolvedValue({
      data: {
        ...harnessUsageData,
        range: '31d',
      },
    })

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

  it('renders an explicit empty state when no harness data exists', () => {
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...harnessUsageData,
        chart_harnesses: [],
        series: [],
        leaders: [],
      },
    })

    const view = render(<AgentHarnessesPage />)

    expect(within(view.container).getAllByText('No harness data yet')).toHaveLength(2)
  })

  it('shows loading skeletons while a range refresh is in flight', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: harnessUsageData })
    refreshObservabilityHarnessUsageMock.mockImplementation(() => new Promise(() => undefined))

    const view = render(<AgentHarnessesPage />)
    const scope = within(view.container)
    fireEvent.click(scope.getByRole('radio', { name: 'Last 31 days' }))

    await waitFor(() => {
      expect(scope.getByTestId('harness-chart-skeleton')).toBeInTheDocument()
      expect(scope.getByTestId('harness-table-skeleton')).toBeInTheDocument()
    })
  })

  it('labels each UTC day once on the axis and names the zone in the tooltip', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...harnessUsageData,
        series: [
          '2026-03-01T12:00:00Z',
          '2026-03-02T00:00:00Z',
          '2026-03-02T12:00:00Z',
          '2026-03-03T00:00:00Z',
        ].map((bucket_start) => ({
          bucket_start,
          values: [{ agent_harness_key: 'mastra', request_count: 4 }],
        })),
      },
    })

    const { AgentHarnessesPage } = await import('@/routes/observability/agent-harnesses')

    const view = render(<AgentHarnessesPage />)
    const scope = within(view.container)

    // 12-hour buckets: only the midnight buckets get a tick, so no day repeats.
    expect(
      within(scope.getByTestId('x-axis'))
        .getAllByText(/./)
        .map((tick) => tick.textContent),
    ).toEqual(['Mar 2', 'Mar 3'])
    expect(scope.getByText('1 Mar 2026, 12:00 UTC')).toBeInTheDocument()
    expect(scope.getByText('4 requests')).toBeInTheDocument()
  })

  it('shows what each harness ships above the request activity', () => {
    const harnessModels = [
      // The Claude Code hook key and git-ai's tool name are one harness.
      {
        harness: 'claude',
        model_key: 'claude-sonnet-5',
        session_count: 3,
        shipped_session_count: 2,
        ai_lines: 400,
        request_count: 30,
        priced_request_count: 30,
        unpriced_request_count: 0,
        usage_missing_request_count: 0,
        priced_cost_usd_10000: 32000,
      },
      {
        harness: 'claude_code',
        model_key: 'claude-sonnet-5',
        session_count: 1,
        shipped_session_count: 0,
        ai_lines: 0,
        request_count: 4,
        priced_request_count: 4,
        unpriced_request_count: 0,
        usage_missing_request_count: 0,
        priced_cost_usd_10000: 4000,
      },
      {
        harness: 'codex',
        model_key: 'gpt-6-astra',
        session_count: 2,
        shipped_session_count: 2,
        ai_lines: 100,
        request_count: 10,
        priced_request_count: 10,
        unpriced_request_count: 0,
        usage_missing_request_count: 0,
        priced_cost_usd_10000: 2000,
      },
    ]
    routeMock.useLoaderData.mockReturnValue({
      data: harnessUsageData,
      attribution: { window_days: 7, harness_models: harnessModels },
    })

    const view = render(
      <TooltipProvider>
        <AgentHarnessesPage />
      </TooltipProvider>,
    )
    const section = within(view.getByRole('region', { name: 'Harness attribution' }))

    // Claude Code: $3.60 over 400 AI lines is $0.90 per 100; 2 of 4 sessions shipped.
    expect(section.getByText('4 sessions')).toBeInTheDocument()
    expect(section.getAllByText('$0.90').length).toBeGreaterThan(0)
    expect(section.getAllByText('Needs new data').length).toBe(2)
    const matrix = within(section.getByRole('table', { name: 'Harness by model' }))
    expect(matrix.getByText('gpt-6-astra')).toBeInTheDocument()
    expect(matrix.getByText('$0.20')).toBeInTheDocument()
    // The section's own window toggle never collides with the chart's.
    expect(
      section.getByRole('radio', { name: 'Attribution for the last 7 days' }),
    ).toBeInTheDocument()
    expect(view.getByRole('radio', { name: 'Last 7 days' })).toBeInTheDocument()
  })
})
