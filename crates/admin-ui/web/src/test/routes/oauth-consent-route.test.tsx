import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { OauthAuthorizationRequestView } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
}
const decideMock = vi.fn()
const toastMock = vi.hoisted(() => ({ error: vi.fn() }))

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
}))

vi.mock('sonner', () => ({ toast: toastMock }))

vi.mock('@/server/admin-data.functions', () => ({
  decideOauthAuthorization: (...args: unknown[]) => decideMock(...args),
  getOauthAuthorization: vi.fn(),
}))

function request(
  overrides: Partial<OauthAuthorizationRequestView> = {},
): OauthAuthorizationRequestView {
  return {
    request_id: 'request-1',
    client_id: 'client-1',
    client_name: 'Editor',
    client_uri: null,
    redirect_uri: 'https://editor.example.com/callback',
    redirect_host: 'editor.example.com',
    redirect_is_loopback: false,
    resource: 'https://oceans.example.com/mcp',
    resource_kind: 'aggregate',
    server_key: null,
    server_display_name: null,
    scopes: [{ scope: 'mcp:aggregate', description: 'Use the aggregate endpoint' }],
    expires_at: '2026-10-15T12:00:00Z',
    ...overrides,
  }
}

describe('OauthConsentPage', () => {
  const assignMock = vi.fn()

  beforeEach(() => {
    decideMock.mockReset()
    assignMock.mockReset()
    toastMock.error.mockReset()
    routeMock.useRouteContext.mockReturnValue({ session: { user: { email: 'me@example.com' } } })
    vi.stubGlobal('location', { assign: assignMock })
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('shows the client, redirect host, access, and signed-in user', async () => {
    routeMock.useLoaderData.mockReturnValue({ status: 'ready', request: request() })
    const { OauthConsentPage } = await import('@/routes/oauth/consent')

    render(<OauthConsentPage />)

    expect(screen.getByText('Allow Editor to use Oceans MCP?')).toBeInTheDocument()
    expect(screen.getByText(/Signed in as me@example.com/)).toBeInTheDocument()
    expect(screen.getByTestId('redirect-host')).toHaveTextContent('editor.example.com')
    expect(screen.getByText('All MCP servers you have access to')).toBeInTheDocument()
    expect(screen.getByText('mcp:aggregate')).toBeInTheDocument()
    expect(screen.queryByTestId('loopback-warning')).not.toBeInTheDocument()
  })

  it('warns when the client only redirects to this device', async () => {
    routeMock.useLoaderData.mockReturnValue({
      status: 'ready',
      request: request({
        redirect_is_loopback: true,
        redirect_host: '127.0.0.1:43123',
        resource_kind: 'server',
        server_key: 'github',
        server_display_name: 'GitHub',
      }),
    })
    const { OauthConsentPage } = await import('@/routes/oauth/consent')

    render(<OauthConsentPage />)

    expect(screen.getByTestId('loopback-warning')).toBeInTheDocument()
    expect(screen.getByText('This device')).toBeInTheDocument()
    expect(screen.getByText('GitHub')).toBeInTheDocument()
  })

  it('returns to the client with the decision redirect', async () => {
    routeMock.useLoaderData.mockReturnValue({ status: 'ready', request: request() })
    decideMock.mockResolvedValue({ redirect_to: 'https://editor.example.com/callback?code=x' })
    const { OauthConsentPage } = await import('@/routes/oauth/consent')

    render(<OauthConsentPage />)
    fireEvent.click(screen.getByRole('button', { name: 'Allow' }))

    await waitFor(() =>
      expect(decideMock).toHaveBeenCalledWith({ data: { requestId: 'request-1', approve: true } }),
    )
    await waitFor(() =>
      expect(assignMock).toHaveBeenCalledWith('https://editor.example.com/callback?code=x'),
    )
  })

  it('sends a denial and reports failures', async () => {
    routeMock.useLoaderData.mockReturnValue({ status: 'ready', request: request() })
    decideMock.mockRejectedValue(new Error('authorization request was already decided'))
    const { OauthConsentPage } = await import('@/routes/oauth/consent')

    render(<OauthConsentPage />)
    fireEvent.click(screen.getByRole('button', { name: 'Deny' }))

    await waitFor(() =>
      expect(decideMock).toHaveBeenCalledWith({ data: { requestId: 'request-1', approve: false } }),
    )
    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith('authorization request was already decided'),
    )
    expect(assignMock).not.toHaveBeenCalled()
  })

  it('explains an expired or used request', async () => {
    routeMock.useLoaderData.mockReturnValue({ status: 'unavailable' })
    const { OauthConsentPage } = await import('@/routes/oauth/consent')

    render(<OauthConsentPage />)

    expect(screen.getByTestId('oauth-consent-unavailable')).toHaveTextContent(
      'This sign-in request is no longer valid',
    )
  })
})
