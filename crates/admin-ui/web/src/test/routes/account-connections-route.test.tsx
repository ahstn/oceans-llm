import { cleanup, render, waitFor } from '@testing-library/react'
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
}))

function connection(overrides: Partial<McpOauthConnectionView> = {}): McpOauthConnectionView {
  return {
    server_id: 'server-1',
    server_key: 'google_drive',
    display_name: 'Google Drive',
    provider_key: 'google',
    status: 'disconnected',
    required_scopes: ['https://www.googleapis.com/auth/drive.readonly'],
    granted_scopes: [],
    expires_at: null,
    availability_error: null,
    ...overrides,
  }
}

describe('ConnectionsPage connect links', () => {
  const assignMock = vi.fn()

  beforeEach(() => {
    navigateMock.mockReset()
    connectMock.mockReset()
    assignMock.mockReset()
    Object.values(toastMock).forEach((mock) => mock.mockReset())
    vi.stubGlobal('location', { assign: assignMock })
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('starts the OAuth flow for the named server and drops the parameter', async () => {
    routeMock.useLoaderData.mockReturnValue([connection()])
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
    routeMock.useLoaderData.mockReturnValue([connection({ status: 'expired' })])
    routeMock.useSearch.mockReturnValue({ connect: 'google_drive' })
    connectMock.mockResolvedValue({ authorization_url: 'https://accounts.example/consent' })
    const { ConnectionsPage } = await import('@/routes/account/connections')

    render(<ConnectionsPage />)

    await waitFor(() => expect(connectMock).toHaveBeenCalledTimes(1))
  })

  it('does not start the flow when the server is already connected', async () => {
    routeMock.useLoaderData.mockReturnValue([connection({ status: 'connected' })])
    routeMock.useSearch.mockReturnValue({ connect: 'google_drive' })
    const { ConnectionsPage } = await import('@/routes/account/connections')

    render(<ConnectionsPage />)

    await waitFor(() =>
      expect(toastMock.info).toHaveBeenCalledWith('Google Drive is already connected'),
    )
    expect(connectMock).not.toHaveBeenCalled()
  })

  it('reports unknown and unavailable servers instead of starting the flow', async () => {
    routeMock.useLoaderData.mockReturnValue([
      connection({ availability_error: 'OAuth provider `google` is not configured' }),
    ])
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

  it('names the server the OAuth callback returned from', async () => {
    routeMock.useLoaderData.mockReturnValue([connection({ status: 'connected' })])
    routeMock.useSearch.mockReturnValue({ oauth: 'connected', server: 'server-1' })
    const { ConnectionsPage } = await import('@/routes/account/connections')

    render(<ConnectionsPage />)

    await waitFor(() => expect(toastMock.success).toHaveBeenCalledWith('Google Drive connected'))
    expect(navigateMock).not.toHaveBeenCalled()
  })
})
