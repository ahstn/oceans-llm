import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { TooltipProvider } from '@/components/ui/tooltip'
import { regularUserSession } from '@/test/auth-session'
import type { RequestLogView } from '@/types/api'

const getObservabilityRequestLogDetailMock = vi.fn()
const navigateMock = vi.fn()

const routeMock = {
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
  useSearch: vi.fn(),
}

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

async function renderPage() {
  const { RequestLogsPage } = await import('@/routes/observability/request-logs')
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

describe('RequestLogsPage table', () => {
  beforeEach(resetMocks)

  it('renders dedicated mobile and desktop log layouts from the same payload', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    await renderPage()

    expect(screen.getByTestId('request-log-mobile-list')).toBeInTheDocument()
    expect(screen.getByTestId('request-log-desktop-table')).toBeInTheDocument()
    // 12 rows × 56 px (requestLogDesktopPreviewRows × requestLogRowEstimatePx)
    expect(screen.getByTestId('request-log-desktop-table-viewport')).toHaveStyle({
      height: '672px',
    })
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
    expect(screen.getAllByText('2026-03-10 11:32:00')).toHaveLength(2)
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

  it('renders service-account and unknown callers with sensible fallbacks', async () => {
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

    await renderPage()

    // The mocked virtualizer renders only the first row in the desktop table,
    // so the second item is asserted via the mobile list alone.
    expect(screen.getAllByText('Batch Jobs Account')).toHaveLength(2)
    expect(screen.getAllByText('service account').length).toBeGreaterThan(0)
    expect(screen.queryByText('Nightly Rollup Key')).not.toBeInTheDocument()
    expect(screen.getAllByText('Unknown')).toHaveLength(1)
  })

  it('shows a cache-hit indicator with cached token share and n/a fallbacks', async () => {
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

    const view = await renderPage()
    const scope = within(view.container)

    // Only the cached row (mobile + desktop) renders the indicator.
    const cacheIcons = scope.getAllByTestId('request-log-cache-hit')
    expect(cacheIcons).toHaveLength(2)
    expect(scope.getAllByText('—')).toHaveLength(1)
    expect(scope.getAllByText('0 / n/a')).toHaveLength(1)

    fireEvent.focus(cacheIcons[0])

    expect(
      (await screen.findAllByText('300 of 400 prompt tokens cached (75%)')).length,
    ).toBeGreaterThan(0)
  })
})

describe('RequestLogsPage detail', () => {
  beforeEach(resetMocks)

  it('renders request-log detail without fallback-era fields', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })
    getObservabilityRequestLogDetailMock.mockResolvedValue({
      data: {
        log: items[0],
        user_agent_raw: 'opencode/1.2.3',
        payload: {
          request_json: { body: { prompt: 'ping' } },
          response_json: { body: { output: 'pong' } },
        },
        attempts: [
          {
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
          },
        ],
      },
    })

    await renderPage()
    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])

    await waitFor(() => {
      expect(screen.getByText('Request Log Detail')).toBeInTheDocument()
    })

    expect(
      screen.getByText('Review summary fields and sanitized request and response payloads.'),
    ).toBeInTheDocument()
    const dialog = screen.getByRole('dialog')
    expect(within(dialog).getByText('Operation')).toBeInTheDocument()
    expect(within(dialog).getByText('Chat Completions')).toBeInTheDocument()
    expect(within(dialog).getByText('API Key')).toBeInTheDocument()
    expect(within(dialog).getByText('Checkout Service Key')).toBeInTheDocument()
    expect(within(dialog).getByText('Caller')).toBeInTheDocument()
    expect(within(dialog).getByText('Alice Example · alice@example.com')).toBeInTheDocument()
    expect(within(dialog).getByText('service:checkout')).toBeInTheDocument()
    expect(within(dialog).getByText('feature:guest_checkout')).toBeInTheDocument()
    expect(screen.queryByText('Attempt Count')).not.toBeInTheDocument()
    expect(screen.queryByText('Fallback')).not.toBeInTheDocument()
    expect(screen.getByText('MCP & Tools')).toBeInTheDocument()
    expect(screen.getByText('Agent Harness')).toBeInTheDocument()
    expect(screen.getByText('Opencode')).toBeInTheDocument()
    expect(screen.getByText('opencode/1.2.3')).toBeInTheDocument()
    expect(screen.getByText('Tools Called')).toBeInTheDocument()
    expect(within(dialog).getByText('Request Tools')).toBeInTheDocument()
    expect(within(dialog).getByText('req_1')).toBeInTheDocument()
    expect(within(dialog).getByText('300 of 400 prompt tokens cached (75%)')).toBeInTheDocument()
    expect(screen.getAllByText('0').length).toBeGreaterThan(0)
    expect(screen.getAllByText('n/a').length).toBeGreaterThan(0)
    expect(screen.getByText('Provider Attempts')).toBeInTheDocument()
    expect(screen.getByText('#1')).toBeInTheDocument()
    expect(screen.getAllByText('success').length).toBeGreaterThan(0)
    expect(screen.getAllByText('openai').length).toBeGreaterThan(0)
    expect(screen.getAllByText('gpt-4.1-mini').length).toBeGreaterThan(0)
    expect(screen.getByText('terminal')).toBeInTheDocument()
    expect(screen.getByText('final response')).toBeInTheDocument()
    expect(screen.queryByText('Payload Policy')).not.toBeInTheDocument()
    expect(screen.getByText(/"prompt": "ping"/)).toBeInTheDocument()
    expect(screen.getByText(/"output": "pong"/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('radio', { name: 'Request' }))

    expect(screen.getByText(/"prompt": "ping"/)).toBeInTheDocument()
    expect(screen.queryByText(/"output": "pong"/)).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('radio', { name: 'Response' }))

    expect(screen.queryByText(/"prompt": "ping"/)).not.toBeInTheDocument()
    expect(screen.getByText(/"output": "pong"/)).toBeInTheDocument()
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

    await renderPage()
    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])

    await waitFor(() => {
      expect(screen.getAllByRole('dialog').length).toBeGreaterThan(0)
    })

    const dialogs = screen.getAllByRole('dialog')
    const dialog = dialogs[dialogs.length - 1]
    expect(within(dialog).getByText('Operation')).toBeInTheDocument()
    expect(within(dialog).getByText('Decisions')).toBeInTheDocument()
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

    await renderPage()
    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])

    await waitFor(() => {
      expect(screen.getAllByText('No payload stored')).toHaveLength(2)
    })
  })

  it('renders an error banner when detail lookup fails', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })
    getObservabilityRequestLogDetailMock.mockRejectedValue(new Error('request log missing'))

    await renderPage()

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

    const view = await renderPage()
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

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith(
        expect.objectContaining({ to: '/observability/request-logs' }),
      )
    })
    expect(navigatedSearch(0)).toEqual(
      expect.objectContaining({ tag_key: 'feature', tag_value: 'guest_checkout' }),
    )
  })

  it('renders URL filters as removable chips', async () => {
    routeMock.useSearch.mockReturnValue({ request_id: 'req_1', q: 'alice' })
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    const view = await renderPage()
    const scope = within(view.container)

    expect(scope.getByTestId('request-log-filter-request-id')).toHaveValue('req_1')
    expect(scope.getByRole('button', { name: /Filters/ })).toHaveTextContent('1')
    expect(scope.getByRole('searchbox', { name: /model or user/ })).toHaveValue('alice')

    fireEvent.click(scope.getByRole('button', { name: 'Remove Request ID filter' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalled()
    })
    const next = navigatedSearch(0, { request_id: 'req_1', q: 'alice' })
    expect(next.q).toBe('alice')
    expect(next.request_id).toBeUndefined()
  })

  it('keeps a chip edit typed while an earlier filter navigation was loading', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    const view = await renderPage()
    const scope = within(view.container)

    fireEvent.keyDown(scope.getByRole('button', { name: /Filters/ }), { key: 'Enter' })
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Environment' }))
    fireEvent.change(await scope.findByTestId('request-log-filter-env'), {
      target: { value: 'staging' },
    })

    // An earlier provider filter lands while the environment value is still uncommitted.
    routeMock.useSearch.mockReturnValue({ provider_key: 'openai' })
    const { RequestLogsPage } = await import('@/routes/observability/request-logs')
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

    const view = await renderPage()
    const search = within(view.container).getByPlaceholderText('Search by model or user…')

    fireEvent.change(search, { target: { value: 'gpt' } })
    expect(navigateMock).not.toHaveBeenCalled()

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith(
        expect.objectContaining({ to: '/observability/request-logs', replace: true }),
      )
    })
    expect(navigatedSearch(0).q).toBe('gpt')
  })

  it('merges the debounced search into filters committed after typing started', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    const view = await renderPage()
    fireEvent.change(within(view.container).getByPlaceholderText('Search by model or user…'), {
      target: { value: 'gpt' },
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalled()
    })
    // The URL gained a provider chip before the timer fired; the search must not drop it.
    expect(navigatedSearch(0, { provider_key: 'openai' })).toEqual(
      expect.objectContaining({ provider_key: 'openai', q: 'gpt' }),
    )
  })
})
