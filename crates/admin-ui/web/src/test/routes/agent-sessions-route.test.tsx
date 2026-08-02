import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ComponentProps, ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AgentSessionsPage } from '@/routes/observability/agent-sessions'
import type { AgentSessionDetailView, AgentSessionSummaryView } from '@/types/api'

const { getAgentSessionDetailMock, navigateMock, routeMock } = vi.hoisted(() => ({
  getAgentSessionDetailMock: vi.fn(),
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
}))

const session: AgentSessionSummaryView = {
  session_id: 'session_1',
  session_source_id: 'session_source_1',
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
    expect(screen.getByText('Opencode')).toBeInTheDocument()
    expect(document.querySelector('[data-agent-harness-icon="opencode"]')).toBeInTheDocument()
    expect(screen.getByText('Score not shown')).toBeInTheDocument()
    expect(screen.queryByText('82')).not.toBeInTheDocument()
    const sessionRow = screen.getByRole('row', { name: /Opencode/ })
    expect(sessionRow).toHaveTextContent('8')
    expect(sessionRow).toHaveTextContent('2')
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
    expect(screen.getByText('mise run gateway-reset-local-demo')).toBeInTheDocument()
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

  it('deep-links to session diagnostics with request outcomes and retained observations', async () => {
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 50, session_id: 'session_1' })
    getAgentSessionDetailMock.mockResolvedValue({ data: detail })

    render(<AgentSessionsPage />)

    await waitFor(() => {
      expect(getAgentSessionDetailMock).toHaveBeenCalledWith({ data: { sessionId: 'session_1' } })
      expect(screen.getByText('Score not shown')).toBeInTheDocument()
    })
    const sessionSummary = screen.getByRole('region', { name: 'Session summary' })
    expect(within(sessionSummary).getByText('Session score')).toBeInTheDocument()
    expect(within(sessionSummary).getByText('Normalised cost')).toBeInTheDocument()
    expect(within(sessionSummary).getByText('Total time')).toBeInTheDocument()
    expect(within(sessionSummary).getByText('1.0 min')).toBeInTheDocument()
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
    expect(identityTrigger).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByText('External session ID')).not.toBeInTheDocument()
    fireEvent.click(identityTrigger)
    expect(screen.getByText('External session ID')).toBeInTheDocument()

    const comparisonTrigger = screen.getByRole('button', {
      name: 'Score confidence and comparison data',
    })
    expect(comparisonTrigger).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByText('Comparison snapshot')).not.toBeInTheDocument()
    fireEvent.click(comparisonTrigger)
    expect(screen.getByText('Comparison group')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Token and cache use' }))
    expect(screen.getByText('Cache read cost')).toBeInTheDocument()
    expect(screen.getByText('Cache write cost')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Tools and changes' }))
    expect(screen.getByText('Tool server · github')).toBeInTheDocument()
    expect(screen.getByText(/schema tokens per request.*without cache/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Reliability and retries' }))
    expect(screen.getByText('Wasted attempts')).toBeInTheDocument()
    expect(screen.getByText('1 of 2 attempts · 500 ms')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Skills' }))
    expect(screen.getByText('Skill · repository-review')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Outcome evidence' }))
    expect(screen.getByText('Rework ratio')).toBeInTheDocument()
    expect(screen.getAllByText('25.0%').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('93%').length).toBeGreaterThanOrEqual(1)
    expect(screen.getByText('Failed file operations')).toBeInTheDocument()
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

    fireEvent.click(screen.getByRole('button', { name: 'Token and cache use' }))
    expect(
      screen.getByText('Unknown — the required telemetry was not measured for this session.'),
    ).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Reliability and retries' }))
    expect(
      screen.getAllByText('Unknown — the required telemetry was not measured for this session.'),
    ).toHaveLength(2)

    fireEvent.click(screen.getByRole('button', { name: 'Tools and changes' }))
    expect(
      screen.getAllByText('Unknown — the required telemetry was not measured for this session.'),
    ).toHaveLength(3)

    fireEvent.click(screen.getByRole('button', { name: 'Skills' }))
    expect(screen.getByText('Disabled by the analysis configuration.')).toBeInTheDocument()
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
