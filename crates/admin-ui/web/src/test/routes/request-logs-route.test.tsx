import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
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
    user_id: 'user_1',
    team_id: null,
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
      stream: false,
    },
    payload_policy: {
      capture_mode: 'redacted_payloads',
      request_max_bytes: 65536,
      response_max_bytes: 65536,
      stream_max_events: 128,
      version: 'builtin:v1',
    },
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
    expect(
      screen.getByText(
        'Inspect single-route request execution, latency, and sanitized payloads without dropping into raw traces.',
      ),
    ).toBeInTheDocument()
    expect(screen.getAllByText('gpt-4.1-mini')).toHaveLength(2)
    expect(screen.getAllByText('openai')).toHaveLength(2)
    expect(screen.getAllByText('req_1')).toHaveLength(2)
  })

  it('renders request-log detail without fallback-era fields', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items, total: 1 } })
    getObservabilityRequestLogDetailMock.mockResolvedValue({
      data: {
        log: items[0],
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
    expect(screen.queryByText('Attempt Count')).not.toBeInTheDocument()
    expect(screen.queryByText('Fallback')).not.toBeInTheDocument()
    expect(screen.getByText('Payload Policy')).toBeInTheDocument()
    expect(screen.getByText('Provider Attempts')).toBeInTheDocument()
    expect(screen.getByText('#1')).toBeInTheDocument()
    expect(screen.getAllByText('success').length).toBeGreaterThan(0)
    expect(screen.getAllByText('openai').length).toBeGreaterThan(0)
    expect(screen.getAllByText('gpt-4.1-mini').length).toBeGreaterThan(0)
    expect(screen.getByText('terminal')).toBeInTheDocument()
    expect(screen.getByText('final response')).toBeInTheDocument()
    expect(screen.getAllByText('redacted payloads').length).toBeGreaterThan(0)
    expect(screen.getByText(/"prompt": "ping"/)).toBeInTheDocument()
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
        payload: null,
        attempts: [],
      },
    })

    const { RequestLogsPage } = await import('@/routes/observability/request-logs')

    render(<RequestLogsPage />)
    fireEvent.click(screen.getAllByRole('button', { name: 'Inspect' })[0])

    await waitFor(() => {
      expect(screen.getByText('Payload capture state')).toBeInTheDocument()
    })

    expect(screen.getAllByText('No payload stored')).toHaveLength(2)
    expect(screen.getAllByText('summary only').length).toBeGreaterThan(0)
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
