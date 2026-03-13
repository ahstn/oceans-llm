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
})
