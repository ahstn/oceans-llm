import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import { regularUserSession } from '@/test/auth-session'
import type { RequestLogView } from '@/types/api'
import { RequestLogsPage } from '@/routes/observability/request-logs'

const getObservabilityRequestLogDetailMock = vi.fn()
const navigateMock = vi.fn()

// Hoisted with the mocks: the route module reads it as it loads.
const routeMock = vi.hoisted(() => ({
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
  useSearch: vi.fn(),
}))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  Link: ({ children }: { children: ReactNode }) => <a>{children}</a>,
  useRouter: () => ({
    navigate: navigateMock,
  }),
}))

vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: () => ({
    getVirtualItems: () => [{ index: 0, size: 36, start: 0 }],
    getTotalSize: () => 36,
  }),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getRequestLogs: vi.fn(),
  getObservabilityRequestLogDetail: (...args: unknown[]) =>
    getObservabilityRequestLogDetailMock(...args),
}))

const items: RequestLogView[] = [
  {
    request_log_id: 'reqlog_1',
    request_id: 'req_1',
    api_key_id: 'api_key_1',
    api_key_name: 'Checkout Service Key',
    user_id: 'user_1',
    user_name: 'Alice Example',
    user_email: 'alice@example.com',
    team_id: null,
    service_account_name: null,
    model_key: 'gpt-4.1-mini',
    resolved_model_key: 'gpt-4.1-mini',
    provider_key: 'openai',
    status_code: 200,
    latency_ms: 482,
    prompt_tokens: 400,
    completion_tokens: 942,
    total_tokens: 1342,
    cache_read_tokens: 300,
    cost_usd_10000: 457,
    error_code: null,
    has_payload: true,
    request_payload_truncated: false,
    response_payload_truncated: false,
    request_tags: {
      service: 'checkout',
      component: 'pricing_api',
      env: 'prod',
      bespoke: [{ key: 'feature', value: 'guest_checkout' }],
    },
    metadata: {
      operation: 'chat_completions',
      stream: false,
    },
    payload_policy: {
      capture_mode: 'redacted_payloads',
      request_max_bytes: 65536,
      response_max_bytes: 65536,
      stream_max_events: 128,
      version: 'builtin:v1',
    },
    tool_cardinality: {
      referenced_mcp_server_count: null,
      exposed_tool_count: 2,
      request_tool_count: 5,
      invoked_tool_count: 0,
      invoked_distinct_tool_count: 0,
      invoked_mcp_tool_count: 0,
      filtered_tool_count: null,
    },
    agent_harness_key: 'opencode',
    agent_harness_label: 'Opencode',
    occurred_at: '2026-03-10T11:32:00Z',
  },
]

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

function renderPage() {
  return render(
    <TooltipProvider>
      <RequestLogsPage />
    </TooltipProvider>,
  )
}

function resetMocks() {
  routeMock.useLoaderData.mockReset()
  routeMock.useRouteContext.mockReset()
  routeMock.useRouteContext.mockReturnValue({
    session: regularUserSession(),
  })
  routeMock.useSearch.mockReset()
  getObservabilityRequestLogDetailMock.mockReset()
  navigateMock.mockReset()
  routeMock.useSearch.mockReturnValue({})
  vi.stubGlobal('ResizeObserver', ResizeObserverMock)
}

/** Resolves the functional `search` passed to `router.navigate` against a prior URL search. */
function navigatedSearch(callIndex: number, previous: Record<string, unknown> = {}) {
  const search = navigateMock.mock.calls[callIndex][0].search as (
    previous: Record<string, unknown>,
  ) => Record<string, unknown>
  return search(previous)
}

/**
 * Filter navigations run in a list transition. Wait for the toolbar to show it pending and then
 * finished, so the transition settles inside the test.
 */
async function waitForListTransition() {
  expect(await screen.findByText('Loading…')).toBeInTheDocument()
  expect(await screen.findByText('Showing 1 of 1')).toBeInTheDocument()
}

describe('RequestLogsPage table', () => {
  beforeEach(resetMocks)

  it('renders dedicated mobile and desktop log layouts from the same payload', () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    renderPage()

    expect(screen.getByTestId('request-log-mobile-list')).toBeInTheDocument()
    expect(screen.getByTestId('request-log-desktop-table')).toBeInTheDocument()
    // 12 rows × 56 px (requestLogDesktopPreviewRows × requestLogRowEstimatePx)
    expect(screen.getByTestId('request-log-desktop-table')).toHaveStyle({
      height: '672px',
    })
    // The table scrolls on both axes itself, so the Inspect column can stay pinned to its right edge.
    expect(screen.getByTestId('request-log-desktop-table')).toHaveClass('overflow-auto')
    for (const inspect of within(screen.getByTestId('request-log-desktop-table')).getAllByText(
      'Inspect',
    )) {
      expect(inspect.closest('.sticky')).toHaveClass('right-0', 'shadow-sticky-edge-end')
    }
    expect(
      screen.getByText(
        'Review each request, how long it took, and the data that the system stored.',
      ),
    ).toBeInTheDocument()
    expect(screen.getAllByText('gpt-4.1-mini')).toHaveLength(2)
    expect(screen.getAllByText('openai')).toHaveLength(2)
    // The overview leads with the request time; request id and key name live in the detail sheet.
    expect(screen.queryByText('req_1')).not.toBeInTheDocument()
    expect(screen.queryByText('Checkout Service Key')).not.toBeInTheDocument()
    expect(screen.getAllByText('10 Mar 2026, 11:32:00 UTC')).toHaveLength(2)
    expect(screen.getByText('Time')).toBeInTheDocument()
    expect(screen.getAllByText('Tools (Used / Total)')).toHaveLength(2)
    expect(screen.getAllByTestId('request-log-tool-usage').map((el) => el.textContent)).toEqual([
      '0 / 5',
      '0 / 5',
    ])
    expect(screen.getAllByText('$0.0457')).toHaveLength(2)
    expect(screen.getAllByText('Alice Example')).toHaveLength(2)
    expect(screen.getByText('alice@example.com')).toBeInTheDocument()
    expect(screen.getByText('Showing 1 of 1')).toBeInTheDocument()
    expect(screen.queryByText('Chat Completions')).not.toBeInTheDocument()
    expect(screen.queryByText('redacted payloads')).not.toBeInTheDocument()
    expect(screen.queryByText('payload')).not.toBeInTheDocument()
  })

  it('counts distinct tools used and falls back to call counts for older logs', () => {
    const repeatedToolItem: RequestLogView = {
      ...items[0],
      request_log_id: 'reqlog_repeat',
      tool_cardinality: {
        ...items[0].tool_cardinality,
        invoked_tool_count: 3,
        invoked_distinct_tool_count: 1,
      },
    }
    const legacyItem: RequestLogView = {
      ...items[0],
      request_log_id: 'reqlog_legacy',
      tool_cardinality: {
        ...items[0].tool_cardinality,
        invoked_tool_count: 2,
        invoked_distinct_tool_count: null,
      },
    }
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [repeatedToolItem, legacyItem], total: 2 },
    })

    renderPage()

    const usage = screen.getAllByTestId('request-log-tool-usage').map((el) => el.textContent)
    expect(usage).toContain('1 / 5')
    expect(usage).toContain('2 / 5')
    expect(usage).not.toContain('3 / 5')
  })

  it('renders service-account and unknown callers with sensible fallbacks', () => {
    const serviceAccountItem: RequestLogView = {
      ...items[0],
      request_log_id: 'reqlog_2',
      request_id: 'req_2',
      api_key_name: 'Nightly Rollup Key',
      user_id: null,
      user_name: null,
      user_email: null,
      service_account_name: 'Batch Jobs Account',
    }
    const unknownCallerItem: RequestLogView = {
      ...items[0],
      request_log_id: 'reqlog_3',
      request_id: 'req_3',
      api_key_name: null,
      user_id: null,
      user_name: null,
      user_email: null,
      service_account_name: null,
    }
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [serviceAccountItem, unknownCallerItem], total: 2 },
    })

    renderPage()

    // The mocked virtualizer renders only the first row in the desktop table,
    // so the second item is asserted via the mobile list alone.
    expect(screen.getAllByText('Batch Jobs Account')).toHaveLength(2)
    expect(screen.getAllByText('service account').length).toBeGreaterThan(0)
    expect(screen.queryByText('Nightly Rollup Key')).not.toBeInTheDocument()
    expect(screen.getAllByText('Unknown')).toHaveLength(1)
  })

  it('shows a cache-hit indicator with cached token share and em dash fallbacks', async () => {
    const uncachedLegacyItem: RequestLogView = {
      ...items[0],
      request_log_id: 'reqlog_4',
      cache_read_tokens: null,
      cost_usd_10000: null,
      tool_cardinality: {
        ...items[0].tool_cardinality,
        request_tool_count: null,
      },
    }
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [items[0], uncachedLegacyItem], total: 2 },
    })

    const view = renderPage()
    const scope = within(view.container)

    // Only the cached row (mobile + desktop) renders the indicator.
    const cacheIcons = scope.getAllByTestId('request-log-cache-hit')
    expect(cacheIcons).toHaveLength(2)
    expect(scope.getAllByText('—')).toHaveLength(1)
    expect(scope.getAllByText('0 / —')).toHaveLength(1)

    fireEvent.focus(cacheIcons[0])

    expect(
      (await screen.findAllByText('300 of 400 prompt tokens cached (75%)')).length,
    ).toBeGreaterThan(0)
  })
})

describe('RequestLogsPage detail', () => {
  beforeEach(resetMocks)

  const successAttempt = {
    request_attempt_id: 'attempt_1',
    request_log_id: 'reqlog_1',
    request_id: 'req_1',
    attempt_number: 1,
    route_id: 'route_1',
    provider_key: 'openai',
    upstream_model: 'gpt-4.1-mini',
    status: 'success',
    status_code: 200,
    error_code: null,
    error_detail: null,
    error_detail_truncated: false,
    retryable: false,
    terminal: true,
    produced_final_response: true,
    stream: false,
    started_at: '2026-03-10T11:32:00Z',
    completed_at: '2026-03-10T11:32:01Z',
    latency_ms: 482,
    metadata: {},
  }

  async function openDetail(data: Record<string, unknown>) {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })
    getObservabilityRequestLogDetailMock.mockResolvedValue({ data })
    renderPage()
    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])
    // The sheet opens before the detail request resolves, so wait for the loaded body.
    return within(await screen.findByTestId('request-log-detail-body'))
  }

  it('renders the header, key metrics and facts without fallback-era fields', async () => {
    const body = await openDetail({
      log: items[0],
      user_agent_raw: 'opencode/1.2.3',
      payload: {
        request_json: { body: { prompt: 'ping' } },
        response_json: { body: { output: 'pong' } },
      },
      attempts: [successAttempt],
    })

    // The header carries the request's identity: status, request ID and model route.
    const sheet = within(screen.getByTestId('request-log-detail-sheet'))
    expect(sheet.getByRole('heading', { name: /req_1/ })).toBeInTheDocument()
    expect(sheet.getAllByText('200').length).toBeGreaterThan(0)
    expect(within(sheet.getByTestId('request-log-model-route')).getByText('openai')).toBeVisible()

    const metrics = within(body.getByTestId('request-log-key-metrics'))
    expect(metrics.getByText('482 ms')).toBeInTheDocument()
    expect(metrics.getByText('1,342')).toBeInTheDocument()
    expect(metrics.getByText('400 in · 942 out')).toBeInTheDocument()
    expect(metrics.getByText('$0.0457')).toBeInTheDocument()
    expect(metrics.getByText('75%')).toBeInTheDocument()
    expect(metrics.getByText('300 of 400 prompt tokens')).toBeInTheDocument()

    const facts = within(body.getByTestId('request-log-facts'))
    expect(facts.getByText('Operation')).toBeInTheDocument()
    expect(facts.getByText('Chat Completions')).toBeInTheDocument()
    expect(facts.getByText('API key')).toBeInTheDocument()
    expect(facts.getByText('Checkout Service Key')).toBeInTheDocument()
    expect(facts.getByText('Alice Example · alice@example.com')).toBeInTheDocument()
    expect(facts.getByText('Opencode')).toBeInTheDocument()
    expect(facts.getByText('opencode/1.2.3')).toBeInTheDocument()
    expect(facts.getByText('Stream')).toBeInTheDocument()
    expect(facts.getByText('No')).toBeInTheDocument()
    expect(facts.getByText('service:checkout')).toBeInTheDocument()
    expect(facts.getByText('feature:guest_checkout')).toBeInTheDocument()
    // Identifiers truncate on one line, with the full value on hover, instead of breaking.
    expect(facts.getByTitle('reqlog_1')).toHaveClass('truncate')
    expect(facts.getByRole('button', { name: 'Copy request log ID' })).toBeInTheDocument()

    expect(screen.queryByText('Attempt Count')).not.toBeInTheDocument()
    expect(screen.queryByText('Fallback')).not.toBeInTheDocument()
    expect(screen.queryByText('Payload Policy')).not.toBeInTheDocument()
    expect(screen.queryByText('n/a')).not.toBeInTheDocument()
    expect(body.getAllByText('—').length).toBeGreaterThan(0)
  })

  it('renders tool counts, provider attempts and the payload view toggle', async () => {
    const body = await openDetail({
      log: items[0],
      user_agent_raw: null,
      payload: {
        request_json: { body: { prompt: 'ping' } },
        response_json: { body: { output: 'pong' } },
      },
      attempts: [successAttempt],
      mcp_token_overhead: {
        provider_family: 'openai',
        model_or_encoding: 'gpt-4.1-mini',
        exposed_tool_count: 2,
        estimated_definition_tokens: 3105,
        estimated_result_tokens: null,
        estimator_source: 'local_tokenizer',
        confidence: 'high',
        cache_hit_count: 25,
        cache_miss_count: 2,
        context_window_tokens: 131072,
        context_window_percent_bps: 236,
        metadata: {},
      },
    })

    const tools = within(body.getByTestId('request-log-tools'))
    expect(tools.getByText('MCP & tools')).toBeInTheDocument()
    // The router Link mock renders a bare anchor, so match the text rather than the link role.
    expect(tools.getByText('View MCP invocations')).toBeInTheDocument()
    const counts = within(tools.getByTestId('request-log-tool-counts'))
    for (const label of [
      'MCP servers',
      'Request tools',
      'Tools exposed',
      'Tools filtered',
      'Tool calls',
      'MCP tool calls',
      'Distinct tools called',
    ]) {
      expect(counts.getByText(label)).toBeInTheDocument()
    }
    expect(counts.getByText('5')).toBeInTheDocument()
    expect(counts.getAllByText('0').length).toBeGreaterThan(0)
    // Counts the gateway did not record read as em dashes, not zeroes.
    expect(counts.getAllByText('—')).toHaveLength(2)

    const overhead = within(tools.getByTestId('request-log-mcp-overhead'))
    expect(overhead.getByText('3,105')).toBeInTheDocument()
    expect(overhead.getByText(/2\.36% of a 131,072-token context window/)).toBeInTheDocument()
    expect(overhead.getByText('local_tokenizer')).toBeInTheDocument()

    const attempts = within(body.getByTestId('request-log-attempts'))
    expect(attempts.getByText('Provider attempts')).toBeInTheDocument()
    expect(attempts.getByText('#1')).toBeInTheDocument()
    expect(attempts.getByText('success')).toBeInTheDocument()
    expect(attempts.getByText('terminal')).toBeInTheDocument()
    expect(attempts.getByText('final response')).toBeInTheDocument()

    const payloads = () => body.getByTestId('request-log-payloads')
    expect(payloads()).toHaveTextContent('"prompt": "ping"')
    expect(payloads()).toHaveTextContent('"output": "pong"')
    expect(within(payloads()).getAllByText('full')).toHaveLength(2)

    fireEvent.click(body.getByRole('radio', { name: 'Request' }))
    expect(payloads()).toHaveTextContent('"prompt": "ping"')
    expect(payloads()).not.toHaveTextContent('pong')

    fireEvent.click(body.getByRole('radio', { name: 'Response' }))
    expect(payloads()).not.toHaveTextContent('ping')
    expect(payloads()).toHaveTextContent('"output": "pong"')
  })

  it('shows error details only for the attempts that recorded one', async () => {
    const failedAttempt = {
      ...successAttempt,
      request_attempt_id: 'attempt_1',
      status: 'upstream_error',
      status_code: 503,
      error_detail: 'upstream returned 503: overloaded',
      error_detail_truncated: true,
      terminal: false,
      produced_final_response: false,
    }
    const retried = { ...successAttempt, request_attempt_id: 'attempt_2', attempt_number: 2 }
    const detail = await openDetail({
      log: items[0],
      user_agent_raw: null,
      payload: null,
      attempts: [failedAttempt, retried],
    })

    expect(detail.getByText('Attempt #1 error detail')).toBeInTheDocument()
    expect(detail.getByText('upstream returned 503: overloaded')).toBeInTheDocument()
    expect(detail.queryByText('Attempt #2 error detail')).not.toBeInTheDocument()
  })

  it('hides the MCP invocations link from sessions without that page', async () => {
    routeMock.useRouteContext.mockReturnValue({
      session: regularUserSession(['api_keys', 'request_logs']),
    })
    const body = await openDetail({
      log: items[0],
      user_agent_raw: null,
      payload: null,
      attempts: [],
    })

    expect(body.getByTestId('request-log-tool-counts')).toBeInTheDocument()
    expect(body.queryByText('View MCP invocations')).not.toBeInTheDocument()
  })
})

describe('RequestLogsPage detail states', () => {
  beforeEach(resetMocks)

  it('labels decisions request-log operations explicitly', async () => {
    const decisionsItem: RequestLogView = {
      ...items[0],
      model_key: 'jev',
      resolved_model_key: 'jev',
      provider_key: 'openrouter',
      metadata: { operation: 'decisions', stream: false },
    }
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [decisionsItem], total: 1 },
    })
    getObservabilityRequestLogDetailMock.mockResolvedValue({
      data: {
        log: decisionsItem,
        user_agent_raw: null,
        payload: null,
        attempts: [],
      },
    })

    renderPage()
    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])

    // The sheet opens before the detail request resolves, so wait for the loaded body.
    const facts = within(await screen.findByTestId('request-log-facts'))
    expect(facts.getByText('Operation')).toBeInTheDocument()
    expect(facts.getByText('Decisions')).toBeInTheDocument()
  })

  it('shows the error code of a pre-provider budget rejection with no attempts', async () => {
    const budgetItem: RequestLogView = {
      ...items[0],
      status_code: 422,
      error_code: 'budget_exceeded',
      prompt_tokens: null,
      completion_tokens: null,
      total_tokens: null,
      cache_read_tokens: null,
      cost_usd_10000: null,
    }
    routeMock.useLoaderData.mockReturnValue({ data: { items: [budgetItem], total: 1 } })
    getObservabilityRequestLogDetailMock.mockResolvedValue({
      data: {
        log: budgetItem,
        user_agent_raw: null,
        payload: {
          request_json: { body: { model: 'gpt-4.1-mini' } },
          response_json: {
            body: { error: { type: 'budget_error', code: 'budget_exceeded' } },
          },
        },
        attempts: [],
      },
    })

    renderPage()
    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])

    await waitFor(() => {
      expect(screen.getByText('No provider attempts recorded')).toBeInTheDocument()
    })
    const facts = within(screen.getByTestId('request-log-facts'))
    expect(facts.getByText('Error code')).toBeInTheDocument()
    expect(facts.getByText('budget_exceeded')).toBeInTheDocument()
    const sheet = within(screen.getByTestId('request-log-detail-sheet'))
    expect(sheet.getAllByText('422').length).toBeGreaterThan(0)
    // Without usage the key metrics fall back to em dashes rather than zeroes.
    const metrics = within(sheet.getByTestId('request-log-key-metrics'))
    expect(metrics.getAllByText('—')).toHaveLength(2)
    expect(metrics.getByText('None')).toBeInTheDocument()
  })

  it('renders the summary-only no-payload state in detail', async () => {
    const summaryOnlyItem = {
      ...items[0],
      has_payload: false,
      payload_policy: {
        capture_mode: 'summary_only',
        request_max_bytes: 65536,
        response_max_bytes: 65536,
        stream_max_events: 128,
        version: 'builtin:v1',
      },
    }
    routeMock.useLoaderData.mockReturnValue({
      data: { items: [summaryOnlyItem], total: 1 },
    })
    getObservabilityRequestLogDetailMock.mockResolvedValue({
      data: {
        log: summaryOnlyItem,
        user_agent_raw: null,
        payload: null,
        attempts: [],
      },
    })

    renderPage()
    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])

    await waitFor(() => {
      expect(screen.getAllByText('No payload stored')).toHaveLength(2)
    })
  })

  it('renders an error banner when detail lookup fails', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })
    getObservabilityRequestLogDetailMock.mockRejectedValue(new Error('request log missing'))

    renderPage()

    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])

    await waitFor(() => {
      expect(screen.getByText('request log missing')).toBeInTheDocument()
    })
  })
})

describe('RequestLogsPage filter chips', () => {
  beforeEach(resetMocks)

  it('adds a tag filter chip and treats whitespace-only input as empty', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    const view = renderPage()
    const scope = within(view.container)

    fireEvent.keyDown(scope.getByRole('button', { name: /Filters/ }), {
      key: 'Enter',
    })
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Tag' }))

    const tagKeyInput = await scope.findByTestId('request-log-filter-tag-key')
    const tagValueInput = scope.getByTestId('request-log-filter-tag-value')

    fireEvent.change(tagKeyInput, { target: { value: '   ' } })
    fireEvent.change(tagValueInput, { target: { value: 'guest_checkout' } })
    fireEvent.keyDown(tagValueInput, { key: 'Enter' })

    expect(
      scope.getByText('Provide both a tag key and tag value to filter bespoke request tags.'),
    ).toBeInTheDocument()
    expect(navigateMock).not.toHaveBeenCalled()

    fireEvent.change(tagKeyInput, { target: { value: ' feature ' } })
    fireEvent.keyDown(tagKeyInput, { key: 'Enter' })

    await waitForListTransition()
    expect(navigateMock).toHaveBeenCalledWith(
      expect.objectContaining({ to: '/observability/request-logs' }),
    )
    expect(navigatedSearch(0)).toEqual(
      expect.objectContaining({ tag_key: 'feature', tag_value: 'guest_checkout' }),
    )
  })

  it('renders URL filters as removable chips', async () => {
    routeMock.useSearch.mockReturnValue({ request_id: 'req_1', q: 'alice' })
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    const view = renderPage()
    const scope = within(view.container)

    expect(scope.getByTestId('request-log-filter-request-id')).toHaveValue('req_1')
    expect(scope.getByRole('button', { name: /Filters/ })).toHaveTextContent('1')
    expect(scope.getByRole('searchbox', { name: /model or user/ })).toHaveValue('alice')

    fireEvent.click(scope.getByRole('button', { name: 'Remove Request ID filter' }))

    await waitForListTransition()
    expect(navigateMock).toHaveBeenCalled()
    const next = navigatedSearch(0, { request_id: 'req_1', q: 'alice' })
    expect(next.q).toBe('alice')
    expect(next.request_id).toBeUndefined()
  })

  it('keeps a chip edit typed while an earlier filter navigation was loading', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    const view = renderPage()
    const scope = within(view.container)

    fireEvent.keyDown(scope.getByRole('button', { name: /Filters/ }), { key: 'Enter' })
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Environment' }))
    fireEvent.change(await scope.findByTestId('request-log-filter-env'), {
      target: { value: 'staging' },
    })

    // An earlier provider filter lands while the environment value is still uncommitted.
    routeMock.useSearch.mockReturnValue({ provider_key: 'openai' })
    view.rerender(
      <TooltipProvider>
        <RequestLogsPage />
      </TooltipProvider>,
    )

    expect(scope.getByTestId('request-log-filter-provider-key')).toHaveValue('openai')
    expect(scope.getByTestId('request-log-filter-env')).toHaveValue('staging')
  })
})

describe('RequestLogsPage search', () => {
  beforeEach(resetMocks)

  it('debounces the model/user search into the q URL param', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    const view = renderPage()
    const search = within(view.container).getByPlaceholderText('Search by model or user…')

    fireEvent.change(search, { target: { value: 'gpt' } })
    expect(navigateMock).not.toHaveBeenCalled()

    await waitForListTransition()
    expect(navigateMock).toHaveBeenCalledWith(
      expect.objectContaining({ to: '/observability/request-logs', replace: true }),
    )
    expect(navigatedSearch(0).q).toBe('gpt')
  })

  it('merges the debounced search into filters committed after typing started', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    const view = renderPage()
    fireEvent.change(within(view.container).getByPlaceholderText('Search by model or user…'), {
      target: { value: 'gpt' },
    })

    await waitForListTransition()
    expect(navigateMock).toHaveBeenCalled()
    // The URL gained a provider chip before the timer fired; the search must not drop it.
    expect(navigatedSearch(0, { provider_key: 'openai' })).toEqual(
      expect.objectContaining({ provider_key: 'openai', q: 'gpt' }),
    )
  })
})
