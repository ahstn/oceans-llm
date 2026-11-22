import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ComponentProps, ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import { AgentSessionsPage } from '@/routes/observability/agent-sessions'
import type { AgentSessionDetailView, AgentSessionSummaryView } from '@/types/api'

const {
  getAgentSessionDetailMock,
  getHarnessUsageMock,
  getModelOptionsMock,
  navigateMock,
  routeMock,
} = vi.hoisted(() => ({
  getAgentSessionDetailMock: vi.fn(),
  getHarnessUsageMock: vi.fn(),
  getModelOptionsMock: vi.fn(),
  navigateMock: vi.fn(),
  routeMock: {
    useLoaderData: vi.fn(),
    useRouteContext: vi.fn(),
    useSearch: vi.fn(),
  },
}))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  Link: ({
    children,
    to,
    search,
    ...props
  }: {
    children: ReactNode
    to: string
    search?: { request_id?: string }
  } & Omit<ComponentProps<'a'>, 'href'>) => (
    <a href={`${to}${search?.request_id ? `?request_id=${search.request_id}` : ''}`} {...props}>
      {children}
    </a>
  ),
  useRouter: () => ({ navigate: navigateMock }),
}))

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: () => ({
    getVirtualItems: () => [{ index: 0, size: 36, start: 0 }],
    getTotalSize: () => 36,
  }),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getAgentSessions: vi.fn(),
  getObservabilityAgentSessionDetail: (...args: unknown[]) => getAgentSessionDetailMock(...args),
  getObservabilityHarnessUsage: (...args: unknown[]) => getHarnessUsageMock(...args),
  getModelOptions: (...args: unknown[]) => getModelOptionsMock(...args),
}))

const session: AgentSessionSummaryView = {
  session_id: 'session_1',
  session_source_id: 'session_source_1',
  session_source_hash: 'sha256:safe-session-source',
  session_source_observed: true,
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
  report_schema_version: 'agent-session-report-v3',
  analyzer_version: 'session-efficiency-v2',
  score_policy_version: 'outcome-cost-time-v1',
  pricing_policy_version: 'cache-aware-v1',
}

const detail: AgentSessionDetailView = {
  session,
  session_source: null,
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
      parser_version: 'passive-observations-v2',
      evidence: 'direct',
      occurred_at: '2026-07-21T10:00:10Z',
      facts: {
        attributes: {},
        tool_name: 'read',
        tool_schema_token_estimate: 120,
        supplied_tools: [
          { name: 'read', server_key: null, token_estimate: 120 },
          { name: 'search', server_key: null, token_estimate: 110 },
          { name: 'edit', server_key: null, token_estimate: 90 },
          { name: 'create', server_key: null, token_estimate: 80 },
          { name: 'browser', server_key: null, token_estimate: 70 },
          { name: 'task', server_key: null, token_estimate: 60 },
          { name: 'bash', server_key: null, token_estimate: 50 },
          { name: 'write', server_key: null, token_estimate: 40 },
        ],
        supplied_skills: [
          {
            name: 'repository-review',
            description_token_estimate: 50,
            body_token_estimate: 500,
            resource_token_estimate: 100,
            used: true,
            abandoned: false,
          },
        ],
        file_interactions: [
          {
            opaque_file_id: 'file-1',
            operation: 'edit',
            tool_name: 'edit',
            succeeded: true,
            error_signature: null,
          },
        ],
        reasoning_config_hash: 'sha256:reasoning',
        cache_requested: true,
        finish_reason: 'stop',
        incomplete_reason: null,
      },
      limitations: ['semantic_verification_unavailable'],
    },
  ],
  request_history_truncated: false,
  observation_history_truncated: false,
  analysis: {
    analysis_id: 'analysis_1',
    input_watermark_at: '2026-07-21T10:00:42Z',
    observation_set_id: 'observation_set_1',
    boundary_policy_version: 'passive-gap-v1',
    observation_parser_version: 'passive-observations-v2',
    pricing_policy_version: 'cache-aware-v1',
    cohort_version: 'successful-boundary-group-v2',
    cohort_fallback_level: 0,
    cohort_sample_size: 16,
    cohort_snapshot_digest: 'sha256:cohort-snapshot',
    analyzed_at: '2026-07-21T10:00:43Z',
    expires_at: '2026-10-19T10:00:43Z',
  },
  report: {
    report_schema_version: 'agent-session-report-v3',
    analyzer_version: 'session-efficiency-v2',
    score_policy_version: 'outcome-cost-time-v1',
    observation_parser_version: 'passive-observations-v2',
    calibration_approval_id: 'calibration_1',
    configuration_version: 'sha256:analysis-config',
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
      response_payload_count: 1,
      truncated_response_count: 0,
      cohort_percent: 100,
      overall_percent: 93,
    },
    diagnostics: {
      token_and_cache: {
        fresh_input_tokens: 1_200,
        cache_read_tokens: 800,
        cache_creation_tokens: 200,
        total_input_tokens: 2_200,
        output_tokens: 300,
        reasoning_tokens: 120,
        visible_output_tokens: 180,
        cache_creation_5m_tokens: 80,
        cache_creation_30m_tokens: 0,
        cache_creation_1h_tokens: 120,
        provider_total_tokens: 2_620,
        normalized_cost_10000: 123,
        legacy_cost_10000: 130,
        cache_read_cost_10000: 8,
        cache_creation_cost_10000: 20,
        uncached_input_cost_10000: 160,
        cache_savings_10000: 37,
        cache_savings_basis_points: 2_313,
        cache_read_write_ratio_basis_points: 40_000,
        cache_write_amplification_basis_points: 2_500,
        silent_cache_threshold_miss_requests: 0,
        provider_model_switches: 1,
        cache_key_switches: 1,
        reasoning_config_switches: 0,
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
        tool_servers: [
          {
            server_key: 'github',
            exposed_tool_definitions: 6,
            invoked_tool_definitions: 1,
            invocation_count: 2,
            failed_count: 1,
            schema_token_estimate_per_request: 600,
            estimated_uncached_schema_cost_10000: 12,
          },
        ],
      },
      context: {
        initial_prompt_tokens: 1_200,
        median_prompt_tokens: 1_600,
        p90_prompt_tokens: 2_000,
        maximum_prompt_tokens: 2_200,
        input_boundary_tokens: 220_000,
        reserved_output_tokens: 128_000,
        peak_input_utilization_basis_points: 100,
        requests_over_input_boundary: 0,
        repeated_requests_over_input_boundary: 0,
        score_penalty_points: 0,
        prompt_growth_per_turn: 250,
        prompt_growth_per_active_minute: 1_429,
        suspected_compactions: 1,
        suspected_context_resets: 0,
      },
      skills: {
        instrumented_request_count: 1,
        available_skill_count: 1,
        used_skill_count: 1,
        unused_skill_count: 0,
        description_tokens_per_request: 50,
        loaded_body_tokens: 500,
        loaded_resource_tokens: 100,
        items: [
          {
            name: 'repository-review',
            available_request_count: 1,
            used_request_count: 1,
            abandoned_request_count: 0,
            description_token_estimate: 50,
            loaded_body_tokens: 500,
            loaded_resource_tokens: 100,
          },
        ],
      },
      reliability: {
        attempt_coverage_percent: 100,
        total_attempts: 2,
        wasted_attempts: 1,
        wasted_attempt_latency_ms: 500,
        wasted_attempt_cost_10000: null,
        tool_invocations: 2,
        failed_tool_invocations: 1,
        truncated_tool_results: 0,
        attempts: [
          {
            request_id: 'req_1',
            attempt_number: 1,
            produced_final_response: false,
            retryable: true,
            status: 'provider_error',
            status_code: 500,
            error_code: 'upstream',
            latency_ms: 500,
            provider_key: 'anthropic',
            upstream_model: 'claude-opus-4-1',
            occurred_at_unix_ms: 0,
          },
          {
            request_id: 'req_1',
            attempt_number: 2,
            produced_final_response: true,
            retryable: false,
            status: 'succeeded',
            status_code: 200,
            error_code: null,
            latency_ms: 41_500,
            provider_key: 'openai',
            upstream_model: 'gpt-5',
            occurred_at_unix_ms: 500,
          },
        ],
        tools: [
          {
            server_key: 'github',
            tool_key: 'search_code',
            invocation_count: 2,
            failed_count: 1,
            truncated_result_count: 0,
            latency_ms: 250,
            post_error_input_tokens: 1_800,
          },
        ],
      },
      outcome: {
        file_signal_coverage_percent: 100,
        cost_per_file_touched_10000: 123,
        cost_per_successful_session_10000: 123,
        rework_ratio_basis_points: 2_500,
        verification_rate_basis_points: 5_000,
        zero_outcome: false,
        repeated_file_interactions_suspected: 1,
        files_with_repeated_interactions_suspected: 1,
        failed_file_interactions: 0,
      },
      finish_reasons: {
        instrumented_request_count: 1,
        length_limited_requests: 0,
        items: [{ reason: 'stop', count: 1 }],
      },
      enabled_metrics: {
        token_metrics: true,
        cache_metrics: true,
        context_metrics: true,
        tool_metrics: true,
        skill_metrics: true,
        reliability_metrics: true,
        outcome_metrics: true,
        finish_reason_metrics: true,
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

/** Filter navigations merge a patch into the latest search; replay one against `previous`. */
function navigatedSearch(callIndex: number, previous: Record<string, unknown> = {}) {
  const search = navigateMock.mock.calls[callIndex][0].search as (
    previous: Record<string, unknown>,
  ) => Record<string, unknown>
  return search(previous)
}

/**
 * Keeps filter navigations pending until the test ends. React 19 entangles async transitions, so
 * a navigation that never settles would leave later tests' transitions pending too.
 */
const heldNavigations: Array<() => void> = []
function holdNavigations() {
  navigateMock.mockImplementation(
    () => new Promise<void>((resolve) => heldNavigations.push(resolve)),
  )
}

afterEach(() => {
  for (const release of heldNavigations.splice(0)) release()
})

function openFilterMenu() {
  fireEvent.keyDown(
    within(screen.getByTestId('agent-session-toolbar')).getByRole('button', { name: /^Filters/ }),
    { key: 'Enter' },
  )
}

async function addFilter(label: string) {
  openFilterMenu()
  fireEvent.click(await screen.findByRole('menuitem', { name: label }))
}

function setCapabilities(calibratedScoreVisible: boolean, platformAdmin = false) {
  routeMock.useRouteContext.mockReturnValue({
    session: {
      capabilities: {
        calibrated_score_visible: calibratedScoreVisible,
        platform_admin: platformAdmin,
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

    getAgentSessionDetailMock.mockReset()
    navigateMock.mockReset()
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [session], page: 1, page_size: 50, total: 1 },
    })
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50 })
    setCapabilities(false)
  })
  it('retries a failed detail request without closing the session sheet', async () => {
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, session_id: 'session_1' })
    getAgentSessionDetailMock
      .mockRejectedValueOnce(new Error('temporary failure'))
      .mockResolvedValueOnce({ data: detail })

    render(<AgentSessionsPage />)

    expect(await screen.findByText('temporary failure')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))

    await waitFor(() => {
      expect(getAgentSessionDetailMock).toHaveBeenCalledTimes(2)
      expect(screen.getByRole('button', { name: /Event stream/ })).toHaveAttribute(
        'aria-expanded',
        'true',
      )
    })
  })

  it('shows calibration data without showing the session score', () => {
    render(<AgentSessionsPage />)

    expect(screen.getByRole('heading', { name: 'Agent sessions' })).toBeInTheDocument()
    expect(screen.getByText('1 session')).toBeInTheDocument()
    const table = within(screen.getByTestId('agent-session-table'))
    expect(table.getByText('Opencode')).toBeInTheDocument()
    expect(
      screen
        .getByTestId('agent-session-table')
        .querySelector('[data-agent-harness-icon="opencode"]'),
    ).toBeInTheDocument()
    expect(table.getByText('Score not shown')).toBeInTheDocument()
    expect(screen.queryByText('82')).not.toBeInTheDocument()
    const sessionRow = screen.getByRole('row', { name: /Opencode/ })
    expect(sessionRow).toHaveTextContent('8')
    expect(sessionRow).toHaveTextContent('2')
  })

  it('keeps Enter on a compact count tooltip from opening the session', () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [{ ...session, request_count: 150_000 }], page: 1, page_size: 50, total: 1 },
    })
    render(
      <TooltipProvider>
        <AgentSessionsPage />
      </TooltipProvider>,
    )

    const sessionRow = screen.getByRole('row', { name: /Opencode/ })
    const count = within(sessionRow).getByTestId('compact-count')
    expect(count).toHaveTextContent('150K')
    fireEvent.keyDown(count, { key: 'Enter' })
    fireEvent.keyDown(count, { key: ' ' })
    expect(navigateMock).not.toHaveBeenCalled()

    fireEvent.keyDown(sessionRow, { key: 'Enter' })
    expect(navigateMock).toHaveBeenCalledTimes(1)
  })

  it('right-aligns numeric columns in both the header and the cells', () => {
    render(<AgentSessionsPage />)

    const headers = screen.getAllByRole('columnheader')
    const cells = within(screen.getByRole('row', { name: /Opencode/ })).getAllByRole('cell')
    const rightAligned = (element: HTMLElement) => element.classList.contains('text-right')

    expect(headers.filter(rightAligned).map((header) => header.textContent)).toEqual([
      'Score',
      'Cost',
      'Active time',
      'Requests',
      'Tool calls',
      'MCP calls',
    ])
    expect(cells.map(rightAligned)).toEqual(headers.map(rightAligned))
  })

  it('explains missing call metrics from an outdated gateway contract', () => {
    routeMock.useLoaderData.mockReturnValue({
      data: {
        items: [{ ...session, tool_call_count: undefined, mcp_call_count: undefined }],
        page: 1,
        page_size: 50,
        total: 1,
      },
    })

    render(<AgentSessionsPage />)

    expect(screen.getByText('Tool-call data is not available')).toBeInTheDocument()
    expect(screen.getByText('mise run //gateway:reset-local-demo')).toBeInTheDocument()
  })

  it('opens session diagnostics from the keyboard and labels the current page', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [session], page: 1, page_size: 50, total: 51 },
    })
    render(<AgentSessionsPage />)

    const sessionRow = screen.getByRole('row', { name: /Opencode/ })
    expect(sessionRow).toHaveAttribute('tabindex', '0')
    fireEvent.keyDown(sessionRow, { key: 'Enter' })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/agent-sessions',
        search: expect.objectContaining({ session_id: 'session_1' }),
      })
    })
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Go to page 1' })).toHaveAttribute(
        'aria-current',
        'page',
      )
    })
  })

  it('closes session diagnostics by removing the session ID from search', async () => {
    routeMock.useSearch.mockReturnValue({
      page: 1,
      page_size: 50,
      harness_key: 'opencode',
      session_id: 'session_1',
    })
    getAgentSessionDetailMock.mockResolvedValue({ data: detail })

    render(<AgentSessionsPage />)
    fireEvent.click(screen.getByRole('button', { name: 'Close' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/agent-sessions',
        search: { page: 1, page_size: 50, harness_key: 'opencode' },
      })
    })
  })

  it('clears active filters through route search and keeps the page size', async () => {
    routeMock.useSearch.mockReturnValue({
      page: 3,
      page_size: 25,
      harness_key: 'opencode',
    })

    render(<AgentSessionsPage />)
    fireEvent.click(
      within(screen.getByTestId('agent-session-toolbar')).getByRole('button', {
        name: 'Clear',
      }),
    )

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0, { page: 3, page_size: 25, harness_key: 'opencode' })).toEqual({
      page: 1,
      page_size: 25,
    })
  })

  it('changes pages through route search', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [session], page: 1, page_size: 50, total: 51 },
    })

    render(<AgentSessionsPage />)
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/agent-sessions',
        search: { page: 2, page_size: 50 },
      })
    })
  })

  it('deep-links to session diagnostics with request outcomes and retained observations', async () => {
    setCapabilities(false, true)
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, session_id: 'session_1' })
    getAgentSessionDetailMock.mockResolvedValue({ data: detail })

    render(<AgentSessionsPage />)

    await waitFor(() => {
      expect(getAgentSessionDetailMock).toHaveBeenCalledWith({ data: { sessionId: 'session_1' } })
    })
    // The session table also says "Score not shown", so wait for the loaded detail's summary.
    const sessionSummary = await screen.findByRole('region', { name: 'Session summary' })
    expect(within(sessionSummary).getByText('Score not shown')).toBeInTheDocument()
    expect(within(sessionSummary).getByText('Session score')).toBeInTheDocument()
    expect(within(sessionSummary).getByText('Normalised cost')).toBeInTheDocument()
    expect(within(sessionSummary).getByText('Total time')).toBeInTheDocument()
    expect(within(sessionSummary).getByText('1 min')).toBeInTheDocument()
    expect(within(sessionSummary).queryByText('Outcome')).not.toBeInTheDocument()
    expect(within(sessionSummary).queryByText('Active time')).not.toBeInTheDocument()
    const eventStreamTrigger = screen.getByRole('button', { name: /Event stream/ })
    expect(eventStreamTrigger).toHaveAttribute('aria-expanded', 'true')
    expect(eventStreamTrigger).toHaveTextContent('1 request')
    expect(screen.getByRole('heading', { name: 'Request 1' })).toBeInTheDocument()
    const requestLink = screen.getByRole('link', {
      name: 'Open request req_1 in request logs',
    })
    expect(requestLink).toHaveAttribute('href', '/observability/request-logs?request_id=req_1')
    expect(within(requestLink).getByText('Succeeded')).toBeInTheDocument()
    expect(within(requestLink).getByText('High confidence')).toBeInTheDocument()
    expect(within(requestLink).getByText('42.0 s')).toBeInTheDocument()
    expect(within(requestLink).getByText('2 attempts')).toBeInTheDocument()
    expect(
      within(requestLink).getByText(
        'anthropic/claude-opus-4-1: Provider Error → openai/gpt-5: Succeeded',
      ),
    ).toBeInTheDocument()
    expect(within(requestLink).getByText('Edit file-1')).toBeInTheDocument()
    expect(within(requestLink).getByText('1 activity')).toBeInTheDocument()

    const toolExposureTrigger = screen.getByRole('button', { name: /Tool exposure/ })
    expect(toolExposureTrigger).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(toolExposureTrigger)
    expect(screen.getByRole('heading', { name: 'Used at least once' })).toBeInTheDocument()
    const neverCalledPanel = screen
      .getByRole('heading', { name: 'Never called' })
      .closest('section')
    expect(neverCalledPanel).not.toBeNull()
    expect(within(neverCalledPanel!).getAllByRole('listitem')[0]).toHaveTextContent('search')
    expect(within(neverCalledPanel!).queryByText('write')).not.toBeInTheDocument()
    fireEvent.click(within(neverCalledPanel!).getByRole('button', { name: 'Show all 7' }))
    expect(within(neverCalledPanel!).getByText('write')).toBeInTheDocument()

    const identityTrigger = screen.getByRole('button', { name: 'Session identity' })
    const detailSheet = screen.getByRole('dialog')
    expect(identityTrigger).toHaveAttribute('aria-expanded', 'false')
    expect(within(detailSheet).queryByText('External session ID')).not.toBeInTheDocument()
    fireEvent.click(identityTrigger)
    expect(within(detailSheet).getByText('Model')).toBeInTheDocument()
    expect(within(detailSheet).getByText('Harness')).toBeInTheDocument()
    expect(within(detailSheet).getByText('Gateway analysis session ID')).toBeInTheDocument()
    expect(within(detailSheet).getByText('External session source')).toBeInTheDocument()
    expect(within(detailSheet).getByText('Observed')).toBeInTheDocument()
    expect(within(detailSheet).getByText('External session source hash')).toBeInTheDocument()
    expect(within(detailSheet).getByText('sha256:safe-session-source')).toHaveClass(
      'min-w-0',
      'max-w-full',
      'break-all',
    )
    expect(within(detailSheet).queryByText('Operation')).not.toBeInTheDocument()
    expect(within(detailSheet).queryByText('Caller class')).not.toBeInTheDocument()
    expect(within(detailSheet).queryByText('External session ID')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Score components' }))
    expect(screen.getByText('Cost efficiency')).toBeInTheDocument()
    expect(screen.getByText('Elapsed time')).toBeInTheDocument()
    expect(screen.queryByText('Outcome factor')).not.toBeInTheDocument()
    expect(screen.queryByText('Active-time efficiency')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Token and cache use' }))
    expect(screen.getByText('Cache read cost')).toBeInTheDocument()
    expect(screen.getByText('Cache write cost')).toBeInTheDocument()
    expect(screen.getByText('Provider/model switches')).toBeInTheDocument()
    expect(screen.queryByText('Cache creation by lifetime')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Tools and changes' }))
    expect(screen.getByText('Tool server · github')).toBeInTheDocument()
    expect(screen.getByText(/schema tokens per request.*without cache/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Prompt context' }))
    expect(screen.getByText('Initial prompt tokens')).toBeInTheDocument()
    expect(screen.getByText('Maximum prompt tokens')).toBeInTheDocument()
    expect(screen.queryByText('P90 prompt tokens')).not.toBeInTheDocument()
    expect(screen.queryByText('Peak input utilisation')).not.toBeInTheDocument()
    expect(screen.queryByText('Requests above the input boundary')).not.toBeInTheDocument()
    expect(screen.queryByText('Prompt token growth per active minute')).not.toBeInTheDocument()
    expect(screen.queryByText('Possible context resets')).not.toBeInTheDocument()
    expect(screen.queryByText('Answer verification')).not.toBeInTheDocument()

    expect(
      screen.queryByRole('button', { name: 'Score confidence and comparison data' }),
    ).not.toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: 'Reliability and retries' }),
    ).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Skills' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Outcome evidence' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Finish reasons' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Analysis versions' })).not.toBeInTheDocument()
  })

  it('separates quantified data defects from analysis capability notices', async () => {
    const limitedDetail = structuredClone(detail)
    limitedDetail.session.request_count = 155
    limitedDetail.session.limitations = [
      'payload_truncated',
      'request_incomplete',
      'semantic_verification_unavailable',
    ]
    limitedDetail.report!.coverage.response_payload_count = 155
    limitedDetail.report!.coverage.truncated_response_count = 1
    limitedDetail.report!.components.outcome.incomplete_requests = 1
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, session_id: 'session_1' })
    getAgentSessionDetailMock.mockResolvedValue({ data: limitedDetail })

    render(<AgentSessionsPage />)

    const dataLimits = await screen.findByText('Data limits')
    const dataAlert = dataLimits.closest('[data-slot="alert"]')
    expect(dataAlert).not.toBeNull()
    expect(dataAlert).toHaveTextContent('1 of 155 response was truncated')
    expect(dataAlert).toHaveTextContent('1 of 155 request did not complete')
    expect(dataAlert).not.toHaveTextContent('Answer verification is not available')

    const analysisContext = screen.getByText('Analysis context')
    const contextAlert = analysisContext.closest('[data-slot="alert"]')
    expect(contextAlert).not.toBeNull()
    expect(contextAlert).toHaveTextContent('Answer verification is not available')
  })

  it('does not link team admins to platform-only request logs', async () => {
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, session_id: 'session_1' })
    getAgentSessionDetailMock.mockResolvedValue({ data: detail })

    render(<AgentSessionsPage />)

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Request 1' })).toBeInTheDocument()
    })
    expect(
      screen.queryByRole('link', { name: 'Open request req_1 in request logs' }),
    ).not.toBeInTheDocument()
  })

  it('shows unknown and disabled states instead of false zero metrics', async () => {
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, session_id: 'session_1' })
    const incompleteDetail = structuredClone(detail)
    const incompleteDiagnostics = incompleteDetail.report!.diagnostics
    incompleteDiagnostics.token_and_cache.fresh_input_tokens = null
    incompleteDiagnostics.token_and_cache.cache_read_tokens = null
    incompleteDiagnostics.token_and_cache.cache_creation_tokens = null
    incompleteDiagnostics.token_and_cache.output_tokens = null
    incompleteDiagnostics.reliability.attempt_coverage_percent = 0
    incompleteDiagnostics.reliability.total_attempts = 0
    incompleteDiagnostics.reliability.wasted_attempts = 0
    incompleteDiagnostics.reliability.attempts = []
    incompleteDiagnostics.reliability.tool_invocations = 0
    incompleteDiagnostics.reliability.tools = []
    incompleteDiagnostics.tools_and_changes.observed_tool_calls = 0
    incompleteDiagnostics.tools_and_changes.tool_servers = []
    incompleteDetail.observations[0].facts.tool_name = null
    incompleteDetail.observations[0].facts.supplied_tools = []
    incompleteDiagnostics.enabled_metrics.skill_metrics = false
    getAgentSessionDetailMock.mockResolvedValue({ data: incompleteDetail })

    render(<AgentSessionsPage />)
    expect(await screen.findByText('Agent session details')).toBeInTheDocument()

    // The sheet opens before the detail request resolves, so wait for the loaded body.
    fireEvent.click(await screen.findByRole('button', { name: 'Token and cache use' }))
    expect(
      screen.getByText('Unknown — the required telemetry was not measured for this session.'),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Tools and changes' }))
    expect(
      screen.getAllByText('Unknown — the required telemetry was not measured for this session.'),
    ).toHaveLength(2)
  })

  it('warns when retained request or observation history exceeds the detail cap', async () => {
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, session_id: 'session_1' })
    getAgentSessionDetailMock.mockResolvedValue({
      data: {
        ...detail,
        request_history_truncated: true,
        observation_history_truncated: true,
      },
    })

    render(<AgentSessionsPage />)

    expect(await screen.findByText('Some history is not shown')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Event stream/ })).toHaveTextContent('1+ request')
  })
})

describe('AgentSessionsPage filters', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useSearch.mockReset()
    navigateMock.mockReset()
    getHarnessUsageMock.mockReset()
    getModelOptionsMock.mockReset()
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [session], page: 1, page_size: 50, total: 1 },
    })
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50 })
    setCapabilities(false)
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('shows a toolbar with a count and no always-visible filter fields', () => {
    render(<AgentSessionsPage />)

    const toolbar = within(screen.getByTestId('agent-session-toolbar'))
    expect(toolbar.getByRole('status')).toHaveTextContent('Showing 1 of 1')
    expect(toolbar.getByRole('button', { name: 'Filters' })).toBeInTheDocument()
    expect(toolbar.getByRole('button', { name: 'Columns' })).toBeInTheDocument()
    expect(screen.queryByTestId('agent-session-filter-chips')).not.toBeInTheDocument()
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
    expect(document.querySelector('input[type="date"]')).toBeNull()
  })

  it('groups every filter under a Filter by menu', async () => {
    render(<AgentSessionsPage />)

    openFilterMenu()
    const menu = await screen.findByRole('menu')
    expect(within(menu).getByText('Filter by')).toBeInTheDocument()
    expect(
      within(menu)
        .getAllByRole('menuitem')
        .map((item) => item.textContent),
    ).toEqual([
      'State',
      'Outcome',
      'Harness',
      'Model',
      'Operation',
      'Caller class',
      'Score maturity',
      'Score confidence',
      'Minimum coverage',
      'User ID',
      'Team ID',
      'Service account ID',
      'Session source ID',
      'Session source hash',
      'Request tag',
      'Started after',
      'Started before',
    ])
  })

  it('renders URL filters as removable chips with readable values', () => {
    routeMock.useSearch.mockReturnValue({
      page: 2,
      page_size: 25,
      lifecycle: 'finalized',
      minimum_coverage_percent: 75,
      started_after: '2026-07-01T00:00:00.000Z',
      user_id: 'user_1',
      request_tag_key: 'team',
      request_tag_value: 'platform',
    })

    render(<AgentSessionsPage />)

    const toolbar = within(screen.getByTestId('agent-session-toolbar'))
    expect(toolbar.getByRole('button', { name: /^Filters/ })).toHaveTextContent('5')
    expect(toolbar.getByTestId('agent-session-filter-lifecycle')).toHaveTextContent('Finalized')
    expect(toolbar.getByTestId('agent-session-filter-minimum-coverage-percent')).toHaveTextContent(
      '75%',
    )
    expect(toolbar.getByTestId('agent-session-filter-started-after')).toHaveTextContent(
      '1 Jul 2026',
    )
    expect(toolbar.getByTestId('agent-session-filter-user-id')).toHaveValue('user_1')
    expect(toolbar.getByTestId('agent-session-filter-request-tag-key')).toHaveValue('team')
    expect(toolbar.getByTestId('agent-session-filter-request-tag-value')).toHaveValue('platform')
    // Chips span the row on narrow screens instead of overflowing it.
    expect(toolbar.getByTestId('agent-session-filter-chip-request_tag')).toHaveClass(
      'w-full',
      'sm:w-auto',
    )
  })

  it('applies a picked state at once and returns to the first page', async () => {
    routeMock.useSearch.mockReturnValue({ page: 3, page_size: 25 })
    render(<AgentSessionsPage />)

    await addFilter('State')
    fireEvent.click(await screen.findByRole('option', { name: 'Finalized' }))

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigateMock).toHaveBeenCalledWith(
      expect.objectContaining({ to: '/observability/agent-sessions' }),
    )
    expect(navigatedSearch(0, { page: 3, page_size: 25 })).toEqual({
      page: 1,
      page_size: 25,
      lifecycle: 'finalized',
    })
  })

  it('does not accept a typed value for a closed set such as the state', async () => {
    render(<AgentSessionsPage />)

    await addFilter('State')
    fireEvent.change(await screen.findByPlaceholderText('Search states…'), {
      target: { value: 'paused' },
    })

    expect(screen.queryByText(/Filter by “paused”/)).not.toBeInTheDocument()
  })

  it('picks a model from the catalog by its key and still accepts a retired key', async () => {
    getModelOptionsMock.mockResolvedValue({
      data: {
        items: [
          {
            id: 'claude-opus-4-1',
            model_id: 'uuid-1',
            resolved_model_key: 'claude-opus-4-1',
            alias_of: null,
            provider_label: 'Anthropic',
          },
        ],
      },
    })
    render(<AgentSessionsPage />)

    await addFilter('Model')
    expect(await screen.findByRole('option', { name: /claude-opus-4-1/ })).toBeInTheDocument()
    expect(getModelOptionsMock).toHaveBeenCalledOnce()
    expect(getModelOptionsMock).toHaveBeenCalledWith({ signal: expect.any(AbortSignal) })

    fireEvent.change(screen.getByPlaceholderText('Search models…'), {
      target: { value: 'retired-model' },
    })
    fireEvent.click(await screen.findByRole('option', { name: 'Filter by “retired-model”' }))

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0, { page: 1, page_size: 50 })).toEqual(
      expect.objectContaining({ requested_model_key: 'retired-model' }),
    )
  })

  it('offers harnesses that sent recent traffic', async () => {
    getHarnessUsageMock.mockResolvedValue({
      data: {
        leaders: [
          { agent_harness_key: 'opencode', agent_harness_label: 'Opencode' },
          { agent_harness_key: 'claude_code', agent_harness_label: 'Claude Code' },
        ],
      },
    })
    render(<AgentSessionsPage />)

    await addFilter('Harness')
    fireEvent.click(await screen.findByRole('option', { name: /Claude Code/ }))

    expect(getHarnessUsageMock).toHaveBeenCalledWith({
      data: { range: '31d' },
      signal: expect.any(AbortSignal),
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0)).toEqual(expect.objectContaining({ harness_key: 'claude_code' }))
  })

  it('keeps the harness filter usable when its list cannot load', async () => {
    getHarnessUsageMock.mockRejectedValue(new Error('forbidden'))
    render(<AgentSessionsPage />)

    await addFilter('Harness')
    expect(
      await screen.findByText('Could not load options. Type a value instead.'),
    ).toBeInTheDocument()
    fireEvent.change(screen.getByPlaceholderText('Search or type a harness key…'), {
      target: { value: 'gemini_cli' },
    })
    fireEvent.click(await screen.findByRole('option', { name: 'Filter by “gemini_cli”' }))

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0)).toEqual(expect.objectContaining({ harness_key: 'gemini_cli' }))
  })

  it('turns typed start days into whole-day UTC bounds', async () => {
    render(<AgentSessionsPage />)

    await addFilter('Started after')
    fireEvent.change(await screen.findByPlaceholderText('Search or type YYYY-MM-DD…'), {
      target: { value: '2026-02-30' },
    })
    expect(screen.queryByText(/Filter by/)).not.toBeInTheDocument()
    fireEvent.change(screen.getByPlaceholderText('Search or type YYYY-MM-DD…'), {
      target: { value: '2026-07-01' },
    })
    fireEvent.click(await screen.findByRole('option', { name: 'Filter by “1 Jul 2026”' }))
    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0)).toEqual(
      expect.objectContaining({ started_after: '2026-07-01T00:00:00.000Z' }),
    )

    await addFilter('Started before')
    fireEvent.change(
      await screen.findByRole('combobox', { name: /Search started before values/ }),
      { target: { value: '2026-07-31' } },
    )
    fireEvent.click(await screen.findByRole('option', { name: 'Filter by “31 Jul 2026”' }))
    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(2))
    expect(navigatedSearch(1)).toEqual(
      expect.objectContaining({ started_before: '2026-07-31T23:59:59.999Z' }),
    )
  })

  it('finds a preset day by its typed date', async () => {
    const today = new Date().toISOString().slice(0, 10)
    render(<AgentSessionsPage />)

    await addFilter('Started after')
    fireEvent.change(await screen.findByPlaceholderText('Search or type YYYY-MM-DD…'), {
      target: { value: today },
    })
    fireEvent.click(await screen.findByRole('option', { name: /Today/ }))

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0)).toEqual(
      expect.objectContaining({ started_after: `${today}T00:00:00.000Z` }),
    )
  })

  it('accepts a typed coverage percentage and the route turns it into a number', async () => {
    render(<AgentSessionsPage />)

    await addFilter('Minimum coverage')
    fireEvent.change(await screen.findByPlaceholderText('Search or type a percentage…'), {
      target: { value: '101' },
    })
    expect(screen.queryByText(/Filter by/)).not.toBeInTheDocument()
    fireEvent.change(screen.getByPlaceholderText('Search or type a percentage…'), {
      target: { value: '60%' },
    })
    fireEvent.click(await screen.findByRole('option', { name: 'Filter by “60%”' }))

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0)).toEqual(expect.objectContaining({ minimum_coverage_percent: 60 }))
  })

  it('commits an ID chip on Enter and waits for both halves of a tag', async () => {
    render(<AgentSessionsPage />)

    await addFilter('Request tag')
    const tagKey = await screen.findByTestId('agent-session-filter-request-tag-key')
    fireEvent.change(tagKey, { target: { value: 'team' } })
    fireEvent.keyDown(tagKey, { key: 'Enter' })
    expect(
      screen.getByText('Provide both a tag key and tag value to filter sessions by a request tag.'),
    ).toBeInTheDocument()
    expect(navigateMock).not.toHaveBeenCalled()

    await addFilter('User ID')
    const userId = await screen.findByTestId('agent-session-filter-user-id')
    fireEvent.change(userId, { target: { value: ' user_1 ' } })
    fireEvent.keyDown(userId, { key: 'Enter' })

    // The user ID applies; the half-typed tag stays a draft.
    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    const next = navigatedSearch(0)
    expect(next).toEqual(expect.objectContaining({ user_id: 'user_1' }))
    expect(next).not.toHaveProperty('request_tag_key')
  })

  it('removes one chip and keeps the other filters', async () => {
    const search = { page: 2, page_size: 25, harness_key: 'opencode', team_id: 'team_1' }
    routeMock.useSearch.mockReturnValue(search)
    render(<AgentSessionsPage />)

    fireEvent.click(screen.getByRole('button', { name: 'Remove Team ID filter' }))

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0, search)).toEqual({
      page: 1,
      page_size: 25,
      harness_key: 'opencode',
    })
  })

  it('removes a chip whose value is still being applied', async () => {
    holdNavigations()
    render(<AgentSessionsPage />)

    await addFilter('User ID')
    const userId = await screen.findByTestId('agent-session-filter-user-id')
    fireEvent.change(userId, { target: { value: 'user_1' } })
    // Blur applies the value; the navigation never settles, so the URL keeps no user ID.
    fireEvent.blur(userId)
    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    fireEvent.click(screen.getByRole('button', { name: 'Remove User ID filter' }))

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(2))
    expect(navigatedSearch(1, navigatedSearch(0))).not.toHaveProperty('user_id')
    expect(screen.queryByTestId('agent-session-filter-chip-user_id')).not.toBeInTheDocument()
  })

  it('retries a filter whose navigation ended without changing the URL', async () => {
    render(<AgentSessionsPage />)

    await addFilter('User ID')
    const userId = await screen.findByTestId('agent-session-filter-user-id')
    fireEvent.change(userId, { target: { value: 'user_1' } })
    fireEvent.keyDown(userId, { key: 'Enter' })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    // The navigation settles but the URL still has no user ID, as after Back or a cancel.
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Showing'))

    fireEvent.keyDown(userId, { key: 'Enter' })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(2))
    expect(navigatedSearch(1)).toEqual(expect.objectContaining({ user_id: 'user_1' }))
  })

  it('clears filters while a blur commit is still navigating', async () => {
    holdNavigations()
    render(<AgentSessionsPage />)

    await addFilter('User ID')
    const userId = await screen.findByTestId('agent-session-filter-user-id')
    fireEvent.change(userId, { target: { value: 'user_1' } })
    fireEvent.blur(userId)
    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Loading…'))

    const clear = screen.getByRole('button', { name: 'Clear' })
    expect(clear).toBeEnabled()
    fireEvent.click(clear)

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(2))
    expect(navigatedSearch(1, navigatedSearch(0))).not.toHaveProperty('user_id')
  })

  it('returns focus to the Filters button when its menu closes without a pick', async () => {
    render(<AgentSessionsPage />)

    openFilterMenu()
    fireEvent.keyDown(await screen.findByRole('menu'), { key: 'Escape' })

    await waitFor(() =>
      expect(
        within(screen.getByTestId('agent-session-toolbar')).getByRole('button', {
          name: /^Filters/,
        }),
      ).toHaveFocus(),
    )
  })

  it('offers to clear the filters when nothing matches', async () => {
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, operation: 'images' })
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [], page: 1, page_size: 50, total: 0 },
    })
    render(<AgentSessionsPage />)

    const table = within(screen.getByTestId('agent-session-table'))
    expect(table.getByText('No agent sessions match these filters.')).toBeInTheDocument()
    fireEvent.click(table.getByRole('button', { name: 'Clear filters' }))

    await waitFor(() => expect(navigateMock).toHaveBeenCalledTimes(1))
    expect(navigatedSearch(0, { page: 1, page_size: 50, operation: 'images' })).toEqual({
      page: 1,
      page_size: 50,
    })
  })

  it('hides and restores table columns from the Columns popover', async () => {
    render(<AgentSessionsPage />)

    const table = within(screen.getByTestId('agent-session-table'))
    const headers = () => table.getAllByRole('columnheader').map((header) => header.textContent)
    expect(headers()).toHaveLength(11)

    fireEvent.click(screen.getByRole('button', { name: 'Columns' }))
    expect(await screen.findByRole('heading', { name: 'Table columns' })).toBeInTheDocument()
    expect(screen.queryByRole('checkbox', { name: 'Started' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('checkbox', { name: 'MCP calls' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'Data quality' }))

    expect(headers()).not.toContain('MCP calls')
    expect(headers()).not.toContain('Data quality')
    const cells = within(table.getByRole('row', { name: /Opencode/ })).getAllByRole('cell')
    expect(cells).toHaveLength(9)
    expect(navigateMock).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('checkbox', { name: 'MCP calls' }))
    expect(headers()).toContain('MCP calls')
  })

  it('renders sessions as cards below md that open details from a button', async () => {
    render(<AgentSessionsPage />)

    const cards = screen.getByTestId('agent-session-cards')
    expect(cards).toHaveClass('grid-cols-1', 'md:hidden')
    expect(screen.getByTestId('agent-session-table')).toHaveClass('hidden', 'md:block', 'min-w-0')
    const card = within(cards).getByRole('article', { name: /Session started/ })
    expect(card).toHaveClass('min-w-0')
    // Card facts come from the table's column definitions, labels included.
    expect(
      within(card)
        .getAllByRole('term')
        .map((term) => term.textContent),
    ).toEqual(['Score', 'Cost', 'Active time', 'Requests', 'Data quality'])
    // Between md and the table's minimum width, the table scrolls inside its own container.
    expect(
      screen.getByTestId('agent-session-table').querySelector('[data-slot="table-container"]'),
    ).toHaveClass('overflow-x-auto')
    fireEvent.click(within(card).getByRole('button', { name: 'View session' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/agent-sessions',
        search: expect.objectContaining({ session_id: 'session_1' }),
      })
    })
  })
})
