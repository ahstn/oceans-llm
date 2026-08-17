import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { platformAdminSession, regularUserSession } from '@/test/auth-session'
import type { BatchResultsView, BatchView } from '@/types/api'

const cancelGatewayBatchMock = vi.fn()
const getBatchResultPageMock = vi.fn()
const invalidateMock = vi.fn()
const navigateMock = vi.fn()

const routeMock = {
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
  useSearch: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({
    invalidate: invalidateMock,
    navigate: navigateMock,
  }),
}))

vi.mock('@/server/admin-data.functions', () => ({
  cancelGatewayBatch: (...args: unknown[]) => cancelGatewayBatchMock(...args),
  getBatchResultPage: (...args: unknown[]) => getBatchResultPageMock(...args),
  getBatches: vi.fn(),
  getServiceAccounts: vi.fn(),
  getUsers: vi.fn(),
}))

const completedBatch: BatchView = {
  batch_id: 'batch_completed',
  status: 'completed',
  endpoint: 'responses',
  model: 'openai-fast',
  resolved_model: 'gpt-5.6-sol',
  upstream_model: 'gpt-5.6-sol',
  provider: 'openai-prod',
  route_id: 'route_openai',
  provider_batch_id: 'provider_batch_completed',
  caller: {
    api_key_id: 'api_key_alice',
    api_key_name: 'Alice Personal Key',
    user_id: 'user_alice',
    user_name: 'Alice Platform Lead',
    team_id: 'team_platform',
    service_account_id: null,
    service_account_name: null,
  },
  request_count: 2,
  completed_count: 2,
  failed_count: 0,
  cost_usd: 0.0184,
  pricing_status: 'provider_reported',
  provider_usage: { total_tokens: 2584 },
  error: null,
  created_at: '2026-08-16T09:00:00Z',
  submitted_at: '2026-08-16T09:01:00Z',
  completed_at: '2026-08-16T09:18:00Z',
  updated_at: '2026-08-16T09:18:00Z',
}

const queuedBatch: BatchView = {
  ...completedBatch,
  batch_id: 'batch_queued',
  status: 'queued',
  provider_batch_id: null,
  caller: {
    api_key_id: 'api_key_ci',
    api_key_name: 'Local CI Runner Key',
    user_id: null,
    user_name: null,
    team_id: 'team_platform',
    service_account_id: 'service_account_ci',
    service_account_name: 'Local CI Runner',
  },
  request_count: 3,
  completed_count: 0,
  cost_usd: null,
  pricing_status: 'pending',
  created_at: '2026-08-16T10:48:00Z',
  submitted_at: null,
  completed_at: null,
  updated_at: '2026-08-16T10:48:00Z',
}

const completedDetail: BatchResultsView = {
  batch: completedBatch,
  items: [
    {
      custom_id: 'retention-summary',
      status: 'succeeded',
      request: { input: 'Summarize retention signals.' },
      response: { output_text: 'Activation and repeat use are the strongest signals.' },
      error: null,
      provider_request_id: 'req_retention',
      provider_usage: { input_tokens: 920, output_tokens: 338 },
      cost_usd: 0.0089,
      completed_at: '2026-08-16T09:16:00Z',
    },
  ],
  page: 1,
  page_size: 1_000,
  total: 1,
}

describe('BatchesPage', () => {
  afterEach(() => cleanup())

  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useRouteContext.mockReset()
    routeMock.useSearch.mockReset()
    cancelGatewayBatchMock.mockReset()
    getBatchResultPageMock.mockReset()
    invalidateMock.mockReset()
    navigateMock.mockReset()
    routeMock.useSearch.mockReturnValue({ page: 1, page_size: 30 })
    routeMock.useRouteContext.mockReturnValue({ session: platformAdminSession() })
    routeMock.useLoaderData.mockReturnValue({
      batchPage: {
        items: [completedBatch, queuedBatch],
        page: 1,
        page_size: 30,
        total: 2,
      },
      users: [
        {
          id: 'user_alice',
          name: 'Alice Platform Lead',
          email: 'alice@platform.local',
        },
      ],
      serviceAccounts: [
        {
          id: 'service_account_ci',
          name: 'Local CI Runner',
        },
      ],
    })
  })

  it('renders mobile and desktop batch operations with gateway-backed filters', async () => {
    const { BatchesPage } = await import('@/routes/batches')

    render(<BatchesPage />)

    expect(screen.getByRole('heading', { level: 1, name: 'Batch requests' })).toBeInTheDocument()
    expect(screen.getByTestId('batch-mobile-list')).toBeInTheDocument()
    expect(screen.getByTestId('batch-desktop-table')).toBeInTheDocument()
    expect(screen.getAllByText('Alice Platform Lead').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Local CI Runner').length).toBeGreaterThan(0)
    expect(screen.getAllByText('2 of 2').length).toBeGreaterThan(0)
    expect(screen.getAllByText('0 of 3').length).toBeGreaterThan(0)
    expect(screen.getByLabelText('Created from')).toBeInTheDocument()
    expect(screen.getByLabelText('Created through')).toBeInTheDocument()
    expect(screen.getByTestId('batch-filter-status')).toBeInTheDocument()
    expect(screen.getByTestId('batch-filter-user')).toBeInTheDocument()
    expect(screen.getByTestId('batch-filter-service-account')).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Created from'), { target: { value: '2026-08-01' } })
    fireEvent.change(screen.getByLabelText('Created through'), { target: { value: '2026-08-16' } })
    fireEvent.click(screen.getByRole('button', { name: 'Apply filters' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/batches',
        search: {
          page: 1,
          page_size: 30,
          created_at_start: expect.any(String),
          created_at_end: expect.any(String),
        },
      })
    })
  })

  it('does not offer cross-user filters to a non-platform session', async () => {
    routeMock.useRouteContext.mockReturnValue({ session: regularUserSession() })
    const { BatchesPage } = await import('@/routes/batches')

    render(<BatchesPage />)

    expect(screen.getByLabelText('Created from')).toBeInTheDocument()
    expect(screen.getByTestId('batch-filter-status')).toBeInTheDocument()
    expect(screen.queryByTestId('batch-filter-user')).not.toBeInTheDocument()
    expect(screen.queryByTestId('batch-filter-service-account')).not.toBeInTheDocument()
  })

  it('loads normalized response details in the batch sheet', async () => {
    getBatchResultPageMock.mockResolvedValue(completedDetail)
    const { BatchesPage } = await import('@/routes/batches')

    render(<BatchesPage />)
    fireEvent.click(screen.getAllByRole('button', { name: 'View' })[0])

    await waitFor(() => {
      expect(getBatchResultPageMock).toHaveBeenCalledWith({
        data: { batchId: 'batch_completed', page: 1, pageSize: 100 },
      })
    })
    expect(await screen.findByRole('heading', { name: 'Batch responses' })).toBeInTheDocument()
    expect(screen.getByText('retention-summary')).toBeInTheDocument()
    expect(
      screen.getByText(/Activation and repeat use are the strongest signals/),
    ).toBeInTheDocument()
    expect(screen.getByText('Request payload')).toBeInTheDocument()
    expect(screen.queryByText('req_retention')).not.toBeInTheDocument()
  })

  it('loads later result pages from the gateway', async () => {
    getBatchResultPageMock
      .mockResolvedValueOnce({ ...completedDetail, page_size: 100, total: 201 })
      .mockResolvedValueOnce({
        ...completedDetail,
        items: [{ ...completedDetail.items[0], custom_id: 'page-two-result' }],
        page: 2,
        page_size: 100,
        total: 201,
      })
    const { BatchesPage } = await import('@/routes/batches')

    render(<BatchesPage />)
    fireEvent.click(screen.getAllByRole('button', { name: 'View' })[0])
    fireEvent.click(await screen.findByRole('button', { name: 'Next' }))

    await waitFor(() => {
      expect(getBatchResultPageMock).toHaveBeenLastCalledWith({
        data: { batchId: 'batch_completed', page: 2, pageSize: 100 },
      })
    })
    expect(await screen.findByText('page-two-result')).toBeInTheDocument()
  })

  it('confirms cancellation before invalidating the list', async () => {
    cancelGatewayBatchMock.mockResolvedValue({ ...queuedBatch, status: 'cancelled' })
    invalidateMock.mockResolvedValue(undefined)
    const { BatchesPage } = await import('@/routes/batches')

    render(<BatchesPage />)
    fireEvent.click(screen.getAllByRole('button', { name: 'Cancel' })[0])

    expect(screen.getByRole('heading', { name: 'Cancel this batch?' })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Cancel batch' }))

    await waitFor(() => {
      expect(cancelGatewayBatchMock).toHaveBeenCalledWith({ data: { batchId: 'batch_queued' } })
      expect(invalidateMock).toHaveBeenCalled()
    })
  })
})
