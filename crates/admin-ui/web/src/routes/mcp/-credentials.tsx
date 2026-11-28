import { useMemo, useState, type FormEvent } from 'react'
import { ArrowDown01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Field, FieldDescription, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { AdminTableHead, AdminTableHeader } from '@/components/shared/admin-table-header'
import { Table, TableBody, TableCell, TableRow } from '@/components/ui/table'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { formatCount, formatTimestamp } from '@/lib/format'
import { cn } from '@/lib/utils'
import type { McpCredentialBindingView } from '@/types/api'

import type { CredentialOwnerOption, CredentialOwnerOptions } from './-credential-owners'
import {
  CREDENTIAL_EXPIRY_OPTIONS,
  credentialExpiresAt,
  credentialOwnerChosen,
  type CredentialBindingFormState,
} from './-server-forms'

const CREDENTIAL_SCOPE_LABELS: Record<string, string> = {
  user: 'User',
  team: 'Team shared',
  service_account: 'Service account',
}

const CREDENTIAL_MATERIAL_LABELS: Record<string, string> = {
  bearer_token: 'Bearer token',
  static_header: 'Static header',
  oauth_tokens: 'OAuth bearer',
}

const CREDENTIAL_STORAGE_LABELS: Record<string, string> = {
  encrypted_blob: 'Encrypted secret',
  secret_ref: 'Secret reference',
}

// Platform admins pick owners by name once that owner's list loads; team admins, and anyone whose
// directory read failed, type the ID instead.
function CredentialOwnerField({
  form,
  owners,
  onFormChange,
}: {
  form: CredentialBindingFormState
  owners: CredentialOwnerOptions | null
  onFormChange: (form: CredentialBindingFormState) => void
}) {
  if (form.owner_scope_kind === 'user') {
    return owners?.users ? (
      <OwnerPicker
        id="mcp-credential-owner-user"
        label="User"
        placeholder="Choose a user"
        value={form.owner_user_id}
        options={owners.users}
        onValueChange={(value) => onFormChange({ ...form, owner_user_id: value })}
      />
    ) : (
      <OwnerIdInput
        id="mcp-credential-owner-user"
        label="User ID"
        value={form.owner_user_id}
        onValueChange={(value) => onFormChange({ ...form, owner_user_id: value })}
      />
    )
  }
  if (form.owner_scope_kind === 'team') {
    return owners?.teams ? (
      <OwnerPicker
        id="mcp-credential-owner-team"
        label="Team"
        placeholder="Choose a team"
        value={form.owner_team_id}
        options={owners.teams}
        onValueChange={(value) => onFormChange({ ...form, owner_team_id: value })}
      />
    ) : (
      <OwnerIdInput
        id="mcp-credential-owner-team"
        label="Team ID"
        value={form.owner_team_id}
        onValueChange={(value) => onFormChange({ ...form, owner_team_id: value })}
      />
    )
  }
  const serviceAccounts = owners?.serviceAccounts
  if (serviceAccounts) {
    // A service account belongs to one team, so picking it fills the team too.
    return (
      <OwnerPicker
        id="mcp-credential-owner-service-account"
        label="Service account"
        placeholder="Choose a service account"
        value={form.owner_service_account_id}
        options={serviceAccounts}
        onValueChange={(value) =>
          onFormChange({
            ...form,
            owner_service_account_id: value,
            owner_team_id:
              serviceAccounts.find((account) => account.id === value)?.teamId ?? form.owner_team_id,
          })
        }
      />
    )
  }
  return (
    <>
      <OwnerIdInput
        id="mcp-credential-owner-team"
        label="Team ID"
        value={form.owner_team_id}
        onValueChange={(value) => onFormChange({ ...form, owner_team_id: value })}
      />
      <OwnerIdInput
        id="mcp-credential-owner-service-account"
        label="Service account ID"
        value={form.owner_service_account_id}
        onValueChange={(value) => onFormChange({ ...form, owner_service_account_id: value })}
      />
    </>
  )
}

// The directory can hold thousands of owners, so the picker mounts its list only while open and
// shows at most this many matches; typing narrows the rest.
const OWNER_RESULT_LIMIT = 50

function OwnerPicker({
  id,
  label,
  placeholder,
  value,
  options,
  onValueChange,
}: {
  id: string
  label: string
  placeholder: string
  value: string
  options: CredentialOwnerOption[]
  onValueChange: (value: string) => void
}) {
  const [open, setOpen] = useState(false)
  const selected = options.find((option) => option.id === value)

  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            id={id}
            type="button"
            variant="outline"
            role="combobox"
            aria-label={label}
            aria-expanded={open}
            className="w-full justify-between font-normal"
          >
            <span className={cn('truncate', !selected && 'text-muted-foreground')}>
              {selected?.label ?? placeholder}
            </span>
            <AppIcon icon={ArrowDown01Icon} className="text-muted-foreground" />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-(--radix-popover-trigger-width) p-0">
          <OwnerSearchList
            label={label}
            options={options}
            onSelect={(ownerId) => {
              onValueChange(ownerId)
              setOpen(false)
            }}
          />
        </PopoverContent>
      </Popover>
    </Field>
  )
}

/**
 * The open picker's search and results. It mounts only while the popover is open, so a closed
 * picker never scans the directory, and the search clears each time it closes.
 */
function OwnerSearchList({
  label,
  options,
  onSelect,
}: {
  label: string
  options: CredentialOwnerOption[]
  onSelect: (ownerId: string) => void
}) {
  const [search, setSearch] = useState('')
  // Lowercase each label once per list, not on every keystroke.
  const index = useMemo(
    () => options.map((option) => ({ option, key: option.label.toLowerCase() })),
    [options],
  )
  const matches = useMemo(() => matchingOwners(index, search), [index, search])

  return (
    <Command shouldFilter={false}>
      <CommandInput
        aria-label={`Search ${label.toLowerCase()}`}
        placeholder="Search by name or email"
        value={search}
        onValueChange={setSearch}
      />
      <CommandList>
        <CommandEmpty>No matches.</CommandEmpty>
        <CommandGroup>
          {matches.shown.map((option) => (
            <CommandItem key={option.id} value={option.id} onSelect={() => onSelect(option.id)}>
              {option.label}
            </CommandItem>
          ))}
        </CommandGroup>
      </CommandList>
      {matches.total > matches.shown.length ? (
        <p className="text-muted-foreground border-t px-3 py-2 text-xs">
          Showing {matches.shown.length} of {formatCount(matches.total)}. Type to narrow.
        </p>
      ) : null}
    </Command>
  )
}

function matchingOwners(index: { option: CredentialOwnerOption; key: string }[], search: string) {
  const query = search.trim().toLowerCase()
  if (!query) {
    return {
      shown: index.slice(0, OWNER_RESULT_LIMIT).map((entry) => entry.option),
      total: index.length,
    }
  }
  const shown: CredentialOwnerOption[] = []
  let total = 0
  for (const entry of index) {
    if (!entry.key.includes(query)) continue
    total += 1
    if (shown.length < OWNER_RESULT_LIMIT) shown.push(entry.option)
  }
  return { shown, total }
}

function OwnerIdInput({
  id,
  label,
  value,
  onValueChange,
}: {
  id: string
  label: string
  value: string
  onValueChange: (value: string) => void
}) {
  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <Input
        id={id}
        value={value}
        onChange={(event) => onValueChange(event.target.value)}
        required
      />
    </Field>
  )
}

export function CredentialBindingsPanel({
  bindings,
  form,
  pending,
  error,
  teamScoped = false,
  owners = null,
  onFormChange,
  onSubmit,
  onRevoke,
}: {
  bindings: McpCredentialBindingView[]
  form: CredentialBindingFormState
  pending: boolean
  error: string | null
  /** Team admins set only team and service-account credentials. */
  teamScoped?: boolean
  /** Named owners to pick from; without them the form takes typed IDs. */
  owners?: CredentialOwnerOptions | null
  onFormChange: (form: CredentialBindingFormState) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
  onRevoke: (binding: McpCredentialBindingView) => void
}) {
  const activeBindings = bindings.filter((binding) => !binding.revoked_at)
  return (
    <div className="min-w-0 rounded-md border">
      <div className="flex flex-col gap-1 border-b p-4">
        <h3 className="font-medium">Credential bindings</h3>
        <p className="text-subtle-foreground text-sm">
          Principal-scoped upstream credentials for user passthrough and OAuth on-behalf-of modes.
        </p>
      </div>

      {error ? (
        <div className="border-destructive text-destructive m-4 rounded-md border p-3 text-sm">
          {error}
        </div>
      ) : null}

      <CredentialBindingForm
        form={form}
        pending={pending}
        teamScoped={teamScoped}
        owners={owners}
        onFormChange={onFormChange}
        onSubmit={onSubmit}
      />
      <CredentialBindingsTable bindings={bindings} pending={pending} onRevoke={onRevoke} />
      <div className="text-subtle-foreground border-t px-4 py-3 text-sm">
        {activeBindings.length} active binding{activeBindings.length === 1 ? '' : 's'}
      </div>
    </div>
  )
}

function CredentialBindingForm({
  form,
  pending,
  teamScoped,
  owners,
  onFormChange,
  onSubmit,
}: {
  form: CredentialBindingFormState
  pending: boolean
  teamScoped: boolean
  owners: CredentialOwnerOptions | null
  onFormChange: (form: CredentialBindingFormState) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
}) {
  return (
    <form className="grid gap-4 border-b p-4" onSubmit={onSubmit}>
      <FieldGroup className="grid gap-3 md:grid-cols-3">
        <CredentialKindFields form={form} teamScoped={teamScoped} onFormChange={onFormChange} />
        <CredentialOwnerField form={form} owners={owners} onFormChange={onFormChange} />
        {form.material_kind === 'static_header' ? (
          <Field>
            <FieldLabel htmlFor="mcp-credential-header-name">Header name</FieldLabel>
            <Input
              id="mcp-credential-header-name"
              value={form.header_name}
              onChange={(event) => onFormChange({ ...form, header_name: event.target.value })}
              placeholder="X-API-Key"
              required
            />
          </Field>
        ) : null}
        {form.storage_mode === 'secret' ? (
          <Field>
            <FieldLabel htmlFor="mcp-credential-secret">Secret</FieldLabel>
            <Input
              id="mcp-credential-secret"
              type="password"
              value={form.secret}
              onChange={(event) => onFormChange({ ...form, secret: event.target.value })}
              required
            />
          </Field>
        ) : (
          <Field>
            <FieldLabel htmlFor="mcp-credential-secret-ref">Secret reference</FieldLabel>
            <Input
              id="mcp-credential-secret-ref"
              value={form.secret_ref}
              onChange={(event) => onFormChange({ ...form, secret_ref: event.target.value })}
              placeholder="env/OCEANS_MCP_CREDENTIAL_GITHUB"
              required
            />
          </Field>
        )}
        <CredentialExpiryField form={form} onFormChange={onFormChange} />
      </FieldGroup>
      <div className="flex justify-end">
        <Button type="submit" disabled={pending || !credentialOwnerChosen(form)}>
          {pending ? 'Saving...' : 'Add binding'}
        </Button>
      </div>
    </form>
  )
}

/** What the credential is: who it belongs to, what it holds, and where the secret lives. */
function CredentialKindFields({
  form,
  teamScoped,
  onFormChange,
}: {
  form: CredentialBindingFormState
  teamScoped: boolean
  onFormChange: (form: CredentialBindingFormState) => void
}) {
  return (
    <>
      <Field>
        <FieldLabel>Owner scope</FieldLabel>
        <Select
          value={form.owner_scope_kind}
          onValueChange={(value) =>
            onFormChange({
              ...form,
              owner_scope_kind: value as CredentialBindingFormState['owner_scope_kind'],
              // Team shared may have changed the team since an account was picked. Clearing the
              // account means picking it again, which sets its own team.
              owner_service_account_id:
                value === 'service_account' ? '' : form.owner_service_account_id,
            })
          }
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              {teamScoped ? null : <SelectItem value="user">User</SelectItem>}
              <SelectItem value="team">Team shared</SelectItem>
              <SelectItem value="service_account">Service account</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
      </Field>
      <Field>
        <FieldLabel>Material</FieldLabel>
        <Select
          value={form.material_kind}
          onValueChange={(value) =>
            onFormChange({
              ...form,
              material_kind: value as CredentialBindingFormState['material_kind'],
            })
          }
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="bearer_token">Bearer token</SelectItem>
              <SelectItem value="static_header">Static header</SelectItem>
              <SelectItem value="oauth_tokens">OAuth bearer</SelectItem>
            </SelectGroup>
          </SelectContent>
        </Select>
      </Field>
      <Field>
        <FieldLabel>Storage</FieldLabel>
        <Select
          value={form.storage_mode}
          onValueChange={(value) =>
            onFormChange({
              ...form,
              storage_mode: value as CredentialBindingFormState['storage_mode'],
            })
          }
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectItem value="secret">Encrypted secret</SelectItem>
              {/* A reference names a gateway-wide variable, so it is platform-only. */}
              {teamScoped ? null : <SelectItem value="secret_ref">Secret reference</SelectItem>}
            </SelectGroup>
          </SelectContent>
        </Select>
      </Field>
    </>
  )
}

// Preset lifetimes rather than a native date-time input, which DESIGN.md rules out.
function CredentialExpiryField({
  form,
  onFormChange,
}: {
  form: CredentialBindingFormState
  onFormChange: (form: CredentialBindingFormState) => void
}) {
  return (
    <Field>
      <FieldLabel id="mcp-credential-expiry-label">Expires</FieldLabel>
      <ToggleGroup
        type="single"
        variant="checked"
        size="sm"
        spacing={1}
        value={form.expires_in}
        onValueChange={(value) => {
          const option = CREDENTIAL_EXPIRY_OPTIONS.find((item) => item.value === value)
          if (option) onFormChange({ ...form, expires_in: option.value })
        }}
        aria-labelledby="mcp-credential-expiry-label"
        className="w-full flex-wrap"
      >
        {CREDENTIAL_EXPIRY_OPTIONS.map((option) => (
          <ToggleGroupItem key={option.value} value={option.value} className="flex-1">
            {option.label}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>
      <FieldDescription>
        {form.expires_in === 'never'
          ? 'The credential stays valid until it is revoked.'
          : `Expires ${formatTimestamp(credentialExpiresAt(form.expires_in))}.`}
      </FieldDescription>
    </Field>
  )
}

function CredentialBindingsTable({
  bindings,
  pending,
  onRevoke,
}: {
  bindings: McpCredentialBindingView[]
  pending: boolean
  onRevoke: (binding: McpCredentialBindingView) => void
}) {
  if (bindings.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No credential bindings</EmptyTitle>
          <EmptyDescription>
            Execution will use gateway-managed auth or require a binding.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }
  return (
    <div className="overflow-x-auto">
      <Table className="min-w-[72rem]">
        <AdminTableHeader>
          <AdminTableHead>Status</AdminTableHead>
          <AdminTableHead>Owner</AdminTableHead>
          <AdminTableHead>Material</AdminTableHead>
          <AdminTableHead>Storage</AdminTableHead>
          <AdminTableHead>Secret ref</AdminTableHead>
          <AdminTableHead>Expires</AdminTableHead>
          <AdminTableHead>Last used</AdminTableHead>
          {/* Pinned, so Revoke stays reachable while the wide table scrolls sideways. */}
          <AdminTableHead className="bg-surface-muted shadow-sticky-edge-end sticky right-0 z-10 text-right">
            Actions
          </AdminTableHead>
        </AdminTableHeader>
        <TableBody>
          {bindings.map((binding) => (
            <TableRow key={binding.id}>
              <TableCell>
                <Badge variant={binding.revoked_at ? 'destructive' : 'success'}>
                  {binding.revoked_at ? 'Revoked' : 'Active'}
                </Badge>
              </TableCell>
              <TableCell>
                <div className="flex min-w-0 flex-col gap-1">
                  <span>
                    {CREDENTIAL_SCOPE_LABELS[binding.owner_scope_kind] ?? binding.owner_scope_kind}
                  </span>
                  <span className="text-subtle-foreground truncate font-mono text-xs">
                    {binding.owner_scope_key}
                  </span>
                </div>
              </TableCell>
              <TableCell>
                {CREDENTIAL_MATERIAL_LABELS[binding.material_kind] ?? binding.material_kind}
                {binding.header_name ? ` (${binding.header_name})` : ''}
              </TableCell>
              <TableCell>
                {CREDENTIAL_STORAGE_LABELS[binding.storage_kind] ?? binding.storage_kind}
              </TableCell>
              <TableCell className="font-mono text-xs">
                {binding.secret_ref ?? 'Redacted'}
              </TableCell>
              <TableCell className="whitespace-nowrap">
                {binding.expires_at ? formatTimestamp(binding.expires_at) : 'Never'}
              </TableCell>
              <TableCell className="whitespace-nowrap">
                {formatTimestamp(binding.last_used_at)}
              </TableCell>
              <TableCell className="bg-popover shadow-sticky-edge-end sticky right-0 z-10 text-right">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => onRevoke(binding)}
                  disabled={pending || Boolean(binding.revoked_at)}
                >
                  Revoke
                </Button>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
