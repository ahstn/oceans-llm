import { fireEvent, render, screen, waitFor } from '@testing-library/react'
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
    requestLogId: 'reqlog_1',
    requestId: 'req_1',
    apiKeyId: 'api_key_1',
    userId: 'user_1',
    teamId: null,
    modelKey: 'gpt-4.1-mini',
    resolvedModelKey: 'gpt-4.1-mini',
    providerKey: 'openai',
    statusCode: 200,
    latencyMs: 482,
    promptTokens: 400,
    completionTokens: 942,
    totalTokens: 1342,
    errorCode: null,
    hasPayload: true,
    requestPayloadTruncated: false,
    responsePayloadTruncated: false,
    requestTags: {
      service: 'checkout',
      component: 'pricing_api',
      env: 'prod',
      bespoke: [{ key: 'feature', value: 'guest_checkout' }],
    },
    metadata: {
      stream: false,
      fallback_used: false,
      attempt_count: 1,
    },
    occurredAt: '2026-03-10T11:32:00Z',
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
    expect(screen.getAllByText('gpt-4.1-mini')).toHaveLength(2)
    expect(screen.getAllByText('openai')).toHaveLength(2)
    expect(screen.getAllByText('req_1')).toHaveLength(2)
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

    render(<RequestLogsPage />)

    fireEvent.change(screen.getByTestId('request-log-filter-tag-key'), {
      target: { value: '   ' },
    })
    fireEvent.change(screen.getByTestId('request-log-filter-tag-value'), {
      target: { value: 'guest_checkout' },
    })

    expect(screen.getByRole('button', { name: 'Apply Filters' })).toBeDisabled()
    expect(
      screen.getByText('Provide both a tag key and tag value to filter bespoke request tags.'),
    ).toBeInTheDocument()

    fireEvent.change(screen.getByTestId('request-log-filter-tag-key'), {
      target: { value: ' feature ' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Apply Filters' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/request-logs',
        search: {
          tagKey: 'feature',
          tagValue: 'guest_checkout',
        },
      })
    })
  })
})
