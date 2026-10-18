import { useMemo } from 'react'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { EntityTagsField } from '@/routes/identity/-entity-tags'
import { adminOptionsFor } from '@/routes/identity/-teams/team-form'
import type { TeamEditor } from '@/routes/identity/-teams/use-team-editor'
import { UserMultiSelectField } from '@/routes/identity/-teams/user-multi-select-field'
import type { TeamAssignableUserView } from '@/types/api'

/** The `Add team` and `Edit team` dialog: the team's name, its admins, and its tags. */
export function TeamEditorDialog({
  editor,
  users,
  isPending,
}: {
  editor: TeamEditor
  users: TeamAssignableUserView[]
  isPending: boolean
}) {
  const { form, setForm, editingTeam, isEditing } = editor
  const adminOptions = useMemo(() => adminOptionsFor(users, editingTeam), [users, editingTeam])

  return (
    <Dialog open={editor.isOpen} onOpenChange={(open) => !open && editor.close()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{isEditing ? 'Edit team' : 'Add team'}</DialogTitle>
          <DialogDescription>
            {isEditing
              ? 'Rename the team and manage which users hold team admin access.'
              : 'Create a team now and optionally assign team admins from existing users.'}
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-6" onSubmit={editor.saveTeam}>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="team-name">Name</FieldLabel>
              <Input
                id="team-name"
                value={form.name}
                onChange={(event) =>
                  setForm((current) => ({ ...current, name: event.target.value }))
                }
                placeholder="Core Platform"
                required
              />
            </Field>

            <UserMultiSelectField
              label="Team admins"
              description="Admins can manage the team after creation. You can leave this empty and assign them later."
              placeholder="Select team admins"
              users={adminOptions}
              selectedUserIds={form.admin_user_ids}
              onChange={(adminUserIds) =>
                setForm((current) => ({ ...current, admin_user_ids: adminUserIds }))
              }
              emptyTitle="No assignable admins yet"
              emptyDescription="Create the team now and return later once users exist."
            />

            <EntityTagsField
              label="Tags"
              tags={form.tags}
              onChange={(tags) => setForm((current) => ({ ...current, tags }))}
            />
          </FieldGroup>

          <DialogFooter>
            <Button type="button" variant="secondary" onClick={editor.close}>
              Cancel
            </Button>
            <Button type="submit" disabled={isPending}>
              {saveLabel(isEditing, isPending)}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function saveLabel(isEditing: boolean, isPending: boolean) {
  if (isEditing) {
    return isPending ? 'Saving…' : 'Save changes'
  }
  return isPending ? 'Creating…' : 'Create team'
}
