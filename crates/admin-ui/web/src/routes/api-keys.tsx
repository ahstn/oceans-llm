import { useMemo, useState, useTransition, type FormEvent } from 'react'
import { SearchIcon } from '@hugeicons/core-free-icons'
import { createFileRoute, useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
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
import { requireAdminSession } from '@/routes/-admin-guard'
import {
  createGatewayApiKey,
  getApiKeys,
  revokeGatewayApiKey,
} from '@/server/admin-data.functions'
import type { ApiKeysPayload, CreateApiKeyInput, CreateApiKeyResult } from '@/types/api'

export const Route = createFileRoute('/api-keys')({
  beforeLoad: ({ location }) => requireAdminSession(location),
  loader: () => getApiKeys(),
  component: ApiKeysPage,
})

const initialForm: CreateApiKeyInput = {
  name: '',
  owner_kind: 'user',
  owner_user_id: null,
  owner_team_id: null,
  model_keys: [],
}

type RevokeDialogState = { mode: 'closed' } | { mode: 'open'; apiKeyId: string }

function ApiKeysPage() {
  const router = useRouter()
  const {
    data: { items, users, teams, models },
  } = Route.useLoaderData() as { data: ApiKeysPayload }
  const [isCreateOpen, setIsCreateOpen] = useState(false)
  const [form, setForm] = useState<CreateApiKeyInput>(initialForm)
  const [createdResult, setCreatedResult] = useState<CreateApiKeyResult | null>(null)
  const [revokeDialog, setRevokeDialog] = useState<RevokeDialogState>({ mode: 'closed' })
  const [isPending, startTransition] = useTransition()

  const selectedOwnerLabel = useMemo(() => {
    if (form.owner_kind === 'user') {
      return users.find((user) => user.id === form.owner_user_id)?.name ?? 'Select a user'
    }
    return teams.find((team) => team.id === form.owner_team_id)?.name ?? 'Select a team'
  }, [form.owner_kind, form.owner_team_id, form.owner_user_id, teams, users])

  const revokeTarget =
    revokeDialog.mode === 'open'
      ? (items.find((item) => item.id === revokeDialog.apiKeyId) ?? null)
      : null

  async function refreshApiKeys() {
    await router.invalidate()
  }

  function openCreateDialog() {
    setForm(initialForm)
    setIsCreateOpen(true)
  }

  function closeCreateDialog() {
    setForm(initialForm)
    setIsCreateOpen(false)
  }

  function openRevokeDialog(apiKeyId: string) {
    setRevokeDialog({ mode: 'open', apiKeyId })
  }

  function closeRevokeDialog() {
    setRevokeDialog({ mode: 'closed' })
  }

  function updateOwnerKind(ownerKind: CreateApiKeyInput['owner_kind']) {
    setForm((current) => ({
      ...current,
      owner_kind: ownerKind,
      owner_user_id: ownerKind === 'user' ? current.owner_user_id : null,
      owner_team_id: ownerKind === 'team' ? current.owner_team_id : null,
    }))
  }

  function toggleModelKey(modelKey: string, checked: boolean) {
    setForm((current) => ({
      ...current,
      model_keys: checked
        ? [...current.model_keys, modelKey]
        : current.model_keys.filter((existing) => existing !== modelKey),
    }))
  }

  async function handleCreateApiKey(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    startTransition(async () => {
      try {
        const response = await createGatewayApiKey({
          data: {
            ...form,
            name: form.name.trim(),
          },
        })
        setCreatedResult(response.data)
        toast.success('API key created')
        await refreshApiKeys()
        closeCreateDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleRevokeApiKey() {
    if (revokeDialog.mode !== 'open') {
      return
    }

    startTransition(async () => {
      try {
        await revokeGatewayApiKey({
          data: { apiKeyId: revokeDialog.apiKeyId },
        })
        toast.success('API key revoked')
        await refreshApiKeys()
        closeRevokeDialog()
      } catch (error) {
        toast.error(getErrorMessage(error))
      }
    })
  }

  async function handleCopy(value: string, successMessage: string) {
    try {
      await navigator.clipboard.writeText(value)
      toast.success(successMessage)
    } catch {
      toast.error('Clipboard access failed')
    }
  }

  return (
    <div className="flex flex-col gap-4">
      {createdResult ? (
        <Alert>
          <AlertTitle>Copy the new key now</AlertTitle>
          <AlertDescription className="flex flex-col gap-3">
            <p>
              The raw secret is shown once. It is not stored in the control plane and cannot be
              revealed again later.
            </p>
            <div className="rounded-lg border border-[color:var(--color-border)] bg-[color:var(--color-surface-muted)] p-3">
              <p
                data-testid="new-api-key-raw-key"
                className="break-all font-mono text-xs text-[var(--color-text)]"
              >
                {createdResult.raw_key}
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                onClick={() => handleCopy(createdResult.raw_key, 'API key copied')}
              >
                Copy API key
              </Button>
              <Button type="button" variant="ghost" onClick={() => setCreatedResult(null)}>
                Dismiss
              </Button>
            </div>
          </AlertDescription>
        </Alert>
      ) : null}

      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <CardTitle>API Keys</CardTitle>
            <CardDescription>
              Issue gateway credentials with explicit owners and model grants, then revoke them when
              access should stop.
            </CardDescription>
          </div>
          <Button type="button" onClick={openCreateDialog}>
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
                <Button type="button" onClick={openCreateDialog}>
                  Create the first key
                </Button>
              </EmptyContent>
            </Empty>
          ) : (
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
                          {item.prefix}
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
                        onClick={() => handleCopy(item.prefix, 'Key prefix copied')}
                      >
                        Copy prefix
                      </Button>
                      {item.status === 'active' ? (
                        <Button
                          type="button"
                          variant="secondary"
                          className="bg-[#5e2a1f] text-white hover:bg-[#4a2017]"
                          onClick={() => openRevokeDialog(item.id)}
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
                              onClick={() => handleCopy(item.prefix, 'Key prefix copied')}
                            >
                              Copy prefix
                            </Button>
                            {item.status === 'active' ? (
                              <Button
                                type="button"
                                variant="secondary"
                                className="bg-[#5e2a1f] text-white hover:bg-[#4a2017]"
                                onClick={() => openRevokeDialog(item.id)}
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
          )}
        </CardContent>
      </Card>

      <Dialog open={isCreateOpen} onOpenChange={(open) => !open && closeCreateDialog()}>
        <DialogContent className="w-[min(760px,calc(100vw-32px))]">
          <DialogHeader>
            <DialogTitle>Create API key</DialogTitle>
            <DialogDescription>
              Keys are created with an explicit owner and model grant set. The raw secret is only
              shown once after creation.
            </DialogDescription>
          </DialogHeader>

          <form className="flex flex-col gap-6" onSubmit={handleCreateApiKey}>
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="api-key-name">Name</FieldLabel>
                <Input
                  id="api-key-name"
                  value={form.name}
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      name: event.currentTarget.value,
                    }))
                  }
                  placeholder="Production web"
                  autoComplete="off"
                />
                <FieldDescription>Use a name that makes rotation and revocation obvious later.</FieldDescription>
              </Field>

              <Field>
                <FieldLabel>Owner type</FieldLabel>
                <Select value={form.owner_kind} onValueChange={updateOwnerKind}>
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
                      ? (form.owner_user_id ?? undefined)
                      : (form.owner_team_id ?? undefined)
                  }
                  onValueChange={(value) =>
                    setForm((current) => ({
                      ...current,
                      owner_user_id: current.owner_kind === 'user' ? value : null,
                      owner_team_id: current.owner_kind === 'team' ? value : null,
                    }))
                  }
                >
                  <SelectTrigger aria-label={form.owner_kind === 'user' ? 'Owner user' : 'Owner team'}>
                    <SelectValue placeholder={selectedOwnerLabel} />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {form.owner_kind === 'user'
                        ? users.map((user) => (
                            <SelectItem key={user.id} value={user.id}>
                              {user.name} ({user.email})
                            </SelectItem>
                          ))
                        : teams.map((team) => (
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
                {models.map((model) => (
                  <label
                    key={model.key}
                    className="flex items-start gap-3 rounded-md border border-[color:var(--color-border)] bg-[color:var(--color-surface)] p-3"
                  >
                    <input
                      type="checkbox"
                      checked={form.model_keys.includes(model.key)}
                      onChange={(event) => toggleModelKey(model.key, event.currentTarget.checked)}
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
              <Button type="button" variant="secondary" onClick={closeCreateDialog}>
                Cancel
              </Button>
              <Button
                type="submit"
                disabled={
                  isPending ||
                  form.name.trim().length === 0 ||
                  form.model_keys.length === 0 ||
                  (form.owner_kind === 'user' ? !form.owner_user_id : !form.owner_team_id)
                }
              >
                {isPending ? 'Creating...' : 'Create API key'}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={revokeDialog.mode === 'open'}
        onOpenChange={(open) => !open && closeRevokeDialog()}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Revoke API key</DialogTitle>
            <DialogDescription>
              {revokeTarget
                ? `Revoke ${revokeTarget.name}. Existing callers will stop authenticating immediately.`
                : 'Revoke this key.'}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="secondary" onClick={closeRevokeDialog}>
              Cancel
            </Button>
            <Button
              type="button"
              variant="secondary"
              className="bg-[#5e2a1f] text-white hover:bg-[#4a2017]"
              onClick={handleRevokeApiKey}
              disabled={isPending}
            >
              {isPending ? 'Revoking...' : 'Revoke key'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

function formatOwner(item: ApiKeysPayload['items'][number]) {
  if (item.owner_kind === 'user') {
    return item.owner_email ? `${item.owner_name} (${item.owner_email})` : item.owner_name
  }
  return item.owner_team_key ? `${item.owner_name} (${item.owner_team_key})` : item.owner_name
}

function getErrorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }
  return 'Request failed'
}
