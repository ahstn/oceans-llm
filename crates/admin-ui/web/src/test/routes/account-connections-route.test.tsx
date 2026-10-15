import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { McpOauthConnectionView } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
  useSearch: vi.fn(),
}
const navigateMock = vi.fn()
const connectMock = vi.fn()
const toastMock = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn(), info: vi.fn() }))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useNavigate: () => navigateMock,
  useRouter: () => ({ invalidate: vi.fn() }),
}))

vi.mock('sonner', () => ({ toast: toastMock }))

vi.mock('@/server/admin-data.functions', () => ({
  connectMcpOauthServer: (...args: unknown[]) => connectMock(...args),
  disconnectMcpOauthServer: vi.fn(),
  getMcpOauthConnections: vi.fn(),
  getAuthorizedMcpClients: vi.fn(),
  disconnectAuthorizedMcpClient: vi.fn(),
}))

function loaderData(connections: McpOauthConnectionView[]) {
  return { connections, authorizedClients: { enabled: false, items: [] } }
}

function connection(overrides: Partial<McpOauthConnectionView> = {}): McpOauthConnectionView {
  return {
    server_id: 'server-1',
    server_key: 'google_drive',
    display_name: 'Google Drive',
    provider_key: 'google',
    provider_label: 'google',
    status: 'disconnected',
    required_scopes: ['https://www.googleapis.com/auth/drive.readonly'],
    granted_scopes: [],
    expires_at: null,
    availability_error: null,
    ...overrides,
  }
}

const assignMock = vi.fn()

function resetPageMocks() {
  navigateMock.mockReset()
  connectMock.mockReset()
  assignMock.mockReset()
  Object.values(toastMock).forEach((mock) => mock.mockReset())
  vi.stubGlobal('location', { assign: assignMock })
}

function restorePageMocks() {
  cleanup()
  vi.unstubAllGlobals()
}

describe('ConnectionsPage connect links', () => {
  beforeEach(resetPageMocks)
  afterEach(restorePageMocks)

  it('starts the OAuth flow for the named server and drops the parameter', async () => {
    routeMock.useLoaderData.mockReturnValue(loaderData([connection()]))
    routeMock.useSearch.mockReturnValue({ connect: 'google_drive' })
    connectMock.mockResolvedValue({ authorization_url: 'https://accounts.example/consent' })
    const { ConnectionsPage } = await import('@/routes/account/connections')

    render(<ConnectionsPage />)

    await waitFor(() =>
      expect(connectMock).toHaveBeenCalledWith({ data: { serverId: 'server-1' } }),
    )
    await waitFor(() => expect(assignMock).toHaveBeenCalledWith('https://accounts.example/consent'))
    expect(navigateMock).toHaveBeenCalledWith(expect.objectContaining({ replace: true }))
    const { search } = navigateMock.mock.calls[0][0]
    expect(search({ connect: 'google_drive', oauth: undefined })).toEqual({
      connect: undefined,
      oauth: undefined,
    })
  })

  it('reconnects an expired connection', async () => {
    routeMock.useLoaderData.mockReturnValue(loaderData([connection({ status: 'expired' })]))
    routeMock.useSearch.mockReturnValue({ connect: 'google_drive' })
    connectMock.mockResolvedValue({ authorization_url: 'https://accounts.example/consent' })
    const { ConnectionsPage } = await import('@/routes/account/connections')

    render(<ConnectionsPage />)

    await waitFor(() => expect(connectMock).toHaveBeenCalledTimes(1))
  })

  it('does not start the flow when the server is already connected', async () => {
    routeMock.useLoaderData.mockReturnValue(loaderData([connection({ status: 'connected' })]))
    routeMock.useSearch.mockReturnValue({ connect: 'google_drive' })
    const { ConnectionsPage } = await import('@/routes/account/connections')

    render(<ConnectionsPage />)

    await waitFor(() =>
      expect(toastMock.info).toHaveBeenCalledWith('Google Drive is already connected'),
    )
    expect(connectMock).not.toHaveBeenCalled()
  })

  it('reports unknown and unavailable servers instead of starting the flow', async () => {
    routeMock.useLoaderData.mockReturnValue(
      loaderData([connection({ availability_error: 'OAuth provider `google` is not configured' })]),
    )
    routeMock.useSearch.mockReturnValue({ connect: 'google_drive' })
    const { ConnectionsPage } = await import('@/routes/account/connections')

    const { unmount } = render(<ConnectionsPage />)
    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith('OAuth provider `google` is not configured'),
    )
    unmount()

    routeMock.useSearch.mockReturnValue({ connect: 'unknown_server' })
    render(<ConnectionsPage />)
    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith(
        'No workspace connection is available for MCP server unknown_server.',
      ),
    )
    expect(connectMock).not.toHaveBeenCalled()
  })

  it('handles a second link to the same server after the first was consumed', async () => {
    routeMock.useLoaderData.mockReturnValue(loaderData([connection()]))
    routeMock.useSearch.mockReturnValue({ connect: 'google_drive' })
    connectMock.mockRejectedValue(new Error('consent failed'))
    const { ConnectionsPage } = await import('@/routes/account/connections')

    const { rerender } = render(<ConnectionsPage />)
    await waitFor(() => expect(connectMock).toHaveBeenCalledTimes(1))

    routeMock.useSearch.mockReturnValue({})
    rerender(<ConnectionsPage />)
    routeMock.useSearch.mockReturnValue({ connect: 'google_drive' })
    rerender(<ConnectionsPage />)

    await waitFor(() => expect(connectMock).toHaveBeenCalledTimes(2))
  })
})

describe('ConnectionsPage callbacks and discovered servers', () => {
  beforeEach(resetPageMocks)
  afterEach(restorePageMocks)

  it('names the server the OAuth callback returned from and toasts once', async () => {
    routeMock.useLoaderData.mockReturnValue(loaderData([connection({ status: 'connected' })]))
    routeMock.useSearch.mockReturnValue({ oauth: 'connected', server: 'server-1' })
    const { ConnectionsPage } = await import('@/routes/account/connections')

    const { rerender } = render(<ConnectionsPage />)

    await waitFor(() => expect(toastMock.success).toHaveBeenCalledWith('Google Drive connected'))
    expect(navigateMock).toHaveBeenCalledWith(expect.objectContaining({ replace: true }))
    const { search } = navigateMock.mock.calls[0][0]
    expect(search({ oauth: 'connected', server: 'server-1' })).toEqual({
      oauth: undefined,
      oauth_error: undefined,
      server: undefined,
    })

    // A loader refresh, such as the one after a disconnect, must not repeat the toast.
    routeMock.useSearch.mockReturnValue({})
    routeMock.useLoaderData.mockReturnValue(loaderData([connection({ status: 'disconnected' })]))
    rerender(<ConnectionsPage />)
    expect(toastMock.success).toHaveBeenCalledTimes(1)
  })

  it('shows servers that use a discovered authorization server', async () => {
    routeMock.useLoaderData.mockReturnValue(
      loaderData([
        connection({
          server_id: 'server-2',
          server_key: 'linear',
          display_name: 'Linear',
          provider_key: 'mcp_server',
          provider_label: 'auth.linear.example',
          required_scopes: [],
        }),
      ]),
    )
    routeMock.useSearch.mockReturnValue({ connect: 'linear' })
    connectMock.mockResolvedValue({ authorization_url: 'https://auth.linear.example/authorize' })
    const { ConnectionsPage } = await import('@/routes/account/connections')

    render(<ConnectionsPage />)

    expect(screen.getByText('linear · via auth.linear.example')).toBeTruthy()
    expect(screen.getByText('Provider default')).toBeTruthy()
    await waitFor(() =>
      expect(connectMock).toHaveBeenCalledWith({ data: { serverId: 'server-2' } }),
    )
    await waitFor(() =>
      expect(assignMock).toHaveBeenCalledWith('https://auth.linear.example/authorize'),
    )
  })
})
