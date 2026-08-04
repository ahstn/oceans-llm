import type * as React from 'react'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { IdentityDirectoryTeamsPayload, IdentityTeamsPayload } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
}

const routerMock = {
  invalidate: vi.fn(async () => {}),
}

vi.mock('@tanstack/react-router', () => ({
  createFileRoute: () => () => routeMock,
  Link: ({
    children,
    search: _search,
    to,
    ...props
  }: React.ComponentProps<'a'> & { search?: unknown; to?: string }) => (
    <a href={to} {...props}>
      {children}
    </a>
  ),
  useRouter: () => routerMock,
}))

vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/server/admin-data.functions', () => ({
  addIdentityTeamMembers: vi.fn(),
  createIdentityTeam: vi.fn(),
  createIdentityUser: vi.fn(),
  getTeams: vi.fn(),
  getTeamDirectory: vi.fn(),
  removeIdentityTeamMember: vi.fn(),
  transferIdentityTeamMember: vi.fn(),
  updateIdentityTeam: vi.fn(),
}))

const basePayload: IdentityTeamsPayload = {
  teams: [],
  users: [],
  oidc_providers: [],
}

describe('TeamsPage', () => {
  beforeEach(() => {
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
    routerMock.invalidate.mockClear()
  })

  it('shows all team membership without mutation controls to regular users', async () => {
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
        ...basePayload,
        teams: [
          {
            id: 'team_1',
            name: 'Research',
            status: 'active',
            member_count: 1,
            members: [
              {
                id: 'user_2',
                name: 'Other User',
                email: 'other@example.com',
                status: 'active',
                role: 'member',
              },
            ],
          },
        ],
      } satisfies IdentityDirectoryTeamsPayload,
    })

    const { TeamsPage } = await import('@/routes/identity/teams')
    render(<TeamsPage />)

    expect(screen.getByText('Research')).toBeInTheDocument()
    expect(screen.getByText('Other User')).toBeInTheDocument()
    expect(screen.getByText('other@example.com')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add team' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Edit team' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add members' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Remove' })).not.toBeInTheDocument()
    expect(screen.queryByText('research')).not.toBeInTheDocument()
    expect(screen.getByText(/Only platform administrators can change teams/)).toBeInTheDocument()
  })

  it('teaches the next step when no teams exist', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: basePayload })

    const { TeamsPage } = await import('@/routes/identity/teams')

    render(<TeamsPage />)

    expect(screen.getByText('No teams created yet')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Create first team' }))

    expect(
      screen.getByText('Create a team now and optionally assign team admins from existing users.'),
    ).toBeInTheDocument()
  })

  it('keeps member rosters collapsed until a team is expanded', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: {
        teams: [
          {
            id: 'team_1',
            name: 'Core Platform',
            key: 'core-platform',
            status: 'active',
            tags: [],
            member_count: 1,
            admins: [
              {
                id: 'user_1',
                name: 'Jane Admin',
                email: 'jane@example.com',
                status: 'active',
              },
            ],
            members: [],
          },
        ],
        users: [
          {
            id: 'user_1',
            name: 'Jane Admin',
            email: 'jane@example.com',
            status: 'active',
            team_id: 'team_1',
            team_name: 'Core Platform',
            team_role: 'owner',
          },
        ],
        oidc_providers: [],
      } satisfies IdentityTeamsPayload,
    })

    const { TeamsPage } = await import('@/routes/identity/teams')

    render(<TeamsPage />)

    expect(screen.getAllByLabelText('Team avatar for Core Platform').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Jane Admin').length).toBeGreaterThan(0)
    expect(
      screen.queryByText('Owner memberships cannot be removed or transferred in this slice.'),
    ).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Transfer' })).not.toBeInTheDocument()

    const showMembersButtons = screen.getAllByRole('button', { name: 'Show 1 member' })
    expect(showMembersButtons[0]).toHaveAttribute('aria-expanded', 'false')
    expect(showMembersButtons[0].querySelector('[data-icon="inline-start"]')).toBeInTheDocument()

    fireEvent.click(showMembersButtons[0])

    const hideMembersButton = screen.getAllByRole('button', { name: 'Hide 1 member' })[0]
    expect(hideMembersButton).toHaveAttribute('aria-expanded', 'true')
    expect(hideMembersButton.querySelector('[data-icon="inline-start"]')).toBeInTheDocument()
    expect(
      screen.getAllByText('Owner memberships cannot be removed or transferred in this slice.')
        .length,
    ).toBeGreaterThan(0)
    expect(screen.getAllByLabelText('User avatar for Jane Admin').length).toBeGreaterThan(0)
    expect(screen.getAllByRole('button', { name: 'Transfer' })[0]).toBeDisabled()
    expect(screen.getAllByRole('button', { name: 'Remove' })[0]).toBeDisabled()
  })
})
