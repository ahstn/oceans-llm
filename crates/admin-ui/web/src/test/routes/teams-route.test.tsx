import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { IdentityTeamsPayload } from '@/types/api'

const routeMock = {
  useLoaderData: vi.fn(),
}

const routerMock = {
  invalidate: vi.fn(async () => {}),
}

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
  addIdentityTeamMembers: vi.fn(),
  createIdentityTeam: vi.fn(),
  createIdentityUser: vi.fn(),
  getTeams: vi.fn(),
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
    routerMock.invalidate.mockClear()
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

  it('shows member roster actions and blocks owner transfers', async () => {
    routeMock.useLoaderData.mockReturnValue({
      data: {
        teams: [
          {
            id: 'team_1',
            name: 'Core Platform',
            key: 'core-platform',
            status: 'active',
            member_count: 1,
            admins: [
              {
                id: 'user_1',
                name: 'Jane Admin',
                email: 'jane@example.com',
                status: 'active',
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
      } satisfies IdentityTeamsPayload,
    })

    const { TeamsPage } = await import('@/routes/identity/teams')

    render(<TeamsPage />)

    expect(screen.getAllByText('Jane Admin').length).toBeGreaterThan(0)
    expect(
      screen.getAllByText('Owner memberships cannot be removed or transferred in this slice.').length,
    ).toBeGreaterThan(0)
    expect(screen.getAllByRole('button', { name: 'Transfer' })[0]).toBeDisabled()
    expect(screen.getAllByRole('button', { name: 'Remove' })[0]).toBeDisabled()
  })
})
