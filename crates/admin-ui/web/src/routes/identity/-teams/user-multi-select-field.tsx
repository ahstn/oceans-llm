import { useState } from 'react'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { TeamAssignableUserView } from '@/types/api'

/** A user the picker lists. A disabled user is shown with its reason but can't be picked. */
export interface UserOption {
  user: TeamAssignableUserView
  disabled: boolean
  reason: string | null
}

/** A searchable multi-select of users, with the picked users shown as badges below it. */
export function UserMultiSelectField({
  label,
  description,
  placeholder,
  users,
  selectedUserIds,
  onChange,
  emptyTitle,
  emptyDescription,
}: {
  label: string
  description: string
  placeholder: string
  users: UserOption[]
  selectedUserIds: string[]
  onChange: (userIds: string[]) => void
  emptyTitle: string
  emptyDescription: string
}) {
  const [open, setOpen] = useState(false)
  const { selectedIds, selectedUsers, selectableCount } = summarizeOptions(users, selectedUserIds)

  function toggleUser(userId: string) {
    if (selectedIds.has(userId)) {
      onChange(selectedUserIds.filter((value) => value !== userId))
      return
    }

    onChange([...selectedUserIds, userId])
  }

  return (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="secondary"
            className="w-full justify-between"
            disabled={users.length === 0}
          >
            <span className="truncate text-left">
              {selectedUsers.length > 0
                ? `${selectedUsers.length} selected`
                : selectableCount > 0
                  ? placeholder
                  : emptyTitle}
            </span>
            <span className="text-muted-foreground text-xs">▼</span>
          </Button>
        </PopoverTrigger>
        <PopoverContent className="p-0">
          <Command>
            <CommandInput placeholder="Search users…" />
            <CommandList>
              <CommandEmpty>No matching users.</CommandEmpty>
              <CommandGroup heading={label}>
                {users.map(({ user, disabled, reason }) => (
                  <CommandItem
                    key={user.id}
                    value={`${user.name} ${user.email}`}
                    disabled={disabled}
                    onSelect={() => {
                      if (!disabled) {
                        toggleUser(user.id)
                      }
                    }}
                  >
                    <span className="text-muted-foreground w-4">
                      {selectedIds.has(user.id) ? '✓' : ''}
                    </span>
                    <GeneratedAvatar kind="user" name={user.name || user.email} size={28} />
                    <div className="flex min-w-0 flex-1 flex-col gap-1">
                      <span className="truncate">{user.name}</span>
                      <span className="text-muted-foreground truncate text-xs">
                        {user.email}
                        {reason ? ` · ${reason}` : ''}
                      </span>
                    </div>
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>

      {selectedUsers.length > 0 ? (
        <div className="flex flex-wrap gap-2">
          {selectedUsers.map((user) => (
            <Badge key={user.id}>{user.name}</Badge>
          ))}
        </div>
      ) : (
        <FieldDescription>{description}</FieldDescription>
      )}

      {selectableCount === 0 ? (
        <Alert>
          <AlertTitle>{emptyTitle}</AlertTitle>
          <AlertDescription>{emptyDescription}</AlertDescription>
        </Alert>
      ) : null}
    </Field>
  )
}

/** The picked users in list order, and how many users can be picked, in one pass. */
function summarizeOptions(users: UserOption[], selectedUserIds: string[]) {
  const selectedIds = new Set(selectedUserIds)
  const selectedUsers: TeamAssignableUserView[] = []
  let selectableCount = 0
  for (const { user, disabled } of users) {
    if (selectedIds.has(user.id)) {
      selectedUsers.push(user)
    }
    if (!disabled) {
      selectableCount += 1
    }
  }
  return { selectedIds, selectedUsers, selectableCount }
}
