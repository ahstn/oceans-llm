import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { IdentityUsersPayload } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
}

const routerMock = {
  invalidate: vi.fn(async () => {}),
}

const createIdentityUserMock = vi.fn()
const resendInviteMock = vi.fn()

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
  reactivateIdentityUser: vi.fn(),
  resetIdentityUserOnboarding: vi.fn(),
  resendIdentityUserPasswordInvite: (...args: unknown[]) => resendInviteMock(...args),
  updateIdentityUser: vi.fn(),
}))

const basePayload: IdentityUsersPayload = {
  users: [],
  teams: [],
  oidc_providers: [],
}

describe('UsersPage', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routerMock.invalidate.mockClear()
    createIdentityUserMock.mockReset()
    resendInviteMock.mockReset()
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
          status: 'invited',
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
            onboarding: null,
          },
        ],
        teams: [{ id: 'team_1', name: 'Core Platform' }],
        oidc_providers: [],
      } satisfies IdentityUsersPayload,
    })

    const { UsersPage } = await import('@/routes/identity/users')

    render(<UsersPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Manage' })[0])

    expect(screen.getByText('Owner membership is locked')).toBeInTheDocument()
    expect(screen.getByText('Auth mode is locked after activation; use reset onboarding to reissue credentials.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Reset onboarding' })).toBeDisabled()
    expect(screen.getAllByLabelText('Auth method')[1]).toBeDisabled()
  })
})
