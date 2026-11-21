import type * as React from 'react'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
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

/** Opens a Radix dropdown from its trigger; jsdom has no pointer events, so use the keyboard. */
function openMenu(name: string) {
  fireEvent.keyDown(screen.getAllByRole('button', { name })[0], { key: 'Enter' })
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

    const expandButton = screen.getByRole('button', { name: 'Expand Core Platform' })
    expect(expandButton).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getAllByRole('button', { name: 'Show 1 member' })[0]).toHaveAttribute(
      'aria-expanded',
      'false',
    )

    fireEvent.click(expandButton)

    expect(screen.getByRole('button', { name: 'Collapse Core Platform' })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
    // Cards and table share expanded state, but the roster mounts only in the visible layout.
    const teamTable = screen.getByRole('table', { name: 'Teams' })
    expect(screen.getAllByRole('table', { name: 'Members of Core Platform' })).toHaveLength(1)
    expect(within(teamTable).getByRole('table', { name: 'Members of Core Platform' })).toBeVisible()
    expect(screen.getAllByLabelText('User avatar for Jane Admin').length).toBeGreaterThan(0)

    // An owner's actions stay in the menu, disabled, with the reason beside them.
    openMenu('Actions for Jane Admin')
    expect(
      screen.getByText('Owner memberships cannot be removed or transferred in this slice.'),
    ).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: 'Transfer' })).toHaveAttribute('data-disabled')
    expect(screen.getByRole('menuitem', { name: 'Remove' })).toHaveAttribute('data-disabled')
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
    fireEvent.click(screen.getByRole('button', { name: 'Expand Core Platform' }))
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

    openMenu('Actions for Core Platform')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Edit team' }))
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

    fireEvent.click(screen.getByRole('button', { name: 'Expand Core Platform' }))
    openMenu('Actions for Ben Member')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Remove' }))
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

    openMenu('Actions for Core Platform')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Add members' }))
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

  it('shows two tags per row, the rest behind +N, and filters by a chosen tag', async () => {
    const [coreTeam, researchTeam] = rosterPayload.teams
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...rosterPayload,
        teams: [
          {
            ...coreTeam,
            tags: [
              { key: 'cost-center', value: 'platform' },
              { key: 'env', value: 'prod' },
              { key: 'owner', value: 'infra' },
              { key: 'region', value: 'eu-west' },
            ],
          },
          { ...researchTeam, tags: [{ key: 'env', value: 'staging' }] },
        ],
      } satisfies IdentityTeamsPayload,
    })

    const { TeamsPage } = await import('@/routes/identity/teams')
    render(<TeamsPage />)

    // The fourth tag only appears in the overflow popover.
    expect(
      screen.queryByRole('button', { name: 'Filter by region:eu-west' }),
    ).not.toBeInTheDocument()
    expect(screen.getAllByRole('button', { name: 'Show all 4 tags' }).length).toBeGreaterThan(0)
    expect(screen.getByText('Showing 2 of 2 teams')).toBeInTheDocument()

    // Choosing a tag in the desktop row filters the list without expanding that row.
    const teamTable = screen.getByRole('table', { name: 'Teams' })
    fireEvent.click(within(teamTable).getByRole('button', { name: 'Filter by env:prod' }))
    expect(screen.getByText('Showing 1 of 2 teams')).toBeInTheDocument()
    expect(screen.queryByText('Research')).not.toBeInTheDocument()
    // The applied chip truncates long values but never shrinks its remove button away.
    const removeEnv = screen.getByRole('button', { name: 'Remove env filter' })
    expect(removeEnv).toHaveClass('shrink-0')
    expect(removeEnv.parentElement).toHaveClass('max-w-full', 'min-w-0')
    expect(screen.getByRole('button', { name: 'Expand Core Platform' })).toHaveAttribute(
      'aria-expanded',
      'false',
    )

    fireEvent.click(screen.getByRole('button', { name: 'Remove env filter' }))
    expect(screen.getByText('Showing 2 of 2 teams')).toBeInTheDocument()

    // Blank space in the Tags cell still toggles the row; only the chips keep their clicks.
    const tagCell = within(teamTable)
      .getByRole('button', { name: 'Filter by env:prod' })
      .closest('td')!
    fireEvent.click(tagCell)
    expect(screen.getByRole('button', { name: 'Collapse Core Platform' })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
  })

  it('keeps tag chips and roster actions inside narrow containers', async () => {
    const [coreTeam, ...rest] = rosterPayload.teams
    routeMock.useLoaderData.mockReturnValue({
      data: {
        ...rosterPayload,
        teams: [
          {
            ...coreTeam,
            tags: [
              { key: 'cost-center', value: 'platform-engineering-shared' },
              { key: 'owner', value: 'infrastructure-reliability' },
              { key: 'region', value: 'eu-west' },
            ],
          },
          ...rest,
        ],
      } satisfies IdentityTeamsPayload,
    })

    const { TeamsPage } = await import('@/routes/identity/teams')
    render(<TeamsPage />)

    // A minmax(0, 1fr) track stops long content widening the mobile cards past the viewport.
    expect(screen.getByTestId('team-cards')).toHaveClass('grid-cols-1')
    // Chips shrink and truncate rather than pushing +N out of the card.
    for (const chip of screen.getAllByRole('button', {
      name: 'Filter by cost-center:platform-engineering-shared',
    })) {
      expect(chip).toHaveClass('min-w-0')
      expect(chip).not.toHaveClass('shrink-0')
    }

    fireEvent.click(screen.getByRole('button', { name: 'Expand Core Platform' }))
    for (const detail of screen.getAllByTestId('team-detail')) {
      expect(detail).toHaveClass('min-w-0', 'grid-cols-1')
    }
    // The roster scrolls sideways, with its action column pinned right.
    for (const roster of screen.getAllByRole('table', { name: 'Members of Core Platform' })) {
      expect(roster.parentElement).toHaveClass('overflow-x-auto')
      expect(roster.querySelector('th[aria-label="Actions"]')).toHaveClass('sticky', 'right-0')
      for (const action of within(roster).getAllByRole('button', { name: /^Actions for / })) {
        expect(action.closest('td')).toHaveClass('sticky', 'right-0', 'bg-card')
      }
    }
  })

  it('mounts an expanded roster only in the mobile card on narrow screens', async () => {
    const width = window.innerWidth
    window.innerWidth = 375
    try {
      routeMock.useLoaderData.mockReturnValue({ data: rosterPayload })
      const { TeamsPage } = await import('@/routes/identity/teams')
      render(<TeamsPage />)

      fireEvent.click(screen.getAllByRole('button', { name: /^Show \d+ members?$/ })[0])
      const rosters = screen.getAllByRole('table', { name: /^Members of / })
      expect(rosters).toHaveLength(1)
      expect(rosters[0].closest('article')).not.toBeNull()
      expect(
        within(screen.getByRole('table', { name: 'Teams' })).queryByRole('table', {
          name: /^Members of /,
        }),
      ).not.toBeInTheDocument()
    } finally {
      window.innerWidth = width
    }
  })

  it('expands a team from anywhere on its row', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: rosterPayload })

    const { TeamsPage } = await import('@/routes/identity/teams')
    render(<TeamsPage />)

    const teamTable = screen.getByRole('table', { name: 'Teams' })
    fireEvent.click(within(teamTable).getByText('core-platform'))
    expect(screen.getByRole('button', { name: 'Collapse Core Platform' })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
    expect(within(teamTable).getByRole('table', { name: 'Members of Core Platform' })).toBeVisible()
  })

  it('narrows the list to teams without admins, and by search', async () => {
    routeMock.useLoaderData.mockReturnValue({ data: rosterPayload })

    const { TeamsPage } = await import('@/routes/identity/teams')
    render(<TeamsPage />)

    openMenu('Filter')
    fireEvent.click(screen.getByRole('menuitemcheckbox', { name: 'Without admins' }))
    // The menu stays open for more picks; close it to reach the page again.
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    expect(screen.getByText('Showing 1 of 2 teams')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Remove Admins filter' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    fireEvent.change(screen.getByLabelText('Search teams'), { target: { value: 'nothing-here' } })
    expect(screen.getByText('No teams match these filters.')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }))
    expect(screen.getByText('Showing 2 of 2 teams')).toBeInTheDocument()
  })
})
