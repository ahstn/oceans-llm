import type * as React from 'react'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  createIdentityUser,
  removeIdentityTeamMember,
  updateIdentityTeam,
} from '@/server/admin-data.functions'
import { allAdminPages, platformAdminSession, regularUserSession } from '@/test/auth-session'
import type { IdentityDirectoryTeamsPayload, IdentityTeamsPayload } from '@/types/api'
import { TeamsPage } from '@/routes/identity/teams'

// Hoisted with the mocks: the route module reads it as it loads.
const routeMock = vi.hoisted(() => ({
  useLoaderData: vi.fn(),
  useRouteContext: vi.fn(),
}))

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
  oauth_providers: [],
  oidc_providers: [],
}

// Core Platform has Jane as its admin and Ben as a member. Dev has no team.
const rosterPayload: IdentityTeamsPayload = {
  teams: [
    {
      id: 'team_1',
      name: 'Core Platform',
      key: 'core-platform',
      status: 'active',
      tags: [],
      member_count: 2,
      admins: [{ id: 'user_1', name: 'Jane Admin', email: 'jane@example.com', status: 'active' }],
      members: [
        {
          id: 'user_1',
          name: 'Jane Admin',
          email: 'jane@example.com',
          status: 'active',
          role: 'admin',
        },
        {
          id: 'user_2',
          name: 'Ben Member',
          email: 'ben@example.com',
          status: 'active',
          role: 'member',
        },
      ],
    },
    {
      id: 'team_2',
      name: 'Research',
      key: 'research',
      status: 'active',
      tags: [],
      member_count: 0,
      admins: [],
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
      team_role: 'admin',
    },
    {
      id: 'user_2',
      name: 'Ben Member',
      email: 'ben@example.com',
      status: 'active',
      team_id: 'team_1',
      team_name: 'Core Platform',
      team_role: 'member',
    },
    {
      id: 'user_3',
      name: 'Dev Teamless',
      email: 'dev@example.com',
      status: 'active',
      team_id: null,
      team_name: null,
      team_role: null,
    },
  ],
  oidc_providers: [],
  oauth_providers: [],
}

describe('TeamsPage', () => {
  beforeEach(() => {
    routeMock.useLoaderData.mockReset()
    routeMock.useRouteContext.mockReset()
    routeMock.useRouteContext.mockReturnValue({ session: platformAdminSession() })
    routerMock.invalidate.mockClear()
    vi.mocked(createIdentityUser).mockReset()
    vi.mocked(removeIdentityTeamMember).mockReset()
    vi.mocked(updateIdentityTeam).mockReset()
  })

  afterEach(() => {
    cleanup()
  })

  it('shows all team membership without mutation controls to regular users', () => {
    routeMock.useRouteContext.mockReturnValue({ session: regularUserSession() })
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

    render(<TeamsPage />)

    expect(screen.getByText('Research')).toBeInTheDocument()
    expect(screen.getByText('Other User')).toBeInTheDocument()
    expect(screen.getByText('other@example.com')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add team' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Edit team' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Add members' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Remove' })).not.toBeInTheDocument()
    expect(screen.queryByText('research')).not.toBeInTheDocument()
    expect(screen.getByText(/Only administrators can make changes/)).toBeInTheDocument()
  })

  it('teaches the next step when no teams exist', () => {
    routeMock.useLoaderData.mockReturnValue({ data: basePayload })

    render(<TeamsPage />)

    expect(screen.getByText('No teams created yet')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Create first team' }))

    expect(
      screen.getByText('Create a team now and optionally assign team admins from existing users.'),
    ).toBeInTheDocument()
  })

  it('keeps member rosters collapsed until a team is expanded', () => {
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
            members: [
              {
                id: 'user_1',
                name: 'Jane Admin',
                email: 'jane@example.com',
                status: 'active',
                role: 'owner',
              },
            ],
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
        oauth_providers: [],
      } satisfies IdentityTeamsPayload,
    })

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

  it('badges an inactive team and member roles in neutral variants', async () => {
    const [coreTeam, researchTeam] = rosterPayload.teams
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...rosterPayload,
        teams: [
          {
            ...coreTeam,
            members: coreTeam.members.map((member) =>
              member.id === 'user_1' ? { ...member, role: 'owner' } : member,
            ),
          },
          { ...researchTeam, status: 'inactive' },
        ],
        users: rosterPayload.users.map((user) =>
          user.id === 'user_1' ? { ...user, team_role: 'owner' } : user,
        ),
      } satisfies IdentityTeamsPayload,
    })

    const { TeamsPage } = await import('@/routes/identity/teams')
    render(<TeamsPage />)

    // Inactive is not pending: the same badge the read-only directory shows.
    for (const badge of screen.getAllByText('inactive')) {
      expect(badge).toHaveAttribute('data-variant', 'secondary')
    }

    // A role is a category, so it takes neither a status colour nor the lilac action colour.
    fireEvent.click(screen.getAllByRole('button', { name: 'Show 2 members' })[0])
    for (const badge of screen.getAllByText('owner')) {
      expect(badge).toHaveAttribute('data-variant', 'secondary')
    }
    for (const badge of screen.getAllByText('member')) {
      expect(badge).toHaveAttribute('data-variant', 'outline')
    }
  })

  it('links team admins to the users page only for viewers who can open it', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: rosterPayload })

    const { TeamsPage } = await import('@/routes/identity/teams')
    const { rerender } = render(<TeamsPage />)

    expect(screen.getAllByRole('link', { name: 'Open Jane Admin' }).length).toBeGreaterThan(0)

    routeMock.useRouteContext.mockReturnValue({
      session: platformAdminSession(allAdminPages.filter((page) => page !== 'users')),
    })
    rerender(<TeamsPage />)

    expect(screen.queryByRole('link', { name: 'Open Jane Admin' })).not.toBeInTheDocument()
    expect(screen.getAllByText('Jane Admin').length).toBeGreaterThan(0)
  })

  it('saves a team edited from its row', async () => {
    vi.mocked(updateIdentityTeam).mockResolvedValue({ data: rosterPayload.teams[0] } as never)
    routeMock.useLoaderData.mockReturnValue({ data: rosterPayload })

    const { TeamsPage } = await import('@/routes/identity/teams')
    render(<TeamsPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Edit team' })[0])
    expect(screen.getByRole('heading', { name: 'Edit team' })).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: ' Platform ' } })
    fireEvent.submit(screen.getByRole('button', { name: 'Save changes' }).closest('form')!)

    await waitFor(() =>
      expect(updateIdentityTeam).toHaveBeenCalledWith({
        data: {
          teamId: 'team_1',
          input: { name: 'Platform', admin_user_ids: ['user_1'], tags: [] },
        },
      }),
    )
    await waitFor(() =>
      expect(screen.queryByRole('heading', { name: 'Edit team' })).not.toBeInTheDocument(),
    )
    expect(routerMock.invalidate).toHaveBeenCalledTimes(1)
  })

  it('removes a member from the expanded roster', async () => {
    vi.mocked(removeIdentityTeamMember).mockResolvedValue({ data: { status: 'ok' } } as never)
    routeMock.useLoaderData.mockReturnValue({ data: rosterPayload })

    const { TeamsPage } = await import('@/routes/identity/teams')
    render(<TeamsPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Show 2 members' })[0])
    fireEvent.click(screen.getAllByRole('button', { name: 'Remove' })[1])
    expect(
      screen.getByText(
        'Remove Ben Member from Core Platform. This only changes future membership-derived access.',
      ),
    ).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Remove member' }))

    await waitFor(() =>
      expect(removeIdentityTeamMember).toHaveBeenCalledWith({
        data: { teamId: 'team_1', userId: 'user_2' },
      }),
    )
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Remove member' })).not.toBeInTheDocument(),
    )
  })

  it('shows the member invite and its URL after inviting into a team', async () => {
    vi.mocked(createIdentityUser).mockResolvedValue({
      data: {
        kind: 'password_invite',
        invite_url: 'http://example.test/invite/member-1',
        expires_at: '2026-03-14T12:00:00Z',
        user: { email: 'taylor@example.com' },
      },
    } as never)
    routeMock.useLoaderData.mockReturnValue({ data: rosterPayload })

    const { TeamsPage } = await import('@/routes/identity/teams')
    render(<TeamsPage />)

    fireEvent.click(screen.getAllByRole('button', { name: 'Add members' })[0])
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: ' Taylor Member ' } })
    fireEvent.change(screen.getByLabelText('Email'), { target: { value: 'taylor@example.com' } })
    fireEvent.submit(screen.getByRole('button', { name: 'Invite member' }).closest('form')!)

    await waitFor(() =>
      expect(screen.getByLabelText('Generated URL')).toHaveValue(
        'http://example.test/invite/member-1',
      ),
    )
    expect(createIdentityUser).toHaveBeenCalledWith({
      data: {
        name: 'Taylor Member',
        email: 'taylor@example.com',
        auth_mode: 'password',
        global_role: 'user',
        team_id: 'team_1',
        team_role: 'member',
        oidc_provider_key: null,
        oauth_provider_key: null,
        tags: [],
      },
    })
    expect(screen.getByText('Member invite ready')).toBeInTheDocument()
    expect(screen.getByText('Share this invite before 2026-03-14T12:00:00Z.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument()
  })
})
