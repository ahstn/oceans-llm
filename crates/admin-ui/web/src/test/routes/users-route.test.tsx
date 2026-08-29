import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { IdentityUsersPayload } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
  useSearch: vi.fn(),
}

const routerMock = {
  invalidate: vi.fn(async () => {}),
}

const createIdentityUserMock = vi.fn()
const resendInviteMock = vi.fn()
const resetOnboardingMock = vi.fn()
const updateIdentityUserMock = vi.fn()
const saveProviderCredentialMock = vi.fn()
const removeProviderCredentialMock = vi.fn()

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  useRouter: () => routerMock,
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  deactivateIdentityUser: vi.fn(),
  createIdentityUser: (...args: unknown[]) => createIdentityUserMock(...args),
  getUsers: vi.fn(),
  getUserDirectory: vi.fn(),
  reactivateIdentityUser: vi.fn(),
  resetIdentityUserOnboarding: (...args: unknown[]) => resetOnboardingMock(...args),
  saveIdentityUserProviderCredential: (...args: unknown[]) => saveProviderCredentialMock(...args),
  removeIdentityUserProviderCredential: (...args: unknown[]) =>
    removeProviderCredentialMock(...args),
  resendIdentityUserPasswordInvite: (...args: unknown[]) => resendInviteMock(...args),
  updateIdentityUser: (...args: unknown[]) => updateIdentityUserMock(...args),
}))

const basePayload: IdentityUsersPayload = {
  users: [],
  teams: [],
  oidc_providers: [],
  oauth_providers: [],
  copilot_user_providers: [],
}

type UserPayload = IdentityUsersPayload['users'][number]

function invitedUser(overrides: Partial<UserPayload> = {}): UserPayload {
  return {
    id: 'user_1',
    name: 'Jane Operator',
    email: 'jane@example.com',
    auth_mode: 'password',
    global_role: 'user',
    team_id: null,
    team_name: null,
    team_role: null,
    request_logging_enabled: true,
    status: 'invited',
    tags: [],
    onboarding: null,
    ...overrides,
  }
}

describe('UsersPage', () => {
  beforeEach(() => {
    if (!Element.prototype.hasPointerCapture) {
      Element.prototype.hasPointerCapture = () => false
    }
    if (!Element.prototype.releasePointerCapture) {
      Element.prototype.releasePointerCapture = () => {}
    }
    routeMock.useLoaderData.mockReset()
    routeMock.useRouteContext.mockReset()
    routeMock.useRouteContext.mockReturnValue({
      session: {
        must_change_password: false,
        user: {
          id: 'admin_1',
          name: 'Admin User',
          email: 'admin@example.com',
          global_role: 'platform_admin',
        },
      },
    })
    routeMock.useSearch.mockReturnValue({ user_id: undefined, user_section: 'overview' })
    routerMock.invalidate.mockClear()
    createIdentityUserMock.mockReset()
    resendInviteMock.mockReset()
    resetOnboardingMock.mockReset()
    updateIdentityUserMock.mockReset()
    saveProviderCredentialMock.mockReset()
    removeProviderCredentialMock.mockReset()
  })

  afterEach(() => {
    cleanup()
  })

  it('shows the user directory without mutation controls to regular users', async () => {
    routeMock.useRouteContext.mockReturnValue({
      session: {
        must_change_password: false,
        user: {
          id: 'user_1',
          name: 'Regular User',
          email: 'regular@example.com',
          global_role: 'user',
        },
      },
    })
    routeMock.useLoaderData.mockReturnValue({
      data: {
        users: [
          {
            id: 'user_1',
            name: 'Regular User',
            email: 'regular@example.com',
            global_role: 'user',
            status: 'active',
            team_id: 'team_1',
            team_name: 'Platform',
            team_role: 'member',
          },
          {
            id: 'user_2',
            name: 'Other User',
            email: 'other@example.com',
            global_role: 'user',
            status: 'active',
            team_id: 'team_2',
            team_name: 'Research',
            team_role: 'admin',
          },
        ],
      },
    })

    const { UsersPage } = await import('@/routes/identity/users')
    render(<UsersPage />)

    expect(screen.getByText('Regular User')).toBeInTheDocument()
    expect(screen.getByText('Other User')).toBeInTheDocument()
    expect(screen.getByText('other@example.com')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add user' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Manage' })).not.toBeInTheDocument()
    expect(screen.queryByText('Sign-in')).not.toBeInTheDocument()
    expect(screen.getByText(/Only administrators can make changes/)).toBeInTheDocument()
  })

  it('teaches the next step when no users exist', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: basePayload })

    const { UsersPage } = await import('@/routes/identity/users')

    render(<UsersPage />)

    expect(screen.getByText('No users provisioned yet')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Create first user' }))

    expect(
      screen.getByText('Pre-provision the account and generate the onboarding URL to share.'),
    ).toBeInTheDocument()
  })

  it('renders the generated URL inside an input group after provisioning', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: basePayload })
    createIdentityUserMock.mockResolvedValue({
      data: {
        kind: 'password_invite',
        invite_url: 'http://example.test/invite/user-1',
        expires_at: '2026-03-14T12:00:00Z',
        user: {
          id: 'user_1',
          name: 'Jane Operator',
          email: 'jane@example.com',
          auth_mode: 'password',
          global_role: 'user',
          team_id: null,
          team_name: null,
          team_role: null,
          request_logging_enabled: true,
          status: 'invited',
          tags: [],
          onboarding: null,
        },
      },
    })

    const { UsersPage } = await import('@/routes/identity/users')

    render(<UsersPage />)

    fireEvent.click(screen.getByRole('button', { name: 'Add user' }))
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Jane Operator' } })
    fireEvent.change(screen.getByLabelText('Email'), { target: { value: 'jane@example.com' } })
    fireEvent.submit(screen.getByRole('button', { name: 'Create user' }).closest('form')!)

    await waitFor(() => expect(createIdentityUserMock).toHaveBeenCalledTimes(1))

    await waitFor(() =>
      expect(screen.getByLabelText('Generated URL')).toHaveValue(
        'http://example.test/invite/user-1',
      ),
    )

    const group = screen.getByLabelText('Generated URL').closest('[role="group"]')

    expect(group).not.toBeNull()
    expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument()
  })

  it('locks owner membership controls and invited-only auth mode edits', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: {
        users: [
          {
            id: 'user_1',
            name: 'Jane Admin',
            email: 'jane@example.com',
            auth_mode: 'password',
            global_role: 'platform_admin',
            team_id: 'team_1',
            team_name: 'Core Platform',
            team_role: 'owner',
            status: 'active',
            request_logging_enabled: true,
            tags: [],
            onboarding: null,
          },
        ],
        teams: [{ id: 'team_1', name: 'Core Platform' }],
        oidc_providers: [],
        oauth_providers: [],
        copilot_user_providers: [],
      } satisfies IdentityUsersPayload,
    })

    routeMock.useSearch.mockReturnValue({ user_id: 'user_1', user_section: 'configuration' })

    const { UsersPage } = await import('@/routes/identity/users')

    const { rerender } = render(<UsersPage />)

    expect(screen.getAllByLabelText('User avatar for Jane Admin').length).toBeGreaterThan(0)
    expect(screen.getByText('Owner membership is locked')).toBeInTheDocument()

    routeMock.useSearch.mockReturnValue({ user_id: 'user_1', user_section: 'auth' })
    rerender(<UsersPage />)

    expect(screen.getByText('Auth mode is locked after activation')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Reset onboarding' })).toBeDisabled()
    const authMethodControls = screen.getAllByLabelText('Auth method')
    expect(authMethodControls[authMethodControls.length - 1]).toBeDisabled()
  })

  it('keeps a reset onboarding URL visible after the user list refreshes', async () => {
    const user = invitedUser()
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...basePayload,
        users: [user],
      },
    })
    routeMock.useSearch.mockReturnValue({ user_id: 'user_1', user_section: 'auth' })
    resetOnboardingMock.mockResolvedValue({
      data: {
        kind: 'password_invite',
        invite_url: 'http://example.test/invite/reset-user-1',
        expires_at: '2026-03-14T12:00:00Z',
        user,
      },
    })

    const { UsersPage } = await import('@/routes/identity/users')

    const { rerender } = render(<UsersPage />)

    fireEvent.click(screen.getByRole('button', { name: 'Reset onboarding' }))

    await waitFor(() =>
      expect(screen.getByLabelText('Generated URL')).toHaveValue(
        'http://example.test/invite/reset-user-1',
      ),
    )

    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...basePayload,
        users: [{ ...user }],
      },
    })
    rerender(<UsersPage />)

    expect(screen.getByLabelText('Generated URL')).toHaveValue(
      'http://example.test/invite/reset-user-1',
    )
  })

  it('clears a stale reset onboarding URL before retrying', async () => {
    const user = invitedUser()
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...basePayload,
        users: [user],
      },
    })
    routeMock.useSearch.mockReturnValue({ user_id: 'user_1', user_section: 'auth' })
    resetOnboardingMock
      .mockResolvedValueOnce({
        data: {
          kind: 'password_invite',
          invite_url: 'http://example.test/invite/reset-user-1',
          expires_at: '2026-03-14T12:00:00Z',
          user,
        },
      })
      .mockRejectedValueOnce(new Error('reset failed'))

    const { UsersPage } = await import('@/routes/identity/users')

    render(<UsersPage />)

    fireEvent.click(screen.getByRole('button', { name: 'Reset onboarding' }))
    await waitFor(() =>
      expect(screen.getByLabelText('Generated URL')).toHaveValue(
        'http://example.test/invite/reset-user-1',
      ),
    )

    await waitFor(() => expect(routerMock.invalidate).toHaveBeenCalledTimes(1))
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Reset onboarding' })).toBeEnabled(),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Reset onboarding' }))

    await waitFor(() => expect(resetOnboardingMock).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.queryByLabelText('Generated URL')).not.toBeInTheDocument())
  })

  it('renders an OAuth reset onboarding URL', async () => {
    const user = invitedUser({
      auth_mode: 'oauth',
      onboarding: {
        kind: 'oauth_sign_in',
        provider_key: 'github',
        provider_label: 'GitHub',
        sign_in_url: 'http://example.test/api/v1/auth/oauth/start?provider=github',
      },
    })
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...basePayload,
        users: [user],
        oauth_providers: [{ key: 'github', label: 'GitHub' }],
      },
    })
    routeMock.useSearch.mockReturnValue({ user_id: 'user_1', user_section: 'auth' })
    resetOnboardingMock.mockResolvedValue({
      data: {
        kind: 'oauth_sign_in',
        provider_label: 'github',
        sign_in_url: 'http://example.test/api/v1/auth/oauth/start?provider=github',
      },
    })

    const { UsersPage } = await import('@/routes/identity/users')

    render(<UsersPage />)

    fireEvent.click(screen.getByRole('button', { name: 'Reset onboarding' }))

    await waitFor(() =>
      expect(screen.getByLabelText('Generated URL')).toHaveValue(
        'http://example.test/api/v1/auth/oauth/start?provider=github',
      ),
    )
    expect(updateIdentityUserMock).not.toHaveBeenCalled()
    expect(
      screen.getByText('Share this URL with the user so they can finish SSO onboarding.'),
    ).toBeInTheDocument()
  })

  it('submits a Copilot token through its own form without updating the user', async () => {
    const user = invitedUser({ status: 'active' })
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...basePayload,
        users: [user],
        copilot_user_providers: [
          {
            provider_key: 'github-copilot-user',
            credentials: [
              {
                user_id: user.id,
                configured: false,
                updated_at: null,
                last_used_at: null,
              },
            ],
          },
        ],
      },
    })
    routeMock.useSearch.mockReturnValue({
      user_id: user.id,
      user_section: 'provider-configuration',
    })
    saveProviderCredentialMock.mockResolvedValue({ data: { configured: true } })

    const { UsersPage } = await import('@/routes/identity/users')
    render(<UsersPage />)

    expect(screen.getAllByText('Provider Configuration').length).toBeGreaterThan(0)
    expect(screen.getByText(/no extra GitHub OAuth scope/i)).toBeInTheDocument()
    expect(screen.getByText('gh auth refresh --hostname github.com')).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('GitHub token'), {
      target: { value: 'gho_secret-user-token' },
    })
    fireEvent.submit(screen.getByLabelText('GitHub token').closest('form')!)

    await waitFor(() =>
      expect(saveProviderCredentialMock).toHaveBeenCalledWith({
        data: {
          userId: user.id,
          providerKey: 'github-copilot-user',
          token: 'gho_secret-user-token',
        },
      }),
    )
    await waitFor(() => expect(screen.getByLabelText('GitHub token')).toHaveValue(''))
    expect(screen.queryByDisplayValue('gho_secret-user-token')).not.toBeInTheDocument()
    expect(updateIdentityUserMock).not.toHaveBeenCalled()
  })

  it('removes the selected user provider credential', async () => {
    const user = invitedUser({ status: 'active' })
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...basePayload,
        users: [user],
        copilot_user_providers: [
          {
            provider_key: 'github-copilot-user',
            credentials: [
              {
                user_id: user.id,
                configured: true,
                updated_at: '2026-08-29T09:00:00Z',
                last_used_at: null,
              },
            ],
          },
        ],
      },
    })
    routeMock.useSearch.mockReturnValue({
      user_id: user.id,
      user_section: 'provider-configuration',
    })
    removeProviderCredentialMock.mockResolvedValue({ data: { status: 'deleted' } })

    const { UsersPage } = await import('@/routes/identity/users')
    render(<UsersPage />)

    fireEvent.click(screen.getByRole('button', { name: 'Remove token' }))

    await waitFor(() =>
      expect(removeProviderCredentialMock).toHaveBeenCalledWith({
        data: {
          userId: user.id,
          providerKey: 'github-copilot-user',
        },
      }),
    )
    expect(screen.getByLabelText('GitHub token')).toHaveValue('')
  })

  it('sanitizes onboarding updates to auth fields and persisted role/team membership', async () => {
    const { sanitizeOnboardingUpdateForm } = await import('@/routes/identity/-user-form')

    const input = sanitizeOnboardingUpdateForm(
      {
        auth_mode: 'oauth',
        global_role: 'platform_admin',
        team_id: 'team_1',
        team_role: 'admin',
        tags: [{ key: 'department', value: 'engineering' }],
        oidc_provider_key: 'oidc',
        oauth_provider_key: 'github',
      },
      invitedUser({ global_role: 'user', team_id: 'team_existing', team_role: 'admin' }),
      [{ key: 'oidc', label: 'OIDC' }],
      [{ key: 'github', label: 'GitHub' }],
    )

    expect(input).toEqual({
      global_role: 'user',
      team_id: 'team_existing',
      team_role: 'admin',
      auth_mode: 'oauth',
      oidc_provider_key: null,
      oauth_provider_key: 'github',
    })
  })
})
