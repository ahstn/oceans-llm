import type { FormEvent } from 'react'
import { Copy01Icon } from '@hugeicons/core-free-icons'
import { toast } from 'sonner'

import { AppIcon } from '@/components/icons/app-icon'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from '@/components/ui/empty'
import { Field, FieldGroup, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Textarea } from '@/components/ui/textarea'
import type {
  CreateMcpServerInput,
  McpCredentialBindingView,
  McpServerView,
  McpToolView,
  RecommendedMcpServerView,
  UpdateMcpServerInput,
  UpsertMcpCredentialBindingInput,
} from '@/types/api'

const AUTH_MODES = [
  { value: 'none', label: 'None' },
  { value: 'gateway_static_header', label: 'Gateway static header' },
  { value: 'gateway_bearer_token', label: 'Gateway bearer token' },
  { value: 'user_passthrough', label: 'User passthrough' },
  { value: 'oauth_obo', label: 'OAuth on-behalf-of' },
] as const

export type ServerFormState = {
  server_key: string
  display_name: string
  description: string
  server_url: string
  auth_mode: string
  auth_config: string
  timeout_ms: string
}

export type CredentialBindingFormState = {
  owner_scope_kind: 'user' | 'team' | 'service_account'
  owner_user_id: string
  owner_team_id: string
  owner_service_account_id: string
  material_kind: 'static_header' | 'bearer_token' | 'oauth_tokens'
  header_name: string
  storage_mode: 'secret' | 'secret_ref'
  secret: string
  secret_ref: string
  expires_at: string
}

export function ServerDetailHeader({
  server,
  refreshStatus,
  actionPending,
  onEdit,
  onDisable,
  onRefresh,
}: {
  server: McpServerView
  refreshStatus: string | null
  actionPending: boolean
  onEdit: (server: McpServerView) => void
  onDisable: (server: McpServerView) => void
  onRefresh: (server: McpServerView) => void
}) {
  return (
    <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
      <div className="min-w-0">
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="truncate text-lg font-semibold">{server.display_name}</h2>
          <ServerStatusBadge status={server.status} />
          <DiscoveryStatusBadge status={server.last_discovery_status} />
        </div>
        <div className="mt-1 truncate font-mono text-xs text-[var(--color-text-muted)]">
          /mcp/{server.server_key}
        </div>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          variant="outline"
          onClick={() => onRefresh(server)}
          disabled={actionPending || server.status !== 'active'}
        >
          {refreshStatus === 'pending' ? 'Refreshing...' : 'Refresh'}
        </Button>
        <Button type="button" variant="outline" onClick={() => onEdit(server)}>
          Edit
        </Button>
        <Button
          type="button"
          variant="destructive"
          onClick={() => onDisable(server)}
          disabled={actionPending || server.status !== 'active'}
        >
          Disable
        </Button>
      </div>
    </div>
  )
}

export function ServerOverviewPanel({
  server,
  refreshStatus,
  refreshErrorSummary,
}: {
  server: McpServerView
  refreshStatus: string | null
  refreshErrorSummary: string | null
}) {
  return (
    <div className="flex min-w-0 flex-col gap-4">
      {refreshStatus && refreshStatus !== 'pending' ? (
        <Alert variant={refreshStatus === 'success' ? 'default' : 'destructive'}>
          <AlertTitle>Discovery {refreshStatus}</AlertTitle>
          <AlertDescription>
            {refreshErrorSummary ??
              server.last_error_summary ??
              'Discovery metadata has been refreshed.'}
          </AlertDescription>
        </Alert>
      ) : null}

      <div className="grid gap-3 text-sm sm:grid-cols-2 xl:grid-cols-4">
        <DetailMetric label="Server URL" value={server.server_url} mono />
        <DetailMetric label="Auth mode" value={server.auth_mode} />
        <DetailMetric label="Timeout" value={`${server.timeout_ms} ms`} />
        <DetailMetric label="Last discovery" value={server.last_discovery_at ?? 'never'} />
        <DetailMetric label="Last success" value={server.last_successful_discovery_at ?? 'never'} />
        <DetailMetric label="Discovered tools" value={String(server.last_tool_count ?? 0)} />
        <DetailMetric label="Created" value={server.created_at} />
        <DetailMetric label="Updated" value={server.updated_at} />
      </div>
    </div>
  )
}

export function ServerToolsPanel({
  tools,
  toolsPending,
  toolsError,
  selectedToolIds,
  onToggleTool,
  onClearSelection,
  onAddToToolset,
}: {
  tools: McpToolView[]
  toolsPending: boolean
  toolsError: string | null
  selectedToolIds: string[]
  onToggleTool: (toolId: string) => void
  onClearSelection: () => void
  onAddToToolset: () => void
}) {
  const selected = new Set(selectedToolIds)

  return (
    <div className="min-w-0 rounded-md border" data-testid="mcp-server-tools">
      <div className="flex items-center justify-between gap-2 border-b p-4">
        <div>
          <h3 className="font-medium">Discovered tools</h3>
          <p className="text-sm text-[var(--color-text-muted)]">
            Select tools to bundle into a toolset — no UUID copy-paste required.
          </p>
        </div>
        <Badge variant="secondary">{tools.length}</Badge>
      </div>

      {selectedToolIds.length > 0 ? (
        <div className="flex flex-wrap items-center justify-between gap-2 border-b bg-[var(--color-muted)] px-4 py-3">
          <span className="text-sm font-medium">
            {selectedToolIds.length} tool{selectedToolIds.length === 1 ? '' : 's'} selected
          </span>
          <div className="flex gap-2">
            <Button type="button" variant="ghost" size="sm" onClick={onClearSelection}>
              Clear
            </Button>
            <Button type="button" size="sm" onClick={onAddToToolset}>
              Add to toolset
            </Button>
          </div>
        </div>
      ) : null}

      {toolsError ? (
        <Alert variant="destructive" className="m-4">
          <AlertTitle>Tool load failed</AlertTitle>
          <AlertDescription>{toolsError}</AlertDescription>
        </Alert>
      ) : toolsPending ? (
        <div className="grid gap-2 p-4">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : tools.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No tools discovered</EmptyTitle>
            <EmptyDescription>Run discovery after the server is reachable.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="overflow-x-auto">
          <Table className="min-w-[72rem]">
            <TableHeader>
              <TableRow>
                <TableHead className="w-8" />
                <TableHead>Tool</TableHead>
                <TableHead>Tool ID</TableHead>
                <TableHead>Status</TableHead>
                <TableHead>Schema</TableHead>
                <TableHead>Version</TableHead>
                <TableHead>First seen</TableHead>
                <TableHead>Last seen</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {tools.map((tool) => (
                <TableRow key={tool.id} data-state={selected.has(tool.id) ? 'selected' : undefined}>
                  <TableCell>
                    <input
                      type="checkbox"
                      aria-label={`Select ${tool.display_name}`}
                      className="size-4 cursor-pointer accent-[var(--color-primary)]"
                      checked={selected.has(tool.id)}
                      onChange={() => onToggleTool(tool.id)}
                    />
                  </TableCell>
                  <TableCell>
                    <div className="flex min-w-0 flex-col gap-1">
                      <span className="truncate font-medium">{tool.display_name}</span>
                      <span className="truncate font-mono text-xs text-[var(--color-text-muted)]">
                        {tool.upstream_name}
                      </span>
                    </div>
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-1">
                      <span className="max-w-[10rem] truncate font-mono text-xs text-[var(--color-text-muted)]">
                        {tool.id}
                      </span>
                      <CopyButton value={tool.id} label={`Copy ${tool.display_name} ID`} />
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge variant={tool.is_active ? 'default' : 'secondary'}>
                      {tool.is_active ? 'Active' : 'Inactive'}
                    </Badge>
                  </TableCell>
                  <TableCell className="font-mono text-xs">{tool.schema_hash}</TableCell>
                  <TableCell>{tool.schema_version}</TableCell>
                  <TableCell>{tool.first_discovered_at}</TableCell>
                  <TableCell>{tool.last_discovered_at}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  )
}

function CopyButton({ value, label }: { value: string; label: string }) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon-sm"
      aria-label={label}
      onClick={() => {
        void navigator.clipboard
          ?.writeText(value)
          .then(() => toast.success('Tool ID copied'))
          .catch(() => toast.error('Clipboard access failed'))
      }}
    >
      <AppIcon icon={Copy01Icon} size={14} stroke={1.5} aria-hidden />
    </Button>
  )
}

export function CredentialBindingsPanel({
  bindings,
  form,
  pending,
  error,
  onFormChange,
  onSubmit,
  onRevoke,
}: {
  bindings: McpCredentialBindingView[]
  form: CredentialBindingFormState
  pending: boolean
  error: string | null
  onFormChange: (form: CredentialBindingFormState) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
  onRevoke: (binding: McpCredentialBindingView) => void
}) {
  const activeBindings = bindings.filter((binding) => !binding.revoked_at)
  return (
    <div className="min-w-0 rounded-md border">
      <div className="flex flex-col gap-1 border-b p-4">
        <h3 className="font-medium">Credential bindings</h3>
        <p className="text-sm text-[var(--color-text-muted)]">
          Principal-scoped upstream credentials for user passthrough and OAuth on-behalf-of modes.
        </p>
      </div>

      {error ? (
        <div className="m-4 rounded-md border border-[var(--color-danger)] p-3 text-sm text-[var(--color-danger)]">
          {error}
        </div>
      ) : null}

      <form className="grid gap-4 border-b p-4" onSubmit={onSubmit}>
        <FieldGroup className="grid gap-3 md:grid-cols-3">
          <Field>
            <FieldLabel>Owner scope</FieldLabel>
            <Select
              value={form.owner_scope_kind}
              onValueChange={(value) =>
                onFormChange({
                  ...form,
                  owner_scope_kind: value as CredentialBindingFormState['owner_scope_kind'],
                })
              }
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="user">User</SelectItem>
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
                  <SelectItem value="secret_ref">Secret reference</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>

          {form.owner_scope_kind === 'user' ? (
            <Field>
              <FieldLabel htmlFor="mcp-credential-owner-user">User ID</FieldLabel>
              <Input
                id="mcp-credential-owner-user"
                value={form.owner_user_id}
                onChange={(event) => onFormChange({ ...form, owner_user_id: event.target.value })}
                required
              />
            </Field>
          ) : null}
          {form.owner_scope_kind === 'team' || form.owner_scope_kind === 'service_account' ? (
            <Field>
              <FieldLabel htmlFor="mcp-credential-owner-team">Team ID</FieldLabel>
              <Input
                id="mcp-credential-owner-team"
                value={form.owner_team_id}
                onChange={(event) => onFormChange({ ...form, owner_team_id: event.target.value })}
                required
              />
            </Field>
          ) : null}
          {form.owner_scope_kind === 'service_account' ? (
            <Field>
              <FieldLabel htmlFor="mcp-credential-owner-service-account">
                Service account ID
              </FieldLabel>
              <Input
                id="mcp-credential-owner-service-account"
                value={form.owner_service_account_id}
                onChange={(event) =>
                  onFormChange({ ...form, owner_service_account_id: event.target.value })
                }
                required
              />
            </Field>
          ) : null}
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
          <Field>
            <FieldLabel htmlFor="mcp-credential-expires-at">Expires at</FieldLabel>
            <Input
              id="mcp-credential-expires-at"
              type="datetime-local"
              value={form.expires_at}
              onChange={(event) => onFormChange({ ...form, expires_at: event.target.value })}
            />
          </Field>
        </FieldGroup>
        <div className="flex justify-end">
          <Button type="submit" disabled={pending}>
            {pending ? 'Saving...' : 'Add binding'}
          </Button>
        </div>
      </form>

      {bindings.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No credential bindings</EmptyTitle>
            <EmptyDescription>
              Execution will use gateway-managed auth or require a binding.
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="overflow-x-auto">
          <Table className="min-w-[72rem]">
            <TableHeader>
              <TableRow>
                <TableHead>Status</TableHead>
                <TableHead>Owner</TableHead>
                <TableHead>Material</TableHead>
                <TableHead>Storage</TableHead>
                <TableHead>Secret ref</TableHead>
                <TableHead>Expires</TableHead>
                <TableHead>Last used</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {bindings.map((binding) => (
                <TableRow key={binding.id}>
                  <TableCell>{binding.revoked_at ? 'Revoked' : 'Active'}</TableCell>
                  <TableCell>
                    <div className="flex min-w-0 flex-col gap-1">
                      <span>{binding.owner_scope_kind}</span>
                      <span className="truncate font-mono text-xs text-[var(--color-text-muted)]">
                        {binding.owner_scope_key}
                      </span>
                    </div>
                  </TableCell>
                  <TableCell>
                    {binding.material_kind}
                    {binding.header_name ? ` (${binding.header_name})` : ''}
                  </TableCell>
                  <TableCell>{binding.storage_kind}</TableCell>
                  <TableCell className="font-mono text-xs">
                    {binding.secret_ref ?? 'redacted'}
                  </TableCell>
                  <TableCell>{binding.expires_at ?? 'never'}</TableCell>
                  <TableCell>{binding.last_used_at ?? 'never'}</TableCell>
                  <TableCell className="text-right">
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
      )}
      <div className="border-t px-4 py-3 text-sm text-[var(--color-text-muted)]">
        {activeBindings.length} active binding{activeBindings.length === 1 ? '' : 's'}
      </div>
    </div>
  )
}

export function ServerFormDialog({
  mode,
  open,
  pending,
  form,
  onOpenChange,
  onFormChange,
  onSubmit,
}: {
  mode: 'create' | 'edit'
  open: boolean
  pending: boolean
  form: ServerFormState
  onOpenChange: (open: boolean) => void
  onFormChange: (form: ServerFormState) => void
  onSubmit: (event: FormEvent<HTMLFormElement>) => void
}) {
  const isCreate = mode === 'create'
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{isCreate ? 'Add MCP server' : 'Edit MCP server'}</DialogTitle>
          <DialogDescription>
            {isCreate
              ? 'Register a Streamable HTTP MCP endpoint.'
              : 'Update endpoint and auth settings.'}
          </DialogDescription>
        </DialogHeader>
        <form className="flex flex-col gap-4" onSubmit={onSubmit}>
          <ServerFormFields
            mode={mode}
            form={form}
            onFormChange={onFormChange}
          />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={pending}>
              {pending ? 'Saving...' : isCreate ? 'Add server' : 'Save changes'}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function ServerFormFields({
  mode,
  form,
  onFormChange,
}: {
  mode: 'create' | 'edit'
  form: ServerFormState
  onFormChange: (form: ServerFormState) => void
}) {
  const isCreate = mode === 'create'
  return (
    <FieldGroup className="grid gap-3 md:grid-cols-2">
      {isCreate ? (
        <Field>
          <FieldLabel htmlFor="mcp-server-key">Server key</FieldLabel>
          <Input
            id="mcp-server-key"
            value={form.server_key}
            onChange={(event) => onFormChange({ ...form, server_key: event.target.value })}
            placeholder="github"
            required
          />
        </Field>
      ) : null}
      <Field>
        <FieldLabel htmlFor="mcp-display-name">Display name</FieldLabel>
        <Input
          id="mcp-display-name"
          value={form.display_name}
          onChange={(event) => onFormChange({ ...form, display_name: event.target.value })}
          required
        />
      </Field>
      <Field className="md:col-span-2">
        <FieldLabel htmlFor="mcp-description">Description</FieldLabel>
        <Textarea
          id="mcp-description"
          value={form.description}
          onChange={(event) => onFormChange({ ...form, description: event.target.value })}
          rows={2}
        />
      </Field>
      <Field className="md:col-span-2">
        <FieldLabel htmlFor="mcp-server-url">Server URL</FieldLabel>
        <Input
          id="mcp-server-url"
          value={form.server_url}
          onChange={(event) => onFormChange({ ...form, server_url: event.target.value })}
          placeholder="https://example.com/mcp"
          required
        />
      </Field>
      <Field>
        <FieldLabel>Auth mode</FieldLabel>
        <Select
          value={form.auth_mode}
          onValueChange={(value) => onFormChange({ ...form, auth_mode: value })}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              {AUTH_MODES.map((authMode) => (
                <SelectItem key={authMode.value} value={authMode.value}>
                  {authMode.label}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
      </Field>
      <Field>
        <FieldLabel htmlFor="mcp-timeout-ms">Timeout ms</FieldLabel>
        <Input
          id="mcp-timeout-ms"
          type="number"
          min={1000}
          max={120000}
          value={form.timeout_ms}
          onChange={(event) => onFormChange({ ...form, timeout_ms: event.target.value })}
        />
      </Field>
      <Field className="md:col-span-2">
        <FieldLabel htmlFor="mcp-auth-config">Auth config JSON</FieldLabel>
        <Textarea
          id="mcp-auth-config"
          className="font-mono"
          value={form.auth_config}
          onChange={(event) => onFormChange({ ...form, auth_config: event.target.value })}
          rows={5}
        />
      </Field>
    </FieldGroup>
  )
}

export function ServerStatusBadge({ status }: { status: string }) {
  return (
    <Badge variant={status === 'active' ? 'default' : 'secondary'}>
      {status === 'active' ? 'Active' : 'Disabled'}
    </Badge>
  )
}

export function MetricLabel({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <div className="tracking-wide uppercase">{label}</div>
      <div className="truncate font-medium text-[var(--color-text)]">{value}</div>
    </div>
  )
}

export function DiscoveryStatusBadge({ status }: { status?: string | null }) {
  const label = status ?? 'not run'
  const variant = status === 'success' ? 'default' : status ? 'secondary' : 'outline'
  return <Badge variant={variant}>{label}</Badge>
}

function DetailMetric({
  label,
  value,
  mono = false,
}: {
  label: string
  value: string
  mono?: boolean
}) {
  return (
    <div className="min-w-0 rounded-md border bg-[var(--color-muted)] p-3">
      <div className="text-xs text-[var(--color-text-muted)] uppercase">{label}</div>
      <div className={`mt-1 truncate ${mono ? 'font-mono text-xs' : 'text-sm font-medium'}`}>
        {value}
      </div>
    </div>
  )
}

export function emptyServerForm(): ServerFormState {
  return {
    server_key: '',
    display_name: '',
    description: '',
    server_url: '',
    auth_mode: 'none',
    auth_config: '{}',
    timeout_ms: '30000',
  }
}

export function emptyCredentialBindingForm(): CredentialBindingFormState {
  return {
    owner_scope_kind: 'user',
    owner_user_id: '',
    owner_team_id: '',
    owner_service_account_id: '',
    material_kind: 'bearer_token',
    header_name: '',
    storage_mode: 'secret',
    secret: '',
    secret_ref: '',
    expires_at: '',
  }
}

export function formFromRecommended(server: RecommendedMcpServerView): ServerFormState {
  return {
    server_key: server.catalog_key,
    display_name: server.display_name,
    description: server.description ?? '',
    server_url: server.server_url,
    auth_mode: server.auth_mode,
    auth_config: JSON.stringify(server.auth_config ?? {}, null, 2),
    timeout_ms: '30000',
  }
}

export function formFromServer(server: McpServerView): ServerFormState {
  return {
    server_key: server.server_key,
    display_name: server.display_name,
    description: server.description ?? '',
    server_url: server.server_url,
    auth_mode: server.auth_mode,
    auth_config: JSON.stringify(server.auth_config ?? {}, null, 2),
    timeout_ms: String(server.timeout_ms),
  }
}

export function formToCreateInput(form: ServerFormState): CreateMcpServerInput | null {
  const authConfig = parseAuthConfig(form.auth_config)
  if (!authConfig) {
    return null
  }
  return {
    server_key: form.server_key.trim(),
    display_name: form.display_name.trim(),
    description: optionalString(form.description),
    server_url: form.server_url.trim(),
    transport: 'streamable_http',
    auth_mode: form.auth_mode,
    auth_config: authConfig,
    timeout_ms: optionalNumber(form.timeout_ms),
  }
}

export function formToUpdateInput(form: ServerFormState): UpdateMcpServerInput | null {
  const authConfig = parseAuthConfig(form.auth_config)
  if (!authConfig) {
    return null
  }
  return {
    display_name: form.display_name.trim(),
    description: optionalString(form.description),
    server_url: form.server_url.trim(),
    auth_mode: form.auth_mode,
    auth_config: authConfig,
    timeout_ms: optionalNumber(form.timeout_ms),
  }
}

export function formToCredentialBindingInput(
  serverId: string,
  form: CredentialBindingFormState,
): UpsertMcpCredentialBindingInput | null {
  const expiresAt = optionalDateTime(form.expires_at)
  if (expiresAt === undefined) {
    return null
  }
  return {
    server_id: serverId,
    owner_scope_kind: form.owner_scope_kind,
    owner_user_id: form.owner_scope_kind === 'user' ? requiredString(form.owner_user_id) : null,
    owner_team_id:
      form.owner_scope_kind === 'team' || form.owner_scope_kind === 'service_account'
        ? requiredString(form.owner_team_id)
        : null,
    owner_service_account_id:
      form.owner_scope_kind === 'service_account'
        ? requiredString(form.owner_service_account_id)
        : null,
    material_kind: form.material_kind,
    header_name: form.material_kind === 'static_header' ? requiredString(form.header_name) : null,
    secret: form.storage_mode === 'secret' ? requiredString(form.secret) : null,
    secret_ref: form.storage_mode === 'secret_ref' ? requiredString(form.secret_ref) : null,
    expires_at: expiresAt,
    metadata: {},
  }
}

function parseAuthConfig(value: string): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(value || '{}') as unknown
    if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
      toast.error('Auth config must be a JSON object')
      return null
    }
    return parsed as Record<string, unknown>
  } catch {
    toast.error('Auth config is not valid JSON')
    return null
  }
}

function optionalString(value: string) {
  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : null
}

function optionalNumber(value: string) {
  const trimmed = value.trim()
  return trimmed.length > 0 ? Number(trimmed) : null
}

function requiredString(value: string) {
  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : null
}

function optionalDateTime(value: string): string | null | undefined {
  const trimmed = value.trim()
  if (trimmed.length === 0) {
    return null
  }
  const date = new Date(trimmed)
  if (Number.isNaN(date.getTime())) {
    toast.error('Credential expiry is not a valid date')
    return undefined
  }
  return date.toISOString()
}
