import { render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const routeMock = {
  useSearch: vi.fn(),
  useLoaderData: vi.fn(),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  getOidcLoginOptions: vi.fn(),
  loginAdminWithPassword: vi.fn(),
}))

beforeEach(() => {
  routeMock.useSearch.mockReturnValue({})
  routeMock.useLoaderData.mockReturnValue({
    oidcProviders: {
      data: { providers: [{ key: 'google', label: 'Google' }] },
      meta: { generated_at: '2026-08-04T00:00:00.000Z' },
    },
    oauthProviders: {
      data: { providers: [{ key: 'github', label: 'GitHub' }] },
      meta: { generated_at: '2026-08-04T00:00:00.000Z' },
    },
    startOrigin: 'https://gateway.example',
  })
})

describe('login SSO errors', () => {
  it('explains GitHub unverified primary email failures', async () => {
    const { ssoErrorMessage } = await import('@/routes/-login-messages')

    expect(ssoErrorMessage('github_unverified_email')).toContain(
      'https://github.com/settings/emails',
    )
  })
})

describe('LoginPage', () => {
  it('uses user-facing sign-in copy and provider actions', async () => {
    const { LoginPage } = await import('@/routes/login')

    render(<LoginPage />)

    expect(screen.getByRole('heading', { name: 'Sign in' })).toBeInTheDocument()
    expect(
      screen.getByText('Use your Oceans credentials or a supported SSO provider.'),
    ).toBeInTheDocument()
    expect(screen.queryByText('Admin access')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Show password' })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Google' })).toHaveAttribute(
      'href',
      'https://gateway.example/api/v1/auth/oidc/start?provider_key=google&redirect_to=%2Fadmin',
    )
    expect(screen.getByRole('link', { name: 'GitHub' })).toHaveAttribute(
      'href',
      'https://gateway.example/api/v1/auth/oauth/start?provider_key=github&redirect_to=%2Fadmin',
    )
  })
})
