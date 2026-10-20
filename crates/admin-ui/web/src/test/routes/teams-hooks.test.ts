import { act, renderHook, waitFor } from '@testing-library/react'
import type { FormEvent } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  adminOptionsFor,
  canTransferMember,
  existingMemberOptionsFor,
  initialInviteForm,
  initialTeamForm,
  initialTransferForm,
  sanitizeInviteForm,
  sanitizeTeamForm,
  sanitizeTransferForm,
} from '@/routes/identity/-teams/team-form'
import { useAddMembers } from '@/routes/identity/-teams/use-add-members'
import { useMembershipChange } from '@/routes/identity/-teams/use-membership-change'
import { useTeamEditor } from '@/routes/identity/-teams/use-team-editor'
import { useIdentityTransition } from '@/routes/identity/-use-identity-transition'
import type { IdentityTeamsPayload, TeamAssignableUserView, TeamManagementView } from '@/types/api'

const mocks = vi.hoisted(() => ({
  router: { invalidate: vi.fn(async () => {}) },
  toast: { success: vi.fn(), error: vi.fn() },
  createIdentityTeam: vi.fn(),
  updateIdentityTeam: vi.fn(),
  addIdentityTeamMembers: vi.fn(),
  createIdentityUser: vi.fn(),
  removeIdentityTeamMember: vi.fn(),
  transferIdentityTeamMember: vi.fn(),
}))

vi.mock('@tanstack/react-router', () => ({ useRouter: () => mocks.router }))
vi.mock('sonner', () => ({ toast: mocks.toast }))
vi.mock('@/server/admin-data.functions', () => ({
  createIdentityTeam: mocks.createIdentityTeam,
  updateIdentityTeam: mocks.updateIdentityTeam,
  addIdentityTeamMembers: mocks.addIdentityTeamMembers,
  createIdentityUser: mocks.createIdentityUser,
  removeIdentityTeamMember: mocks.removeIdentityTeamMember,
  transferIdentityTeamMember: mocks.transferIdentityTeamMember,
}))

const oidcProviders: IdentityTeamsPayload['oidc_providers'] = [
  { id: 'oidc_1', key: 'okta', label: 'Okta' },
]
const oauthProviders: IdentityTeamsPayload['oauth_providers'] = [
  { id: 'oauth_1', key: 'github', label: 'GitHub' },
  { id: 'oauth_2', key: 'gitlab', label: 'GitLab' },
]

function team(overrides: Partial<TeamManagementView> = {}): TeamManagementView {
  return {
    id: 'team_platform',
    name: 'Platform',
    key: 'platform',
    status: 'active',
    tags: [],
    member_count: 2,
    admins: [],
    members: [],
    ...overrides,
  }
}

function assignableUser(overrides: Partial<TeamAssignableUserView> = {}): TeamAssignableUserView {
  return {
    id: 'user_dev',
    name: 'Dev Teamless',
    email: 'dev@example.com',
    status: 'active',
    team_id: null,
    team_name: null,
    team_role: null,
    ...overrides,
  }
}

const platform = team({
  admins: [{ id: 'user_ada', name: 'Ada Admin', email: 'ada@example.com', status: 'active' }],
  tags: [{ key: 'cost-center', value: 'platform' }],
})
const research = team({ id: 'team_research', name: 'Research', key: 'research', member_count: 1 })
const teams = [platform, research]

const ada = assignableUser({
  id: 'user_ada',
  name: 'Ada Admin',
  email: 'ada@example.com',
  team_id: 'team_platform',
  team_name: 'Platform',
  team_role: 'admin',
})
const ben = assignableUser({
  id: 'user_ben',
  name: 'Ben Member',
  email: 'ben@example.com',
  team_id: 'team_platform',
  team_name: 'Platform',
  team_role: 'member',
})
const cara = assignableUser({
  id: 'user_cara',
  name: 'Cara Research',
  email: 'cara@example.com',
  team_id: 'team_research',
  team_name: null,
  team_role: 'member',
})
const dev = assignableUser()
const users = [ada, ben, cara, dev]

function submitEvent() {
  const preventDefault = vi.fn()
  return { event: { preventDefault } as unknown as FormEvent<HTMLFormElement>, preventDefault }
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('team form helpers', () => {
  it('lets a new team pick only teamless admins', () => {
    expect(adminOptionsFor(users, null)).toEqual([
      { user: ada, disabled: true, reason: 'Already in Platform' },
      { user: ben, disabled: true, reason: 'Already in Platform' },
      { user: cara, disabled: true, reason: 'Already in another team' },
      { user: dev, disabled: false, reason: null },
    ])
  })

  it('lets an edited team pick its own members as admins', () => {
    expect(adminOptionsFor(users, platform)).toEqual([
      { user: ada, disabled: false, reason: null },
      { user: ben, disabled: false, reason: null },
      { user: cara, disabled: true, reason: 'Already in another team' },
      { user: dev, disabled: false, reason: null },
    ])
  })

  it('marks the team’s own members and blocks members of other teams', () => {
    expect(existingMemberOptionsFor(users, research)).toEqual([
      { user: ada, disabled: true, reason: 'Already in Platform' },
      { user: ben, disabled: true, reason: 'Already in Platform' },
      { user: cara, disabled: false, reason: 'Already on this team' },
      { user: dev, disabled: false, reason: null },
    ])
  })

  it('allows a transfer only to another team and never for an owner', () => {
    const toResearch = { destination_team_id: 'team_research', destination_role: 'member' }
    const owner = { ...ben, team_role: 'owner' }

    expect(canTransferMember(initialTransferForm, platform, ben)).toBe(false)
    expect(
      canTransferMember({ ...toResearch, destination_team_id: 'team_platform' }, platform, ben),
    ).toBe(false)
    expect(canTransferMember(toResearch, platform, owner)).toBe(false)
    expect(canTransferMember(toResearch, platform, ben)).toBe(true)
  })

  it('drops a transfer destination that is the member’s current team', () => {
    expect(
      sanitizeTransferForm(
        { destination_team_id: 'team_platform', destination_role: 'admin' },
        platform,
      ),
    ).toEqual({ destination_team_id: '', destination_role: 'admin' })
  })

  it('trims the team name, drops repeated admins, and drops blank tags', () => {
    expect(
      sanitizeTeamForm({
        name: '  Core Platform ',
        admin_user_ids: ['user_ada', 'user_ben', 'user_ada'],
        tags: [
          { key: ' cost-center ', value: 'platform ' },
          { key: ' ', value: '' },
        ],
      }),
    ).toEqual({
      name: 'Core Platform',
      admin_user_ids: ['user_ada', 'user_ben'],
      tags: [{ key: 'cost-center', value: 'platform' }],
    })
  })

  it('invites a user-role member and falls back to the only SSO provider', () => {
    expect(
      sanitizeInviteForm(
        {
          ...initialInviteForm,
          name: ' Taylor Member ',
          email: ' taylor@example.com ',
          auth_mode: 'oidc',
          global_role: 'platform_admin',
          team_id: 'team_platform',
          team_role: 'admin',
          oauth_provider_key: 'github',
        },
        oidcProviders,
        oauthProviders,
      ),
    ).toEqual({
      name: 'Taylor Member',
      email: 'taylor@example.com',
      auth_mode: 'oidc',
      global_role: 'user',
      team_id: 'team_platform',
      team_role: 'member',
      oidc_provider_key: 'okta',
      oauth_provider_key: null,
      tags: [],
    })
  })
})

describe('useTeamEditor', () => {
  function renderTeamEditor() {
    return renderHook(() => {
      const transition = useIdentityTransition()
      return useTeamEditor({ teams, transition })
    })
  }

  it('opens blank for a new team and loads an existing team for editing', () => {
    const { result } = renderTeamEditor()
    expect(result.current.isOpen).toBe(false)

    act(() => result.current.openCreate())
    expect(result.current).toMatchObject({ isOpen: true, isEditing: false, editingTeam: null })
    expect(result.current.form).toEqual(initialTeamForm)

    act(() => result.current.openEdit(platform))
    expect(result.current).toMatchObject({ isOpen: true, isEditing: true, editingTeam: platform })
    expect(result.current.form).toEqual({
      name: 'Platform',
      admin_user_ids: ['user_ada'],
      tags: [{ key: 'cost-center', value: 'platform' }],
    })
  })

  it('creates a team from the sanitized form, then refreshes and closes', async () => {
    mocks.createIdentityTeam.mockResolvedValue({ data: platform })
    const { result } = renderTeamEditor()
    const { event, preventDefault } = submitEvent()

    act(() => result.current.openCreate())
    act(() =>
      result.current.setForm({
        name: ' Core Platform ',
        admin_user_ids: ['user_dev', 'user_dev'],
        tags: [],
      }),
    )
    await act(async () => result.current.saveTeam(event))

    expect(preventDefault).toHaveBeenCalled()
    expect(mocks.createIdentityTeam).toHaveBeenCalledWith({
      data: { name: 'Core Platform', admin_user_ids: ['user_dev'], tags: [] },
    })
    await waitFor(() => expect(result.current.isOpen).toBe(false))
    expect(result.current.form).toEqual(initialTeamForm)
    expect(mocks.toast.success).toHaveBeenCalledWith('Team created')
    expect(mocks.router.invalidate).toHaveBeenCalledTimes(1)
  })

  it('updates the team being edited', async () => {
    mocks.updateIdentityTeam.mockResolvedValue({ data: platform })
    const { result } = renderTeamEditor()

    act(() => result.current.openEdit(platform))
    await act(async () => result.current.saveTeam(submitEvent().event))

    expect(mocks.updateIdentityTeam).toHaveBeenCalledWith({
      data: {
        teamId: 'team_platform',
        input: {
          name: 'Platform',
          admin_user_ids: ['user_ada'],
          tags: [{ key: 'cost-center', value: 'platform' }],
        },
      },
    })
    expect(mocks.createIdentityTeam).not.toHaveBeenCalled()
    await waitFor(() => expect(mocks.toast.success).toHaveBeenCalledWith('Team updated'))
  })

  it('still updates, never creates, when the edited team drops out of the list', async () => {
    mocks.updateIdentityTeam.mockRejectedValue(new Error('team not found'))
    const { result, rerender } = renderHook(
      ({ teams }: { teams: TeamManagementView[] }) => {
        const transition = useIdentityTransition()
        return useTeamEditor({ teams, transition })
      },
      { initialProps: { teams } },
    )

    act(() => result.current.openEdit(research))
    rerender({ teams: [platform] })
    expect(result.current).toMatchObject({ isEditing: true, editingTeam: null })
    await act(async () => result.current.saveTeam(submitEvent().event))

    expect(mocks.updateIdentityTeam).toHaveBeenCalledWith({
      data: { teamId: 'team_research', input: { name: 'Research', admin_user_ids: [], tags: [] } },
    })
    expect(mocks.createIdentityTeam).not.toHaveBeenCalled()
    await waitFor(() => expect(mocks.toast.error).toHaveBeenCalledWith('team not found'))
    expect(result.current.isOpen).toBe(true)
  })

  it('keeps the dialog open when saving fails', async () => {
    mocks.createIdentityTeam.mockRejectedValue(new Error('Team name already exists'))
    const { result } = renderTeamEditor()

    act(() => result.current.openCreate())
    await act(async () => result.current.saveTeam(submitEvent().event))

    await waitFor(() => expect(mocks.toast.error).toHaveBeenCalledWith('Team name already exists'))
    expect(result.current.isOpen).toBe(true)
  })
})

describe('useAddMembers', () => {
  function renderAddMembers() {
    return renderHook(() => {
      const transition = useIdentityTransition()
      return useAddMembers({ teams, oidcProviders, oauthProviders, transition })
    })
  }

  it('opens with a cleared selection and an invite into the team', () => {
    const { result } = renderAddMembers()

    act(() => result.current.open(platform))
    act(() => result.current.setSelectedUserIds(['user_dev']))
    act(() => result.current.open(research))

    expect(result.current).toMatchObject({
      isOpen: true,
      team: research,
      selectedUserIds: [],
      inviteResult: null,
    })
    expect(result.current.inviteForm).toEqual({
      ...initialInviteForm,
      team_id: 'team_research',
      team_role: 'member',
    })
  })

  it('switches auth mode with the same provider rules as the users page', () => {
    const { result } = renderAddMembers()

    act(() => result.current.open(platform))
    act(() => result.current.setInviteAuthMode('oidc'))
    expect(result.current.inviteForm).toMatchObject({
      auth_mode: 'oidc',
      oidc_provider_key: 'okta',
      oauth_provider_key: null,
    })

    act(() => result.current.setInviteAuthMode('oauth'))
    expect(result.current.inviteForm).toMatchObject({
      auth_mode: 'oauth',
      oidc_provider_key: null,
      oauth_provider_key: null,
    })
  })

  it('adds the selected users, then clears the selection', async () => {
    mocks.addIdentityTeamMembers.mockResolvedValue({ data: platform })
    const { result } = renderAddMembers()
    const { event, preventDefault } = submitEvent()

    act(() => result.current.open(platform))
    act(() => result.current.setSelectedUserIds(['user_dev']))
    await act(async () => result.current.addSelectedUsers(event))

    expect(preventDefault).toHaveBeenCalled()
    expect(mocks.addIdentityTeamMembers).toHaveBeenCalledWith({
      data: { teamId: 'team_platform', input: { user_ids: ['user_dev'] } },
    })
    await waitFor(() => expect(result.current.selectedUserIds).toEqual([]))
    expect(mocks.toast.success).toHaveBeenCalledWith('Members added')
    expect(mocks.router.invalidate).toHaveBeenCalledTimes(1)
    expect(result.current.isOpen).toBe(true)
  })

  it('adds nobody when no user is selected', async () => {
    const { result } = renderAddMembers()
    const { event, preventDefault } = submitEvent()

    act(() => result.current.open(platform))
    await act(async () => result.current.addSelectedUsers(event))

    expect(preventDefault).toHaveBeenCalled()
    expect(mocks.addIdentityTeamMembers).not.toHaveBeenCalled()
  })

  it('invites a member into the team and keeps the sign-in URL until closed', async () => {
    const signIn = {
      kind: 'oidc_sign_in' as const,
      provider_label: 'Okta',
      sign_in_url: 'http://example.test/api/v1/auth/oidc/start?provider=okta',
      user: { email: 'taylor@example.com' },
    }
    mocks.createIdentityUser.mockResolvedValue({ data: signIn })
    const { result } = renderAddMembers()

    act(() => result.current.open(platform))
    act(() =>
      result.current.setInviteForm((current) => ({
        ...current,
        name: ' Taylor Member ',
        email: 'taylor@example.com',
      })),
    )
    act(() => result.current.setInviteAuthMode('oidc'))
    await act(async () => result.current.inviteMember(submitEvent().event))

    expect(mocks.createIdentityUser).toHaveBeenCalledWith({
      data: {
        name: 'Taylor Member',
        email: 'taylor@example.com',
        auth_mode: 'oidc',
        global_role: 'user',
        team_id: 'team_platform',
        team_role: 'member',
        oidc_provider_key: 'okta',
        oauth_provider_key: null,
        tags: [],
      },
    })
    await waitFor(() => expect(result.current.inviteResult).toEqual(signIn))
    expect(mocks.toast.success).toHaveBeenCalledWith('Member sign-in URL created')
    expect(mocks.router.invalidate).toHaveBeenCalledTimes(1)

    act(() => result.current.close())
    expect(result.current).toMatchObject({ isOpen: false, team: null, inviteResult: null })
    expect(result.current.inviteForm).toEqual(initialInviteForm)
  })
})

describe('useMembershipChange', () => {
  function renderMembershipChange() {
    return renderHook(() => {
      const transition = useIdentityTransition()
      return useMembershipChange({ teams, users, transition })
    })
  }

  it('removes the member, then refreshes and closes', async () => {
    mocks.removeIdentityTeamMember.mockResolvedValue({ data: { status: 'removed' } })
    const { result } = renderMembershipChange()

    act(() => result.current.openRemove('team_platform', 'user_ben'))
    expect(result.current).toMatchObject({ mode: 'remove', team: platform, user: ben })

    await act(async () => result.current.removeMember())

    expect(mocks.removeIdentityTeamMember).toHaveBeenCalledWith({
      data: { teamId: 'team_platform', userId: 'user_ben' },
    })
    await waitFor(() => expect(result.current.mode).toBe('closed'))
    expect(mocks.toast.success).toHaveBeenCalledWith('Member removed')
    expect(mocks.router.invalidate).toHaveBeenCalledTimes(1)
  })

  it('transfers from a fresh form, then refreshes and closes', async () => {
    mocks.transferIdentityTeamMember.mockResolvedValue({ data: { status: 'transferred' } })
    const { result } = renderMembershipChange()
    const { event, preventDefault } = submitEvent()

    act(() => result.current.openTransfer('team_platform', 'user_ben'))
    act(() =>
      result.current.setTransferForm({
        destination_team_id: 'team_research',
        destination_role: 'admin',
      }),
    )
    act(() => result.current.close())
    act(() => result.current.openTransfer('team_platform', 'user_ben'))
    expect(result.current.transferForm).toEqual(initialTransferForm)

    act(() =>
      result.current.setTransferForm({
        destination_team_id: 'team_research',
        destination_role: 'admin',
      }),
    )
    await act(async () => result.current.transferMember(event))

    expect(preventDefault).toHaveBeenCalled()
    expect(mocks.transferIdentityTeamMember).toHaveBeenCalledWith({
      data: {
        teamId: 'team_platform',
        userId: 'user_ben',
        input: { destination_team_id: 'team_research', destination_role: 'admin' },
      },
    })
    await waitFor(() => expect(result.current.mode).toBe('closed'))
    expect(result.current.transferForm).toEqual(initialTransferForm)
    expect(mocks.toast.success).toHaveBeenCalledWith('Member transferred')
  })

  it('keeps the transfer dialog open when the transfer fails', async () => {
    mocks.transferIdentityTeamMember.mockRejectedValue(new Error('Destination team is inactive'))
    const { result } = renderMembershipChange()

    act(() => result.current.openTransfer('team_platform', 'user_ben'))
    await act(async () => result.current.transferMember(submitEvent().event))

    await waitFor(() =>
      expect(mocks.toast.error).toHaveBeenCalledWith('Destination team is inactive'),
    )
    expect(result.current.mode).toBe('transfer')
  })

  it('only removes from the remove dialog and only transfers from the transfer dialog', async () => {
    const { result } = renderMembershipChange()

    act(() => result.current.openRemove('team_platform', 'user_ben'))
    await act(async () => result.current.transferMember(submitEvent().event))
    act(() => result.current.openTransfer('team_platform', 'user_ben'))
    await act(async () => result.current.removeMember())

    expect(mocks.transferIdentityTeamMember).not.toHaveBeenCalled()
    expect(mocks.removeIdentityTeamMember).not.toHaveBeenCalled()
  })
})
