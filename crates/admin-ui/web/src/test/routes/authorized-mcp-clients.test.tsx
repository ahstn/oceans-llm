import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { AuthorizedMcpClientView } from '@/types/api'

const invalidateMock = vi.fn()
const disconnectMock = vi.fn()
const toastMock = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }))

vi.mock('@tanstack/react-router', () => ({
  useRouter: () => ({ invalidate: invalidateMock }),
}))

vi.mock('sonner', () => ({ toast: toastMock }))

vi.mock('@/server/admin-data.functions', () => ({
  disconnectAuthorizedMcpClient: (...args: unknown[]) => disconnectMock(...args),
}))

function client(overrides: Partial<AuthorizedMcpClientView> = {}): AuthorizedMcpClientView {
  return {
    grant_id: 'grant-1',
    client_id: 'client-1',
    client_name: 'Editor',
    client_uri: null,
    resource: 'https://oceans.example.com/mcp',
    resource_kind: 'aggregate',
    server_key: null,
    scopes: [{ scope: 'mcp:aggregate', description: 'Aggregate endpoint' }],
    authorized_at: '2026-10-15T10:00:00Z',
    last_token_issued_at: '2026-10-15T11:00:00Z',
    expires_at: '2026-11-14T10:00:00Z',
    ...overrides,
  }
}

describe('AuthorizedMcpClientsCard', () => {
  beforeEach(() => {
    invalidateMock.mockReset()
    disconnectMock.mockReset()
    toastMock.success.mockReset()
    toastMock.error.mockReset()
  })

  afterEach(() => cleanup())

  it('renders nothing when OAuth sign-in is disabled', async () => {
    const { AuthorizedMcpClientsCard } = await import('@/routes/account/-authorized-mcp-clients')
    render(<AuthorizedMcpClientsCard clients={{ enabled: false, has_more: false, items: [] }} />)
    expect(screen.queryByTestId('authorized-mcp-clients')).not.toBeInTheDocument()
  })

  it('keeps clients disconnectable while OAuth sign-in is off', async () => {
    const { AuthorizedMcpClientsCard } = await import('@/routes/account/-authorized-mcp-clients')
    render(
      <AuthorizedMcpClientsCard clients={{ enabled: false, has_more: false, items: [client()] }} />,
    )
    expect(screen.getByTestId('authorized-mcp-clients-dormant')).toHaveTextContent(
      'They will work again if it is turned back on.',
    )
    expect(screen.getAllByRole('button', { name: 'Disconnect Editor' }).length).toBeGreaterThan(0)
  })

  it('shows an empty state', async () => {
    const { AuthorizedMcpClientsCard } = await import('@/routes/account/-authorized-mcp-clients')
    render(<AuthorizedMcpClientsCard clients={{ enabled: true, has_more: false, items: [] }} />)
    expect(screen.getByText('No MCP clients are authorized.')).toBeInTheDocument()
  })

  it('lists clients with their access and keeps the table contained', async () => {
    const { AuthorizedMcpClientsCard } = await import('@/routes/account/-authorized-mcp-clients')
    render(
      <AuthorizedMcpClientsCard
        clients={{
          enabled: true,
          has_more: false,
          items: [
            client(),
            client({
              grant_id: 'grant-2',
              client_id: 'client-2',
              client_name: null,
              resource_kind: 'server',
              server_key: 'github',
            }),
          ],
        }}
      />,
    )
    const card = screen.getByTestId('authorized-mcp-clients')
    expect(within(card).getAllByText('Editor').length).toBeGreaterThan(0)
    expect(within(card).getAllByText('Unnamed MCP client').length).toBeGreaterThan(0)
    expect(within(card).getAllByText('All your MCP servers').length).toBeGreaterThan(0)
    expect(within(card).getAllByText('MCP server github').length).toBeGreaterThan(0)
    expect(card.querySelector('[data-slot="card-content"]')).toHaveClass('min-w-0')
    expect(screen.getByTestId('authorized-mcp-clients-table')).toHaveClass(
      'max-w-full',
      'overflow-x-auto',
    )
  })

  it('says when older clients are not listed', async () => {
    const { AuthorizedMcpClientsCard } = await import('@/routes/account/-authorized-mcp-clients')
    render(
      <AuthorizedMcpClientsCard clients={{ enabled: true, has_more: true, items: [client()] }} />,
    )
    expect(screen.getByTestId('authorized-mcp-clients-more')).toHaveTextContent(
      'Showing your 1 most recent clients.',
    )
  })

  it('disconnects a client and refreshes the list', async () => {
    disconnectMock.mockResolvedValue(undefined)
    const { AuthorizedMcpClientsCard } = await import('@/routes/account/-authorized-mcp-clients')
    render(
      <AuthorizedMcpClientsCard clients={{ enabled: true, has_more: false, items: [client()] }} />,
    )

    fireEvent.click(screen.getAllByRole('button', { name: 'Disconnect Editor' })[0])

    await waitFor(() =>
      expect(disconnectMock).toHaveBeenCalledWith({
        data: { grantId: 'grant-1' },
      }),
    )
    await waitFor(() => expect(invalidateMock).toHaveBeenCalled())
    expect(toastMock.success).toHaveBeenCalledWith('Editor disconnected')
  })

  it('keeps each overlapping disconnect pending until its own request settles', async () => {
    const settle: Record<string, () => void> = {}
    disconnectMock.mockImplementation(
      ({ data }: { data: { grantId: string } }) =>
        new Promise<void>((resolve) => {
          settle[data.grantId] = resolve
        }),
    )
    const { AuthorizedMcpClientsCard } = await import('@/routes/account/-authorized-mcp-clients')
    render(
      <AuthorizedMcpClientsCard
        clients={{
          enabled: true,
          has_more: false,
          items: [client(), client({ grant_id: 'grant-2', client_name: 'Notebook' })],
        }}
      />,
    )
    const button = (name: string) => screen.getAllByRole('button', { name })[0]

    fireEvent.click(button('Disconnect Editor'))
    fireEvent.click(button('Disconnect Notebook'))
    await waitFor(() => expect(disconnectMock).toHaveBeenCalledTimes(2))
    expect(button('Disconnect Editor')).toBeDisabled()
    expect(button('Disconnect Notebook')).toBeDisabled()

    settle['grant-2']()
    await waitFor(() => expect(button('Disconnect Notebook')).not.toBeDisabled())
    expect(button('Disconnect Editor')).toBeDisabled()
    settle['grant-1']()
    await waitFor(() => expect(button('Disconnect Editor')).not.toBeDisabled())
  })

  it('reports a failed disconnect', async () => {
    disconnectMock.mockRejectedValue(new Error('authorized MCP client was not found'))
    const { AuthorizedMcpClientsCard } = await import('@/routes/account/-authorized-mcp-clients')
    render(
      <AuthorizedMcpClientsCard clients={{ enabled: true, has_more: false, items: [client()] }} />,
    )

    fireEvent.click(screen.getAllByRole('button', { name: 'Disconnect Editor' })[0])

    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith('authorized MCP client was not found'),
    )
    expect(invalidateMock).not.toHaveBeenCalled()
  })
})
