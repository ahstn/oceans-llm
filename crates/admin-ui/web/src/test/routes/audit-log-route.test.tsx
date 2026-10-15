import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const navigateMock = vi.fn()
const routeMock = {
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => ({ navigate: navigateMock }),
}))

vi.mock('@/server/admin-data.functions', () => ({
  getAuditLogPage: vi.fn(),
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
  before: { display_name: 'Tickets' },
  after: { display_name: 'Ticket Desk' },
  metadata: { request_id: 'req-1' },
  request_ip: '203.0.113.9',
  user_agent: 'Browser/1.0',
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
  })

  async function renderPage() {
    const { AuditLogPage } = await import('@/routes/observability/audit-log')
    render(<AuditLogPage />)
  }

  it('lists entries with actor, target and request address', async () => {
    await renderPage()

    expect(screen.getByRole('heading', { level: 1, name: 'Audit Log' })).toBeInTheDocument()
    expect(screen.getByText('tickets')).toBeInTheDocument()
    expect(screen.getByText('Updated')).toBeInTheDocument()
    expect(screen.getByText('admin@example.test')).toBeInTheDocument()
    expect(screen.getByText('203.0.113.9')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /save|delete|revoke/i })).not.toBeInTheDocument()
  })

  it('applies an action filter through route search', async () => {
    await renderPage()

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
    await renderPage()

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
    await renderPage()

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
    await renderPage()

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
    await renderPage()

    fireEvent.click(screen.getByText('tickets'))

    const sheet = await screen.findByTestId('audit-entry-sheet')
    expect(within(sheet).getByText('Browser/1.0')).toBeInTheDocument()
    const after = within(sheet).getByTestId('audit-json-after')
    expect(after).toHaveTextContent('Ticket Desk')
    expect(after).toHaveClass('max-w-full', 'overflow-x-auto')
    expect(within(sheet).getByTestId('audit-json-before')).toHaveTextContent('Tickets')
  })

  it('shows an empty state when nothing matches', async () => {
    routeMock.useLoaderData.mockReturnValue(loaderData({ items: [], next_cursor: null }))
    await renderPage()

    expect(screen.getByText('No audit entries')).toBeInTheDocument()
  })
})
