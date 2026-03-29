import type { FormEvent } from 'react'
import { SearchIcon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
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
} from '@/types/api'

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
            className="break-all font-mono text-xs text-[var(--color-text)]"
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
  onCopyPrefix,
  onRevoke,
}: {
  items: ApiKeyView[]
  onCreate: () => void
  onCopyPrefix: (value: string, successMessage: string) => void | Promise<void>
  onRevoke: (apiKeyId: string) => void
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
          <ApiKeyList items={items} onCopyPrefix={onCopyPrefix} onRevoke={onRevoke} />
        )}
      </CardContent>
    </Card>
  )
}

export function ApiKeyList({
  items,
  onCopyPrefix,
  onRevoke,
}: {
  items: ApiKeyView[]
  onCopyPrefix: (value: string, successMessage: string) => void | Promise<void>
  onRevoke: (apiKeyId: string) => void
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
                <p className="font-mono text-xs text-[var(--color-text-soft)]">{item.prefix}</p>
              </div>
              <Badge variant={item.status === 'active' ? 'success' : 'warning'}>{item.status}</Badge>
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
                <dd className="text-[var(--color-text)]">{item.created_at}</dd>
              </div>
              <div>
                <dt className="text-[var(--color-text-soft)]">Last used</dt>
                <dd className="text-[var(--color-text)]">{item.last_used_at ?? 'Never'}</dd>
              </div>
            </dl>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button
                type="button"
                variant="ghost"
                onClick={() => onCopyPrefix(item.prefix, 'Key prefix copied')}
              >
                Copy prefix
              </Button>
              {item.status === 'active' ? (
                <Button
                  type="button"
                  variant="secondary"
                  className="bg-[#5e2a1f] text-white hover:bg-[#4a2017]"
                  onClick={() => onRevoke(item.id)}
                >
                  Revoke
                </Button>
              ) : null}
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
                      {item.prefix}
                    </span>
                  </div>
                </td>
                <td className="px-3 py-3 text-[var(--color-text)]">{formatOwner(item)}</td>
                <td className="px-3 py-3 text-[var(--color-text-muted)]">
                  {item.model_keys.join(', ')}
                </td>
                <td className="px-3 py-3 text-[var(--color-text-muted)]">{item.created_at}</td>
                <td className="px-3 py-3 text-[var(--color-text-muted)]">
                  {item.last_used_at ?? 'Never'}
                </td>
                <td className="px-3 py-3">
                  <Badge variant={item.status === 'active' ? 'success' : 'warning'}>
                    {item.status}
                  </Badge>
                </td>
                <td className="px-3 py-3">
                  <div className="flex flex-wrap gap-2">
                    <Button
                      type="button"
                      variant="ghost"
                      onClick={() => onCopyPrefix(item.prefix, 'Key prefix copied')}
                    >
                      Copy prefix
                    </Button>
                    {item.status === 'active' ? (
                      <Button
                        type="button"
                        variant="secondary"
                        className="bg-[#5e2a1f] text-white hover:bg-[#4a2017]"
                        onClick={() => onRevoke(item.id)}
                      >
                        Revoke
                      </Button>
                    ) : null}
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
      <DialogContent className="w-[min(760px,calc(100vw-32px))]">
        <DialogHeader>
          <DialogTitle>Create API key</DialogTitle>
          <DialogDescription>
            Keys are created with an explicit owner and model grant set. The raw secret is only
            shown once after creation.
          </DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-6" onSubmit={onSubmit}>
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
              <FieldDescription>Use a name that makes rotation and revocation obvious later.</FieldDescription>
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
                  form.owner_kind === 'user'
                    ? (form.owner_user_id ?? '')
                    : (form.owner_team_id ?? '')
                }
                onValueChange={onOwnerSelectionChange}
              >
                <SelectTrigger aria-label={form.owner_kind === 'user' ? 'Owner user' : 'Owner team'}>
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

          <Field>
            <FieldLabel>Granted models</FieldLabel>
            <FieldDescription>
              Choose the exact gateway models this key can access. No implicit grants are added.
            </FieldDescription>
            <div className="grid max-h-72 gap-3 overflow-auto rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-3">
              {modelOptions.map((model) => (
                <label
                  key={model.key}
                  className="flex items-start gap-3 rounded-md border border-[color:var(--color-border)] bg-[color:var(--color-surface)] p-3"
                >
                  <input
                    type="checkbox"
                    checked={form.model_keys.includes(model.key)}
                    onChange={(event) => onModelToggle(model.key, event.currentTarget.checked)}
                  />
                  <span className="flex flex-col gap-1">
                    <span className="font-medium text-[var(--color-text)]">{model.key}</span>
                    {model.description ? (
                      <span className="text-sm text-[var(--color-text-muted)]">
                        {model.description}
                      </span>
                    ) : null}
                    {model.tags.length > 0 ? (
                      <span className="text-xs text-[var(--color-text-soft)]">
                        {model.tags.join(' • ')}
                      </span>
                    ) : null}
                  </span>
                </label>
              ))}
            </div>
          </Field>

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

export function RevokeApiKeyDialog({
  isPending,
  open,
  target,
  onConfirm,
  onOpenChange,
}: {
  isPending: boolean
  open: boolean
  target: ApiKeyView | null
  onConfirm: () => void | Promise<void>
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Revoke API key</DialogTitle>
          <DialogDescription>
            {target
              ? `Revoke ${target.name}. Existing callers will stop authenticating immediately.`
              : 'Revoke this key.'}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button type="button" variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            type="button"
            variant="secondary"
            className="bg-[#5e2a1f] text-white hover:bg-[#4a2017]"
            onClick={onConfirm}
            disabled={isPending}
          >
            {isPending ? 'Revoking...' : 'Revoke key'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function formatOwner(item: ApiKeyView) {
  if (item.owner_kind === 'user') {
    return item.owner_email ? `${item.owner_name} (${item.owner_email})` : item.owner_name
  }
  return item.owner_team_key ? `${item.owner_name} (${item.owner_team_key})` : item.owner_name
}
