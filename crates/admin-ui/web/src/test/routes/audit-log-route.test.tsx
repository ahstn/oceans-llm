import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AuditLogPage } from '@/routes/observability/audit-log'

const navigateMock = vi.fn()
// Hoisted with the mocks: the route module reads it as it loads.
const routeMock = vi.hoisted(() => ({
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({ navigate: navigateMock }),
}))

const getAuditLogEntry = vi.fn()

vi.mock('@/server/admin-data.functions', () => ({
  getAuditLogPage: vi.fn(),
  getAuditLogEntry: (input: unknown) => getAuditLogEntry(input),
}))

const ADMIN_ID = '22222222-2222-4222-8222-222222222222'

const entry = {
  id: '7',
  occurred_at: '2026-10-15T09:30:00Z',
  actor: { kind: 'user', user_id: ADMIN_ID, api_key_id: null, label: 'admin@example.test' },
  action: 'mcp_server.updated',
  target_type: 'mcp_server',
  target_id: '33333333-3333-4333-8333-333333333333',
  target_label: 'tickets',
  request_ip: '203.0.113.9',
  user_agent: 'Browser/1.0',
}

const detail = {
  ...entry,
  before: { display_name: 'Tickets' },
  after: { display_name: 'Ticket Desk' },
  metadata: { request_id: 'req-1' },
}

function loaderData(overrides: Record<string, unknown> = {}) {
  return {
    page: {
      items: [entry],
      next_cursor: '1800000000000_6',
      limit: 50,
      target_types: ['api_key', 'mcp_server', 'team'],
      ...overrides,
    },
  }
}

describe('AuditLogPage', () => {
  afterEach(cleanup)

  beforeEach(() => {
    navigateMock.mockReset()
    routeMock.useLoaderData.mockReturnValue(loaderData())
    routeMock.useSearch.mockReturnValue({})
    getAuditLogEntry.mockReset()
    getAuditLogEntry.mockResolvedValue({ data: detail })
  })

  function renderPage() {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={queryClient}>
        <AuditLogPage />
      </QueryClientProvider>,
    )
  }

  it('lists entries with actor, target and request address', () => {
    renderPage()

    expect(screen.getByRole('heading', { level: 1, name: 'Audit Log' })).toBeInTheDocument()
    expect(screen.getByText('tickets')).toBeInTheDocument()
    expect(screen.getByText('Updated')).toBeInTheDocument()
    expect(screen.getByText('admin@example.test')).toBeInTheDocument()
    expect(screen.getByText('203.0.113.9')).toBeInTheDocument()
    expect(screen.getByText('MCP server')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /save|delete|revoke/i })).not.toBeInTheDocument()
  })

  it('applies an action filter through route search', async () => {
    renderPage()

    fireEvent.change(screen.getByLabelText('Action'), {
      target: { value: 'api_key.created' },
    })
    fireEvent.submit(screen.getByLabelText('Action').closest('form')!)

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/audit-log',
        search: { action: 'api_key.created' },
      })
    })
  })

  it('filters by actor from a row without opening the detail sheet', async () => {
    renderPage()

    fireEvent.click(screen.getByRole('button', { name: 'admin@example.test' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/audit-log',
        search: { actor_user_id: ADMIN_ID },
      })
    })
    expect(screen.queryByTestId('audit-entry-sheet')).not.toBeInTheDocument()
  })

  it('pages older with the cursor and keeps filters', async () => {
    routeMock.useSearch.mockReturnValue({ target_type: 'mcp_server' })
    renderPage()

    fireEvent.click(screen.getByRole('button', { name: 'Older' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/audit-log',
        search: { target_type: 'mcp_server', cursor: '1800000000000_6' },
      })
    })
  })

  it('returns to the newest page and hides Older on the last page', async () => {
    routeMock.useLoaderData.mockReturnValue(loaderData({ next_cursor: null }))
    routeMock.useSearch.mockReturnValue({ action: 'team.created', cursor: '1800000000000_6' })
    renderPage()

    expect(screen.queryByRole('button', { name: 'Older' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Newest' }))

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith({
        to: '/observability/audit-log',
        search: { action: 'team.created' },
      })
    })
  })

  it('opens a contained detail sheet with before and after summaries', async () => {
    renderPage()

    fireEvent.click(screen.getByText('tickets'))

    const sheet = await screen.findByTestId('audit-entry-sheet')
    expect(within(sheet).getByText('Browser/1.0')).toBeInTheDocument()
    const after = await within(sheet).findByTestId('audit-json-after')
    expect(getAuditLogEntry).toHaveBeenCalledWith({ data: { id: '7' } })
    expect(after).toHaveTextContent('Ticket Desk')
    expect(after).toHaveClass('max-w-full', 'overflow-x-auto')
    expect(within(sheet).getByTestId('audit-json-before')).toHaveTextContent('Tickets')
  })

  it('names the API key that made a key-authenticated change', async () => {
    const keyEntry = {
      ...entry,
      id: '8',
      actor: {
        kind: 'api_key',
        user_id: ADMIN_ID,
        api_key_id: '44444444-4444-4444-8444-444444444444',
        label: 'admin@example.test',
      },
    }
    routeMock.useLoaderData.mockReturnValue(loaderData({ items: [keyEntry] }))
    renderPage()

    fireEvent.click(screen.getByText('tickets'))

    const sheet = await screen.findByTestId('audit-entry-sheet')
    expect(within(sheet).getByText('API key')).toBeInTheDocument()
    expect(within(sheet).getByText('44444444-4444-4444-8444-444444444444')).toBeInTheDocument()
  })

  it('shows an error when an entry cannot be loaded', async () => {
    getAuditLogEntry.mockRejectedValue(new Error('audit log entry `7` not found'))
    renderPage()

    fireEvent.click(screen.getByText('tickets'))

    const sheet = await screen.findByTestId('audit-entry-sheet')
    expect(await within(sheet).findByText(/not found/)).toBeInTheDocument()
  })

  it('shows an empty state when nothing matches', () => {
    routeMock.useLoaderData.mockReturnValue(loaderData({ items: [], next_cursor: null }))
    renderPage()

    expect(screen.getByText('No audit entries')).toBeInTheDocument()
  })
})
