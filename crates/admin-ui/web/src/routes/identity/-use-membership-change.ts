import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { removeIdentityTeamMember, transferIdentityTeamMember } from '@/server/admin-data.functions'
import { initialTransferForm, sanitizeTransferForm } from '@/routes/identity/-team-form'
import type { UsersTransition } from '@/routes/identity/-use-users-transition'
import type {
  TeamAssignableUserView,
  TeamManagementView,
  TransferTeamMemberInput,
} from '@/types/api'

type MembershipChangeTarget =
  | { mode: 'closed' }
  | { mode: 'remove'; teamId: string; userId: string }
  | { mode: 'transfer'; teamId: string; userId: string }

export type MembershipChange = ReturnType<typeof useMembershipChange>

/**
 * State and actions for the `Remove member` and `Transfer member` dialogs: the member they act on,
 * the transfer destination, and the change itself. A change that succeeds closes its dialog.
 */
export function useMembershipChange({
  teams,
  users,
  transition,
}: {
  teams: TeamManagementView[]
  users: TeamAssignableUserView[]
  transition: UsersTransition
}) {
  const [target, setTarget] = useState<MembershipChangeTarget>({ mode: 'closed' })
  const [transferForm, setTransferForm] = useState<TransferTeamMemberInput>(initialTransferForm)
  const team =
    target.mode === 'closed'
      ? null
      : (teams.find((candidate) => candidate.id === target.teamId) ?? null)
  const user =
    target.mode === 'closed'
      ? null
      : (users.find((candidate) => candidate.id === target.userId) ?? null)

  function openRemove(teamId: string, userId: string) {
    setTarget({ mode: 'remove', teamId, userId })
  }

  function openTransfer(teamId: string, userId: string) {
    setTarget({ mode: 'transfer', teamId, userId })
    setTransferForm(initialTransferForm)
  }

  function close() {
    setTarget({ mode: 'closed' })
    setTransferForm(initialTransferForm)
  }

  function removeMember() {
    if (target.mode !== 'remove') {
      return
    }
    const { teamId, userId } = target

    transition.run(async () => {
      await removeIdentityTeamMember({ data: { teamId, userId } })
      toast.success('Member removed')
      await transition.refreshUsers()
      close()
    })
  }

  function transferMember(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (target.mode !== 'transfer' || !team) {
      return
    }
    const { teamId, userId } = target

    transition.run(async () => {
      await transferIdentityTeamMember({
        data: {
          teamId,
          userId,
          input: sanitizeTransferForm(transferForm, team),
        },
      })
      toast.success('Member transferred')
      await transition.refreshUsers()
      close()
    })
  }

  return {
    mode: target.mode,
    team,
    user,
    transferForm,
    setTransferForm,
    openRemove,
    openTransfer,
    close,
    removeMember,
    transferMember,
  }
}
