import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { RequestLogView } from '@/types/api'

const getObservabilityRequestLogDetailMock = vi.fn()
const navigateMock = vi.fn()

const routeMock = {
  useLoaderData: vi.fn(),
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
      invoked_tool_count: 0,
      filtered_tool_count: null,
    },
    agent_harness_key: 'opencode',
    agent_harness_label: 'Opencode',
    occurred_at: '2026-03-10T11:32:00Z',
  },
]

describe('RequestLogsPage', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useSearch.mockReset()
    getObservabilityRequestLogDetailMock.mockReset()
    navigateMock.mockReset()
    routeMock.useSearch.mockReturnValue({})
  })

  it('renders dedicated mobile and desktop log layouts from the same payload', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    const { RequestLogsPage } = await import('@/routes/observability/request-logs')

    render(<RequestLogsPage />)

    expect(screen.getByTestId('request-log-mobile-list')).toBeInTheDocument()
    expect(screen.getByTestId('request-log-desktop-table')).toBeInTheDocument()
    expect(screen.getByTestId('request-log-desktop-table-viewport')).toHaveStyle({
      height: '672px',
    })
    expect(
      screen.getByText(
        'Inspect single-route request execution, latency, and sanitized payloads without dropping into raw traces.',
      ),
    ).toBeInTheDocument()
    expect(screen.getAllByText('gpt-4.1-mini')).toHaveLength(2)
    expect(screen.getAllByText('openai')).toHaveLength(2)
    expect(screen.getAllByText('req_1')).toHaveLength(2)
    expect(screen.getAllByText(/exposed 2/)).toHaveLength(2)
    expect(screen.getAllByText(/called 0/)).toHaveLength(2)
    expect(screen.getAllByText(/MCP n\/a/)).toHaveLength(2)
    expect(screen.getAllByText('Checkout Service Key')).toHaveLength(2)
    expect(screen.getAllByText('Alice Example')).toHaveLength(2)
    expect(screen.getByText('alice@example.com')).toBeInTheDocument()
    expect(screen.getAllByText('2026-03-10 11:32')).toHaveLength(2)
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

    const { RequestLogsPage } = await import('@/routes/observability/request-logs')

    render(<RequestLogsPage />)

    // The mocked virtualizer renders only the first row in the desktop table,
    // so the second item is asserted via the mobile list alone.
    expect(screen.getAllByText('Batch Jobs Account')).toHaveLength(2)
    expect(screen.getAllByText('service account').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Nightly Rollup Key')).toHaveLength(2)
    expect(screen.getAllByText('Unknown')).toHaveLength(1)
    // Keys without a resolved name fall back to the raw api key id.
    expect(screen.getAllByText('api_key_1')).toHaveLength(1)
  })

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

    const { RequestLogsPage } = await import('@/routes/observability/request-logs')

    render(<RequestLogsPage />)
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
    routeMock.useLoaderData.mockReturnValue({ data: { items: [summaryOnlyItem], total: 1 } })
    getObservabilityRequestLogDetailMock.mockResolvedValue({
      data: {
        log: summaryOnlyItem,
        user_agent_raw: null,
        payload: null,
        attempts: [],
      },
    })

    const { RequestLogsPage } = await import('@/routes/observability/request-logs')

    render(<RequestLogsPage />)
    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])

    await waitFor(() => {
      expect(screen.getAllByText('No payload stored')).toHaveLength(2)
    })
  })

  it('renders an error banner when detail lookup fails', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })
    getObservabilityRequestLogDetailMock.mockRejectedValue(new Error('request log missing'))

    const { RequestLogsPage } = await import('@/routes/observability/request-logs')

    render(<RequestLogsPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])

    await waitFor(() => {
      expect(screen.getByText('request log missing')).toBeInTheDocument()
    })
  })

  it('treats whitespace-only tag input as empty when gating filters', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })

    const { RequestLogsPage } = await import('@/routes/observability/request-logs')

    const view = render(<RequestLogsPage />)
    const scope = within(view.container)

    const tagKeyInput = scope.getByTestId('request-log-filter-tag-key')
    const tagValueInput = scope.getByTestId('request-log-filter-tag-value')

    fireEvent.change(tagKeyInput, { target: { value: '   ' } })
    fireEvent.change(tagValueInput, { target: { value: 'guest_checkout' } })

    expect(scope.getByRole('button', { name: 'Apply Filters' })).toBeDisabled()
    expect(
      scope.getByText('Provide both a tag key and tag value to filter bespoke request tags.'),
    ).toBeInTheDocument()

    fireEvent.change(tagKeyInput, { target: { value: ' feature ' } })
    fireEvent.click(scope.getByRole('button', { name: 'Apply Filters' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/request-logs',
        search: {
          tag_key: 'feature',
          tag_value: 'guest_checkout',
        },
      })
    })
  })
})
