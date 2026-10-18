import { useState, type FormEvent } from 'react'
import { toast } from 'sonner'

import { createIdentityTeam, updateIdentityTeam } from '@/server/admin-data.functions'
import { initialTeamForm, sanitizeTeamForm } from '@/routes/identity/-teams/team-form'
import type { IdentityTransition } from '@/routes/identity/-use-identity-transition'
import type { CreateTeamInput, TeamManagementView } from '@/types/api'

type TeamEditorTarget = { mode: 'closed' } | { mode: 'create' } | { mode: 'edit'; teamId: string }

export type TeamEditor = ReturnType<typeof useTeamEditor>

/**
 * State and the save for the `Add team` and `Edit team` dialog. Opening the dialog loads the form,
 * and closing it clears the form.
 */
export function useTeamEditor({
  teams,
  transition,
}: {
  teams: TeamManagementView[]
  transition: IdentityTransition
}) {
  const [target, setTarget] = useState<TeamEditorTarget>({ mode: 'closed' })
  const [form, setForm] = useState<CreateTeamInput>(initialTeamForm)
  const editingTeam =
    target.mode === 'edit' ? (teams.find((team) => team.id === target.teamId) ?? null) : null

  function openCreate() {
    setForm(initialTeamForm)
    setTarget({ mode: 'create' })
  }

  function openEdit(team: TeamManagementView) {
    setForm({
      name: team.name,
      admin_user_ids: team.admins.map((admin) => admin.id),
      tags: team.tags,
    })
    setTarget({ mode: 'edit', teamId: team.id })
  }

  function close() {
    setTarget({ mode: 'closed' })
    setForm(initialTeamForm)
  }

  function saveTeam(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    transition.run(async () => {
      if (target.mode === 'edit' && editingTeam) {
        await updateIdentityTeam({
          data: {
            teamId: editingTeam.id,
            input: sanitizeTeamForm(form),
          },
        })
        toast.success('Team updated')
      } else {
        await createIdentityTeam({ data: sanitizeTeamForm(form) })
        toast.success('Team created')
      }
      await transition.refresh()
      close()
    })
  }

  return {
    isOpen: target.mode !== 'closed',
    isEditing: target.mode === 'edit',
    editingTeam,
    form,
    setForm,
    openCreate,
    openEdit,
    close,
    saveTeam,
  }
}
