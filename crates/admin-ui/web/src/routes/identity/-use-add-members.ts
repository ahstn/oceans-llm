import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { addIdentityTeamMembers, createIdentityUser } from '@/server/admin-data.functions'
import { initialInviteForm, sanitizeInviteForm } from '@/routes/identity/-team-form'
import type { UsersTransition } from '@/routes/identity/-use-users-transition'
import { withAuthMode } from '@/routes/identity/-user-form'
import type {
  CreateUserInput,
  CreateUserResult,
  IdentityTeamsPayload,
  TeamManagementView,
} from '@/types/api'

export type AddMembers = ReturnType<typeof useAddMembers>

/**
 * State and actions for one team's `Add members` dialog: the existing users to add, and the invite
 * form with the onboarding URL it produced. Opening or closing the dialog clears all of it.
 */
export function useAddMembers({
  teams,
  oidcProviders,
  oauthProviders,
  transition,
}: {
  teams: TeamManagementView[]
  oidcProviders: IdentityTeamsPayload['oidc_providers']
  oauthProviders: IdentityTeamsPayload['oauth_providers']
  transition: UsersTransition
}) {
  const [teamId, setTeamId] = useState<string | null>(null)
  const [selectedUserIds, setSelectedUserIds] = useState<string[]>([])
  const [inviteForm, setInviteForm] = useState<CreateUserInput>(initialInviteForm)
  const [inviteResult, setInviteResult] = useState<CreateUserResult | null>(null)
  const team = teamId === null ? null : (teams.find((candidate) => candidate.id === teamId) ?? null)

  function open(target: TeamManagementView) {
    setTeamId(target.id)
    setSelectedUserIds([])
    setInviteResult(null)
    setInviteForm({
      ...initialInviteForm,
      team_id: target.id,
      team_role: 'member',
    })
  }

  function close() {
    setTeamId(null)
    setSelectedUserIds([])
    setInviteResult(null)
    setInviteForm(initialInviteForm)
  }

  function setInviteAuthMode(authMode: string) {
    setInviteForm((current) => withAuthMode(current, authMode, oidcProviders, oauthProviders))
  }

  function addSelectedUsers(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!team || selectedUserIds.length === 0) {
      return
    }

    transition.run(async () => {
      await addIdentityTeamMembers({
        data: {
          teamId: team.id,
          input: { user_ids: selectedUserIds },
        },
      })
      toast.success('Members added')
      setSelectedUserIds([])
      await transition.refreshUsers()
    })
  }

  function inviteMember(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!team) {
      return
    }

    transition.run(async () => {
      const response = await createIdentityUser({
        data: sanitizeInviteForm(inviteForm, oidcProviders, oauthProviders),
      })
      setInviteResult(response.data)
      toast.success(
        response.data.kind === 'password_invite'
          ? 'Member invite created'
          : 'Member sign-in URL created',
      )
      await transition.refreshUsers()
    })
  }

  return {
    isOpen: teamId !== null,
    team,
    selectedUserIds,
    setSelectedUserIds,
    inviteForm,
    setInviteForm,
    setInviteAuthMode,
    inviteResult,
    open,
    close,
    addSelectedUsers,
    inviteMember,
  }
}
