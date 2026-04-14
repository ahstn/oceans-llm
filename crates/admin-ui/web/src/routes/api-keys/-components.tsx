import { useState, type FormEvent } from 'react'
import { SearchIcon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type {
  ApiKeyModelOptionView,
  ApiKeyOwnerTeamView,
  ApiKeyOwnerUserView,
  ApiKeyView,
  CreateApiKeyInput,
  CreateApiKeyResult,
  UpdateApiKeyInput,
} from '@/types/api'

const apiKeyDialogContentClassName =
  'flex max-h-[calc(100dvh-2rem)] w-[min(760px,calc(100vw-32px))] flex-col overflow-hidden sm:max-h-[80vh]'

export function CreatedApiKeyAlert({
  result,
  onCopy,
  onDismiss,
}: {
  result: CreateApiKeyResult | null
  onCopy: (value: string, successMessage: string) => void | Promise<void>
  onDismiss: () => void
}) {
  if (!result) {
    return null
  }

  return (
    <Alert>
      <AlertTitle>Copy the new key now</AlertTitle>
      <div className="mt-1 flex flex-col gap-3 text-sm text-[var(--color-text-muted)]">
        <p>
          The raw secret is shown once. It is not stored in the control plane and cannot be
          revealed again later.
        </p>
        <div className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-3">
          <p
            data-testid="new-api-key-raw-key"
            className="font-mono text-xs break-all text-[var(--color-text)]"
          >
            {result.raw_key}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button type="button" onClick={() => onCopy(result.raw_key, 'API key copied')}>
            Copy API key
          </Button>
          <Button type="button" variant="ghost" onClick={onDismiss}>
            Dismiss
          </Button>
        </div>
      </div>
    </Alert>
  )
}

export function ApiKeysCard({
  items,
  onCreate,
  onManage,
}: {
  items: ApiKeyView[]
  onCreate: () => void
  onManage: (apiKeyId: string) => void
}) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div className="flex flex-col gap-1">
          <CardTitle>API Keys</CardTitle>
          <CardDescription>
            Issue gateway credentials with explicit owners and model grants, then revoke them when
            access should stop.
          </CardDescription>
        </div>
        <Button type="button" onClick={onCreate}>
          Create API key
        </Button>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {items.length === 0 ? (
          <Empty>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <AppIcon icon={SearchIcon} size={22} stroke={1.5} />
              </EmptyMedia>
              <EmptyTitle>No API keys yet</EmptyTitle>
              <EmptyDescription>
                Create a gateway key before distributing credentials to downstream clients.
              </EmptyDescription>
            </EmptyHeader>
            <EmptyContent>
              <Button type="button" onClick={onCreate}>
                Create the first key
              </Button>
            </EmptyContent>
          </Empty>
        ) : (
          <ApiKeyList items={items} onManage={onManage} />
        )}
      </CardContent>
    </Card>
  )
}

export function ApiKeyList({
  items,
  onManage,
}: {
  items: ApiKeyView[]
  onManage: (apiKeyId: string) => void
}) {
  return (
    <>
      <div className="grid gap-3 md:hidden">
        {items.map((item) => (
          <div
            key={item.id}
            className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="flex flex-col gap-1">
                <p className="font-semibold text-[var(--color-text)]">{item.name}</p>
                <p className="font-mono text-xs text-[var(--color-text-soft)]">
                  {maskApiKeyPrefix(item.prefix)}
                </p>
              </div>
              <Badge variant={item.status === 'active' ? 'success' : 'warning'}>
                {item.status}
              </Badge>
            </div>
            <dl className="mt-3 grid gap-2 text-sm">
              <div>
                <dt className="text-[var(--color-text-soft)]">Owner</dt>
                <dd className="text-[var(--color-text)]">{formatOwner(item)}</dd>
              </div>
              <div>
                <dt className="text-[var(--color-text-soft)]">Models</dt>
                <dd className="text-[var(--color-text)]">{item.model_keys.join(', ')}</dd>
              </div>
              <div>
                <dt className="text-[var(--color-text-soft)]">Created</dt>
                <dd className="text-[var(--color-text)]">{formatCreatedAt(item.created_at)}</dd>
              </div>
              <div>
                <dt className="text-[var(--color-text-soft)]">Last used</dt>
                <dd className="text-[var(--color-text)]">{formatLastUsedAt(item.last_used_at)}</dd>
              </div>
            </dl>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button type="button" variant="secondary" onClick={() => onManage(item.id)}>
                Manage
              </Button>
            </div>
          </div>
        ))}
      </div>

      <div className="hidden overflow-hidden rounded-md border border-[color:var(--color-border)] md:block">
        <table className="w-full text-left text-sm">
          <thead className="bg-[color:var(--color-surface-muted)] text-[var(--color-text-soft)]">
            <tr>
              <th className="px-3 py-2 font-semibold">Name</th>
              <th className="px-3 py-2 font-semibold">Owner</th>
              <th className="px-3 py-2 font-semibold">Granted models</th>
              <th className="px-3 py-2 font-semibold">Created</th>
              <th className="px-3 py-2 font-semibold">Last used</th>
              <th className="px-3 py-2 font-semibold">Status</th>
              <th className="px-3 py-2 font-semibold">Actions</th>
            </tr>
          </thead>
          <tbody>
            {items.map((item) => (
              <tr key={item.id} className="border-t border-[color:var(--color-border)] align-top">
                <td className="px-3 py-3">
                  <div className="flex flex-col gap-1">
                    <span className="font-semibold text-[var(--color-text)]">{item.name}</span>
                    <span className="font-mono text-xs text-[var(--color-text-soft)]">
                      {maskApiKeyPrefix(item.prefix)}
                    </span>
                  </div>
                </td>
                <td className="px-3 py-3 text-[var(--color-text)]">{formatOwner(item)}</td>
                <td className="px-3 py-3 text-[var(--color-text-muted)]">
                  {item.model_keys.join(', ')}
                </td>
                <td className="px-3 py-3 text-[var(--color-text-muted)]">
                  {formatCreatedAt(item.created_at)}
                </td>
                <td className="px-3 py-3 text-[var(--color-text-muted)]">
                  {formatLastUsedAt(item.last_used_at)}
                </td>
                <td className="px-3 py-3">
                  <Badge variant={item.status === 'active' ? 'success' : 'warning'}>
                    {item.status}
                  </Badge>
                </td>
                <td className="px-3 py-3">
                  <div className="flex flex-wrap gap-2">
                    <Button type="button" variant="secondary" onClick={() => onManage(item.id)}>
                      Manage
                    </Button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  )
}

export function CreateApiKeyDialog({
  form,
  isPending,
  modelOptions,
  open,
  ownerLabel,
  teamOptions,
  userOptions,
  submitDisabled,
  onModelToggle,
  onNameChange,
  onOpenChange,
  onOwnerKindChange,
  onOwnerSelectionChange,
  onSubmit,
}: {
  form: CreateApiKeyInput
  isPending: boolean
  modelOptions: ApiKeyModelOptionView[]
  open: boolean
  ownerLabel: string
  teamOptions: ApiKeyOwnerTeamView[]
  userOptions: ApiKeyOwnerUserView[]
  submitDisabled: boolean
  onModelToggle: (modelKey: string, checked: boolean) => void
  onNameChange: (name: string) => void
  onOpenChange: (open: boolean) => void
  onOwnerKindChange: (ownerKind: CreateApiKeyInput['owner_kind']) => void
  onOwnerSelectionChange: (value: string) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void | Promise<void>
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className={apiKeyDialogContentClassName}>
        <DialogHeader>
          <DialogTitle>Create API key</DialogTitle>
          <DialogDescription>
            Keys are created with an explicit owner and model grant set. The raw secret is only
            shown once after creation.
          </DialogDescription>
        </DialogHeader>

        <form className="flex min-h-0 flex-1 flex-col gap-6" onSubmit={onSubmit}>
          <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto pr-1">
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="api-key-name">Name</FieldLabel>
                <Input
                  id="api-key-name"
                  value={form.name}
                  onChange={(event) => onNameChange(event.currentTarget.value)}
                  placeholder="Production web"
                  autoComplete="off"
                />
                <FieldDescription>
                  Use a name that makes rotation and revocation obvious later.
                </FieldDescription>
              </Field>

              <Field>
                <FieldLabel>Owner type</FieldLabel>
                <Select value={form.owner_kind} onValueChange={onOwnerKindChange}>
                  <SelectTrigger aria-label="Owner type">
                    <SelectValue placeholder="Select an owner type" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="user">User</SelectItem>
                      <SelectItem value="team">Team</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>

              <Field>
                <FieldLabel>{form.owner_kind === 'user' ? 'Owner user' : 'Owner team'}</FieldLabel>
                <Select
                  value={
                    form.owner_kind === 'user' ? (form.owner_user_id ?? '') : (form.owner_team_id ?? '')
                  }
                  onValueChange={onOwnerSelectionChange}
                >
                  <SelectTrigger
                    aria-label={form.owner_kind === 'user' ? 'Owner user' : 'Owner team'}
                  >
                    <SelectValue placeholder={ownerLabel} />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {form.owner_kind === 'user'
                        ? userOptions.map((user) => (
                            <SelectItem key={user.id} value={user.id}>
                              {user.name} ({user.email})
                            </SelectItem>
                          ))
                        : teamOptions.map((team) => (
                            <SelectItem key={team.id} value={team.id}>
                              {team.name} ({team.key})
                            </SelectItem>
                          ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
            </FieldGroup>

            <ModelMultiSelectField
              description="Choose the exact gateway models this key can access. No implicit grants are added."
              label="Granted models"
              modelOptions={modelOptions}
              placeholder="Select models"
              searchPlaceholder="Search models…"
              selectedKeys={form.model_keys}
              onToggle={onModelToggle}
            />
          </div>

          <DialogFooter>
            <Button type="button" variant="secondary" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={submitDisabled}>
              {isPending ? 'Creating...' : 'Create API key'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function ManageApiKeyDialog({
  form,
  isPending,
  modelOptions,
  open,
  submitDisabled,
  target,
  onModelToggle,
  onOpenChange,
  onRevoke,
  onSubmit,
}: {
  form: UpdateApiKeyInput
  isPending: boolean
  modelOptions: ApiKeyModelOptionView[]
  open: boolean
  submitDisabled: boolean
  target: ApiKeyView | null
  onModelToggle: (modelKey: string, checked: boolean) => void
  onOpenChange: (open: boolean) => void
  onRevoke: () => void | Promise<void>
  onSubmit: (event: FormEvent<HTMLFormElement>) => void | Promise<void>
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className={apiKeyDialogContentClassName}>
        <DialogHeader>
          <DialogTitle>Manage API key</DialogTitle>
          <DialogDescription>
            Review model access and lifecycle state for this key. Revocation stops existing callers
            immediately.
          </DialogDescription>
        </DialogHeader>

        {target ? (
          <form className="flex min-h-0 flex-1 flex-col gap-6" onSubmit={onSubmit}>
            <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto pr-1">
              <section className="flex flex-col gap-3 rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex min-w-0 flex-col gap-1">
                    <p className="font-semibold text-[var(--color-text)]">{target.name}</p>
                    <p className="font-mono text-xs text-[var(--color-text-soft)]">
                      {maskApiKeyPrefix(target.prefix)}
                    </p>
                  </div>
                  <Badge variant={target.status === 'active' ? 'success' : 'warning'}>
                    {target.status}
                  </Badge>
                </div>

                <dl className="grid gap-3 text-sm sm:grid-cols-3">
                  <div className="flex flex-col gap-1">
                    <dt className="text-[var(--color-text-soft)]">Owner</dt>
                    <dd className="text-[var(--color-text)]">{formatOwner(target)}</dd>
                  </div>
                  <div className="flex flex-col gap-1">
                    <dt className="text-[var(--color-text-soft)]">Created</dt>
                    <dd className="text-[var(--color-text)]">{formatCreatedAt(target.created_at)}</dd>
                  </div>
                  <div className="flex flex-col gap-1">
                    <dt className="text-[var(--color-text-soft)]">Last used</dt>
                    <dd className="text-[var(--color-text)]">
                      {formatLastUsedAt(target.last_used_at)}
                    </dd>
                  </div>
                </dl>
              </section>

              {target.status === 'revoked' ? (
                <Alert>
                  <AlertTitle>Revoked keys are read-only</AlertTitle>
                  <AlertDescription>
                    This key has already been revoked. Model access can no longer be changed in the
                    admin UI.
                  </AlertDescription>
                </Alert>
              ) : null}

              <ModelMultiSelectField
                description={
                  target.status === 'active'
                    ? 'Save model access changes to apply them immediately.'
                    : 'Current model access is shown for reference only.'
                }
                disabled={target.status !== 'active'}
                label="Granted models"
                modelOptions={modelOptions}
                placeholder="Select models"
                searchPlaceholder="Search models…"
                selectedKeys={form.model_keys}
                onToggle={onModelToggle}
              />

              <section className="flex flex-col gap-3 rounded-lg border border-[color:var(--color-border)] p-4">
                <div className="flex flex-col gap-1">
                  <h3 className="text-sm font-semibold text-[var(--color-text)]">
                    Lifecycle actions
                  </h3>
                  <p className="text-sm text-[var(--color-text-muted)]">
                    Revocation takes effect immediately and cannot be undone in this slice.
                  </p>
                </div>

                {target.status === 'active' ? (
                  <div className="flex flex-wrap gap-2">
                    <Button
                      type="button"
                      variant="destructive"
                      onClick={onRevoke}
                      disabled={isPending}
                    >
                      {isPending ? 'Revoking...' : 'Revoke key'}
                    </Button>
                  </div>
                ) : (
                  <p className="text-sm text-[var(--color-text-muted)]">
                    This key has already been revoked.
                  </p>
                )}
              </section>
            </div>

            <DialogFooter>
              <Button type="button" variant="secondary" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={submitDisabled}>
                {isPending ? 'Saving...' : 'Save access'}
              </Button>
            </DialogFooter>
          </form>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

function ModelMultiSelectField({
  description,
  disabled = false,
  label,
  modelOptions,
  placeholder,
  searchPlaceholder,
  selectedKeys,
  onToggle,
}: {
  description: string
  disabled?: boolean
  label: string
  modelOptions: ApiKeyModelOptionView[]
  placeholder: string
  searchPlaceholder: string
  selectedKeys: string[]
  onToggle: (modelKey: string, checked: boolean) => void
}) {
  const [open, setOpen] = useState(false)
  const selectedModels = modelOptions.filter((model) => selectedKeys.includes(model.key))

  return (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="secondary"
            className="w-full justify-between"
            disabled={disabled || modelOptions.length === 0}
          >
            <span className="truncate text-left">
              {summarizeSelectedModels(selectedModels, placeholder, modelOptions.length === 0)}
            </span>
            <span className="text-xs text-[var(--color-text-soft)]">▼</span>
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-[var(--radix-popover-trigger-width)] p-0">
          <Command>
            <CommandInput placeholder={searchPlaceholder} />
            <CommandList>
              <CommandEmpty>No matching models.</CommandEmpty>
              <CommandGroup heading={label}>
                {modelOptions.map((model) => {
                  const isSelected = selectedKeys.includes(model.key)
                  return (
                    <CommandItem
                      key={model.key}
                      value={`${model.key} ${model.description ?? ''} ${model.tags.join(' ')}`.trim()}
                      disabled={disabled}
                      onSelect={() => {
                        if (!disabled) {
                          onToggle(model.key, !isSelected)
                        }
                      }}
                    >
                      <span className="w-4 text-[var(--color-text-soft)]">
                        {isSelected ? '✓' : ''}
                      </span>
                      <div className="flex min-w-0 flex-1 flex-col gap-1">
                        <span className="truncate font-medium">{model.key}</span>
                        {model.description ? (
                          <span className="truncate text-xs text-[var(--color-text-muted)]">
                            {model.description}
                          </span>
                        ) : null}
                        {model.tags.length > 0 ? (
                          <span className="truncate text-xs text-[var(--color-text-soft)]">
                            {model.tags.join(' • ')}
                          </span>
                        ) : null}
                      </div>
                    </CommandItem>
                  )
                })}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>

      {selectedModels.length > 0 ? (
        <div className="flex flex-col gap-2">
          <div className="flex flex-wrap gap-2">
            {selectedModels.map((model) => (
              <Badge key={model.key} variant="secondary">
                {model.key}
              </Badge>
            ))}
          </div>
          <FieldDescription>{description}</FieldDescription>
        </div>
      ) : (
        <FieldDescription>{description}</FieldDescription>
      )}
    </Field>
  )
}

function summarizeSelectedModels(
  selectedModels: ApiKeyModelOptionView[],
  placeholder: string,
  hasNoOptions: boolean,
) {
  if (hasNoOptions) {
    return 'No models available'
  }

  if (selectedModels.length === 0) {
    return placeholder
  }

  if (selectedModels.length === 1) {
    return selectedModels[0].key
  }

  return `${selectedModels.length} models selected`
}

function formatOwner(item: ApiKeyView) {
  return item.owner_name
}

function maskApiKeyPrefix(prefix: string) {
  return `${prefix.slice(0, 12)}****`
}

function formatCreatedAt(value: string) {
  return formatUtcDate(value)
}

function formatLastUsedAt(value: string | null) {
  return value ? formatUtcDateTime(value) : 'Never'
}

function formatUtcDate(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }

  return `${date.getUTCFullYear()}-${padDatePart(date.getUTCMonth() + 1)}-${padDatePart(date.getUTCDate())}`
}

function formatUtcDateTime(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }

  return `${formatUtcDate(value)} ${padDatePart(date.getUTCHours())}:${padDatePart(date.getUTCMinutes())}`
}

function padDatePart(value: number) {
  return value.toString().padStart(2, '0')
}
