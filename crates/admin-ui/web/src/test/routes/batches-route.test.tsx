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

vi.mock('@/components/reui/filters', () => ({
  createFilter: (field: string, operator: string, values: unknown[]) => ({
    id: `${field}-${operator}`,
    field,
    operator,
    values,
  }),
  Filters: ({
    fields,
    onChange,
  }: {
    fields: Array<{ label: string }>
    onChange: (filters: unknown[]) => void
  }) => (
    <div>
      <span data-testid="batch-filter-fields">{fields.map((field) => field.label).join(',')}</span>
      <button
        type="button"
        onClick={() =>
          onChange([
            {
              id: 'status-is',
              field: 'status',
              operator: 'is',
              values: ['completed'],
            },
          ])
        }
      >
        Apply completed filter
      </button>
    </div>
  ),
}))

vi.mock('@/components/reui/date-selector', () => ({
  DateSelector: () => null,
  formatDateValue: () => 'Selected dates',
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
    expect(screen.getByTestId('batch-filter-fields')).toHaveTextContent(
      'Created,User,Service account,Status',
    )

    fireEvent.click(screen.getByRole('button', { name: 'Apply completed filter' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/batches',
        search: { page: 1, page_size: 30, status: 'completed' },
      })
    })
  })

  it('does not offer cross-user filters to a non-platform session', async () => {
    routeMock.useRouteContext.mockReturnValue({ session: regularUserSession() })
    const { BatchesPage } = await import('@/routes/batches')

    render(<BatchesPage />)

    expect(screen.getByTestId('batch-filter-fields')).toHaveTextContent('Created,Status')
  })

  it('loads normalized response details in the batch sheet', async () => {
    getBatchResultPageMock.mockResolvedValue(completedDetail)
    const { BatchesPage } = await import('@/routes/batches')

    render(<BatchesPage />)
    fireEvent.click(screen.getAllByRole('button', { name: 'View' })[0])

    await waitFor(() => {
      expect(getBatchResultPageMock).toHaveBeenCalledWith({
        data: { batchId: 'batch_completed' },
      })
    })
    expect(await screen.findByRole('heading', { name: 'Batch responses' })).toBeInTheDocument()
    expect(screen.getByText('retention-summary')).toBeInTheDocument()
    expect(
      screen.getByText(/Activation and repeat use are the strongest signals/),
    ).toBeInTheDocument()
    expect(screen.getByText('Request payload')).toBeInTheDocument()
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
