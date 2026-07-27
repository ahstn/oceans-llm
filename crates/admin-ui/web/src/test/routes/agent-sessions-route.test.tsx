import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AgentSessionsPage } from '@/routes/observability/agent-sessions'
import type { AgentTaskDetailView, AgentTaskSummaryView } from '@/types/api'

const { getAgentTaskDetailMock, navigateMock, routeMock } = vi.hoisted(() => ({
  getAgentTaskDetailMock: vi.fn(),
  navigateMock: vi.fn(),
  routeMock: {
    useLoaderData: vi.fn(),
    useRouteContext: vi.fn(),
    useSearch: vi.fn(),
  },
}))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  Link: ({ children }: { children: ReactNode }) => <a>{children}</a>,
  useRouter: () => ({ navigate: navigateMock }),
}))

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: () => ({
    getVirtualItems: () => [{ index: 0, size: 36, start: 0 }],
    getTotalSize: () => 36,
  }),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getAgentTasks: vi.fn(),
  getObservabilityAgentTaskDetail: (...args: unknown[]) => getAgentTaskDetailMock(...args),
}))

const task: AgentTaskSummaryView = {
  task_id: 'task_1',
  session_id: 'session_1',
  external_session_observed: true,
  ownership_scope_key: 'user:user_1',
  user_id: 'user_1',
  team_id: null,
  service_account_id: null,
  harness_key: 'opencode',
  harness_label: 'Opencode',
  requested_model_key: 'claude-opus-4-1',
  operation: 'chat',
  caller_class: 'user',
  lifecycle: 'finalized',
  boundary_confidence: 'high',
  started_at: '2026-07-21T10:00:00Z',
  ended_at: '2026-07-21T10:01:00Z',
  request_count: 1,
  tool_call_count: 8,
  mcp_call_count: 2,
  gateway_outcome: 'succeeded',
  efficiency_score: 82,
  score_confidence: 'high',
  score_maturity: 'calibrated',
  telemetry_coverage_percent: 93,
  cohort_version: 'successful-boundary-group-v2',
  cohort_fallback_level: 0,
  cohort_sample_size: 16,
  calibration_approval_id: 'calibration_1',
  normalized_cost_usd: 0.0123,
  active_time_ms: 42_000,
  wall_time_ms: 60_000,
  limitations: [],
  report_schema_version: 'agent-task-report-v3',
  analyzer_version: 'task-efficiency-v2',
  score_policy_version: 'outcome-cost-time-v1',
  pricing_policy_version: 'cache-aware-v1',
}

const detail: AgentTaskDetailView = {
  task,
  session: null,
  requests: [
    {
      request_id: 'req_1',
      request_log_id: 'log_1',
      usage_event_id: 'usage_1',
      ordinal: 0,
      execution_id: 'turn_1',
      parent_execution_id: null,
      correlation_confidence: 'high',
      limitation_codes: [],
      occurred_at: '2026-07-21T10:00:00Z',
      completed_at: '2026-07-21T10:00:42Z',
      terminal_success: true,
    },
  ],
  observations: [
    {
      observation_id: 'observation_1',
      kind: 'tool_invoked',
      source_request_id: 'req_1',
      parser_version: 'passive-observations-v1',
      evidence: 'direct',
      occurred_at: '2026-07-21T10:00:10Z',
      facts: { attributes: {}, tool_name: 'read' },
      limitations: [],
    },
  ],
  request_history_truncated: false,
  observation_history_truncated: false,
  analysis: {
    analysis_id: 'analysis_1',
    input_watermark_at: '2026-07-21T10:00:42Z',
    observation_set_id: 'observation_set_1',
    boundary_policy_version: 'passive-gap-v1',
    observation_parser_version: 'passive-observations-v1',
    pricing_policy_version: 'cache-aware-v1',
    cohort_version: 'successful-boundary-group-v2',
    cohort_fallback_level: 0,
    cohort_sample_size: 16,
    cohort_snapshot_digest: 'sha256:cohort-snapshot',
    analyzed_at: '2026-07-21T10:00:43Z',
    expires_at: '2026-10-19T10:00:43Z',
  },
  report: {
    report_schema_version: 'agent-task-report-v3',
    analyzer_version: 'task-efficiency-v2',
    score_policy_version: 'outcome-cost-time-v1',
    observation_parser_version: 'passive-observations-v1',
    calibration_approval_id: 'calibration_1',
    maturity: 'experimental',
    confidence: 'high',
    score: 82,
    gateway_outcome: 'succeeded',
    components: {
      outcome: {
        state: 'succeeded',
        factor_basis_points: 10_000,
        successful_requests: 1,
        determinate_requests: 1,
        incomplete_requests: 0,
      },
      cost_efficiency_basis_points: 8_000,
      active_time_efficiency_basis_points: 8_400,
      actual_cost_10000: 123,
      active_time_ms: 42_000,
      wall_time_ms: 60_000,
      summed_work_time_ms: 42_000,
      excluded_gap_time_ms: 1_000,
      overlap_savings_ms: 18_000,
      unknown_wait_time_ms: 0,
      cohort_version: 'exact-v1',
      cohort_fallback_level: 0,
      cohort_sample_size: 16,
    },
    coverage: {
      outcome_percent: 100,
      cost_percent: 100,
      timing_percent: 100,
      payload_percent: 67,
      cohort_percent: 100,
      overall_percent: 93,
    },
    diagnostics: {
      token_and_cache: {
        fresh_input_tokens: 1_200,
        cache_read_tokens: 800,
        cache_creation_tokens: 200,
        output_tokens: 300,
        reasoning_tokens: 120,
        provider_total_tokens: 2_620,
        normalized_cost_10000: 123,
        legacy_cost_10000: 130,
        uncached_input_cost_10000: 160,
        cache_savings_10000: 37,
        cache_savings_basis_points: 2_313,
        cache_read_write_ratio_basis_points: 40_000,
        pricing_policy_versions: ['cache-aware-v1'],
      },
      tools_and_changes: {
        observed_tool_calls: 8,
        classified_tool_calls: 8,
        supplied_tool_definitions: 12,
        supplied_tool_schema_bytes: 4_096,
        direct_mcp_calls: 2,
        direct_mcp_duration_ms: 250,
        file_reads_suspected: 2,
        file_searches_suspected: 1,
        file_edits_suspected: 1,
        file_creates_suspected: 0,
        file_overwrites_suspected: 0,
        unique_opaque_files: 2,
        rework_spans_suspected: 1,
        verification_results_classified: 2,
      },
      context: {
        initial_prompt_tokens: 1_200,
        median_prompt_tokens: 1_600,
        p90_prompt_tokens: 2_000,
        maximum_prompt_tokens: 2_200,
        prompt_growth_per_turn: 250,
        prompt_growth_per_active_minute: 1_429,
        suspected_compactions: 1,
        suspected_context_resets: 0,
      },
      semantic_verification_available: true,
    },
    limitations: [],
  },
  coverage: {
    request_metadata: true,
    response_payload: true,
    response_payload_truncated: false,
  },
}

function setCapabilities(calibratedScoreVisible: boolean) {
  routeMock.useRouteContext.mockReturnValue({
    session: {
      capabilities: {
        calibrated_score_visible: calibratedScoreVisible,
      },
    },
  })
}

describe('AgentSessionsPage', () => {
  afterEach(cleanup)

  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useRouteContext.mockReset()
    routeMock.useSearch.mockReset()

    getAgentTaskDetailMock.mockReset()
    navigateMock.mockReset()
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [task], page: 1, page_size: 50, total: 1 },
    })
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50 })
    setCapabilities(false)
  })
  it('retries a failed detail request without closing the task sheet', async () => {
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, task_id: 'task_1' })
    getAgentTaskDetailMock
      .mockRejectedValueOnce(new Error('temporary failure'))
      .mockResolvedValueOnce({ data: detail })

    render(<AgentSessionsPage />)

    expect(await screen.findByText('temporary failure')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))

    await waitFor(() => {
      expect(getAgentTaskDetailMock).toHaveBeenCalledTimes(2)
      expect(screen.getByText('Requests (1)')).toBeInTheDocument()
    })
  })

  it('renders dense shadow diagnostics without exposing the experimental score', () => {
    render(<AgentSessionsPage />)

    expect(screen.getByRole('heading', { name: 'Agent sessions' })).toBeInTheDocument()
    expect(screen.getByText('1 session')).toBeInTheDocument()
    expect(screen.getByText('Opencode')).toBeInTheDocument()
    expect(screen.getByText('Shadow')).toBeInTheDocument()
    expect(screen.queryByText('82')).not.toBeInTheDocument()
    const sessionRow = screen.getByRole('row', { name: /Opencode/ })
    expect(sessionRow).toHaveTextContent('8')
    expect(sessionRow).toHaveTextContent('2')
  })

  it('opens session diagnostics from the keyboard and labels the current page', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [task], page: 1, page_size: 50, total: 51 },
    })
    render(<AgentSessionsPage />)

    const taskRow = screen.getByRole('row', { name: /Opencode/ })
    expect(taskRow).toHaveAttribute('tabindex', '0')
    fireEvent.keyDown(taskRow, { key: 'Enter' })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/agent-sessions',
        search: expect.objectContaining({ task_id: 'task_1' }),
      })
    })
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Go to page 1' })).toHaveAttribute(
        'aria-current',
        'page',
      )
    })
  })

  it('deep-links to task diagnostics with request outcomes and retained observations', async () => {
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, task_id: 'task_1' })
    getAgentTaskDetailMock.mockResolvedValue({ data: detail })

    render(<AgentSessionsPage />)

    await waitFor(() => {
      expect(getAgentTaskDetailMock).toHaveBeenCalledWith({ data: { taskId: 'task_1' } })
      expect(screen.getByText('Withheld in shadow')).toBeInTheDocument()
    })
    expect(screen.getByText('Requests (1)')).toBeInTheDocument()
    expect(screen.getByText('Succeeded')).toBeInTheDocument()
    expect(screen.getByText('Inferred observations (1)')).toBeInTheDocument()
    expect(screen.getByText('Token and cache diagnostics')).toBeInTheDocument()
    expect(screen.getByText('Tools and changes')).toBeInTheDocument()
    expect(screen.getByText('Context diagnostics')).toBeInTheDocument()
    expect(screen.getAllByText('93%').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('1,200')).toHaveLength(2)
    expect(screen.getByText('Tool Invoked')).toBeInTheDocument()
  })

  it('warns when retained request or observation history exceeds the detail cap', async () => {
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, task_id: 'task_1' })
    getAgentTaskDetailMock.mockResolvedValue({
      data: {
        ...detail,
        request_history_truncated: true,
        observation_history_truncated: true,
      },
    })

    render(<AgentSessionsPage />)

    expect(await screen.findByText('Request history truncated')).toBeInTheDocument()
    expect(screen.getByText('Observation history truncated')).toBeInTheDocument()
    expect(screen.getByText('Requests (1+)')).toBeInTheDocument()
    expect(screen.getByText('Inferred observations (1+)')).toBeInTheDocument()
  })
})
