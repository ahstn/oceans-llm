import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { RequestLogView } from '@/types/api'

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

const items: RequestLogView[] = [
  {
    id: 'req_1',
    model: 'gpt-4.1-mini',
    provider: 'openai',
    statusCode: 200,
    latencyMs: 482,
    tokens: 1342,
    timestamp: '2026-03-10T11:32:00Z',
  },
]

describe('RequestLogsPage', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
  })

  it('renders dedicated mobile and desktop log layouts from the same payload', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: { items } })

    const { RequestLogsPage } = await import('@/routes/observability/request-logs')

    render(<RequestLogsPage />)

    expect(screen.getByTestId('request-log-mobile-list')).toBeInTheDocument()
    expect(screen.getByTestId('request-log-desktop-table')).toBeInTheDocument()
    expect(screen.getAllByText('gpt-4.1-mini')).toHaveLength(2)
    expect(screen.getAllByText('openai')).toHaveLength(2)
  })
})
