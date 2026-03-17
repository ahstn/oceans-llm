import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { RequestLogView } from '@/types/api'

const getObservabilityRequestLogDetailMock = vi.fn()

const routeMock = {
  useLoaderData: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
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
    getObservabilityRequestLogDetailMock.mockReset()
  })

  it('renders dedicated mobile and desktop log layouts from the same payload', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items } })

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
})
